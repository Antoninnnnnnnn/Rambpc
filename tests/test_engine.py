import asyncio
from contextlib import aclosing, asynccontextmanager

import grpc
import pytest

from rambler.engine import ContextProvider, IncompleteSessionError, RamblerEngine, S3SessionError
from rambler.protocol import ProtocolConfig, EditingContext, initial_request, audio_request, context_request, end_request
from rambler.transport import GrpcTransport, DEFAULT_METHOD


@asynccontextmanager
async def server(handler):
    service, method = DEFAULT_METHOD[1:].split('/')
    instance = grpc.aio.server()
    instance.add_generic_rpc_handlers((grpc.method_handlers_generic_handler(service, {
        method: grpc.stream_stream_rpc_method_handler(handler, request_deserializer=lambda x: x, response_serializer=lambda x: x)
    }),))
    port = instance.add_insecure_port('127.0.0.1:0')
    await instance.start()
    transport = GrpcTransport(f'127.0.0.1:{port}', secure=False)
    await transport.open()
    try:
        yield transport
    finally:
        await transport.close()
        await instance.stop(None)
        await instance.wait_for_termination()


async def wait(event):
    await asyncio.wait_for(event.wait(), 3)


async def collect(stream):
    return [event async for event in stream]


class Source:
    """Custom iterator exposes cleanup even before its first read."""
    def __init__(self, chunks=(), *, block=False):
        self.chunks = iter(chunks)
        self.block = block
        self.started = asyncio.Event()
        self.close_count = 0

    def __aiter__(self):
        return self

    async def __anext__(self):
        self.started.set()
        try:
            return next(self.chunks)
        except StopIteration:
            if self.block:
                await asyncio.Event().wait()
            raise StopAsyncIteration

    async def aclose(self):
        await asyncio.sleep(0)
        self.close_count += 1


@pytest.mark.parametrize('responses', [(), (b'',), (b'\x08\x00',), (b'\x08\x03',), (b'\x08\x63',)])
async def test_ok_grpc_without_s3_end_is_explicitly_incomplete(responses):
    async def handler(requests, context):
        async for _ in requests:
            pass
        for payload in responses:
            yield payload
    async with server(handler) as transport:
        source, events = Source([b'audio']), []
        with pytest.raises(IncompleteSessionError):
            async for event in RamblerEngine(transport).stream(source, config=ProtocolConfig()):
                events.append(event)
        assert len(events) == len(responses) and all(e.response is not None for e in events)
        assert source.close_count == 1 and not transport._sessions


async def test_s3_application_error_preserved_redacted_and_drained():
    drained = asyncio.Event()
    async def handler(requests, context):
        async for _ in requests:
            pass
        yield b'\x08\x00'
        yield b'\x08\x02\x10\x07\x1a\x0dprivate-error'
        await asyncio.sleep(.01)
        drained.set()
    async with server(handler) as transport:
        events = []
        with pytest.raises(S3SessionError) as caught:
            async for event in RamblerEngine(transport).stream(Source(), config=ProtocolConfig()):
                events.append(event)
        assert drained.is_set() and [e.response.status for e in events] == [0, 2]
        assert caught.value.response is events[-1].response
        assert caught.value.response.error_message == 'private-error'
        assert caught.value.response.error_code == 7
        assert 'private-error' not in str(caught.value) + repr(caught.value)
        assert not transport._sessions


@pytest.mark.parametrize('last_s3', [b'\x08\x01', b'\x08\x02\x10\x09'])
async def test_late_grpc_error_wins_over_s3_end_or_error(last_s3):
    async def handler(requests, context):
        async for _ in requests:
            pass
        yield b'\x08\x00'
        yield last_s3
        await context.abort(grpc.StatusCode.DATA_LOSS, 'private-grpc-detail')
    async with server(handler) as transport:
        events = []
        with pytest.raises(grpc.aio.AioRpcError) as caught:
            async for event in RamblerEngine(transport).stream(Source(), config=ProtocolConfig()):
                events.append(event)
        assert caught.value.code() is grpc.StatusCode.DATA_LOSS
        assert [e.response.status for e in events] == [0, last_s3[1]]
        assert not transport._sessions


async def test_early_s3_end_does_not_succeed_with_truncated_upload():
    async def handler(requests, context):
        await anext(requests)
        yield b'\x08\x01'
    async with server(handler) as transport:
        source, events = Source([b'audio'], block=True), []
        with pytest.raises(IncompleteSessionError):
            async for event in RamblerEngine(transport).stream(source, config=ProtocolConfig()):
                events.append(event)
        assert [e.response.status for e in events] == [1]
        assert source.close_count == 1 and not transport._sessions


async def test_duplex_late_context_half_close_delayed_end_and_metrics():
    from rambler.wire import parse_fields
    response_seen, upload_finished, saw_eof = (asyncio.Event() for _ in range(3))
    captured = []
    async def audio():
        yield b'first'
        await wait(response_seen)
        yield b''
        yield b'last'
        upload_finished.set()
    async def handler(requests, context):
        async for payload in requests:
            captured.append(payload)
            if any(f.number == 293101 for f in parse_fields(payload)):
                yield b'\x08\x00'
        assert upload_finished.is_set()
        saw_eof.set()
        await asyncio.sleep(.01)
        yield b'\x08\x01'
    async with server(handler) as transport:
        engine = RamblerEngine(transport)
        context = EditingContext('before', 'selected', 'after', True)
        async with aclosing(engine.stream(audio(), config=ProtocolConfig(), context=context)) as events:
            assert (await anext(events)).response.status == 0
            assert not upload_finished.is_set() and not saw_eof.is_set()
            response_seen.set()
            rest = await collect(events)
        assert [e.response.status for e in rest if e.response] == [0, 1]
        assert rest[-1].response is None and saw_eof.is_set()
        assert captured[-2:] == [context_request(context), end_request()]
        chunks = [parse_fields(f.value)[0].value for p in captured for f in parse_fields(p) if f.number == 293101]
        assert chunks == [b'first', b'last']
        metrics = rest[-1].metrics
        assert metrics.requests_produced == len(captured) == 5
        assert metrics.request_protobuf_bytes == sum(map(len, captured))
        assert metrics.responses_received == 3 and metrics.response_protobuf_bytes == 6
        assert metrics.first_response_seconds <= metrics.eof_seconds <= metrics.elapsed_seconds
        assert not engine._sessions and not transport._sessions


async def test_malformed_response_closes_audio_and_rpc():
    cancelled = asyncio.Event()
    async def handler(requests, context):
        try:
            await anext(requests)
            await anext(requests)
            yield b'\x08'
            await asyncio.Event().wait()
        finally:
            cancelled.set()
    async with server(handler) as transport:
        source = Source([b'audio'], block=True)
        with pytest.raises(ValueError):
            await collect(RamblerEngine(transport).stream(source, config=ProtocolConfig()))
        assert source.close_count == 1 and not transport._sessions
        await wait(cancelled)


async def test_original_producer_failure_after_response():
    first_seen = asyncio.Event()
    failure = RuntimeError('producer-owned-error')
    async def audio():
        yield b'audio'
        await wait(first_seen)
        raise failure
    async def handler(requests, context):
        await anext(requests)
        await anext(requests)
        yield b'\x08\x00'
        await asyncio.Event().wait()
    async with server(handler) as transport:
        async with aclosing(RamblerEngine(transport).stream(audio(), config=ProtocolConfig())) as events:
            assert (await anext(events)).response.status == 0
            first_seen.set()
            with pytest.raises(RuntimeError) as caught:
                await anext(events)
            assert caught.value is failure
        assert not transport._sessions


async def test_async_credentials_concurrent_successive_sessions_and_metrics():
    class Provider:
        calls = 0
        async def metadata(self):
            self.calls += 1
            call = self.calls
            await asyncio.sleep(0)
            return (('x-session', str(call)), ('opaque-bin', b'\x00\xff'))
    seen = []
    async def handler(requests, context):
        seen.append(dict(context.invocation_metadata()))
        async for _ in requests:
            pass
        yield b'\x08\x01'
    async with server(handler) as transport:
        provider = Provider()
        engine = RamblerEngine(transport, provider)
        results = await asyncio.gather(*(collect(engine.stream(Source(), config=ProtocolConfig())) for _ in range(3)))
        await collect(engine.stream(Source(), config=ProtocolConfig()))
        assert provider.calls == 4 and {m['x-session'] for m in seen} == {'1', '2', '3', '4'}
        assert all(m['opaque-bin'] == b'\x00\xff' for m in seen)
        assert len({r[-1].metrics.session_id for r in results}) == 3
        assert all(r[-1].metrics.requests_produced == 2 for r in results)
        assert not engine._sessions and not transport._sessions


@pytest.mark.parametrize('action', ['timeout', 'caller_cancel', 'engine_close'])
async def test_blocked_credentials_cleanup_unstarted_audio_without_rpc(action):
    started, closed = asyncio.Event(), asyncio.Event()
    invocations = []
    class Provider:
        async def metadata(self):
            try:
                started.set()
                await asyncio.Event().wait()
            finally:
                closed.set()
    async def handler(requests, context):
        invocations.append(True)
        yield b'\x08\x01'
    async with server(handler) as transport:
        source = Source()
        engine = RamblerEngine(transport, Provider())
        consumer = asyncio.create_task(collect(engine.stream(source, config=ProtocolConfig(), timeout=.05 if action == 'timeout' else 5)))
        await wait(started)
        if action == 'caller_cancel':
            consumer.cancel()
        elif action == 'engine_close':
            await asyncio.wait_for(engine.close(), 3)
        with pytest.raises(TimeoutError if action == 'timeout' else asyncio.CancelledError):
            await consumer
        assert closed.is_set() and not source.started.is_set() and source.close_count == 1
        assert not invocations and not engine._sessions and not transport._sessions
        assert transport._channel is not None


async def test_credentials_time_subtracted_from_rpc_deadline():
    deadlines = []
    class Provider:
        async def metadata(self):
            await asyncio.sleep(.08)
            return ()
    async def handler(requests, context):
        deadlines.append(context.time_remaining())
        async for _ in requests:
            pass
        yield b'\x08\x01'
    async with server(handler) as transport:
        source = Source(block=True)
        with pytest.raises(grpc.aio.AioRpcError) as caught:
            await collect(RamblerEngine(transport, Provider()).stream(source, config=ProtocolConfig(), timeout=.16))
        assert caught.value.code() is grpc.StatusCode.DEADLINE_EXCEEDED
        assert len(deadlines) == 1 and 0 < deadlines[0] < .12
        assert source.close_count == 1 and not transport._sessions


async def test_engine_close_cleans_active_paused_sessions_not_another_engine():
    from rambler.wire import parse_fields
    async def handler(requests, context):
        async for payload in requests:
            if any(f.number == 293101 for f in parse_fields(payload)):
                yield b'\x08\x00'
        yield b'\x08\x01'
    async with server(handler) as transport:
        engine, other = RamblerEngine(transport), RamblerEngine(transport)
        paused_source, active_source, other_source = Source([b'paused'], block=True), Source(block=True), Source([b'other'], block=True)
        paused = engine.stream(paused_source, config=ProtocolConfig())
        other_stream = other.stream(other_source, config=ProtocolConfig())
        active = asyncio.create_task(collect(engine.stream(active_source, config=ProtocolConfig())))
        try:
            assert (await anext(paused)).response.status == 0
            assert (await anext(other_stream)).response.status == 0
            await wait(active_source.started)
            await asyncio.wait_for(engine.close(), 3)
            assert paused_source.close_count == active_source.close_count == 1
            assert other_source.close_count == 0
            assert transport._channel is not None and len(transport._sessions) == 1
            assert not engine._sessions
            with pytest.raises(asyncio.CancelledError):
                await active
            with pytest.raises(asyncio.CancelledError):
                await anext(paused)
            await collect(engine.stream(Source(), config=ProtocolConfig()))
        finally:
            await paused.aclose()
            await other_stream.aclose()
            active.cancel()
            await asyncio.gather(active, return_exceptions=True)
            await engine.close()
            await other.close()
        assert other_source.close_count == 1 and not transport._sessions


async def test_caller_cancel_during_audio_and_input_preflight():
    invocations = []
    async def handler(requests, context):
        invocations.append(True)
        async for _ in requests:
            pass
        yield b'\x08\x01'
    class Provider:
        calls = 0
        async def metadata(self):
            self.calls += 1
            return (('authorization', 'private-secret\r\nheader'),)
    async with server(handler) as transport:
        source, engine = Source(block=True), RamblerEngine(transport)
        consumer = asyncio.create_task(collect(engine.stream(source, config=ProtocolConfig())))
        await wait(source.started)
        consumer.cancel()
        with pytest.raises(asyncio.CancelledError):
            await consumer
        assert source.close_count == 1 and not engine._sessions and not transport._sessions
        invocations.clear()
        provider = Provider()
        engine = RamblerEngine(transport, provider)
        with pytest.raises(ValueError):
            await collect(engine.stream(Source(), config=ProtocolConfig(), context=EditingContext(selected='not-discarded')))
        assert provider.calls == 0 and not invocations
        source = Source()
        with pytest.raises(ValueError) as caught:
            await collect(engine.stream(source, config=ProtocolConfig()))
        assert provider.calls == 1 and not invocations
        assert 'private-secret' not in str(caught.value)
        assert not source.started.is_set() and source.close_count == 1


async def test_failure_does_not_poison_concurrent_or_successive_session():
    from rambler.wire import parse_fields
    both_started = asyncio.Event()
    started = 0
    async def handler(requests, context):
        nonlocal started
        failed = False
        async for payload in requests:
            for item in parse_fields(payload):
                if item.number == 293101:
                    chunk = parse_fields(item.value)[0].value
                    failed = chunk == b'fail'
                    started += 1
                    if started >= 2:
                        both_started.set()
        await wait(both_started)
        yield b'\x08\x02\x10\x07' if failed else b'\x08\x01'
    async with server(handler) as transport:
        engine = RamblerEngine(transport)
        bad_source, good_source = Source([b'fail']), Source([b'good'])
        bad, good = await asyncio.gather(
            collect(engine.stream(bad_source, config=ProtocolConfig())),
            collect(engine.stream(good_source, config=ProtocolConfig())),
            return_exceptions=True,
        )
        assert isinstance(bad, S3SessionError) and good[-1].response is None
        reused = await collect(engine.stream(Source([b'reused']), config=ProtocolConfig()))
        assert reused[-1].response is None
        assert reused[-1].metrics.session_id != good[-1].metrics.session_id
        assert bad_source.close_count == good_source.close_count == 1
        assert not engine._sessions and not transport._sessions


async def test_default_owned_transport_is_closed_with_active_session():
    async def handler(requests, context):
        async for _ in requests:
            pass
        yield b'\x08\x01'
    async with server(handler) as fixture_transport:
        engine = RamblerEngine()
        # Keep the default ownership path, but configure a real loopback endpoint.
        engine.transport.target = fixture_transport.target
        engine.transport.secure = False
        source = Source(block=True)
        async with engine:
            channel = engine.transport._channel
            consumer = asyncio.create_task(collect(engine.stream(source, config=ProtocolConfig())))
            await wait(source.started)
        with pytest.raises(asyncio.CancelledError):
            await consumer
        assert channel is not None and engine.transport._channel is None
        assert source.close_count == 1 and not engine._sessions
        await engine.close()


async def test_context_provider_captures_update_after_last_audio_before_end():
    from rambler.wire import parse_fields
    response_seen, exhausted, source_closed = (asyncio.Event() for _ in range(3))
    editor = {'before': 'stale text'}
    calls, seen = [], []
    async def audio():
        try:
            yield b'first'
            await wait(response_seen)
            yield b'last'
            # Happens only when the writer advances past the LAST audio chunk.
            editor['before'] = 'current text at listening stop'
            exhausted.set()
        finally:
            source_closed.set()
    async def current_context():
        assert exhausted.is_set() and source_closed.is_set()
        calls.append(editor['before'])
        return EditingContext(editor['before'], 'selection', 'after', True)
    provider: ContextProvider = current_context
    async def handler(requests, context):
        async for payload in requests:
            seen.append(payload)
            if any(item.number == 293101 for item in parse_fields(payload)):
                yield b'\x08\x00'
        yield b'\x08\x01'
    async with server(handler) as transport:
        engine = RamblerEngine(transport)
        async with aclosing(engine.stream(audio(), config=ProtocolConfig(), context=provider)) as events:
            assert (await anext(events)).response.status == 0
            assert not calls and not exhausted.is_set()
            response_seen.set()
            rest = await collect(events)
        assert rest[-1].response is None
        assert calls == ['current text at listening stop']
        assert seen[-2:] == [context_request(EditingContext(calls[0], 'selection', 'after', True)), end_request()]
        chunks = [parse_fields(item.value)[0].value for payload in seen for item in parse_fields(payload) if item.number == 293101]
        assert chunks == [b'first', b'last']
        assert not engine._sessions and not transport._sessions


async def test_context_provider_explicit_none_is_a_valid_omission():
    from rambler.wire import parse_fields
    source, seen, calls = Source(), [], []
    async def provider():
        assert source.close_count == 1
        calls.append(True)
        return None
    async def handler(requests, context):
        async for payload in requests:
            seen.append(payload)
        yield b'\x08\x01'
    async with server(handler) as transport:
        events = await collect(RamblerEngine(transport).stream(source, config=ProtocolConfig(), context=provider))
        assert events[-1].response is None and calls == [True]
        assert len(seen) == 2 and seen[-1] == end_request()
        assert not any(item.number == 1203 for payload in seen for item in parse_fields(payload))
        assert source.close_count == 1 and not transport._sessions


@pytest.mark.parametrize('global_deadline', [False, True])
async def test_blocked_context_provider_deadline_cancels_without_context_or_end(global_deadline):
    from rambler.wire import parse_fields
    provider_started, provider_closed, server_closed = (asyncio.Event() for _ in range(3))
    seen = []
    async def provider():
        try:
            provider_started.set()
            await asyncio.Event().wait()
        finally:
            provider_closed.set()
    async def handler(requests, context):
        try:
            async for payload in requests:
                seen.append(payload)
            yield b'\x08\x01'
        finally:
            server_closed.set()
    async with server(handler) as transport:
        engine, source = RamblerEngine(transport), Source([b'audio'])
        with pytest.raises((TimeoutError, grpc.aio.AioRpcError)) as caught:
            await collect(engine.stream(source, config=ProtocolConfig(), context=provider,
                                        timeout=.08 if global_deadline else 3,
                                        context_timeout=5 if global_deadline else .03))
        if isinstance(caught.value, grpc.aio.AioRpcError):
            assert global_deadline and caught.value.code() is grpc.StatusCode.DEADLINE_EXCEEDED
        assert provider_started.is_set() and provider_closed.is_set()
        await wait(server_closed)
        assert source.close_count == 1 and not engine._sessions and not transport._sessions
        assert seen and end_request() not in seen
        assert not any(item.number == 1203 for payload in seen for item in parse_fields(payload))


@pytest.mark.parametrize('action', ['caller_cancel', 'engine_close', 'stream_close'])
async def test_context_provider_cancelled_with_rpc_and_source(action):
    from rambler.wire import parse_fields
    started, closed, server_closed = (asyncio.Event() for _ in range(3))
    seen = []
    async def provider():
        try:
            started.set()
            await asyncio.Event().wait()
        finally:
            closed.set()
    async def handler(requests, context):
        try:
            async for payload in requests:
                seen.append(payload)
                if any(item.number == 293101 for item in parse_fields(payload)):
                    yield b'\x08\x00'
            yield b'\x08\x01'
        finally:
            server_closed.set()
    async with server(handler) as transport:
        engine, source = RamblerEngine(transport), Source([b'audio'])
        events = engine.stream(source, config=ProtocolConfig(), context=provider)
        if action == 'stream_close':
            assert (await anext(events)).response.status == 0
            await wait(started)
            await events.aclose()
        else:
            consumer = asyncio.create_task(collect(events))
            await wait(started)
            if action == 'caller_cancel':
                consumer.cancel()
            else:
                await engine.close()
            with pytest.raises(asyncio.CancelledError):
                await consumer
        assert closed.is_set()
        await wait(server_closed)
        assert source.close_count == 1 and not engine._sessions and not transport._sessions
        assert end_request() not in seen
        assert not any(item.number == 1203 for payload in seen for item in parse_fields(payload))


@pytest.mark.parametrize('result', ['not-context', 42, EditingContext(selected='requires-cursor-aware')])
async def test_context_provider_invalid_result_aborts_before_context_and_end(result):
    from rambler.wire import parse_fields
    seen, calls = [], []
    async def provider():
        calls.append(True)
        return result
    async def handler(requests, context):
        async for payload in requests:
            seen.append(payload)
        yield b'\x08\x01'
    async with server(handler) as transport:
        source, engine = Source([b'audio']), RamblerEngine(transport)
        with pytest.raises(ValueError):
            await collect(engine.stream(source, config=ProtocolConfig(), context=provider))
        assert calls == [True] and source.close_count == 1
        assert end_request() not in seen
        assert not any(item.number == 1203 for payload in seen for item in parse_fields(payload))
        assert not engine._sessions and not transport._sessions


async def test_context_provider_original_exception_is_propagated_not_skipped():
    failure = LookupError('context provider failure')
    called = []
    async def provider():
        called.append(True)
        raise failure
    seen = []
    async def handler(requests, context):
        async for payload in requests:
            seen.append(payload)
        yield b'\x08\x01'
    async with server(handler) as transport:
        source = Source([b'audio'])
        with pytest.raises(LookupError) as caught:
            await collect(RamblerEngine(transport).stream(source, config=ProtocolConfig(), context=provider))
        assert caught.value is failure and called == [True]
        assert end_request() not in seen and source.close_count == 1
        assert not transport._sessions


@pytest.mark.parametrize('failure_mode', ['audio_error', 'audio_cancel', 'server_early_end', 'credentials_error'])
async def test_context_provider_never_called_when_failure_precedes_audio_eof(failure_mode):
    called = []
    async def provider():
        called.append(True)
        return EditingContext('must not be requested')
    async def broken_audio():
        yield b'audio'
        raise RuntimeError('audio failed')
    class BadCredentials:
        async def metadata(self):
            raise RuntimeError('credentials failed')
    async def handler(requests, context):
        if failure_mode == 'server_early_end':
            await anext(requests)
            yield b'\x08\x01'
            return
        async for _ in requests:
            pass
        yield b'\x08\x01'
    async with server(handler) as transport:
        engine = RamblerEngine(transport, BadCredentials() if failure_mode == 'credentials_error' else None)
        source = Source([b'audio'], block=True)
        events = engine.stream(broken_audio() if failure_mode == 'audio_error' else source,
                               config=ProtocolConfig(), context=provider)
        if failure_mode == 'audio_cancel':
            consumer = asyncio.create_task(collect(events))
            await wait(source.started)
            consumer.cancel()
            with pytest.raises(asyncio.CancelledError):
                await consumer
        else:
            with pytest.raises(IncompleteSessionError if failure_mode == 'server_early_end' else RuntimeError):
                await collect(events)
        assert called == [] and not engine._sessions and not transport._sessions
        if failure_mode != 'audio_error':
            assert source.close_count == 1


@pytest.mark.parametrize('timeout', [0, -1, float('inf'), float('nan')])
async def test_invalid_context_timeout_is_prevalidated(timeout):
    calls = []
    async def provider():
        calls.append(True)
        return EditingContext()
    # No open transport: validation must fail locally, before RPC setup.
    engine = RamblerEngine(GrpcTransport('127.0.0.1:1', secure=False))
    with pytest.raises(ValueError, match='context_timeout'):
        await collect(engine.stream(Source(), config=ProtocolConfig(), context=provider, context_timeout=timeout))
    assert calls == [] and not engine._sessions


async def test_synchronous_context_callback_is_rejected_without_end_marker():
    seen = []
    async def handler(requests, context):
        async for payload in requests:
            seen.append(payload)
        yield b'\x08\x01'
    async with server(handler) as transport:
        with pytest.raises(TypeError):
            await collect(RamblerEngine(transport).stream(Source(), config=ProtocolConfig(),
                                                         context=lambda: EditingContext('not async')))
        assert end_request() not in seen and not transport._sessions


async def test_deferred_context_providers_are_isolated_concurrently_and_on_reuse():
    from rambler.wire import parse_fields
    captured = []
    async def handler(requests, context):
        call = []
        async for payload in requests:
            call.append(payload)
        captured.append(call)
        yield b'\x08\x01'
    async with server(handler) as transport:
        engine = RamblerEngine(transport)
        counts = [0, 0, 0]
        def make_session(index):
            source = Source([str(index).encode()])
            async def provider():
                assert source.close_count == 1
                counts[index] += 1
                await asyncio.sleep(0)
                return EditingContext(before=f'context-{index}')
            return engine.stream(source, config=ProtocolConfig(), context=provider)
        results = await asyncio.gather(collect(make_session(0)), collect(make_session(1)))
        results.append(await collect(make_session(2)))
        assert counts == [1, 1, 1] and all(events[-1].response is None for events in results)
        for call in captured:
            audio = [parse_fields(item.value)[0].value for payload in call for item in parse_fields(payload) if item.number == 293101]
            assert len(audio) == 1
            index = int(audio[0])
            assert call[-2:] == [context_request(EditingContext(before=f'context-{index}')), end_request()]
        assert len(captured) == 3 and not engine._sessions and not transport._sessions


async def test_sequence_and_metrics():
    seen = []
    async def handler(requests, context):
        async for item in requests:
            seen.append(item)
        yield b'\x08\x01'
    async def audio():
        yield b'a'
        yield b'b'
    config = ProtocolConfig()
    context = EditingContext(before='private')
    async with server(handler) as transport:
        engine = RamblerEngine(transport)
        events = [e async for e in engine.stream(audio(), config=config, context=context)]
        await engine.close()
        assert transport._channel is not None
    # Timing fields vary with scheduling; compare stable protobuf prefixes.
    assert seen[0].startswith(initial_request(config, elapsed_ms=0)[:8])
    assert seen[1].startswith(audio_request(b'a', elapsed_ms=0)[:4])
    assert seen[-2:] == [context_request(context), end_request()]
    assert len(seen) == 5
    assert events[0].response.status == 1
    assert events[-1].response is None
    assert events[-1].metrics.requests_produced == 5
    assert events[-1].metrics.responses_received == 1
    assert events[-1].metrics.eof_seconds is not None
    assert events[0].metrics.session_id == events[-1].metrics.session_id


async def test_late_grpc_error_and_source_failure():
    async def handler(requests, context):
        async for _ in requests:
            pass
        yield b'\x08\x00'
        await context.abort(grpc.StatusCode.PERMISSION_DENIED, 'not exposed')
    async def audio():
        yield b'x'
    async with server(handler) as transport:
        with pytest.raises(grpc.aio.AioRpcError):
            async for _ in RamblerEngine(transport).stream(audio(), config=ProtocolConfig()):
                pass
    async def broken():
        yield b'x'
        raise RuntimeError('source broken')
    async def consume(requests, context):
        async for _ in requests:
            pass
        yield b''
    async with server(consume) as transport:
        with pytest.raises(RuntimeError, match='source broken'):
            async for _ in RamblerEngine(transport).stream(broken(), config=ProtocolConfig()):
                pass


async def test_early_close_and_deadline_close_source():
    closed = asyncio.Event()
    async def audio():
        try:
            yield b'a'
            await asyncio.Event().wait()
        finally:
            closed.set()
    async def handler(requests, context):
        await anext(requests)
        await anext(requests)
        yield b'\x08\x00'
        await asyncio.Event().wait()
    async with server(handler) as transport:
        async with aclosing(RamblerEngine(transport).stream(audio(), config=ProtocolConfig())) as events:
            await anext(events)
        await asyncio.wait_for(closed.wait(), 2)
    closed.clear()
    async with server(handler) as transport:
        with pytest.raises(grpc.aio.AioRpcError):
            async for _ in RamblerEngine(transport).stream(audio(), config=ProtocolConfig(), timeout=.1):
                pass
        await asyncio.wait_for(closed.wait(), 2)
