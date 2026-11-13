"""Bounded, conservative aggregation of completed Rambler sessions.

Final updates are retained independently, not merged or interpreted as deltas.
A replacement proposal describes a whole context window, never applies it.
"""
from __future__ import annotations

from collections.abc import AsyncIterable
from contextlib import aclosing
from dataclasses import dataclass

from .engine import (ContextProvider, IncompleteSessionError, RamblerEngine,
                     S3SessionError, SessionMetrics)
from .protocol import (EditingContext, Hypothesis, ProtocolConfig, S3Response,
                       S3Status, context_request)


class AmbiguousResultError(ValueError):
    """A unique final hypothesis or requested representation is unavailable."""


class ResultLimitError(ValueError):
    """A configured final-result retention limit was exceeded."""


class UnsupportedEditError(ValueError):
    """The response does not describe a verified history replacement."""


class ContextConflictError(ValueError):
    """The current editor state no longer matches the captured snapshot."""


@dataclass(frozen=True)
class DictationResult:
    # Full responses preserve IDs, wire bytes and unknown fields at every level.
    final_updates: tuple[S3Response, ...]
    context: EditingContext | None
    metrics: SessionMetrics | None = None

    @property
    def single_hypothesis(self) -> Hypothesis:
        if len(self.final_updates) != 1 or len(self.final_updates[0].hypotheses) != 1:
            raise AmbiguousResultError("expected exactly one final update and one hypothesis")
        return self.final_updates[0].hypotheses[0]

    @property
    def text(self) -> str:
        """Render verified edits; never interpret opaque alternatives.

        With no voice edit, expose the unique hypothesis text. For intent 2,
        render field 3 or concatenate the three strings of field 7. For intent 3,
        render field 3 only; this does not establish its replacement scope.
        Other intents and field 2 have no verified text-selection policy here.
        """
        hypothesis = self.single_hypothesis
        edit = hypothesis.voice_edit
        if edit is None:
            if hypothesis.text is None:
                raise AmbiguousResultError("unique hypothesis has no text")
            return hypothesis.text
        if edit.intent in (2, 3) and edit.active_field == 3 and edit.edited_text is not None:
            return edit.edited_text
        if edit.intent == 2:
            if edit.active_field == 7 and edit.edited_context is not None:
                value = edit.edited_context
                return value.before + value.selected + value.after
        raise AmbiguousResultError("no verified text-selection policy for voice edit")


def _limit(value: int, name: str) -> None:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise ValueError(f"{name} must be a positive integer")


async def collect_dictation(engine: RamblerEngine, audio: AsyncIterable[bytes], *,
                            config: ProtocolConfig,
                            context: EditingContext | ContextProvider | None = None,
                            timeout: float = 30,
                            context_timeout: float = 2,
                            max_final_updates: int = 128,
                            max_result_bytes: int = 8 * 1024 * 1024) -> DictationResult:
    """Consume a complete engine stream and retain bounded final responses.

    The byte limit counts original protobuf response bytes of retained updates,
    not Python object overhead. The engine and its channel remain caller-owned.
    """
    _limit(max_final_updates, "max_final_updates")
    _limit(max_result_bytes, "max_result_bytes")
    captured = None
    if isinstance(context, EditingContext):
        context_request(context)  # same static validation as the engine
        captured = context
    elif context is not None:
        if not callable(context):
            raise ValueError("context must be EditingContext, an async provider, or None")
        provider = context
        called = False

        async def capture():
            nonlocal captured, called
            if called:
                raise RuntimeError("context provider invoked more than once")
            called = True
            current = await provider()
            if current is not None:
                context_request(current)
            captured = current
            return current

        context = capture

    updates: list[S3Response] = []
    size = 0
    eof = False
    eof_metrics = None
    end = False
    s3_error = None
    async with aclosing(engine.stream(audio, config=config, context=context,
                                      timeout=timeout, context_timeout=context_timeout)) as events:
        async for event in events:
            response = event.response
            if response is None:
                if eof:
                    raise IncompleteSessionError("duplicate engine EOF")
                eof = True
                eof_metrics = event.metrics
                continue
            if eof:
                raise IncompleteSessionError("response after engine EOF")
            if response.status == S3Status.ERROR or response.error_code not in (None, 0):
                s3_error = response
            if response.status == S3Status.END:
                end = True
            recognition = response.result.recognition if response.result is not None else None
            if recognition is not None and recognition.state == 1:
                size += len(response.raw)
                if len(updates) >= max_final_updates or size > max_result_bytes:
                    raise ResultLimitError("final result retention limit exceeded")
                updates.append(response)
    if s3_error is not None:
        raise S3SessionError(s3_error)
    if not eof or not end:
        raise IncompleteSessionError("successful engine EOF and S3 END required")
    return DictationResult(tuple(updates), captured, eof_metrics)


@dataclass(frozen=True)
class ReplacementProposal:
    """Replacement for precisely the captured context window, not a text delta."""
    snapshot: EditingContext
    expected_revision: str | int | None
    replacement_text: str
    replacement_context: EditingContext | None

    def __post_init__(self) -> None:
        if self.expected_revision is not None and type(self.expected_revision) not in (str, int):
            raise ValueError("expected_revision must be an immutable str or int")

    def validate_current(self, current: EditingContext, *, revision: str | int | None = None) -> None:
        if revision is not None and type(revision) not in (str, int):
            raise ValueError("revision must be an immutable str or int")
        if not isinstance(current, EditingContext) or (
            current.before, current.selected, current.after
        ) != (self.snapshot.before, self.snapshot.selected, self.snapshot.after):
            raise ContextConflictError("editing context changed")
        if self.expected_revision is not None and (type(revision) is not type(self.expected_revision)
                                                   or revision != self.expected_revision):
            raise ContextConflictError("editing revision changed")

    @property
    def selection_utf16_range(self) -> tuple[int, int] | None:
        """UTF-16 code-unit offsets within replacement_text, only for field 7."""
        if self.replacement_context is None:
            return None
        before = len(self.replacement_context.before.encode("utf-16-le")) // 2
        selected = len(self.replacement_context.selected.encode("utf-16-le")) // 2
        return before, before + selected


def replacement_proposal(result: DictationResult, *,
                         expected_revision: str | int | None = None) -> ReplacementProposal:
    """Describe verified intent-2 history, without applying it.

    expected_revision must be the revision associated with the captured context,
    not the current revision after the request. For a delayed provider, capture
    both atomically in the editor's own snapshot operation, for example::

        snapshot_revision = None
        async def provider():
            nonlocal snapshot_revision
            context, snapshot_revision = editor.atomic_snapshot()
            return context
        result = await collect_dictation(engine, audio, config=config, context=provider)
        proposal = replacement_proposal(result, expected_revision=snapshot_revision)

    The editor must provide a stable, immutable str/int revision that changes on
    every edit for ABA detection. Text comparison alone cannot detect ABA.
    """
    hypothesis = result.single_hypothesis
    edit = hypothesis.voice_edit
    if result.context is None or edit is None or edit.intent != 2:
        raise UnsupportedEditError("a captured context and intent 2 are required")
    if edit.active_field == 3 and edit.edited_text is not None:
        return ReplacementProposal(result.context, expected_revision, edit.edited_text, None)
    if edit.active_field == 7 and edit.edited_context is not None:
        value = edit.edited_context
        return ReplacementProposal(result.context, expected_revision,
                                   value.before + value.selected + value.after, value)
    raise UnsupportedEditError("no verified history replacement field")
