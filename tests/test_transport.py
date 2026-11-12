"""Integration tests against real grpc.aio generic byte-stream loopback servers."""

import asyncio
from contextlib import aclosing, asynccontextmanager
import socket

import grpc
import pytest

from rambler import DEFAULT_METHOD, DEFAULT_TARGET, GrpcTransport

@pytest.fixture
def serve():
    @asynccontextmanager
    async def start(handler, *, method=DEFAULT_METHOD):
        service, name = method.lstrip("/").split("/")
        server = grpc.aio.server()
        server.add_generic_rpc_handlers(
            (
                grpc.method_handlers_generic_handler(
                    service,
                    {
                        name: grpc.stream_stream_rpc_method_handler(
                            handler,
                            request_deserializer=lambda value: value,
                            response_serializer=lambda value: value,
                        )
                    },
                ),
            )
        )
        port = server.add_insecure_port("127.0.0.1:0")
        assert port
        await server.start()
        try:
            yield f"127.0.0.1:{port}"
        finally:
            await server.stop(None)
            await server.wait_for_termination()

    return start


async def messages(*values):
    for value in values:
        yield value


async def collect(responses):
    return [response async for response in responses]


async def event_set(event):
    await asyncio.wait_for(event.wait(), 3)


async def echo(requests, context):
    async for request in requests:
        yield request


def assert_clean(transport):
    assert not transport._sessions


def test_defaults_and_exports():
    transport = GrpcTransport()
    assert DEFAULT_TARGET == "speechs3proto2-pa.googleapis.com:443"
    assert DEFAULT_METHOD == "/speech.s3.S3StubbyClientConnectorService/S3Bidi"
    assert transport.target == DEFAULT_TARGET
    assert transport.method == DEFAULT_METHOD
    assert transport.secure is True
    assert transport.max_message_bytes == 8 * 1024 * 1024


@pytest.mark.parametrize("method", ["", "missing/slash", "/service/", "//method", "/a/b/c"])
def test_invalid_method(method):
    with pytest.raises(ValueError):
        GrpcTransport(method=method)


@pytest.mark.parametrize("limit", [0, -1, True, 1.5, 2**31])
def test_invalid_message_limit(limit):
    with pytest.raises(ValueError):
        GrpcTransport(max_message_bytes=limit)


def test_invalid_target():
    with pytest.raises(ValueError):
        GrpcTransport(target="")


@pytest.mark.parametrize("timeout", [0, -1, float("inf"), float("nan")])
async def test_invalid_timeouts(timeout):
    transport = GrpcTransport()
    with pytest.raises(ValueError):
        await transport.open(timeout=timeout)
    with pytest.raises(ValueError):
        await anext(transport.stream(messages(), timeout=timeout))


async def test_stream_requires_open():
    with pytest.raises(RuntimeError, match="not open"):
        await anext(GrpcTransport().stream(messages(b"hello")))


async def test_bidirectional_echo_half_close_and_late_final(serve):
    first_response = asyncio.Event()
    saw_eof = asyncio.Event()
    seen_metadata = []

    async def handler(requests, context):
        seen_metadata.extend(context.invocation_metadata())
        async for request in requests:
            yield b"echo:" + request
        saw_eof.set()
        await asyncio.sleep(0.02)
        yield b"late-final"

    async def producer():
        yield b"\x00first\xff"
        # This cannot progress with sequential upload-then-read implementations.
        await event_set(first_response)
        yield b"second"
        yield b""

    async with serve(handler) as target:
        async with GrpcTransport(target, secure=False) as transport:
            stream = transport.stream(
                producer(), metadata=(("x-test", "opaque"), ("opaque-bin", b"\x00\xff"))
            )
            async with aclosing(stream):
                assert await anext(stream) == b"echo:\x00first\xff"
                first_response.set()
                assert await collect(stream) == [b"echo:second", b"echo:", b"late-final"]
            assert saw_eof.is_set()
            assert ("x-test", "opaque") in seen_metadata
            assert ("opaque-bin", b"\x00\xff") in seen_metadata
            assert_clean(transport)


async def test_non_ok_status_after_partial_is_not_swallowed(serve):
    async def handler(requests, context):
        async for request in requests:
            yield b"partial:" + request
        await context.abort(grpc.StatusCode.DATA_LOSS, "terminal failure")

    async with serve(handler) as target:
        async with GrpcTransport(target, secure=False) as transport:
            async with aclosing(transport.stream(messages(b"one"))) as stream:
                assert await anext(stream) == b"partial:one"
                with pytest.raises(grpc.aio.AioRpcError) as caught:
                    await anext(stream)
                assert caught.value.code() is grpc.StatusCode.DATA_LOSS
                assert caught.value.details() == "terminal failure"
            assert_clean(transport)


async def test_producer_exception_preserves_original_and_cleans_up(serve):
    producer_finished = asyncio.Event()
    received = asyncio.Event()
    failure = RuntimeError("producer failed")

    async def producer():
        try:
            yield b"one"
            await event_set(received)
            raise failure
        finally:
            producer_finished.set()

    async with serve(echo) as target:
        async with GrpcTransport(target, secure=False) as transport:
            async with aclosing(transport.stream(producer())) as stream:
                assert await anext(stream) == b"one"
                received.set()
                with pytest.raises(RuntimeError) as caught:
                    await anext(stream)
                assert caught.value is failure
            assert producer_finished.is_set()
            assert_clean(transport)
            assert await collect(transport.stream(messages(b"reused"))) == [b"reused"]


async def test_producer_error_before_first_message(serve):
    async def producer():
        raise LookupError("before any write")
        yield b"unreachable"

    async with serve(echo) as target:
        async with GrpcTransport(target, secure=False) as transport:
            with pytest.raises(LookupError, match="before any write"):
                await collect(transport.stream(producer()))
            assert_clean(transport)


async def test_non_bytes_producer_is_rejected(serve):
    async with serve(echo) as target:
        async with GrpcTransport(target, secure=False) as transport:
            with pytest.raises(TypeError, match="yield bytes"):
                await collect(transport.stream(messages(bytearray(b"not raw bytes"))))
            assert_clean(transport)


async def test_deadline_covers_blocked_producer(serve):
    producer_started = asyncio.Event()
    producer_finished = asyncio.Event()

    async def producer():
        try:
            producer_started.set()
            await asyncio.Event().wait()
            yield b"never"
        finally:
            producer_finished.set()

    async with serve(echo) as target:
        async with GrpcTransport(target, secure=False) as transport:
            with pytest.raises(grpc.aio.AioRpcError) as caught:
                await asyncio.wait_for(collect(transport.stream(producer(), timeout=0.1)), 3)
            assert caught.value.code() is grpc.StatusCode.DEADLINE_EXCEEDED
            assert producer_started.is_set()
            assert producer_finished.is_set()
            assert_clean(transport)


async def test_deadline_cleans_writer_even_when_consumer_paused(serve):
    producer_finished = asyncio.Event()

    async def producer():
        try:
            yield b"partial"
            await asyncio.Event().wait()
        finally:
            producer_finished.set()

    async with serve(echo) as target:
        async with GrpcTransport(target, secure=False) as transport:
            async with aclosing(transport.stream(producer(), timeout=0.2)) as stream:
                assert await anext(stream) == b"partial"
                await event_set(producer_finished)
                with pytest.raises(grpc.aio.AioRpcError) as caught:
                    await anext(stream)
                assert caught.value.code() is grpc.StatusCode.DEADLINE_EXCEEDED
            assert_clean(transport)


@pytest.mark.parametrize("error", [False, True])
async def test_early_server_terminal_stops_blocked_producer(serve, error):
    producer_finished = asyncio.Event()

    async def producer():
        try:
            yield b"request"
            await asyncio.Event().wait()
        finally:
            producer_finished.set()

    async def handler(requests, context):
        async for request in requests:
            yield b"partial:" + request
            if error:
                await context.abort(grpc.StatusCode.PERMISSION_DENIED, "early status")
            return

    async with serve(handler) as target:
        async with GrpcTransport(target, secure=False) as transport:
            async with aclosing(transport.stream(producer())) as stream:
                assert await anext(stream) == b"partial:request"
                # Do not read EOF yet: the supervisor must still stop the writer.
                await event_set(producer_finished)
                if error:
                    with pytest.raises(grpc.aio.AioRpcError) as caught:
                        await anext(stream)
                    assert caught.value.code() is grpc.StatusCode.PERMISSION_DENIED
                else:
                    with pytest.raises(StopAsyncIteration):
                        await anext(stream)
            assert_clean(transport)


async def test_client_cancellation_cleans_all_tasks(serve):
    producer_started = asyncio.Event()
    producer_finished = asyncio.Event()

    async def producer():
        try:
            producer_started.set()
            await asyncio.Event().wait()
            yield b"never"
        finally:
            producer_finished.set()

    async with serve(echo) as target:
        async with GrpcTransport(target, secure=False) as transport:
            consumer = asyncio.create_task(collect(transport.stream(producer())))
            await event_set(producer_started)
            sessions = tuple(transport._sessions)
            consumer.cancel()
            with pytest.raises(asyncio.CancelledError):
                await consumer
            assert producer_finished.is_set()
            assert_clean(transport)
            assert all(session.writer.done() and session.monitor.done() for session in sessions)
            assert await collect(transport.stream(messages(b"still-open"))) == [b"still-open"]


async def test_aclosing_early_break_stops_producer(serve):
    producer_finished = asyncio.Event()

    async def producer():
        try:
            yield b"one"
            await asyncio.Event().wait()
        finally:
            producer_finished.set()

    async with serve(echo) as target:
        async with GrpcTransport(target, secure=False) as transport:
            async with aclosing(transport.stream(producer())) as stream:
                async for response in stream:
                    assert response == b"one"
                    break
            assert producer_finished.is_set()
            assert_clean(transport)


async def test_close_cancels_active_and_paused_sessions_and_allows_reopen(serve):
    finished = [asyncio.Event(), asyncio.Event()]
    started = [asyncio.Event(), asyncio.Event()]

    async def producer(index):
        try:
            started[index].set()
            if index == 0:
                yield b"paused"
            await asyncio.Event().wait()
        finally:
            finished[index].set()

    async with serve(echo) as target:
        transport = GrpcTransport(target, secure=False)
        await transport.open()
        paused = transport.stream(producer(0))
        active = asyncio.create_task(collect(transport.stream(producer(1))))
        try:
            assert await anext(paused) == b"paused"
            await event_set(started[1])
            sessions = tuple(transport._sessions)
            assert len(sessions) == 2
            await asyncio.wait_for(transport.close(), 3)
            assert all(event.is_set() for event in finished)
            assert all(session.writer.done() and session.monitor.done() for session in sessions)
            assert_clean(transport)
            with pytest.raises(asyncio.CancelledError):
                await active
            with pytest.raises(asyncio.CancelledError):
                await anext(paused)
            await transport.close()
            await transport.open()
            assert await collect(transport.stream(messages(b"reopened"))) == [b"reopened"]
        finally:
            await paused.aclose()
            active.cancel()
            await asyncio.gather(active, return_exceptions=True)
            await transport.close()


async def test_concurrent_and_successive_sessions_are_isolated(serve):
    simultaneous = 6
    all_started = asyncio.Event()
    count = 0

    async def handler(requests, context):
        nonlocal count
        async for request in requests:
            if request.startswith(b"first:"):
                count += 1
                if count == simultaneous:
                    all_started.set()
                await event_set(all_started)
            yield b"reply:" + request
        yield b"final"

    async with serve(handler) as target:
        async with GrpcTransport(target, secure=False) as transport:
            channel = transport._channel
            await transport.open()  # Idempotence must not replace the channel.
            assert transport._channel is channel
            results = await asyncio.wait_for(
                asyncio.gather(
                    *(
                        collect(transport.stream(messages(f"first:{index}".encode())))
                        for index in range(simultaneous)
                    )
                ),
                5,
            )
            assert results == [[f"reply:first:{i}".encode(), b"final"] for i in range(simultaneous)]
            for index in range(3):
                value = f"next:{index}".encode()
                assert await collect(transport.stream(messages(value))) == [b"reply:" + value, b"final"]
            assert transport._channel is channel
            assert_clean(transport)


async def test_cancel_one_session_does_not_cancel_another(serve):
    blocked = asyncio.Event()
    producer_finished = asyncio.Event()

    async def producer():
        try:
            blocked.set()
            await asyncio.Event().wait()
            yield b"never"
        finally:
            producer_finished.set()

    async with serve(echo) as target:
        async with GrpcTransport(target, secure=False) as transport:
            cancelled = asyncio.create_task(collect(transport.stream(producer())))
            await event_set(blocked)
            good = asyncio.create_task(collect(transport.stream(messages(b"independent"))))
            cancelled.cancel()
            with pytest.raises(asyncio.CancelledError):
                await cancelled
            assert await good == [b"independent"]
            assert producer_finished.is_set()
            assert_clean(transport)


async def test_custom_method(serve):
    method = "/loopback.Bytes/Exchange"
    async with serve(echo, method=method) as target:
        async with GrpcTransport(target, method, secure=False) as transport:
            assert await collect(transport.stream(messages(b"opaque"))) == [b"opaque"]


async def test_unknown_method_propagates_status(serve):
    async with serve(echo) as target:
        async with GrpcTransport(target, "/unknown.Service/Unknown", secure=False) as transport:
            with pytest.raises(grpc.aio.AioRpcError) as caught:
                await collect(transport.stream(messages(b"opaque")))
            assert caught.value.code() is grpc.StatusCode.UNIMPLEMENTED
            assert_clean(transport)


@pytest.mark.parametrize("direction", ["send", "receive"])
async def test_configured_message_size_is_enforced(serve, direction):
    async def handler(requests, context):
        async for request in requests:
            yield b"x" * 65 if direction == "receive" else request

    async with serve(handler) as target:
        async with GrpcTransport(target, secure=False, max_message_bytes=64) as transport:
            with pytest.raises(grpc.aio.AioRpcError) as caught:
                await collect(transport.stream(messages(b"x" * (65 if direction == "send" else 1))))
            assert caught.value.code() is grpc.StatusCode.RESOURCE_EXHAUSTED
            assert_clean(transport)


async def test_native_write_backpressure_does_not_exhaust_producer(serve):
    server_started = asyncio.Event()
    producer_finished = asyncio.Event()
    produced = 0
    chunk = b"x" * (256 * 1024)

    async def handler(requests, context):
        server_started.set()
        # Deliberately never consume the request stream / advance its flow window.
        await asyncio.Event().wait()
        yield b"never"

    async def producer():
        nonlocal produced
        try:
            for _ in range(256):
                produced += 1
                yield chunk
        finally:
            producer_finished.set()

    async with serve(handler) as target:
        async with GrpcTransport(target, secure=False) as transport:
            consumer = asyncio.create_task(collect(transport.stream(producer())))
            try:
                await event_set(server_started)
                await asyncio.sleep(0.1)
                assert 0 < produced < 256
                assert not producer_finished.is_set()
            finally:
                consumer.cancel()
                await asyncio.gather(consumer, return_exceptions=True)
            assert producer_finished.is_set()
            assert_clean(transport)


async def test_failed_open_releases_channel_and_can_retry(serve):
    # Keep a bound TCP port unavailable to gRPC; no external network dependency.
    with socket.socket() as reserved:
        reserved.bind(("127.0.0.1", 0))
        target = f"127.0.0.1:{reserved.getsockname()[1]}"
        transport = GrpcTransport(target, secure=False)
        with pytest.raises(TimeoutError):
            await transport.open(timeout=0.05)
        assert transport._channel is None
        assert_clean(transport)
        await transport.close()
    async with serve(echo) as working_target:
        transport.target = working_target
        try:
            await transport.open()
            assert await collect(transport.stream(messages(b"retry-open"))) == [b"retry-open"]
        finally:
            await transport.close()


async def test_cancelled_open_releases_channel():
    with socket.socket() as reserved:
        reserved.bind(("127.0.0.1", 0))
        transport = GrpcTransport(f"127.0.0.1:{reserved.getsockname()[1]}", secure=False)
        opening = asyncio.create_task(transport.open())
        await asyncio.sleep(0.02)
        opening.cancel()
        with pytest.raises(asyncio.CancelledError):
            await opening
        assert transport._channel is None
        await transport.close()


async def test_close_waits_for_async_producer_cleanup_and_cancels_server(serve):
    server_started = asyncio.Event()
    server_finished = asyncio.Event()
    producer_started = asyncio.Event()
    producer_finished = asyncio.Event()

    async def handler(requests, context):
        try:
            server_started.set()
            await asyncio.Event().wait()
            yield b"never"
        finally:
            server_finished.set()

    async def producer():
        try:
            producer_started.set()
            yield b"one"
            await asyncio.Event().wait()
        finally:
            # close() must await cleanup rather than merely send cancellation.
            await asyncio.sleep(0.02)
            producer_finished.set()

    async with serve(handler) as target:
        transport = GrpcTransport(target, secure=False)
        await transport.open()
        consumer = asyncio.create_task(collect(transport.stream(producer())))
        try:
            await event_set(server_started)
            await event_set(producer_started)
            sessions = tuple(transport._sessions)
            readers = tuple(session.reader for session in sessions)
            await asyncio.wait_for(transport.close(), 3)
            assert producer_finished.is_set()
            assert all(reader is not None and reader.done() for reader in readers)
            assert all(session.writer.done() and session.monitor.done() for session in sessions)
            await event_set(server_finished)
            with pytest.raises(asyncio.CancelledError):
                await consumer
            assert_clean(transport)
        finally:
            consumer.cancel()
            await asyncio.gather(consumer, return_exceptions=True)
            await transport.close()


async def test_unavailable_is_not_retried(serve):
    invocations = 0

    async def handler(requests, context):
        nonlocal invocations
        invocations += 1
        await context.abort(grpc.StatusCode.UNAVAILABLE, "no replay")
        yield b"never"

    async with serve(handler) as target:
        async with GrpcTransport(target, secure=False) as transport:
            with pytest.raises(grpc.aio.AioRpcError) as caught:
                await collect(transport.stream(messages(b"not replayable")))
            assert caught.value.code() is grpc.StatusCode.UNAVAILABLE
            assert invocations == 1
            assert_clean(transport)


async def test_early_ok_while_producer_is_still_writing(serve):
    producer_finished = asyncio.Event()

    async def producer():
        try:
            for _ in range(4096):
                yield b"audio" * 1024
        finally:
            producer_finished.set()

    async def handler(requests, context):
        async for request in requests:
            yield b"early-final"
            return

    async with serve(handler) as target:
        async with GrpcTransport(target, secure=False) as transport:
            responses = []
            try:
                async for response in transport.stream(producer()):
                    responses.append(response)
            except grpc.aio.AioRpcError as error:
                # grpcio 1.85.1 can replace the remote OK with a local INTERNAL
                # when a SendMessageOperation races a peer closing its receive
                # side. Never turn that actual RPC failure into a synthetic OK.
                assert error.code() is grpc.StatusCode.INTERNAL
                assert error.details() == "Internal error from Core"
            else:
                assert responses == [b"early-final"]
            assert responses in ([], [b"early-final"])
            assert producer_finished.is_set()
            assert_clean(transport)


async def test_secure_default_does_not_downgrade_to_plaintext(serve):
    async with serve(echo) as target:
        transport = GrpcTransport(target)
        with pytest.raises(TimeoutError):
            await transport.open(timeout=0.1)
        assert transport._channel is None
        await transport.close()
