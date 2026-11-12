import asyncio
from contextlib import asynccontextmanager
import json
import sys
import wave
from types import SimpleNamespace
import grpc
import pytest
from rambler import cli
from rambler.engine import IncompleteSessionError, RamblerEngine, S3SessionError
from rambler.protocol import EditingContext, context_request, end_request
from rambler.transport import DEFAULT_METHOD, GrpcTransport
from rambler.wire import bytes_field, string_field, varint_field


@asynccontextmanager
async def loopback(handler):
    service, method = DEFAULT_METHOD[1:].split('/')
    server = grpc.aio.server()
    server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler(service, {
        method: grpc.stream_stream_rpc_method_handler(handler, request_deserializer=lambda x: x, response_serializer=lambda x: x)
    }),))
    port = server.add_insecure_port('127.0.0.1:0')
    await server.start()
    transport = GrpcTransport(f'127.0.0.1:{port}', secure=False)
    try:
        yield transport
    finally:
        await transport.close()
        await server.stop(None)
        await server.wait_for_termination()


def args_for(path, command='dictate'):
    return SimpleNamespace(command=command, chunk_bytes=4, sample_rate=16000,
                           language='fr-FR', before=None, selected=None, after=None,
                           cursor_aware=False, file=str(path), timeout=2)


def candidate():
    hypothesis = string_field(1, 'candidate')
    recognition = bytes_field(3, bytes_field(3, hypothesis))
    return varint_field(1, 0) + bytes_field(1253625, bytes_field(1, recognition))


async def test_cli_real_engine_complete_sequence_and_empty_context(monkeypatch, capsys, tmp_path):
    path = tmp_path / 'input.ogg'
    path.write_bytes(b'OggSOpusHeadfake-opaque-stream')
    seen = []
    async def handler(requests, context):
        async for payload in requests:
            seen.append(payload)
        yield candidate()
        yield b'\x08\x01'
    async with loopback(handler) as transport:
        engines = []
        def factory(**kwargs):
            engine = RamblerEngine(transport, **kwargs)
            engines.append(engine)
            return engine
        monkeypatch.setattr(cli, 'RamblerEngine', factory)
        assert await cli._run(args_for(path)) == 0
        output = [json.loads(line) for line in capsys.readouterr().out.splitlines()]
        assert [line['type'] for line in output] == ['response', 'response', 'eof']
        assert output[0]['response']['hypotheses'][0]['text'] == 'candidate'
        assert seen[-2:] == [context_request(EditingContext()), end_request()]
        assert not engines[0]._sessions and not transport._sessions


@pytest.mark.parametrize('mode', ['incomplete', 's3_error', 'grpc_error'])
async def test_cli_real_engine_never_emits_success_on_failure(monkeypatch, capsys, tmp_path, mode):
    path = tmp_path / 'input.ogg'
    path.write_bytes(b'OggSOpusHeadfake')
    async def handler(requests, context):
        async for _ in requests:
            pass
        yield candidate()
        if mode == 's3_error':
            yield b'\x08\x02\x10\x07\x1a\x0dprivate-error'
        elif mode == 'grpc_error':
            yield b'\x08\x01'
            await context.abort(grpc.StatusCode.DATA_LOSS, 'private-error')
    async with loopback(handler) as transport:
        monkeypatch.setattr(cli, 'RamblerEngine', lambda **kwargs: RamblerEngine(transport, **kwargs))
        expected = {'incomplete': IncompleteSessionError, 's3_error': S3SessionError, 'grpc_error': grpc.aio.AioRpcError}[mode]
        with pytest.raises(expected):
            await cli._run(args_for(path))
        output = capsys.readouterr().out
        assert 'private-error' not in output and '"type": "eof"' not in output
        assert 'candidate' in output and not transport._sessions


@pytest.mark.parametrize('chunk', [0, -1, True, 1.5])
async def test_file_source_rejects_invalid_chunk_before_open(chunk):
    with pytest.raises(ValueError):
        await anext(cli._file_audio('does-not-need-to-exist', chunk))


async def test_invalid_context_preflight_before_engine_creation(monkeypatch):
    def forbidden(**kwargs):
        raise AssertionError('should not open network for invalid context')
    monkeypatch.setattr(cli, 'RamblerEngine', forbidden)
    args = args_for('unused')
    args.selected = 'not-discarded'
    with pytest.raises(ValueError, match='cursor_aware'):
        await cli._run(args)


def test_cli_cancellation_is_redacted_and_returns_130(monkeypatch, capsys):
    async def cancelled(args):
        raise asyncio.CancelledError('private cancellation detail')
    monkeypatch.setattr(cli, '_run', cancelled)
    assert cli.main(['probe']) == 130
    output = capsys.readouterr().out
    assert 'private' not in output and json.loads(output)['error_type'] == 'Cancelled'


def test_nonfinite_confidences_are_valid_json_null(capsys):
    cli._emit({'confidence': float('nan'), 'nested': [float('inf'), -float('inf')]})
    def reject_constant(value):
        raise ValueError(value)
    assert json.loads(capsys.readouterr().out, parse_constant=reject_constant) == {'confidence': None, 'nested': [None, None]}


async def test_ogg_validation_and_chunking(tmp_path):
    path = tmp_path / 'test.ogg'
    data = b'OggS' + b'OpusHead' + b'12345'
    path.write_bytes(data)
    assert b''.join([x async for x in cli._file_audio(str(path), 4)]) == data
    path.write_bytes(b'RIFF' + b'OpusHead')
    with pytest.raises(ValueError):
        _ = [x async for x in cli._file_audio(str(path), 4)]


def test_cli_redacts_exception(monkeypatch, capsys):
    async def fail(args):
        raise ValueError('sensitive secret text')
    monkeypatch.setattr(cli, '_run', fail)
    assert cli.main(['probe']) == 1
    result = capsys.readouterr().out
    assert 'sensitive' not in result
    assert json.loads(result)['type'] == 'error'


async def test_dictate_requires_s3_end_and_sends_empty_context(monkeypatch, capsys):
    from rambler.protocol import EditingContext
    from rambler.engine import EngineEvent, SessionMetrics
    metrics = SessionMetrics('opaque', 1, 1, 1, 1, 0.0, 0.0, None)
    result = SimpleNamespace(status=0, error_code=None, recognition_state=None,
                             speech_event=None, partials=(),
                             hypotheses=(SimpleNamespace(text='candidate', confidence=None, voice_edit=None),))
    contexts = []
    class Engine:
        def __init__(self, **kwargs): pass
        async def open(self, **kwargs): pass
        async def close(self): pass
        async def stream(self, source, *, config, context, timeout):
            contexts.append(context)
            yield EngineEvent(result, metrics)
            yield EngineEvent(None, metrics)
    monkeypatch.setattr(cli, 'RamblerEngine', Engine)
    args = SimpleNamespace(command='dictate', chunk_bytes=224, sample_rate=16000,
                           language='fr-FR', before=None, selected=None, after=None,
                           cursor_aware=False, file='unused', timeout=1)
    assert await cli._run(args) == 1
    assert contexts == [EditingContext()]
    assert 'IncompleteSession' in capsys.readouterr().out
    result.status = 1
    assert await cli._run(args) == 0


def test_response_contains_edited_context():
    from rambler.protocol import EditingContext
    edit = SimpleNamespace(transcript=None, alternative_text=None, edited_text=None,
                           edited_context=EditingContext(before='a', selected='b', after='c'), intent=1)
    response = SimpleNamespace(status=1, error_code=None, recognition_state=None, speech_event=None,
                               hypotheses=(SimpleNamespace(text=None, confidence=None, voice_edit=edit),), partials=())
    assert cli._response(response)['hypotheses'][0]['voice_edit']['edited_context'] == {
        'before': 'a', 'selected': 'b', 'after': 'c'}


def make_wav(path, rate=16000):
    with wave.open(str(path), 'wb') as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(rate)
        wav.writeframes(b'\x01\x02' * 330)
    return path


def test_wav_missing_av_fails_before_engine(monkeypatch, tmp_path, capsys):
    monkeypatch.setitem(sys.modules, 'av', None)
    monkeypatch.setattr(cli, 'RamblerEngine', lambda **kw: pytest.fail('network attempted'))
    assert cli.main(['dictate-wav', str(make_wav(tmp_path / 'a.wav'))]) == 1
    result = json.loads(capsys.readouterr().out)
    assert result['error_type'] == 'AudioExtraMissing'
    assert 'install' in result['message'] and '[audio]' in result['message']


@pytest.mark.parametrize('rate,page', [(8000, 20), (16000, 19), (16000, 1001)])
def test_wav_preflight_before_engine(monkeypatch, tmp_path, capsys, rate, page):
    monkeypatch.setattr(cli, 'RamblerEngine', lambda **kw: pytest.fail('network attempted'))
    path = make_wav(tmp_path / 'a.wav', rate)
    assert cli.main(['dictate-wav', str(path), '--page-duration-ms', str(page)]) == 1
    assert json.loads(capsys.readouterr().out)['error_type'] == 'ValueError'


def test_wav_mocked_encoding_context_and_exit(monkeypatch, tmp_path, capsys):
    from rambler.engine import EngineEvent, SessionMetrics
    import rambler.encoding as encoding
    import rambler.audio as audio_module
    actual_wav = audio_module.WavAudio
    captured_options = {}
    def wav_factory(path, **kwargs):
        captured_options.update(kwargs)
        # Inspect realtime selection without delaying the unit test.
        return actual_wav(path, chunk_ms=kwargs['chunk_ms'], realtime=False)
    monkeypatch.setattr(audio_module, 'WavAudio', wav_factory)
    monkeypatch.setitem(sys.modules, 'av', SimpleNamespace())
    captured = {}
    async def encoder(source, **kwargs):
        captured['codec'] = kwargs
        captured['pcm'] = [chunk async for chunk in source]
        yield b'encoded'
    monkeypatch.setattr(encoding, 'pcm16_to_ogg', encoder)
    metrics = SessionMetrics('opaque', 1, 1, 1, 1, 0.0, 0.0, None)
    response = SimpleNamespace(status=1, error_code=None, recognition_state=None,
                               speech_event=None, partials=(), hypotheses=(
                                   SimpleNamespace(text='ok', confidence=None, voice_edit=None),))
    class Engine:
        def __init__(self, **kwargs): pass
        async def open(self, **kwargs): pass
        async def close(self): pass
        async def stream(self, source, *, config, context, timeout):
            captured['context'] = context
            captured['encoded'] = [chunk async for chunk in source]
            yield EngineEvent(response, metrics)
            yield EngineEvent(None, metrics)
    monkeypatch.setattr(cli, 'RamblerEngine', Engine)
    path = make_wav(tmp_path / 'a.wav')
    assert cli.main(['dictate-wav', str(path), '--before', 'left', '--selected', 'word',
                     '--after', 'right', '--cursor-aware', '--realtime']) == 0
    assert captured_options == dict(chunk_ms=20, realtime=True)
    assert captured['codec'] == dict(sample_rate=16000, channels=1, bitrate=24000, page_duration_ms=20)
    assert list(map(len, captured['pcm'])) == [640, 20]
    assert captured['encoded'] == [b'encoded']
    assert captured['context'] == EditingContext(before='left', selected='word', after='right', cursor_aware=True)
    assert json.loads(capsys.readouterr().out.splitlines()[0])['type'] == 'response'


def test_parser_defaults_and_no_av_for_core(monkeypatch, capsys):
    monkeypatch.setitem(sys.modules, 'av', None)
    seen = []
    async def capture(args):
        seen.append(args)
        return 0
    monkeypatch.setattr(cli, '_run', capture)
    assert cli.main(['dictate', 'a.ogg']) == 0
    assert cli.main(['probe']) == 0
    assert seen[0].chunk_bytes == 8192 and seen[1].chunk_bytes == 224
    with pytest.raises(SystemExit) as exc:
        cli.main(['--help'])
    assert exc.value.code == 0
