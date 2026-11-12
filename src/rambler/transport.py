"""Opaque bytes over bidirectional gRPC, without application-level decoding.

An OK gRPC status is only transport completion, not a Rambler final result.
No requests are replayed and no metadata or payloads are logged.
"""

from __future__ import annotations

import asyncio
from collections.abc import AsyncIterable, AsyncIterator, Sequence
import math
from typing import Any

import grpc

DEFAULT_TARGET = "speechs3proto2-pa.googleapis.com:443"
DEFAULT_METHOD = "/speech.s3.S3StubbyClientConnectorService/S3Bidi"


def _validate_timeout(timeout: float) -> None:
    if not math.isfinite(timeout) or timeout <= 0:
        raise ValueError("timeout must be finite and positive")


class _Session:
    """Own RPC tasks independently of a consumer paused at an async-generator yield."""

    def __init__(self, call: Any, requests: AsyncIterable[bytes]) -> None:
        self.call = call
        self.error: BaseException | None = None
        self.reader: asyncio.Task[Any] | None = None
        self._stopping_writer = False
        self._cleanup: asyncio.Task[None] | None = None
        self.writer = asyncio.create_task(self._write(requests))
        self.monitor = asyncio.create_task(self._watch_terminal_status())

    async def _write(self, requests: AsyncIterable[bytes]) -> None:
        try:
            iterator = aiter(requests)
            try:
                async for request in iterator:
                    if not isinstance(request, bytes):
                        raise TypeError("requests must yield bytes")
                    try:
                        await self.call.write(request)
                    except (grpc.RpcError, asyncio.InvalidStateError):
                        # A server may finish before our next write. Its read-side
                        # terminal status is authoritative (and may even be OK).
                        return
                try:
                    await self.call.done_writing()
                except (grpc.RpcError, asyncio.InvalidStateError):
                    return
            finally:
                close_iterator = getattr(iterator, "aclose", None)
                if close_iterator is not None:
                    await close_iterator()
        except BaseException as exc:
            if not (isinstance(exc, asyncio.CancelledError) and self._stopping_writer):
                self.error = exc
                self.call.cancel()
            # The supervisor retrieves the task exception. Keeping the original
            # exception here also preserves cancellation semantics for producers.
            raise

    def _stop_writer(self) -> None:
        if not self._stopping_writer:
            self._stopping_writer = True
            if not self.writer.done():
                self.writer.cancel()

    async def _watch_terminal_status(self) -> None:
        # Unlike a read started by the consumer, this remains active even while
        # the consumer is paused after a response. It enforces the RPC deadline
        # against blocked producers and stops them on early server completion.
        try:
            await self.call.code()
        finally:
            self._stop_writer()
            await asyncio.gather(self.writer, return_exceptions=True)

    async def close(self) -> None:
        if self._cleanup is None:
            self.call.cancel()
            self._stop_writer()
            if self.reader is not None:
                self.reader.cancel()
            self._cleanup = asyncio.create_task(self._finish_cleanup())
        await asyncio.shield(self._cleanup)

    async def _finish_cleanup(self) -> None:
        await asyncio.gather(
            self.writer,
            self.monitor,
            *(() if self.reader is None else (self.reader,)),
            return_exceptions=True,
        )


class GrpcTransport:
    """Reusable channel with isolated concurrent calls, confined to one event loop.

    TLS uses gRPC's standard trust roots and hostname verification. ``secure=False``
    is reserved for loopback/test servers; never use it for production traffic.
    Each stream owns its request iterator and closes it on exit when it exposes
    ``aclose``. Reads and writes run concurrently, with native gRPC write-side
    backpressure and no application-level request queue or audio accumulation.

    Exhaust responses to observe the terminal gRPC status, including errors after
    partial responses. For early exit use ``contextlib.aclosing(stream)`` or call
    ``await stream.aclose()``: Python's ``async for ... break`` alone does not close
    an async iterator deterministically. For example::

        async with GrpcTransport() as transport:
            async with contextlib.aclosing(transport.stream(requests)) as responses:
                async for response in responses:
                    ...

    Closing the transport cancels every call and waits for its task cleanup, even
    when its consumer is paused. Producers must cooperate with asyncio cancellation.
    """

    def __init__(
        self,
        target: str = DEFAULT_TARGET,
        method: str = DEFAULT_METHOD,
        *,
        secure: bool = True,
        max_message_bytes: int = 8 * 1024 * 1024,
    ) -> None:
        if not target:
            raise ValueError("target must not be empty")
        if not method.startswith("/") or len(method.split("/")) != 3:
            raise ValueError("method must have the form /service/method")
        if any(not part for part in method.split("/")[1:]):
            raise ValueError("service and method must not be empty")
        if (
            isinstance(max_message_bytes, bool)
            or not isinstance(max_message_bytes, int)
            or not 0 < max_message_bytes <= 2**31 - 1
        ):
            raise ValueError("max_message_bytes must be a positive int32")
        self.target = target
        self.method = method
        self.secure = secure
        self.max_message_bytes = max_message_bytes
        self._channel: grpc.aio.Channel | None = None
        self._lifecycle_lock = asyncio.Lock()
        self._sessions: set[_Session] = set()

    async def __aenter__(self) -> GrpcTransport:
        await self.open()
        return self

    async def __aexit__(self, *exc_info: object) -> None:
        await self.close()

    async def open(self, timeout: float = 10) -> None:
        """Wait for readiness; failed/cancelled opens release their channel.

        Idempotent while open; a closed transport may be opened again. Readiness
        does not establish authentication or support for the configured method.
        """
        _validate_timeout(timeout)
        async with self._lifecycle_lock:
            if self._channel is not None:
                return
            options = (
                ("grpc.enable_retries", 0),
                ("grpc.max_send_message_length", self.max_message_bytes),
                ("grpc.max_receive_message_length", self.max_message_bytes),
            )
            if self.secure:
                channel = grpc.aio.secure_channel(
                    self.target, grpc.ssl_channel_credentials(), options=options
                )
            else:
                channel = grpc.aio.insecure_channel(self.target, options=options)
            try:
                await asyncio.wait_for(channel.channel_ready(), timeout)
            except BaseException:
                await channel.close()
                raise
            self._channel = channel

    async def _close_session(self, session: _Session) -> None:
        try:
            await session.close()
        finally:
            self._sessions.discard(session)

    async def close(self) -> None:
        """Cancel all RPCs and await their reader/writer/supervisor cleanup."""
        async with self._lifecycle_lock:
            channel, self._channel = self._channel, None
            if channel is None:
                return
            try:
                await asyncio.gather(
                    *(self._close_session(session) for session in tuple(self._sessions))
                )
            finally:
                await channel.close()

    async def stream(
        self,
        requests: AsyncIterable[bytes],
        *,
        metadata: Sequence[tuple[str, str | bytes]] = (),
        timeout: float = 30,
    ) -> AsyncIterator[bytes]:
        """Yield raw responses, propagating producer and terminal RPC exceptions.

        The deadline starts on first iteration and covers the whole RPC, including
        a blocked producer. Request exhaustion explicitly half-closes the send side;
        reads continue until the server terminates. Early server termination stops
        the producer. Transport EOF never implies an application-level final result.
        """
        _validate_timeout(timeout)
        channel = self._channel
        if channel is None:
            raise RuntimeError("transport is not open; await open() first")
        rpc = channel.stream_stream(self.method)
        session = _Session(rpc(metadata=tuple(metadata), timeout=timeout), requests)
        self._sessions.add(session)
        try:
            while True:
                session.reader = asyncio.create_task(session.call.read())
                try:
                    response = await session.reader
                except BaseException:
                    if session.error is not None:
                        raise session.error
                    raise
                finally:
                    session.reader = None
                if session.error is not None:
                    raise session.error
                if response is grpc.aio.EOF:
                    return
                yield response
        finally:
            await self._close_session(session)
