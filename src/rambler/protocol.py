"""Statically verified S3 byte messages, not a validated backend/API contract.

Field numbers follow docs/research/protocol.md. Status/recognition/speech enums
remain raw numbers: no response is labelled final and no edit is applied here.
No credentials, Android-device impersonation, or experiment flags are emitted.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import IntEnum
import struct

from .wire import Field, bytes_field, encode_varint, parse_fields, string_field, varint_field

MAX_MESSAGE_BYTES = 8 * 1024 * 1024
MAX_FIELDS = 100_000
# Defensive SDK limits for caller-supplied hints, not observed server limits.
MAX_HINTS_PER_CATEGORY = 1024
MAX_HINT_UTF8_BYTES = 1024 * 1024
MAX_SECONDARY_LANGUAGES = 16
MAX_SECONDARY_LANGUAGE_UTF8_BYTES = 1024

USER_INFO = 293000
AUDIO_INFO = 293100
AUDIO_DATA = 293101
CLIENT_INFO = 294000
RECOGNIZER_INFO = 294500
INPUT_BOX_INFO = 1203
INITIAL_PARAMETER = 29735924
RECOGNITION_RESPONSE = 1253625
VOICE_EDITING_RESULT = 74956224


def _integer(value: int, name: str, *, minimum: int = 0, maximum: int = 2**63 - 1) -> None:
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise ValueError(f"{name} must be an integer in {minimum}..{maximum}")


def _text(value: str, name: str, *, nonempty: bool = False) -> None:
    if not isinstance(value, str) or (nonempty and not value):
        raise ValueError(f"{name} must be {'a nonempty' if nonempty else 'a'} string")
    try:
        if len(value.encode("utf-8")) > MAX_MESSAGE_BYTES:
            raise ValueError(f"{name} exceeds message size limit")
    except UnicodeEncodeError as exc:
        raise ValueError(f"{name} must be valid UTF-8") from exc


def _string_tuple(
    values: tuple[str, ...], name: str, *, max_count: int, remaining_bytes: int,
    nonempty: bool = False,
) -> int:
    """Validate bounded immutable input before wire assembly; return byte budget."""
    if not isinstance(values, tuple):
        raise ValueError(f"{name} must be a tuple of strings")
    if len(values) > max_count:
        raise ValueError(f"{name} exceeds SDK string count limit")
    for value in values:
        if not isinstance(value, str) or (nonempty and not value):
            raise ValueError(f"{name} must contain only {'nonempty ' if nonempty else ''}strings")
        # UTF-8 needs at least one byte per code point. Reject oversized strings
        # before allocating their encoding or any wire payload.
        if len(value) > remaining_bytes:
            raise ValueError(f"{name} exceeds SDK total UTF-8 byte limit")
        try:
            remaining_bytes -= len(value.encode("utf-8"))
        except UnicodeEncodeError as exc:
            raise ValueError(f"{name} must contain valid UTF-8 strings") from exc
        if remaining_bytes < 0:
            raise ValueError(f"{name} exceeds SDK total UTF-8 byte limit")
    return remaining_bytes


def _bounded(payload: bytes) -> bytes:
    if len(payload) > MAX_MESSAGE_BYTES:
        raise ValueError("message exceeds size limit")
    return payload


@dataclass(frozen=True)
class ProtocolConfig:
    language: str = "fr-FR"
    application_id: str = "gboard"
    sample_rate: int = 16000
    encoding: int = 10
    channels: int = 1
    routing: str = "gboard_gemini_v3_streaming_voice_edit_mul"
    platform: str = "Python"
    # Initial InputBoxInfo only; raw caller strings, never collected contacts.
    # Persistence and server-side effects have not been validated.
    bias_terms: tuple[str, ...] = field(default=(), repr=False)
    preferences: tuple[str, ...] = field(default=(), repr=False)
    # Raw secondary locales in UserInfo, without primary-locale normalization.
    secondary_languages: tuple[str, ...] = field(default=(), repr=False)

    def __post_init__(self) -> None:
        for name in ("language", "application_id", "routing", "platform"):
            _text(getattr(self, name), name, nonempty=True)
        _integer(self.sample_rate, "sample_rate", minimum=1, maximum=2**31 - 1)
        _integer(self.encoding, "encoding", maximum=2**31 - 1)
        _integer(self.channels, "channels", minimum=1, maximum=2**31 - 1)
        remaining = MAX_HINT_UTF8_BYTES
        for name in ("bias_terms", "preferences"):
            remaining = _string_tuple(
                getattr(self, name), name, max_count=MAX_HINTS_PER_CATEGORY,
                remaining_bytes=remaining,
            )
        _string_tuple(
            self.secondary_languages, "secondary_languages", max_count=MAX_SECONDARY_LANGUAGES,
            remaining_bytes=MAX_SECONDARY_LANGUAGE_UTF8_BYTES, nonempty=True,
        )


@dataclass(frozen=True)
class EditingContext:
    before: str = ""
    selected: str = ""
    after: str = ""
    cursor_aware: bool = False
    # Populated only when decoding a cursor-context message in a voice edit.
    raw: bytes = field(default=b"", repr=False, compare=False)
    unknown_fields: tuple[Field, ...] = field(default=(), repr=False)

    def __post_init__(self) -> None:
        for name in ("before", "selected", "after"):
            _text(getattr(self, name), name)
        if not isinstance(self.cursor_aware, bool):
            raise ValueError("cursor_aware must be bool")


def initial_request(config: ProtocolConfig, *, elapsed_ms: int = 0) -> bytes:
    """Build the verified Jetson/Rambler init subset (default Ogg/Opus16k mono).

    Configurable audio enums are emitted as supplied; backend support beyond the
    observed encoding10 / 16000Hz / mono is NOT established. ClientInfo8 defaults
    to an honest native-client label, not the APK's Android value. Conditional
    RecognizerInfo7/40/91 and UserInfo20 are omitted: their effective settings
    are unknown. This subset is not claimed to be a byte-identical Android init
    or a server-validated recipe. Optional caller hints use InputBoxInfo repeated
    string fields3/5 only; no editing context or experimental flags are added.
    Secondary languages use UserInfo4 with language1 and verified value2=1;
    the primary language and all secondary strings remain unnormalized.
    Persistence and server-side effects of these options have not been validated.
    """
    if not isinstance(config, ProtocolConfig):
        raise ValueError("config must be ProtocolConfig")
    _integer(elapsed_ms, "elapsed_ms")
    audio = (
        encode_varint((2 << 3) | 5) + struct.pack("<f", config.sample_rate)
        + varint_field(3, config.encoding)
        + varint_field(4, config.channels)
        + varint_field(9, elapsed_ms)
    )
    recognizer = (
        bytes_field(1, varint_field(4, 0))
        + varint_field(5, 1)
        + varint_field(13, 1)
        + varint_field(20, 1)
        + varint_field(41, 1)
        + string_field(50, config.routing)
    )
    client = (
        string_field(2, config.application_id)
        + string_field(8, config.platform)
        + varint_field(26, 1)
    )
    user = bytes_field(2, string_field(1, config.language)) + b"".join(
        bytes_field(4, string_field(1, language) + varint_field(2, 1))
        for language in config.secondary_languages
    )
    hints = b""
    if config.bias_terms or config.preferences:
        hints = bytes_field(
            INPUT_BOX_INFO,
            b"".join(string_field(3, value) for value in config.bias_terms)
            + b"".join(string_field(5, value) for value in config.preferences),
        )
    return _bounded(
        string_field(1, "beyond-a2a-recognizer")
        + varint_field(2, 1)
        + bytes_field(USER_INFO, user)
        + bytes_field(AUDIO_INFO, audio)
        + bytes_field(CLIENT_INFO, client)
        + bytes_field(RECOGNIZER_INFO, recognizer)
        + bytes_field(INITIAL_PARAMETER, varint_field(1, 500))
        + hints
    )


def audio_request(chunk: bytes, *, elapsed_ms: int = 0) -> bytes:
    """Wrap a chunk of a continuous Ogg/Opus stream; do not re-encode or split it."""
    if not isinstance(chunk, bytes):
        raise ValueError("chunk must be bytes")
    _bounded(chunk)
    _integer(elapsed_ms, "elapsed_ms")
    return _bounded(
        bytes_field(AUDIO_INFO, varint_field(9, elapsed_ms))
        + bytes_field(AUDIO_DATA, bytes_field(1, chunk))
    )


def context_request(context: EditingContext) -> bytes:
    """Build only extension1203, intended after audio and before end_request().

    Simple mode represents preceding text only. Selection/after text requires
    cursor-aware mode; rejecting it prevents silently discarding editing context.
    """
    if not isinstance(context, EditingContext):
        raise ValueError("context must be EditingContext")
    if context.cursor_aware:
        cursor = (
            string_field(1, context.before)
            + string_field(2, context.selected)
            + string_field(3, context.after)
        )
        content = bytes_field(4, cursor)
    else:
        if context.selected or context.after:
            raise ValueError("selected/after require cursor_aware=True")
        content = string_field(1, context.before)
    return _bounded(bytes_field(INPUT_BOX_INFO, content))


def end_request() -> bytes:
    """Exact observed end marker; the transport must half-close after sending it."""
    return b"\x10\x01\x18\x01"


class S3Status(IntEnum):
    NORMAL = 0
    END = 1
    ERROR = 2
    NOT_STARTED = 3


class SpeechEvent(IntEnum):
    START_SPEECH = 0
    END_SPEECH = 1
    END_UTTERANCE = 3


@dataclass(frozen=True)
class VoiceEditResult:
    transcript: str | None = None
    alternative_text: str | None = None
    edited_text: str | None = None
    edited_context: EditingContext | None = None
    intent: int | None = None
    # The active oneof member is 2, 3, or 7; never auto-apply these candidates.
    active_field: int | None = None
    unknown_fields: tuple[Field, ...] = ()
    raw: bytes = field(default=b"", repr=False)


@dataclass(frozen=True)
class Hypothesis:
    text: str | None = None
    confidence: float | None = None
    voice_edit: VoiceEditResult | None = None
    unknown_fields: tuple[Field, ...] = ()
    raw: bytes = field(default=b"", repr=False)


@dataclass(frozen=True)
class PartialHypothesis:
    text: str | None = None
    confidence: float | None = None
    unknown_fields: tuple[Field, ...] = ()
    raw: bytes = field(default=b"", repr=False)


@dataclass(frozen=True)
class HypothesisResult:
    hypotheses: tuple[Hypothesis, ...] = ()
    unknown_fields: tuple[Field, ...] = ()
    raw: bytes = field(default=b"", repr=False)


@dataclass(frozen=True)
class PartialResult:
    partials: tuple[PartialHypothesis, ...] = ()
    unknown_fields: tuple[Field, ...] = ()
    raw: bytes = field(default=b"", repr=False)


@dataclass(frozen=True)
class RecognitionResult:
    state: int | None = None
    hypothesis_result: HypothesisResult | None = None
    partial_result: PartialResult | None = None
    unknown_fields: tuple[Field, ...] = ()
    raw: bytes = field(default=b"", repr=False)

    @property
    def hypotheses(self) -> tuple[Hypothesis, ...]:
        return () if self.hypothesis_result is None else self.hypothesis_result.hypotheses

    @property
    def partials(self) -> tuple[PartialHypothesis, ...]:
        return () if self.partial_result is None else self.partial_result.partials


@dataclass(frozen=True)
class SpeechEventResult:
    event: int | None = None
    unknown_fields: tuple[Field, ...] = ()
    raw: bytes = field(default=b"", repr=False)


@dataclass(frozen=True)
class RecognitionResponse:
    recognition: RecognitionResult | None = None
    speech: SpeechEventResult | None = None
    result_id: str | None = None
    alternative_text: str | None = None
    active_field: int | None = None
    active_raw: bytes | None = field(default=None, repr=False)
    unknown_fields: tuple[Field, ...] = ()
    raw: bytes = field(default=b"", repr=False)


@dataclass(frozen=True)
class S3Response:
    status: int | None = None
    error_code: int | None = None
    error_message: str | None = None
    result: RecognitionResponse | None = None
    unknown_fields: tuple[Field, ...] = ()
    raw: bytes = field(default=b"", repr=False)

    @property
    def recognition_state(self) -> int | None:
        if self.result is None or self.result.recognition is None:
            return None
        return self.result.recognition.state

    @property
    def speech_event(self) -> int | None:
        if self.result is None or self.result.speech is None:
            return None
        return self.result.speech.event

    @property
    def hypotheses(self) -> tuple[Hypothesis, ...]:
        if self.result is None or self.result.recognition is None:
            return ()
        return self.result.recognition.hypotheses

    @property
    def partials(self) -> tuple[PartialHypothesis, ...]:
        if self.result is None or self.result.recognition is None:
            return ()
        return self.result.recognition.partials


@dataclass(frozen=True)
class _MergedPayload:
    # Keep occurrence boundaries until parsing: concatenation alone could heal a
    # truncated protobuf field by borrowing bytes from the next occurrence.
    parts: tuple[bytes, ...]


@dataclass
class _Budget:
    remaining: int = MAX_FIELDS


def _int32(value: int) -> int:
    value &= 0xFFFFFFFF
    return value - (1 << 32) if value & (1 << 31) else value


class _Message:
    """Typed field selection; wrong wire types remain unknown, as in protobuf.

    Scalars use the last occurrence; singular messages merge their occurrences.
    Repetitions preserve wire order. Every known string/message occurrence is
    decoded, including overwritten oneof members, so malformed data cannot hide.
    A merged submessage's raw bytes concatenate its original occurrence payloads;
    the root response's raw bytes always retain the exact original encoding.
    """

    def __init__(self, raw: bytes | _MergedPayload, known: set[tuple[int, int]], budget: _Budget):
        parts = raw.parts if isinstance(raw, _MergedPayload) else (raw,)
        self.raw = _bounded(b"".join(parts)) if isinstance(raw, _MergedPayload) else raw
        self.budget = budget
        self.fields = []
        for part in parts:
            fields = parse_fields(part, max_size=MAX_MESSAGE_BYTES, max_fields=budget.remaining)
            budget.remaining -= len(fields)
            self.fields.extend(fields)
        self.unknown_fields = tuple(
            item for item in self.fields if (item.number, item.wire_type) not in known
        )

    def all(self, number: int, wire_type: int) -> tuple[int | bytes, ...]:
        return tuple(
            item.value for item in self.fields
            if item.number == number and item.wire_type == wire_type
        )

    def integer(self, number: int) -> int | None:
        values = self.all(number, 0)
        return _int32(values[-1]) if values else None

    def string(self, number: int) -> str | None:
        values = [value.decode("utf-8", errors="strict") for value in self.all(number, 2)]
        return values[-1] if values else None

    def floating(self, number: int, *, double: bool = False) -> float | None:
        values = self.all(number, 1 if double else 5)
        return struct.unpack("<d" if double else "<f", values[-1])[0] if values else None

    def message(self, number: int, parser):
        values = self.all(number, 2)
        # Parse each fragment once, then apply protobuf MergeFrom semantics to
        # the combined fields without resetting or charging the budget twice.
        return parser(_MergedPayload(values), self.budget) if values else None

    def oneof(self, numbers):
        """Yield runs of the same active member, ignoring independent fields."""
        active = None
        values = []
        for item in self.fields:
            if item.wire_type != 2 or item.number not in numbers:
                continue
            if active is not None and item.number != active:
                yield active, values
                values = []
            active = item.number
            values.append(item.value)
        if active is not None:
            yield active, values


def _context(raw: bytes | _MergedPayload, budget: _Budget) -> EditingContext:
    message = _Message(raw, {(1, 2), (2, 2), (3, 2)}, budget)
    return EditingContext(
        before=message.string(1) or "",
        selected=message.string(2) or "",
        after=message.string(3) or "",
        cursor_aware=True,
        unknown_fields=message.unknown_fields,
        raw=message.raw,
    )


def _voice_edit(raw: bytes | _MergedPayload, budget: _Budget) -> VoiceEditResult:
    message = _Message(raw, {(1, 2), (2, 2), (3, 2), (4, 0), (7, 2)}, budget)
    alternative_text = edited_text = edited_context = None
    active_field = None
    for active_field, values in message.oneof((2, 3, 7)):
        alternative_text = edited_text = edited_context = None
        if active_field == 2:
            alternative_text = [value.decode("utf-8", errors="strict") for value in values][-1]
        elif active_field == 3:
            edited_text = [value.decode("utf-8", errors="strict") for value in values][-1]
        else:
            edited_context = _context(_MergedPayload(tuple(values)), budget)
    return VoiceEditResult(
        transcript=message.string(1),
        alternative_text=alternative_text,
        edited_text=edited_text,
        edited_context=edited_context,
        intent=message.integer(4),
        active_field=active_field,
        unknown_fields=message.unknown_fields,
        raw=message.raw,
    )


def _hypothesis(raw: bytes | _MergedPayload, budget: _Budget) -> Hypothesis:
    message = _Message(raw, {(1, 2), (2, 5), (VOICE_EDITING_RESULT, 2)}, budget)
    return Hypothesis(
        text=message.string(1),
        confidence=message.floating(2),
        voice_edit=message.message(VOICE_EDITING_RESULT, _voice_edit),
        unknown_fields=message.unknown_fields,
        raw=message.raw,
    )


def _partial(raw: bytes | _MergedPayload, budget: _Budget) -> PartialHypothesis:
    message = _Message(raw, {(1, 2), (2, 1)}, budget)
    return PartialHypothesis(
        text=message.string(1),
        confidence=message.floating(2, double=True),
        unknown_fields=message.unknown_fields,
        raw=message.raw,
    )


def _hypothesis_result(raw: bytes | _MergedPayload, budget: _Budget) -> HypothesisResult:
    message = _Message(raw, {(3, 2)}, budget)
    return HypothesisResult(
        hypotheses=tuple(_hypothesis(value, budget) for value in message.all(3, 2)),
        unknown_fields=message.unknown_fields,
        raw=message.raw,
    )


def _partial_result(raw: bytes | _MergedPayload, budget: _Budget) -> PartialResult:
    message = _Message(raw, {(1, 2)}, budget)
    return PartialResult(
        partials=tuple(_partial(value, budget) for value in message.all(1, 2)),
        unknown_fields=message.unknown_fields,
        raw=message.raw,
    )


def _recognition(raw: bytes | _MergedPayload, budget: _Budget) -> RecognitionResult:
    message = _Message(raw, {(1, 0), (3, 2), (4, 2)}, budget)
    return RecognitionResult(
        state=message.integer(1),
        hypothesis_result=message.message(3, _hypothesis_result),
        partial_result=message.message(4, _partial_result),
        unknown_fields=message.unknown_fields,
        raw=message.raw,
    )


def _speech(raw: bytes | _MergedPayload, budget: _Budget) -> SpeechEventResult:
    message = _Message(raw, {(1, 0)}, budget)
    return SpeechEventResult(
        event=message.integer(1), unknown_fields=message.unknown_fields, raw=message.raw
    )


# Verified aapd oneof members. Unknown members' contents remain opaque, but they
# must still clear a preceding recognition/speech alternative. Fields3/4 are NOT
# in the oneof and therefore do not clear anything.
_RESULT_ONEOF = frozenset((1, 2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 18, 19, 20, 21))


def _response_result(raw: bytes | _MergedPayload, budget: _Budget) -> RecognitionResponse:
    message = _Message(raw, {(1, 2), (2, 2), (3, 2), (4, 2)}, budget)
    recognition = speech = None
    active_field = active_raw = None
    for active_field, values in message.oneof(_RESULT_ONEOF):
        recognition = speech = None
        active_raw = b"".join(values)
        if active_field == 1:
            recognition = _recognition(_MergedPayload(tuple(values)), budget)
        elif active_field == 2:
            speech = _speech(_MergedPayload(tuple(values)), budget)
    return RecognitionResponse(
        recognition=recognition,
        speech=speech,
        result_id=message.string(3),
        alternative_text=message.string(4),
        active_field=active_field,
        active_raw=active_raw,
        unknown_fields=message.unknown_fields,
        raw=message.raw,
    )


def decode_response(payload: bytes) -> S3Response:
    """Decode bounded protobuf bytes; malformed protobuf/UTF-8 raises ValueError.

    Missing singular values remain None, unknown enums remain numeric, and unknown
    fields survive at every decoded level. Root raw bytes preserve the exact
    original encoding. Singular messages merge; their raw/active_raw payloads
    concatenate the original occurrences within the active oneof run, if any.
    S3 error statuses are returned, not raised. No recognition/voice-edit candidate
    is inferred to be final or applied.
    """
    message = _Message(payload, {(1, 0), (2, 0), (3, 2), (RECOGNITION_RESPONSE, 2)}, _Budget())
    return S3Response(
        status=message.integer(1),
        error_code=message.integer(2),
        error_message=message.string(3),
        result=message.message(RECOGNITION_RESPONSE, _response_result),
        unknown_fields=message.unknown_fields,
        raw=payload,
    )
