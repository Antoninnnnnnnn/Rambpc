"""Streaming assembly of the reconstructed Ogg/Opus S3 path.

Responses are candidates, never automatically applied edits or final hypotheses.
A successful EOF event requires S3 END and gRPC OK. Use aclosing() on early exit.
"""
from __future__ import annotations

import asyncio
from collections.abc import AsyncIterable, AsyncIterator
from dataclasses import dataclass
import math
import time
from typing import Protocol
import uuid

from .credentials import MetadataProvider, validate_metadata
from .protocol import (EditingContext, ProtocolConfig, S3Response, S3Status, audio_request,
                       context_request, decode_response, end_request, initial_request)
from .transport import GrpcTransport


class ContextProvider(Protocol):
    """Capture current editing context once, after audio exhaustion/listening stop.

    Return None only to explicitly omit context. Implementations must cooperate
    with asyncio cancellation; exceptions and deadlines abort the session.
    """

    async def __call__(self) -> EditingContext | None: ...


class IncompleteSessionError(RuntimeError):
    """gRPC completed without the S3 END status; no successful EOF is emitted."""


class S3SessionError(RuntimeError):
    """An S3 application error, separate from gRPC transport status.

    The original response remains available explicitly. Its server-provided text
    is deliberately excluded from str/repr to avoid logging sensitive details.
    """

    def __init__(self, response: S3Response) -> None:
        super().__init__("S3 reported an application error")
        self.response = response


@dataclass(frozen=True)
class SessionMetrics:
    """Counts are payloads yielded to transport, not completed writes or network bytes."""
    session_id: str
    requests_produced: int
    request_protobuf_bytes: int
    responses_received: int
    response_protobuf_bytes: int
    elapsed_seconds: float
    first_response_seconds: float | None
    eof_seconds: float | None


@dataclass(frozen=True)
class EngineEvent:
    response: S3Response | None
    metrics: SessionMetrics


class _Session:
    """Clean up even if the public engine generator is paused at a yield."""

    def __init__(self, source: AsyncIterator[bytes]) -> None:
        self.source = source
        self.responses: AsyncIterator[bytes] | None = None
        self.pending: asyncio.Task | None = None
        self.closed = False
        self._cleanup: asyncio.Task | None = None
        self._source_cleanup: asyncio.Task | None = None

    async def step(self, factory):
        if self.closed:
            raise asyncio.CancelledError()
        self.pending = asyncio.create_task(factory())
        try:
            return await self.pending
        finally:
            self.pending = None

    async def close_source(self) -> None:
        if self._source_cleanup is None:
            async def cleanup():
                close = getattr(self.source, "aclose", None)
                if close is not None:
                    await close()
            self._source_cleanup = asyncio.create_task(cleanup())
        await asyncio.shield(self._source_cleanup)

    async def close(self) -> None:
        if self._cleanup is None:
            self.closed = True
            pending = self.pending
            if pending is not None:
                pending.cancel()
            self._cleanup = asyncio.create_task(self._finish(pending))
        await asyncio.shield(self._cleanup)

    async def _finish(self, pending) -> None:
        if pending is not None:
            await asyncio.gather(pending, return_exceptions=True)
        try:
            if self.responses is not None:
                await self.responses.aclose()
        finally:
            await self.close_source()


class RamblerEngine:
    """Own a default transport; injected channels remain caller-owned.

    close() always cancels this engine's sessions, including blocked credential
    providers and paused consumers, but never closes an injected channel. Sessions
    may run concurrently on one event loop. Providers/sources must cooperate with
    asyncio cancellation. All responses are drained before reporting application
    errors or success, so a late gRPC failure is never hidden by an S3 response.
    """

    def __init__(self, transport: GrpcTransport | None = None,
                 credentials: MetadataProvider | None = None) -> None:
        self.transport = transport if transport is not None else GrpcTransport()
        self.credentials = credentials
        self._owns_transport = transport is None
        self._sessions: set[_Session] = set()
        self._lifecycle_lock = asyncio.Lock()
        self._closing = False

    async def __aenter__(self) -> RamblerEngine:
        await self.open()
        return self

    async def __aexit__(self, *exc: object) -> None:
        await self.close()

    async def open(self, timeout: float = 10) -> None:
        async with self._lifecycle_lock:
            await self.transport.open(timeout=timeout)

    async def close(self) -> None:
        async with self._lifecycle_lock:
            self._closing = True
            sessions = tuple(self._sessions)
            try:
                results = await asyncio.gather(
                    *(session.close() for session in sessions), return_exceptions=True
                )
                for result in results:
                    if isinstance(result, BaseException):
                        raise result
            finally:
                self._sessions.difference_update(sessions)
                try:
                    if self._owns_transport:
                        await self.transport.close()
                finally:
                    self._closing = False

    async def stream(self, audio: AsyncIterable[bytes], *, config: ProtocolConfig,
                     context: EditingContext | ContextProvider | None = None,
                     timeout: float = 30,
                     context_timeout: float = 2.0) -> AsyncIterator[EngineEvent]:
        """Stream audio, then obtain current context before the end marker.

        A static EditingContext is validated before RPC setup. A ContextProvider
        is invoked exactly once after audio exhaustion (never on failed/cancelled
        audio), with min(context_timeout, remaining session deadline). Its result
        is validated before transmission; failures never become an omitted context
        or a successful end marker. None is an explicit, valid omission only.
        """
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError("timeout must be finite and positive")
        if not math.isfinite(context_timeout) or context_timeout <= 0:
            raise ValueError("context_timeout must be finite and positive")
        if not isinstance(config, ProtocolConfig):
            raise ValueError("config must be ProtocolConfig")
        if config.encoding != 10 or config.sample_rate != 16000 or config.channels != 1:
            raise ValueError("only Ogg/Opus encoding 10 at 16000 Hz mono is reconstructed")
        # Keep static prevalidation; defer only a callable provider's execution.
        context_provider: ContextProvider | None = None
        context_payload = None
        if isinstance(context, EditingContext):
            context_payload = context_request(context)
        elif context is not None:
            if not callable(context):
                raise ValueError("context must be EditingContext, an async provider, or None")
            context_provider = context
        if self._closing:
            raise RuntimeError("engine is closing")
        start = time.monotonic()
        session = _Session(aiter(audio))
        self._sessions.add(session)
        session_id = uuid.uuid4().hex
        sent = sent_bytes = received = received_bytes = 0
        first = eof = None
        s3_end = False
        requests_exhausted = False
        s3_error: S3Response | None = None

        def snapshot() -> SessionMetrics:
            return SessionMetrics(session_id, sent, sent_bytes, received, received_bytes,
                                  time.monotonic() - start, first, eof)

        async def requests() -> AsyncIterator[bytes]:
            nonlocal sent, sent_bytes, requests_exhausted
            def count(payload: bytes) -> bytes:
                nonlocal sent, sent_bytes
                sent += 1
                sent_bytes += len(payload)
                return payload
            elapsed = lambda: int((time.monotonic() - start) * 1000)
            try:
                yield count(initial_request(config, elapsed_ms=elapsed()))
                async for chunk in session.source:
                    if not isinstance(chunk, bytes):
                        raise TypeError("audio must yield bytes")
                    if chunk:
                        yield count(audio_request(chunk, elapsed_ms=elapsed()))
                # Release audio resources at listening stop, before consulting
                # the editor. Also lets pending cancellation stop the callback.
                await session.close_source()
                payload = context_payload
                if context_provider is not None:
                    if session.closed:
                        raise asyncio.CancelledError()
                    remaining = timeout - (time.monotonic() - start)
                    if remaining <= 0:
                        raise TimeoutError("session deadline exceeded before context")
                    current = await asyncio.wait_for(
                        context_provider(), min(context_timeout, remaining)
                    )
                    payload = None if current is None else context_request(current)
                if payload is not None:
                    yield count(payload)
                yield count(end_request())
                requests_exhausted = True
            finally:
                await session.close_source()

        try:
            metadata = ()
            if self.credentials is not None:
                metadata = validate_metadata(await asyncio.wait_for(
                    session.step(self.credentials.metadata), timeout
                ))
            remaining = timeout - (time.monotonic() - start)
            if remaining <= 0:
                raise TimeoutError("session deadline exceeded")
            session.responses = self.transport.stream(requests(), metadata=metadata, timeout=remaining)
            while True:
                try:
                    payload = await session.step(lambda: anext(session.responses))
                except StopAsyncIteration:
                    break
                received += 1
                received_bytes += len(payload)
                if first is None:
                    first = time.monotonic() - start
                response = decode_response(payload)
                if response.status == S3Status.END:
                    s3_end = True
                if response.status == S3Status.ERROR or response.error_code not in (None, 0):
                    s3_error = response
                yield EngineEvent(response, snapshot())
            if s3_error is not None:
                raise S3SessionError(s3_error)
            if not s3_end:
                raise IncompleteSessionError("S3 end status not observed")
            if not requests_exhausted:
                raise IncompleteSessionError("request stream ended before exhaustion")
            eof = time.monotonic() - start
            yield EngineEvent(None, snapshot())
        finally:
            try:
                await session.close()
            finally:
                self._sessions.discard(session)
