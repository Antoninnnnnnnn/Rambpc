import asyncio
from contextlib import asynccontextmanager

import grpc
import pytest

from rambler.engine import IncompleteSessionError, RamblerEngine, S3SessionError
from rambler.protocol import EditingContext, ProtocolConfig, context_request
from rambler.results import (AmbiguousResultError, ContextConflictError, DictationResult, ResultLimitError,
                             UnsupportedEditError, collect_dictation, replacement_proposal)
from rambler.transport import DEFAULT_METHOD, GrpcTransport
from rambler.wire import bytes_field, string_field, varint_field


def final(edit_field=3, *, intent=2, count=1):
    edit = varint_field(4, intent)
    if edit_field == 3:
        edit += string_field(3, 'edited')
    elif edit_field == 7:
        edit += bytes_field(7, string_field(1, 'A😀') + string_field(2, '😃x') + string_field(3, '!'))
    elif edit_field == 2:
        edit += string_field(2, 'opaque')
    hypothesis = string_field(1, 'hypothesis') + bytes_field(74956224, edit)
    result = b''.join(bytes_field(3, hypothesis) for _ in range(count))
    recognition = varint_field(1, 1) + bytes_field(3, result)
    return bytes_field(1253625, bytes_field(1, recognition)) + varint_field(99, 12)


@asynccontextmanager
async def server(handler):
    service, method = DEFAULT_METHOD[1:].split('/')
    instance = grpc.aio.server()
    instance.add_generic_rpc_handlers((grpc.method_handlers_generic_handler(service, {
        method: grpc.stream_stream_rpc_method_handler(handler, request_deserializer=lambda x: x,
                                                      response_serializer=lambda x: x)
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


class Source:
    def __init__(self, block=False):
        self.block = block
        self.closed = 0
        self.started = asyncio.Event()

    def __aiter__(self):
        return self

    async def __anext__(self):
        self.started.set()
        if self.block:
            await asyncio.Event().wait()
        raise StopAsyncIteration

    async def aclose(self):
        self.closed += 1


async def success(transport, *, payload=None, context=None):
    return await collect_dictation(RamblerEngine(transport), Source(), config=ProtocolConfig(), context=context)


def test_manual_result_metrics_defaults_to_none():
    assert DictationResult((), None).metrics is None


async def test_context_is_captured_at_audio_end_and_proposal_checks_snapshot():
    received = []
    async def handler(requests, call):
        async for request in requests:
            received.append(request)
        yield final(7)
        yield b'\x08\x01'
    current = EditingContext('old', 'ten', '.', True)
    calls = 0
    async def provider():
        nonlocal calls
        calls += 1
        return current
    async with server(handler) as transport:
        current = EditingContext('new ', 'ten', '.', True)
        result = await success(transport, context=provider)
        assert received[-2] == context_request(current) and calls == 1
        assert result.context == current and result.final_updates[0].raw == final(7)
        assert result.final_updates[0].unknown_fields[0].number == 99
        assert result.text == 'A😀😃x!'
        assert result.metrics is not None and result.metrics.eof_seconds is not None
        assert result.metrics.responses_received == 2
        proposal = replacement_proposal(result, expected_revision=4)
        assert proposal.expected_revision == 4
        assert proposal.selection_utf16_range == (3, 6)
        proposal.validate_current(current, revision=4)
        with pytest.raises(ContextConflictError):
            proposal.validate_current(EditingContext('new ', 'eleven', '.', True), revision=4)
        with pytest.raises(ContextConflictError):
            proposal.validate_current(current, revision=5)
        with pytest.raises(ValueError):
            replacement_proposal(result, expected_revision=[])
        with pytest.raises(ValueError):
            proposal.validate_current(current, revision=[])


@pytest.mark.parametrize('payload', [final(2), final(3, intent=4)])
async def test_unknown_edit_not_interpreted(payload):
    async def handler(requests, call):
        async for _ in requests: pass
        yield payload
        yield b'\x08\x01'
    async with server(handler) as transport:
        result = await success(transport, context=EditingContext('old'))
        with pytest.raises(AmbiguousResultError): _ = result.text
        with pytest.raises(UnsupportedEditError): replacement_proposal(result)


@pytest.mark.parametrize('payloads', [[final(count=2)], [final(), final()]])
async def test_ambiguity_never_selects_a_candidate(payloads):
    async def handler(requests, call):
        async for _ in requests: pass
        for payload in payloads: yield payload
        yield b'\x08\x01'
    async with server(handler) as transport:
        result = await success(transport)
        with pytest.raises(AmbiguousResultError): _ = result.single_hypothesis
        with pytest.raises(AmbiguousResultError): _ = result.text


async def test_field3_has_no_invented_cursor():
    async def handler(requests, call):
        async for _ in requests: pass
        yield final()
        yield b'\x08\x01'
    async with server(handler) as transport:
        result = await success(transport, context=EditingContext('old'))
        proposal = replacement_proposal(result)
        assert result.text == proposal.replacement_text == 'edited'
        assert proposal.replacement_context is proposal.selection_utf16_range is None


@pytest.mark.parametrize('mode', ['grpc', 's3', 'incomplete'])
async def test_no_result_on_late_failure(mode):
    async def handler(requests, call):
        async for _ in requests: pass
        yield final()
        if mode == 's3':
            yield b'\x08\x02'
        elif mode == 'grpc':
            yield b'\x08\x01'
            await call.abort(grpc.StatusCode.DATA_LOSS, 'late')
    async with server(handler) as transport:
        engine, source = RamblerEngine(transport), Source()
        error = {'grpc': grpc.aio.AioRpcError, 's3': S3SessionError,
                 'incomplete': IncompleteSessionError}[mode]
        with pytest.raises(error):
            await collect_dictation(engine, source, config=ProtocolConfig())
        assert source.closed == 1 and not engine._sessions


@pytest.mark.parametrize('limits', [{'max_final_updates': 1}, {'max_result_bytes': 1}])
async def test_limits_close_session_without_closing_engine(limits):
    async def handler(requests, call):
        async for _ in requests: pass
        yield final()
        yield final()
        yield b'\x08\x01'
    async with server(handler) as transport:
        engine, source = RamblerEngine(transport), Source()
        with pytest.raises(ResultLimitError):
            await collect_dictation(engine, source, config=ProtocolConfig(), **limits)
        assert source.closed == 1 and not engine._sessions
        assert (await collect_dictation(engine, Source(), config=ProtocolConfig())).final_updates


async def test_cancellation_closes_source_and_session():
    async def handler(requests, call):
        async for _ in requests: pass
    async with server(handler) as transport:
        engine, source = RamblerEngine(transport), Source(block=True)
        task = asyncio.create_task(collect_dictation(engine, source, config=ProtocolConfig()))
        await asyncio.wait_for(source.started.wait(), 3)
        task.cancel()
        with pytest.raises(asyncio.CancelledError): await task
        assert source.closed == 1 and not engine._sessions


async def test_result_waits_for_complete_stream_and_context_is_late():
    final_sent, release, audio_stopped = asyncio.Event(), asyncio.Event(), asyncio.Event()
    requests_seen = []
    current = EditingContext('first')
    async def source():
        yield b'audio'
        await audio_stopped.wait()
    async def provider():
        return current
    async def handler(requests, call):
        async for request in requests:
            requests_seen.append(request)
        yield final()
        final_sent.set()
        await release.wait()
        yield b'\x08\x01'
    async with server(handler) as transport:
        task = asyncio.create_task(collect_dictation(RamblerEngine(transport), source(),
                                   config=ProtocolConfig(), context=provider))
        current = EditingContext('updated')
        audio_stopped.set()
        await asyncio.wait_for(final_sent.wait(), 3)
        await asyncio.sleep(0)
        assert not task.done()
        release.set()
        result = await task
        assert result.context == current and requests_seen[-2] == context_request(current)
        assert replacement_proposal(result).snapshot == current
