"""Reproducible real-time paging experiment with FFmpeg or native test sources.

Pairs alternate 1000ms/20ms then 20ms/1000ms on one open channel, with no
concurrent sessions or retries. A producer timestamp is not a completed network
write; final S3 status and gRPC EOF do not imply a final Rambler action.
"""
from __future__ import annotations

import argparse
import asyncio
from contextlib import aclosing
from dataclasses import asdict
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import shutil
import subprocess
import sys
import time

import grpc
from rambler.credentials import EnvironmentCredentials
from rambler.engine import RamblerEngine
from rambler.protocol import EditingContext, ProtocolConfig
from rambler.transport import GrpcTransport, DEFAULT_TARGET

# Load the adjacent reusable helpers even when invoked as a standalone script.
_spec = importlib.util.spec_from_file_location('benchmark_engine_helpers', Path(__file__).with_name('benchmark_engine.py'))
_helpers = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_helpers)


def page_durations(value: str) -> list[int]:
    try:
        pages = [int(x) for x in value.split(',')]
    except ValueError as exc:
        raise argparse.ArgumentTypeError('page durations must be comma-separated integers') from exc
    if len(pages) != 2 or len(set(pages)) != 2 or any(p < 20000 or p > 1000000 for p in pages):
        raise argparse.ArgumentTypeError('provide two distinct page durations between 20000 and 1000000 microseconds')
    return pages


def schedule(pairs: int, pages: list[int]) -> list[int]:
    if not 1 <= pairs <= 3 or len(pages) != 2 or len(set(pages)) != 2 or any(p < 20000 or p > 1000000 for p in pages):
        raise ValueError('invalid pairs or page durations')
    return [p for i in range(pairs) for p in (pages if i % 2 == 0 else pages[::-1])]


def hash_file(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    with path.open('rb') as stream:
        while block := stream.read(65536):
            digest.update(block)
            size += len(block)
    return digest.hexdigest(), size


async def stop_process(process):
    async def drain():
        if process.stdout is not None:
            while await process.stdout.read(65536):
                pass

    # Drain concurrently: a paused stdout pipe must not deadlock wait() after
    # terminating a child whose output was abandoned on early stream closure.
    drainer = asyncio.create_task(drain())
    try:
        if process.returncode is None:
            try:
                process.terminate()
            except ProcessLookupError:
                pass
            try:
                await asyncio.wait_for(process.wait(), 2)
            except asyncio.TimeoutError:
                try:
                    process.kill()
                except ProcessLookupError:
                    pass
                await asyncio.wait_for(process.wait(), 2)
        else:
            await asyncio.wait_for(process.wait(), 2)
    finally:
        if not drainer.done():
            drainer.cancel()
        await asyncio.gather(drainer, return_exceptions=True)


async def decode_pcm(ffmpeg: str, path: Path) -> dict:
    process = await asyncio.create_subprocess_exec(ffmpeg, '-nostdin', '-hide_banner', '-loglevel', 'error',
        '-i', str(path), '-ac', '1', '-ar', '16000', '-f', 's16le', 'pipe:1',
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL)
    digest = hashlib.sha256()
    count = 0
    try:
        while block := await process.stdout.read(65536):
            digest.update(block)
            count += len(block)
        if await process.wait() != 0 or count == 0:
            raise RuntimeError('test-source decode failed')
        return {'sha256': digest.hexdigest(), 'bytes': count, 'format': 'PCM16LE mono 16000Hz'}
    finally:
        await stop_process(process)


def prepare_native() -> tuple[dict, float, object, object, object]:
    """Measure import/prepare in this process, not a system-cold start."""
    started = time.monotonic()
    if sys.version_info < (3, 12):
        raise RuntimeError('native source requires Python >= 3.12 and the audio extra')
    av = importlib.import_module('av')
    if int(av.__version__.split('.', 1)[0]) < 19:
        raise RuntimeError('native source requires PyAV >= 19')
    audio = importlib.import_module('rambler.audio')
    encoding = importlib.import_module('rambler.encoding')
    elapsed = (time.monotonic() - started) * 1000
    metadata = {'pyav_version': av.__version__,
                'library_versions': {name: list(version) for name, version in av.library_versions.items()},
                'native_prepare_ms': elapsed,
                'prepare_note': 'Import and module preparation in this process, not a cold-system measurement.'}
    return metadata, elapsed, av, audio, encoding


def native_decode_pcm(av, path: Path) -> dict:
    """Decode and resample to exactly packed PCM16LE mono; flush the resampler."""
    digest = hashlib.sha256()
    count = 0
    resampler = av.AudioResampler(format='s16', layout='mono', rate=16000)
    def consume(frames):
        nonlocal count
        for frame in frames:
            payload = bytes(frame.planes[0])[:frame.samples * 2]
            if sys.byteorder == 'big':
                swapped = bytearray(payload)
                swapped[0::2], swapped[1::2] = payload[1::2], payload[0::2]
                payload = bytes(swapped)
            digest.update(payload)
            count += len(payload)
    with av.open(str(path), mode='r') as container:
        for frame in container.decode(audio=0):
            consume(resampler.resample(frame))
        consume(resampler.resample(None))
    if not count:
        raise ValueError('test-source decode is empty')
    return {'sha256': digest.hexdigest(), 'bytes': count, 'format': 'PCM16LE mono 16000Hz'}


async def native_pcm_reference(audio, path: Path) -> dict:
    wav = audio.WavAudio(path, chunk_ms=20, realtime=False)
    if wav.format.sample_rate != 16000 or wav.format.channels != 1 or wav.format.sample_width != 2:
        raise ValueError('native source requires PCM16 mono 16000Hz WAV')
    digest = hashlib.sha256()
    count = 0
    async with aclosing(wav.chunks()) as source:
        async for block in source:
            digest.update(block)
            count += len(block)
    if not count:
        raise ValueError('WAV audio is empty')
    return {'sha256': digest.hexdigest(), 'bytes': count, 'format': 'PCM16LE mono 16000Hz'}


async def take_sample(args, engine, ffmpeg: str | None, index: int, page: int, *, native=None, pcm_reference=None) -> dict:
    start = time.monotonic()
    cpu_start = time.process_time()
    clock = lambda: (time.monotonic() - start) * 1000
    output_path = args.encoded_dir / f'{index:02d}-page-{page}.ogg'
    row = {'index': index, 'ogg_page_duration_us': page, 'first_encoded_bytes_ms': None,
           'last_audio_yield_ms': None, 'source_exhausted_ms': None, 'first_response_ms': None,
           'start_speech_ms': None, 'first_text_ms': None, 'last_text_ms': None,
           's3_end_text_ms': None, 's3_end_ms': None, 'eof_ms': None,
           'encoded_bytes_yielded': 0, 'encoded_sha256': None, 'decoded_pcm': None,
           'pcm_bytes_consumed': None, 'pcm_sha256_consumed': None,
           's3_statuses': [], 's3_error_codes': [], 'grpc_status': None,
           'results': [], 'metrics': None, 'success': False, 'error_type': None}
    digest = hashlib.sha256()
    candidates = []
    s3_end = False
    s3_error = False
    eof = False
    async def audio():
        process = await asyncio.create_subprocess_exec(ffmpeg, '-nostdin', '-hide_banner', '-loglevel', 'error',
            '-re', '-i', str(args.wav), '-ac', '1', '-ar', '16000', '-c:a', 'libopus', '-b:a', '24k',
            '-vbr', 'on', '-application', 'voip', '-frame_duration', '20', '-page_duration', str(page),
            '-flush_packets', '1', '-f', 'ogg', 'pipe:1', stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.DEVNULL)
        try:
            with output_path.open('xb') as file:
                while block := await process.stdout.read(224):
                    file.write(block)
                    digest.update(block)
                    row['encoded_bytes_yielded'] += len(block)
                    if row['first_encoded_bytes_ms'] is None:
                        row['first_encoded_bytes_ms'] = clock()
                    row['last_audio_yield_ms'] = clock()
                    yield block
            if await process.wait() != 0:
                raise RuntimeError('test-source encoder failed')
            row['source_exhausted_ms'] = clock()
        finally:
            await stop_process(process)

    async def native_audio():
        _, audio_module, encoding = native
        wav = audio_module.WavAudio(args.wav, chunk_ms=20, realtime=True)
        if wav.format.sample_rate != 16000 or wav.format.channels != 1 or wav.format.sample_width != 2:
            raise ValueError('native source requires PCM16 mono 16000Hz WAV')
        pcm_digest = hashlib.sha256()
        row['pcm_bytes_consumed'] = 0
        async def pcm():
            try:
                async with aclosing(wav.chunks()) as chunks:
                    async for block in chunks:
                        pcm_digest.update(block)
                        row['pcm_bytes_consumed'] += len(block)
                        yield block
                row['source_exhausted_ms'] = clock()
            finally:
                row['pcm_sha256_consumed'] = pcm_digest.hexdigest()
        with output_path.open('xb') as file:
            async with aclosing(encoding.pcm16_to_ogg(pcm(), page_duration_ms=page // 1000)) as encoded:
                async for page_bytes in encoded:
                    for offset in range(0, len(page_bytes), 224):
                        block = page_bytes[offset:offset + 224]
                        file.write(block)
                        digest.update(block)
                        row['encoded_bytes_yielded'] += len(block)
                        if row['first_encoded_bytes_ms'] is None:
                            row['first_encoded_bytes_ms'] = clock()
                        row['last_audio_yield_ms'] = clock()
                        yield block
    try:
        async with asyncio.timeout(args.timeout):
            source = native_audio() if native is not None else audio()
            async with aclosing(engine.stream(source, config=ProtocolConfig(language=args.language),
                                               context=EditingContext(), timeout=args.timeout)) as events:
                async for event in events:
                    row['metrics'] = asdict(event.metrics)
                    response = event.response
                    now = clock()
                    if response is None:
                        eof = True
                        row['grpc_status'] = 'OK'
                        row['eof_ms'] = now
                        continue
                    row['s3_statuses'].append(response.status)
                    if row['first_response_ms'] is None:
                        row['first_response_ms'] = now
                    if response.speech_event == 0 and row['start_speech_ms'] is None:
                        row['start_speech_ms'] = now
                    found = _helpers.texts(response)
                    if found:
                        candidates.extend(found)
                        if row['first_text_ms'] is None:
                            row['first_text_ms'] = now
                        row['last_text_ms'] = now
                        if response.status == 1:
                            row['s3_end_text_ms'] = now
                    if response.status == 1:
                        s3_end = True
                        if row['s3_end_ms'] is None:
                            row['s3_end_ms'] = now
                    if response.status == 2 or response.error_code not in (None, 0):
                        s3_error = True
                        row['s3_error_codes'].append(response.error_code)
            row['encoded_sha256'] = digest.hexdigest()
            if row['source_exhausted_ms'] is None:
                raise RuntimeError('source incomplete')
            if hash_file(output_path) != (row['encoded_sha256'], row['encoded_bytes_yielded']):
                raise RuntimeError('encoded artifact integrity failure')
            if native is not None:
                if pcm_reference is None or (row['pcm_sha256_consumed'], row['pcm_bytes_consumed']) != (pcm_reference['sha256'], pcm_reference['bytes']):
                    raise ValueError('native PCM source integrity failure')
                row['decoded_pcm'] = native_decode_pcm(native[0], output_path)
                if row['decoded_pcm']['bytes'] != pcm_reference['bytes']:
                    raise ValueError('decoded PCM duration mismatch')
            else:
                row['decoded_pcm'] = await decode_pcm(ffmpeg, output_path)
            row['success'] = bool(eof and s3_end and candidates and not s3_error)
            if not row['success']:
                row['error_type'] = 'S3Error' if s3_error else 'IncompleteSession'
    except Exception as exc:
        row['encoded_sha256'] = digest.hexdigest()
        row['error_type'] = type(exc).__name__
        if isinstance(exc, grpc.RpcError):
            code = exc.code()
            row['grpc_status'] = code.name if code is not None else None
    finally:
        row['results'] = _helpers.safe_results(candidates, args.include_results)
        row['elapsed_ms'] = clock()
        row['python_process_cpu_ms'] = (time.process_time() - cpu_start) * 1000
        for field in ('first_text_ms', 'last_text_ms', 's3_end_text_ms', 's3_end_ms', 'eof_ms'):
            key = field.removesuffix('_ms') + '_after_source_eof_ms'
            row[key] = None if row[field] is None or row['source_exhausted_ms'] is None else row[field] - row['source_exhausted_ms']
    return row


def ffmpeg_metadata(executable: str) -> dict:
    path = Path(executable).resolve()
    digest, size = hash_file(path)
    try:
        result = subprocess.run([str(path), '-version'], check=True, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, timeout=5)
        version = result.stdout.decode('utf-8', errors='replace').splitlines()[0][:256]
    except (OSError, subprocess.SubprocessError, IndexError):
        version = None
    return {'sha256': digest, 'bytes': size, 'version': version}


async def run(args, output, ffmpeg: str | None) -> bool:
    wav_hash, wav_size = hash_file(args.wav)
    plan = schedule(args.pairs, args.page_durations)
    native = None
    pcm_reference = None
    native_metadata = None
    if args.source == 'native':
        native_metadata, _, av, audio, encoding = prepare_native()
        native = (av, audio, encoding)
        pcm_reference = await native_pcm_reference(audio, args.wav)
    # Common helper captures engine, wire, protocol, transport, OS and git state.
    common_args = argparse.Namespace(language=args.language, timeout=args.timeout,
        chunk_sizes=[224], samples=args.pairs)
    metadata = _helpers.reproducibility(common_args)
    metadata['source'] = args.source
    metadata['pacing'] = 'realtime_wav_chunk_start' if native else 'realtime_ffmpeg_re'
    metadata['code_sha256']['benchmark_engine_helpers'] = metadata['code_sha256'].pop('harness')
    metadata['code_sha256']['realtime_harness'] = hash_file(Path(__file__))[0]
    if native:
        metadata['native_test_source'] = native_metadata
        metadata['code_sha256']['audio'] = hash_file(Path(native[1].__file__))[0]
        metadata['code_sha256']['encoding'] = hash_file(Path(native[2].__file__))[0]
    else:
        metadata['ffmpeg_test_source'] = ffmpeg_metadata(ffmpeg)
    metadata['page_durations_us'] = args.page_durations
    metadata['pairs'] = args.pairs
    metadata['encoder'] = {'codec': 'libopus', 'bitrate': '24k', 'vbr': 'on', 'application': 'voip',
                           'frame_duration_ms': 20, 'sample_rate': 16000, 'channels': 1,
                           'flush_packets': 1, 'chunk_bytes': 224}
    report = {'reproducibility': metadata, 'source_wav_sha256': wav_hash, 'source_wav_bytes': wav_size,
              'source_pcm_reference': pcm_reference,
              'samples': [], 'decoded_pcm_identical_across_samples': None, 'complete': False,
              'notes': ['FFmpeg is an optional test source and verifier, not an engine dependency.',
                        'Native PyAV/Opus is an optional audio extra, not a core dependency.',
                        'One ready channel; sequential alternating pairs, no retry.',
                        'Audio producer timestamps are not network writes; source_exhausted_ms is not end-request yield.',
                        'Native source EOF is PCM reader exhaustion; FFmpeg source EOF is encoded stdout exhaustion; timings are not directly equivalent.',
                        'Last candidate or S3 end text is not necessarily final Rambler output.',
                        'Python process CPU includes native encoding/decoding but excludes FFmpeg child processes; it may have coarse platform resolution.',
                        'FFmpeg encoder and decoder CPU are not measured.',
                        'Decoded PCM hashes compare samples, not fidelity to the uncompressed WAV.']}
    def save():
        output.seek(0)
        json.dump(report, output, ensure_ascii=False, indent=2, allow_nan=False)
        output.write('\n')
        output.truncate()
        output.flush()
    save()
    transport = GrpcTransport(args.target, secure=not args.insecure)
    try:
        opened = time.monotonic()
        await transport.open(timeout=args.timeout)
        report['channel_open_ms'] = (time.monotonic() - opened) * 1000
        engine = RamblerEngine(transport, EnvironmentCredentials())
        first_pcm = None
        for index, page in enumerate(plan):
            row = await take_sample(args, engine, ffmpeg, index, page, native=native, pcm_reference=pcm_reference)
            report['samples'].append(row)
            if hash_file(args.wav) != (wav_hash, wav_size):
                row['success'] = False
                row['error_type'] = 'WavIntegrityError'
            pcm = row['decoded_pcm']
            if pcm is not None:
                fingerprint = (pcm['sha256'], pcm['bytes'])
                if first_pcm is None:
                    first_pcm = fingerprint
                elif fingerprint != first_pcm:
                    report['decoded_pcm_identical_across_samples'] = False
                    row['success'] = False
                    row['error_type'] = 'DecodedPCMIntegrityError'
            if not row['success']:
                save()
                return False
            save()
        report['decoded_pcm_identical_across_samples'] = True
        report['complete'] = True
        save()
        return True
    except Exception as exc:
        report['error_type'] = type(exc).__name__
        if isinstance(exc, grpc.RpcError):
            code = exc.code()
            report['grpc_status'] = code.name if code is not None else None
        save()
        return False
    finally:
        await transport.close()


def parser():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--live', action='store_true')
    p.add_argument('--source', choices=('ffmpeg', 'native'), default='ffmpeg',
                   help='native requires the audio extra (PyAV, Python >=3.12)')
    p.add_argument('--target', default=DEFAULT_TARGET)
    p.add_argument('--insecure', action='store_true', help='loopback only')
    p.add_argument('--wav', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--encoded-dir', type=Path, required=True)
    p.add_argument('--pairs', type=int, default=3)
    p.add_argument('--page-durations', type=page_durations, default=[1000000, 20000])
    p.add_argument('--include-results', action='store_true', help='synthetic corpus only')
    p.add_argument('--language', default='en-US')
    p.add_argument('--timeout', type=float, default=30)
    return p


def main(argv=None):
    p = parser()
    args = p.parse_args(argv)
    try:
        schedule(args.pairs, args.page_durations)
        if not math.isfinite(args.timeout) or not 0 < args.timeout <= 30:
            raise ValueError('timeout must be positive and at most 30 seconds')
        if not _helpers.loopback(args.target) and not args.live:
            raise ValueError('--live is required outside loopback')
        if args.insecure and not _helpers.loopback(args.target):
            raise ValueError('insecure mode is restricted to loopback')
        if args.source == 'native' and any(page % 1000 for page in args.page_durations):
            raise ValueError('native page durations must be integral milliseconds')
        ffmpeg = None
        if args.source == 'ffmpeg':
            ffmpeg = shutil.which('ffmpeg')
            if ffmpeg is None:
                raise FileNotFoundError('ffmpeg test source unavailable')
        with args.output.open('x', encoding='utf-8') as output:
            args.encoded_dir.mkdir()
            return 0 if asyncio.run(run(args, output, ffmpeg)) else 1
    except Exception as exc:
        print(json.dumps({'error_type': type(exc).__name__, 'message': 'experiment failed'}))
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
