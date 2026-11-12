import asyncio
from contextlib import aclosing
import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace

import pytest

SPEC = importlib.util.spec_from_file_location('benchmark_realtime', Path(__file__).parents[1] / 'tools' / 'benchmark_realtime.py')
bench = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bench)


class Reader:
    def __init__(self, chunks):
        self.chunks = iter(chunks)
    async def read(self, size):
        return next(self.chunks, b'')


class Process:
    def __init__(self, chunks):
        self.stdout = Reader(chunks)
        self.returncode = None
        self.terminated = False
    async def wait(self):
        if self.returncode is None:
            self.returncode = 0
        return self.returncode
    def terminate(self):
        self.terminated = True
        self.returncode = -15
    def kill(self):
        self.returncode = -9


def arguments(tmp_path):
    return SimpleNamespace(wav=tmp_path / 'fixture.wav', encoded_dir=tmp_path,
                           language='en-US', timeout=1, include_results=False)


def test_schedule_bounds():
    assert bench.schedule(3, [1000000, 20000]) == [1000000, 20000, 20000, 1000000, 1000000, 20000]
    for pairs in (0, 4):
        with pytest.raises(ValueError):
            bench.schedule(pairs, [1000000, 20000])
    with pytest.raises(ValueError):
        bench.schedule(1, [20000, 20000])
    with pytest.raises(Exception):
        bench.page_durations('0,20000')


def test_reject_non_loopback_and_no_overwrite(tmp_path, capsys):
    wav = tmp_path / 'a.wav'
    wav.write_bytes(b'fixture')
    output = tmp_path / 'out.json'
    output.write_text('unchanged')
    args = ['--wav', str(wav), '--output', str(output), '--encoded-dir', str(tmp_path / 'encoded')]
    assert bench.main(args) == 1
    assert output.read_text() == 'unchanged'
    assert not (tmp_path / 'encoded').exists()
    assert 'secret' not in capsys.readouterr().out


async def test_sample_tracks_source_and_s3_times(tmp_path, monkeypatch):
    from rambler.engine import EngineEvent, SessionMetrics
    from rambler.protocol import decode_response
    # An S3 END with nested text hypothesis.
    def varint(value):
        out = bytearray()
        while value >= 128:
            out.append((value & 127) | 128)
            value >>= 7
        return bytes(out) + bytes([value])
    def blob(number, payload):
        return varint(number * 8 + 2) + varint(len(payload)) + payload
    response = decode_response(b'\x08\x01' + blob(1253625, blob(1, blob(3, blob(3, blob(1, b'synthetic'))))))
    processes = []
    async def spawn(*command, **kwargs):
        assert '-re' in command and '-page_duration' in command and '20000' in command
        process = Process([b'first', b'second'])
        processes.append(process)
        return process
    async def decode(*args):
        return {'sha256': 'decoded-hash', 'bytes': 10, 'format': 'PCM16LE mono 16000Hz'}
    class Engine:
        async def stream(self, source, **kwargs):
            async with aclosing(source):
                assert b''.join([chunk async for chunk in source]) == b'firstsecond'
            metrics = SessionMetrics('id', 4, 5, 1, 1, .1, .1, .1)
            yield EngineEvent(response, metrics)
            yield EngineEvent(None, metrics)
    monkeypatch.setattr(bench.asyncio, 'create_subprocess_exec', spawn)
    monkeypatch.setattr(bench, 'decode_pcm', decode)
    row = await bench.take_sample(arguments(tmp_path), Engine(), 'ffmpeg', 0, 20000)
    assert row['success'] and row['encoded_bytes_yielded'] == 11
    assert row['source_exhausted_ms'] <= row['s3_end_ms'] <= row['eof_ms']
    assert row['s3_end_text_after_source_eof_ms'] >= 0
    assert row['grpc_status'] == 'OK' and row['s3_statuses'] == [1]
    assert 'synthetic' not in json.dumps(row)
    assert (tmp_path / '00-page-20000.ogg').read_bytes() == b'firstsecond'
    assert processes[0].returncode == 0


async def test_cancellation_terminates_encoder_and_closes_file(tmp_path, monkeypatch):
    entered = asyncio.Event()
    process = Process([b'first'])
    async def spawn(*args, **kwargs):
        return process
    class Engine:
        async def stream(self, source, **kwargs):
            async with aclosing(source):
                await anext(source)
                entered.set()
                await asyncio.Event().wait()
                yield None
    monkeypatch.setattr(bench.asyncio, 'create_subprocess_exec', spawn)
    task = asyncio.create_task(bench.take_sample(arguments(tmp_path), Engine(), 'ffmpeg', 0, 20000))
    await asyncio.wait_for(entered.wait(), 1)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task
    assert process.terminated
    file = tmp_path / '00-page-20000.ogg'
    file.unlink()  # Windows denies deletion when the output handle remains open.
    assert not file.exists()


async def test_decode_failure_terminates_process(monkeypatch):
    process = Process([b''])
    async def spawn(*args, **kwargs):
        return process
    monkeypatch.setattr(bench.asyncio, 'create_subprocess_exec', spawn)
    with pytest.raises(RuntimeError):
        await bench.decode_pcm('ffmpeg', Path('fixture.ogg'))
    assert process.returncode is not None


@pytest.mark.skipif(__import__('shutil').which('ffmpeg') is None, reason='optional FFmpeg test source unavailable')
async def test_optional_ffmpeg_equal_decoded_pcm(tmp_path):
    import shutil
    import wave
    from rambler.engine import EngineEvent, SessionMetrics
    from rambler.protocol import decode_response
    # The transport is mocked: this exercises only the FFmpeg test source/decoder.
    with wave.open(str(tmp_path / 'fixture.wav'), 'wb') as wave_file:
        wave_file.setnchannels(1)
        wave_file.setsampwidth(2)
        wave_file.setframerate(16000)
        wave_file.writeframes(b'\x00\x00' * 3200)
    def varint(value):
        out = bytearray()
        while value >= 128:
            out.append((value & 127) | 128)
            value >>= 7
        return bytes(out) + bytes([value])
    def blob(number, payload):
        return varint(number * 8 + 2) + varint(len(payload)) + payload
    response = decode_response(b'\x08\x01' + blob(1253625, blob(1, blob(3, blob(3, blob(1, b'test'))))))
    class Engine:
        async def stream(self, source, **kwargs):
            async with aclosing(source):
                async for _ in source:
                    pass
            metrics = SessionMetrics('id', 4, 5, 1, 1, .1, .1, .1)
            yield EngineEvent(response, metrics)
            yield EngineEvent(None, metrics)
    args = arguments(tmp_path)
    args.timeout = 10
    rows = [await bench.take_sample(args, Engine(), shutil.which('ffmpeg'), i, page)
            for i, page in enumerate((1000000, 20000))]
    assert all(row['success'] for row in rows)
    assert rows[0]['decoded_pcm'] == rows[1]['decoded_pcm']


async def test_native_source_pages_and_duration_without_ffmpeg(tmp_path, monkeypatch):
    pytest.importorskip('av')
    import hashlib
    import wave
    from rambler.engine import EngineEvent, SessionMetrics
    from rambler.protocol import decode_response
    data = b'\x02\x01' * 3201
    with wave.open(str(tmp_path / 'fixture.wav'), 'wb') as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(16000)
        wav.writeframes(data)
    def varint(value):
        out = bytearray()
        while value >= 128:
            out.append((value & 127) | 128)
            value >>= 7
        return bytes(out) + bytes([value])
    def blob(number, payload):
        return varint(number * 8 + 2) + varint(len(payload)) + payload
    response = decode_response(b'\x08\x01' + blob(1253625, blob(1, blob(3, blob(3, blob(1, b'test'))))))
    class Engine:
        async def stream(self, source, **kwargs):
            async with aclosing(source):
                async for _ in source:
                    pass
            metrics = SessionMetrics('id', 4, 5, 1, 1, .1, .1, .1)
            yield EngineEvent(response, metrics)
            yield EngineEvent(None, metrics)
    async def forbidden(*args, **kwargs):
        raise AssertionError('native path must not execute FFmpeg')
    monkeypatch.setattr(bench.asyncio, 'create_subprocess_exec', forbidden)
    metadata, elapsed, av, audio, encoding = bench.prepare_native()
    assert metadata['pyav_version'] == av.__version__ and metadata['library_versions']
    assert elapsed >= 0
    reference = await bench.native_pcm_reference(audio, tmp_path / 'fixture.wav')
    assert reference['sha256'] == hashlib.sha256(data).hexdigest()
    args = arguments(tmp_path)
    args.timeout = 5
    args.source = 'native'
    rows = [await bench.take_sample(args, Engine(), None, i, page, native=(av, audio, encoding), pcm_reference=reference)
            for i, page in enumerate((1000000, 20000))]
    assert all(row['success'] for row in rows), rows
    assert all(row['pcm_sha256_consumed'] == reference['sha256'] for row in rows)
    assert all(row['pcm_bytes_consumed'] == len(data) and row['decoded_pcm']['bytes'] == len(data) for row in rows)
    assert rows[0]['decoded_pcm'] == rows[1]['decoded_pcm']
    assert all(row['source_exhausted_ms'] is not None for row in rows)


def test_native_options_and_page_validation(tmp_path, monkeypatch):
    parser = bench.parser()
    args = parser.parse_args(['--source', 'native', '--wav', 'a.wav', '--output', 'o.json', '--encoded-dir', 'encoded'])
    assert args.source == 'native' and args.pairs == 3
    calls = []
    monkeypatch.setattr(bench.shutil, 'which', lambda *x: calls.append(x))
    result = bench.main(['--source', 'native', '--target', '127.0.0.1:1', '--insecure',
                         '--page-durations', '20001,1000000', '--wav', str(tmp_path / 'a.wav'),
                         '--output', str(tmp_path / 'out.json'), '--encoded-dir', str(tmp_path / 'encoded')])
    assert result == 1 and not calls and not (tmp_path / 'out.json').exists()


async def test_native_run_metadata_does_not_probe_ffmpeg(tmp_path, monkeypatch):
    pytest.importorskip('av')
    import io
    import wave
    with wave.open(str(tmp_path / 'fixture.wav'), 'wb') as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(16000)
        wav.writeframes(b'\0\0' * 320)
    args = arguments(tmp_path)
    args.source, args.pairs, args.page_durations = 'native', 1, [1000000, 20000]
    args.target, args.insecure = '127.0.0.1:1', True
    class Transport:
        def __init__(self, *args, **kwargs): pass
        async def open(self, **kwargs): pass
        async def close(self): pass
    class Engine:
        def __init__(self, *args, **kwargs): pass
    async def sample(args, engine, ffmpeg, index, page, *, native, pcm_reference):
        assert ffmpeg is None and native is not None
        assert pcm_reference['bytes'] == 640
        return {'success': True, 'decoded_pcm': {'sha256': 'same', 'bytes': 640}}
    def forbidden(*args, **kwargs):
        raise AssertionError('native mode must not query FFmpeg')
    monkeypatch.setattr(bench, 'GrpcTransport', Transport)
    monkeypatch.setattr(bench, 'RamblerEngine', Engine)
    monkeypatch.setattr(bench, 'take_sample', sample)
    monkeypatch.setattr(bench, 'ffmpeg_metadata', forbidden)
    output = io.StringIO()
    assert await bench.run(args, output, None)
    result = json.loads(output.getvalue())
    metadata = result['reproducibility']
    assert result['complete'] and metadata['source'] == 'native'
    assert 'ffmpeg_test_source' not in metadata
    assert metadata['native_test_source']['native_prepare_ms'] >= 0
    assert len(metadata['code_sha256']['encoding']) == 64
    assert len(metadata['code_sha256']['audio']) == 64
