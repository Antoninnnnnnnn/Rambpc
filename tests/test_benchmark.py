import importlib.util
import io
import json
from pathlib import Path
from types import SimpleNamespace

import grpc
import pytest

SPEC = importlib.util.spec_from_file_location('benchmark_engine', Path(__file__).parents[1] / 'tools' / 'benchmark_engine.py')
bench = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bench)


def varint(value):
    data = bytearray()
    while value >= 128:
        data.append((value & 127) | 128)
        value >>= 7
    return bytes(data) + bytes([value])


def blob(number, value):
    return varint(number * 8 + 2) + varint(len(value)) + value


def test_schedule_limits_and_determinism():
    assert bench.schedule(1, [224, 8192]) == [('fresh_channel', 224), ('reused_channel', 224), ('reused_channel', 8192), ('fresh_channel', 8192)]
    assert len(bench.schedule(10, [224, 8192])) == 40
    with pytest.raises(ValueError):
        bench.schedule(10, [1, 2, 3])
    with pytest.raises(ValueError):
        bench.schedule(11, [1])
    assert bench.loopback('127.0.0.1:3000')
    assert bench.loopback('[::1]:3000')
    assert not bench.loopback('localhost.example.com:3000')
    assert not bench.loopback('example.com:443')


def test_stats():
    rows = [{'mode': 'fresh_channel', 'chunk_bytes': 224, 'warmup': False,
             'success': True, 'eof_ms': x} for x in (1, 4, 2, 3)]
    rows.append({'mode': 'fresh_channel', 'chunk_bytes': 224, 'warmup': True,
                 'success': True, 'eof_ms': 100})
    result = bench.summarize(rows)[0]
    assert result['count'] == 4
    assert result['metrics']['eof_ms'] == {'median': 2.5, 'p95_nearest_rank': 4}


def test_reject_live_and_overwrite(tmp_path, capsys):
    file = tmp_path / 'result.json'
    file.write_text('preserve')
    audio = tmp_path / 'audio.ogg'
    audio.write_bytes(b'OggSOpusHead')
    assert bench.main(['--audio', str(audio), '--output', str(file)]) == 1
    assert file.read_text() == 'preserve'
    assert '--live' not in capsys.readouterr().out
    assert bench.main(['--audio', str(audio), '--output', str(file), '--target', '127.0.0.1:1', '--insecure']) == 1
    assert file.read_text() == 'preserve'


async def test_real_engine_loopback(tmp_path, monkeypatch):
    for name in ('RAMBLER_API_KEY', 'RAMBLER_ANDROID_PACKAGE', 'RAMBLER_ANDROID_CERT'):
        monkeypatch.delenv(name, raising=False)
    text = blob(1, b'synthetic test')
    recognition = blob(3, blob(3, text))
    response = b'\x08\x01' + blob(1253625, blob(1, recognition))
    calls = []
    async def handler(requests, context):
        calls.append([x async for x in requests])
        yield response
    server = grpc.aio.server()
    server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler(
        'speech.s3.S3StubbyClientConnectorService', {'S3Bidi': grpc.stream_stream_rpc_method_handler(
            handler, request_deserializer=lambda x: x, response_serializer=lambda x: x)}),))
    port = server.add_insecure_port('127.0.0.1:0')
    await server.start()
    audio = tmp_path / 'test.ogg'
    audio.write_bytes(b'OggSOpusHead' + b'a' * 100)
    raw = tmp_path / 'raw'
    raw.mkdir()
    args = SimpleNamespace(target=f'127.0.0.1:{port}', insecure=True, audio=audio,
                           chunk_sizes=[40], samples=1, timeout=3, language='fr-FR',
                           include_results=False, raw_dir=raw)
    output = io.StringIO()
    try:
        assert await bench.run(args, output)
    finally:
        await server.stop(None)
    result = json.loads(output.getvalue())
    assert result['complete'] and len(result['samples']) == 3 and len(calls) == 3
    assert result['samples'][0]['warmup']
    assert all(x['success'] and x['audio_bytes_consumed'] == 112 for x in result['samples'])
    assert all(x['eof_ms'] >= x['s3_end_ms'] for x in result['samples'])
    assert 'synthetic test' not in output.getvalue()
    assert len(list(raw.iterdir())) == 3
    assert all(call[-1] == b'\x10\x01\x18\x01' for call in calls)


async def test_late_grpc_error_fails_fast_and_redacts(tmp_path, monkeypatch):
    for name in ('RAMBLER_API_KEY', 'RAMBLER_ANDROID_PACKAGE', 'RAMBLER_ANDROID_CERT'):
        monkeypatch.delenv(name, raising=False)
    calls = 0
    async def handler(requests, context):
        nonlocal calls
        calls += 1
        async for _ in requests:
            pass
        yield b'\x08\x02\x10\x07'
        await context.abort(grpc.StatusCode.PERMISSION_DENIED, 'secret details')
    server = grpc.aio.server()
    server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler(
        'speech.s3.S3StubbyClientConnectorService', {'S3Bidi': grpc.stream_stream_rpc_method_handler(
            handler, request_deserializer=lambda x: x, response_serializer=lambda x: x)}),))
    port = server.add_insecure_port('127.0.0.1:0')
    await server.start()
    audio = tmp_path / 'test.ogg'
    audio.write_bytes(b'OggSOpusHead')
    args = SimpleNamespace(target=f'127.0.0.1:{port}', insecure=True, audio=audio,
                           chunk_sizes=[40], samples=1, timeout=3, language='fr-FR',
                           include_results=False, raw_dir=None)
    output = io.StringIO()
    try:
        assert not await bench.run(args, output)
    finally:
        await server.stop(None)
    result = json.loads(output.getvalue())
    assert calls == 1 and not result['complete']
    assert result['samples'][0]['grpc_status'] == 'PERMISSION_DENIED'
    assert result['samples'][0]['s3_error_codes'] == [7]
    assert 'secret details' not in output.getvalue()


def test_decoded_edited_context_serializes_without_wire_bytes():
    from rambler.protocol import decode_response
    # Field 55 is unknown and demonstrates decoded context retaining raw wire.
    context = blob(1, b'before') + blob(2, b'selection') + blob(3, b'after') + blob(55, b'unknown')
    edit = blob(7, context)
    hypothesis = blob(74956224, edit)
    response = decode_response(blob(1253625, blob(1, blob(3, blob(3, hypothesis)))))
    decoded = response.hypotheses[0].voice_edit.edited_context
    assert decoded.raw and decoded.unknown_fields
    result = bench.texts(response)
    assert result[0]['edited_context'] == {'before': 'before', 'selected': 'selection', 'after': 'after', 'cursor_aware': True}
    assert json.loads(json.dumps(bench.safe_results(result, True)))[0]['edited_context']['after'] == 'after'
    assert len(bench.safe_results(result, False)[0]['sha256']) == 64


def test_reproducibility_metadata(monkeypatch):
    monkeypatch.setenv('RAMBLER_API_KEY', 'sensitive-test-secret')
    args = SimpleNamespace(language='fr-FR', timeout=30, chunk_sizes=[224, 8192], samples=5)
    data = bench.reproducibility(args)
    assert data['captured_utc'].endswith('+00:00')
    assert data['config']['language'] == 'fr-FR'
    assert data['pacing'] == 'accelerated'
    assert set(data['code_sha256']) == {'engine', 'protocol', 'transport', 'wire', 'harness'}
    assert all(len(digest) == 64 for digest in data['code_sha256'].values())
    assert 'sensitive-test-secret' not in json.dumps(data)
