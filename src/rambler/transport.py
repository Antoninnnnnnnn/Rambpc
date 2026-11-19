"""Opaque bytes over bidirectional gRPC, without application-level decoding.

An OK gRPC status is only transport completion, not a Rambler final result.
No requests are replayed and no metadata or payloads are logged.
"""

from __future__ import annotations

import asyncio
from collections.abc import AsyncIterable, AsyncIterator, Sequence
import ipaddress
import math
from typing import Any

import grpc

DEFAULT_TARGET = "speechs3proto2-pa.googleapis.com:443"
DEFAULT_METHOD = "/speech.s3.S3StubbyClientConnectorService/S3Bidi"


def _plaintext_loopback_target(target: str) -> str:
    """Return a direct gRPC address for a numeric loopback TCP authority only."""
    error = "plaintext gRPC requires a numeric loopback address and port"
    if not isinstance(target, str):
        raise ValueError(error)
    bracketed = target.startswith("[")
    if bracketed:
        host, separator, port = target[1:].partition("]:")
    else:
        host, separator, port = target.rpartition(":")
    if (
        not separator
        or "%" in host
        or not (0 < len(port) <= 5 and port.isascii() and port.isdecimal())
    ):
        raise ValueError(error)
    number = int(port)
    if not 0 < number <= 65535:
        raise ValueError(error)
    try:
        if bracketed:
            address = ipaddress.IPv6Address(host)
            if address != ipaddress.IPv6Address("::1"):
                raise ValueError
            return f"ipv6:[{address}]:{number}"
        address = ipaddress.IPv4Address(host)
        if not address.is_loopback:
            raise ValueError
        return f"ipv4:{address}:{number}"
    except ValueError:
        # Do not echo an untrusted target in an error that may enter logs.
        raise ValueError(error) from None


def _validate_timeout(timeout: float) -> None:
    if not math.isfinite(timeout) or timeout <= 0:
        raise ValueError("timeout must be finite and positive")


def _observe_owned_task(task: asyncio.Task[None]) -> None:
    if not task.cancelled():
        # A waiter may have been cancelled. Observe without logging; a later
        # join still raises the original task result.
        task.exception()


async def _join_owned_task(task: asyncio.Task[None]) -> None:
    # Waiting neither cancels the inner task nor logs its exception when the
    # waiter is cancelled (unlike asyncio.shield in Python 3.14).
    if not task.done():
        await asyncio.wait((task,))
    task.result()


class _Session:
    """Own RPC tasks independently of a consumer paused at an async-generator yield."""

    def __init__(self, call: Any, requests: AsyncIterable[bytes]) -> None:
        self.call = call
        self.error: BaseException | None = None
        self.reader: asyncio.Task[Any] | None = None
        self._stopping_writer = False
        self._cleanup: asyncio.Task[None] | None = None
        self._request_cleanup: asyncio.Task[None] | None = None
        try:
            self._request_iterator = aiter(requests)
        except BaseException:
            call.cancel()
            raise
        self.writer = asyncio.create_task(self._write(self._request_iterator))
        self.monitor = asyncio.create_task(self._watch_terminal_status())

    async def _write(self, iterator: AsyncIterator[bytes]) -> None:
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
        except BaseException as exc:
            if not (isinstance(exc, asyncio.CancelledError) and self._stopping_writer):
                # Record producer failures before cleanup can race terminal status.
                self.error = exc
                self.call.cancel()
            raise
        finally:
            await self._close_requests(iterator)

    async def _close_requests(self, iterator: AsyncIterator[bytes]) -> None:
        # Terminal status can cancel the writer after done_writing(), while its
        # natural aclose() is already suspended. Own cleanup in a separate task
        # so that this cancellation cannot interrupt resource release.
        if self._request_cleanup is None:
            async def cleanup():
                try:
                    close_iterator = getattr(iterator, "aclose", None)
                    if close_iterator is not None:
                        await close_iterator()
                except BaseException as exc:
                    self.error = exc
                    self.call.cancel()
                    raise
            self._request_cleanup = asyncio.create_task(cleanup())
            self._request_cleanup.add_done_callback(_observe_owned_task)
        await _join_owned_task(self._request_cleanup)

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
            # A task cancelled before its first step never runs its finally.
            # The supervisor still owns and closes that unstarted iterator.
            await self._close_requests(self._request_iterator)

    async def close(self) -> None:
        if self._cleanup is None:
            self.call.cancel()
            self._stop_writer()
            if self.reader is not None:
                self.reader.cancel()
            self._cleanup = asyncio.create_task(self._finish_cleanup())
            self._cleanup.add_done_callback(_observe_owned_task)
        await _join_owned_task(self._cleanup)

    async def _finish_cleanup(self) -> None:
        await asyncio.gather(
            self.writer,
            self.monitor,
            *(() if self.reader is None else (self.reader,)),
            return_exceptions=True,
        )
        # The writer may only have created this task while handling cancellation.
        # Await it after the writer has stopped, and do not swallow close errors.
        if self._request_cleanup is not None:
            await self._request_cleanup


class GrpcTransport:
    """Reusable channel with isolated concurrent calls, confined to one event loop.

    TLS uses gRPC's standard trust roots and hostname verification. ``secure=False``
    accepts only an explicit numeric loopback TCP authority, e.g. ``127.0.0.1:12345``
    or ``[::1]:12345``. Plaintext fixtures use a direct address, without DNS or a
    proxy; use TLS for every other destination.
    Each stream owns its request iterator and closes it once on exit when it
    exposes ``aclose``. This cleanup is independent of writer cancellation and
    awaited before EOF; cleanup failures propagate instead of successful EOF.
    Reads and writes run concurrently, with native gRPC write-side
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
        if not secure:
            _plaintext_loopback_target(target)
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
        self._closing: asyncio.Task[None] | None = None
        self._lifecycle_lock = asyncio.Lock()
        self._sessions: set[_Session] = set()

    async def __aenter__(self) -> GrpcTransport:
        await self.open()
        return self

    async def __aexit__(self, *exc_info: object) -> None:
        await self.close()

    async def open(self, timeout: float = 10) -> None:
        """Wait for readiness; failed/cancelled opens release their channel.

        Idempotent while open. Before reopening, join any pending close even if
        its previous waiter was cancelled. Readiness does not establish
        authentication or support for the configured method.
        """
        _validate_timeout(timeout)
        async with self._lifecycle_lock:
            await self._await_pending_close()
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
                # target and secure are public attributes; validate again before
                # opening or reopening, not just at construction.
                target = _plaintext_loopback_target(self.target)
                channel = grpc.aio.insecure_channel(
                    target, options=(*options, ("grpc.enable_http_proxy", 0))
                )
            try:
                await asyncio.wait_for(channel.channel_ready(), timeout)
            except BaseException:
                await channel.close()
                raise
            self._channel = channel

    def _discard_successful_close(self, session: _Session, cleanup: asyncio.Task[None]) -> None:
        if cleanup.cancelled() or cleanup.exception() is None:
            self._sessions.discard(session)

    async def _close_session(self, session: _Session) -> None:
        try:
            await session.close()
        except asyncio.CancelledError:
            # A cancelled stream waiter must not detach active cleanup. Reap a
            # later success; retain a failure for the transport to observe.
            if (cleanup := session._cleanup) is not None:
                cleanup.add_done_callback(
                    lambda closed: self._discard_successful_close(session, closed)
                )
            raise
        except BaseException:
            self._sessions.discard(session)
            raise
        else:
            self._sessions.discard(session)

    async def _await_pending_close(self) -> None:
        """Join under the lifecycle lock without cancelling owned cleanup."""
        closing = self._closing
        if closing is None:
            return
        try:
            await _join_owned_task(closing)
        except asyncio.CancelledError:
            if closing.cancelled():
                self._closing = None
            raise
        except BaseException:
            self._closing = None
            raise
        else:
            self._closing = None

    async def _finish_close(
        self, channel: grpc.aio.Channel, sessions: tuple[_Session, ...]
    ) -> None:
        try:
            results = await asyncio.gather(
                *(self._close_session(session) for session in sessions),
                return_exceptions=True,
            )
            for result in results:
                if isinstance(result, BaseException):
                    raise result
        finally:
            await channel.close()

    async def close(self) -> None:
        """Cancel all RPCs and await their cleanup even after an interrupted wait.

        A cancelled caller stops waiting, but the transport retains ownership of
        the close. A later close or open waits for that same cleanup.
        """
        async with self._lifecycle_lock:
            channel, self._channel = self._channel, None
            if channel is not None:
                self._closing = asyncio.create_task(
                    self._finish_close(channel, tuple(self._sessions))
                )
                self._closing.add_done_callback(_observe_owned_task)
            await self._await_pending_close()

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
                    # EOF can race the producer's asynchronous cleanup. Observe
                    # its completion/errors before reporting transport success.
                    await session.close()
                    if session.error is not None:
                        raise session.error
                    return
                yield response
        finally:
            await self._close_session(session)
