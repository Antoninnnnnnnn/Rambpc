"""Sequential paging experiments with FFmpeg or native test sources.

Legacy pairs alternate two durations unchanged; grid rounds rotate/reverse the
recorded order, up to 24 sessions on one ready channel, without retries.
A producer timestamp is not a completed network write; final S3 status and gRPC
EOF do not imply a final Rambler action.
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
    if not 2 <= len(pages) <= 4 or len(set(pages)) != len(pages) or any(p < 20000 or p > 1000000 for p in pages):
        raise argparse.ArgumentTypeError('provide 2..4 distinct page durations between 20000 and 1000000 microseconds')
    return pages


def schedule(pairs: int, pages: list[int]) -> list[int]:
    if not 1 <= pairs <= 3 or len(pages) != 2 or len(set(pages)) != 2 or any(p < 20000 or p > 1000000 for p in pages):
        raise ValueError('invalid pairs or page durations')
    return [p for i in range(pairs) for p in (pages if i % 2 == 0 else pages[::-1])]


def grid_schedule(rounds: int, pages: list[int]) -> list[int]:
    """Equal repetitions; rotate positions and reverse alternate cycles."""
    if (type(rounds) is not int or not 1 <= rounds or not 2 <= len(pages) <= 4
            or len(set(pages)) != len(pages) or rounds * len(pages) > 24
            or any(type(p) is not int or not 20000 <= p <= 1000000 for p in pages)):
        raise ValueError('grid requires 2..4 unique durations and at most 24 sessions')
    plan = []
    for index in range(rounds):
        order = pages if (index // len(pages)) % 2 == 0 else pages[::-1]
        offset = index % len(pages)
        plan.extend(order[offset:] + order[:offset])
    return plan


def experiment_schedule(args) -> list[int]:
    rounds = getattr(args, 'rounds', None)
    return grid_schedule(rounds, args.page_durations) if rounds is not None else schedule(args.pairs, args.page_durations)


def native_chunks(data: bytes, policy: str):
    """Never accumulate across muxer writes or wait for a target size."""
    if policy == 'muxer':
        yield data
    elif policy == 'legacy224':
        for offset in range(0, len(data), 224):
            yield data[offset:offset + 224]
    else:
        raise ValueError('unknown native chunk policy')


class OggMilestones:
    """Observe complete Ogg pages without changing bytes or upload boundaries.

    Headers are not useful audio. A complete page containing packet data other
    than OpusHead/OpusTags is the conservative first-audio availability marker.
    """
    def __init__(self):
        self.pending = bytearray()
        self.first_header_ms = None
        self.first_audio_page_ms = None

    def feed(self, data: bytes, now: float):
        if self.first_audio_page_ms is not None:
            return
        self.pending.extend(data)
        while len(self.pending) >= 27:
            if self.pending[:4] != b'OggS':
                raise ValueError('invalid native Ogg page')
            segments = self.pending[26]
            header_size = 27 + segments
            if len(self.pending) < header_size:
                return
            size = header_size + sum(self.pending[27:header_size])
            if len(self.pending) < size:
                return
            body = self.pending[header_size:size]
            if body.startswith(b'OpusHead') or body.startswith(b'OpusTags'):
                if self.first_header_ms is None:
                    self.first_header_ms = now
            elif body:
                self.first_audio_page_ms = now
            del self.pending[:size]
            if self.first_audio_page_ms is not None:
                self.pending.clear()
                return


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
           'results': [], 'metrics': None, 'success': False, 'error_type': None,
           'semantic_result': None, 'pipeline_wall_ms': None, 'pipeline_cpu_ms': None,
           'verification_wall_ms': 0.0, 'verification_cpu_ms': 0.0,
           'artifact_write_wall_ms': 0.0, 'artifact_write_cpu_ms': 0.0,
           'native_first_header_ms': None, 'native_first_audio_page_ms': None,
           'native_encoder_setup_ms': None,
           'native_encoder_setup_note': 'Not separately instrumented; first output includes setup and encoding.'}
    milestones = OggMilestones()
    semantics = _helpers.SemanticHistory()
    semantics.update_row(row)
    def write_artifact(file, block):
        wall, cpu = time.monotonic(), time.process_time()
        file.write(block)
        row['artifact_write_wall_ms'] += (time.monotonic() - wall) * 1000
        row['artifact_write_cpu_ms'] += (time.process_time() - cpu) * 1000
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
                    write_artifact(file, block)
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
                    milestones.feed(page_bytes, clock())
                    row['native_first_header_ms'] = milestones.first_header_ms
                    row['native_first_audio_page_ms'] = milestones.first_audio_page_ms
                    for block in native_chunks(page_bytes, getattr(args, 'native_chunk_policy', 'muxer')):
                        write_artifact(file, block)
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
                    semantics.observe(response)
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
            row['pipeline_wall_ms'] = clock()
            row['pipeline_cpu_ms'] = (time.process_time() - cpu_start) * 1000
            verify_wall, verify_cpu = time.monotonic(), time.process_time()
            try:
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
            finally:
                row['verification_wall_ms'] = (time.monotonic() - verify_wall) * 1000
                row['verification_cpu_ms'] = (time.process_time() - verify_cpu) * 1000
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
        if row['pipeline_wall_ms'] is None:
            row['pipeline_wall_ms'] = clock()
            row['pipeline_cpu_ms'] = (time.process_time() - cpu_start) * 1000
        semantics.update_row(row)
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
    plan = experiment_schedule(args)
    native = None
    pcm_reference = None
    native_metadata = None
    if args.source == 'native':
        native_metadata, _, av, audio, encoding = prepare_native()
        native = (av, audio, encoding)
        pcm_reference = await native_pcm_reference(audio, args.wav)
    # Common helper captures engine, wire, protocol, transport, OS and git state.
    common_args = argparse.Namespace(language=args.language, timeout=args.timeout,
        chunk_sizes=[224], samples=args.pairs, insecure=args.insecure,
        include_results=args.include_results)
    metadata = _helpers.reproducibility(common_args)
    metadata['source'] = args.source
    # Shared helper's chunk_sizes describe its own benchmark, not this one.
    metadata.pop('chunk_sizes', None)
    metadata.pop('samples_per_mode_and_chunk', None)
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
    metadata['pairs'] = args.pairs if getattr(args, 'rounds', None) is None else None
    metadata['rounds'] = getattr(args, 'rounds', None)
    metadata['schedule_mode'] = 'rotating_grid' if metadata['rounds'] is not None else 'legacy_alternating_pairs'
    metadata['scheduled_page_durations_us'] = plan
    metadata['session_limit'] = 24
    metadata['timeout_scope'] = 'pipeline plus verification; native synchronous work cancels cooperatively'
    metadata['context'] = {'before': '', 'selected': '', 'after': '', 'cursor_aware': True}
    metadata['native_chunk_policy'] = getattr(args, 'native_chunk_policy', 'muxer') if native else None
    metadata['include_results'] = args.include_results
    metadata['encoder'] = {'codec': 'libopus', 'bitrate': '24k', 'vbr': 'on', 'application': 'voip',
                           'frame_duration_ms': 20, 'sample_rate': 16000, 'channels': 1,
                           'flush_packets': 1}
    if native:
        metadata['encoder']['pcm_chunk_ms'] = 20
        metadata['encoder']['encoded_chunk_policy'] = (
            'native muxer writes unchanged, immediately' if metadata['native_chunk_policy'] == 'muxer'
            else 'native muxer writes subdivided into at most 224 bytes')
    else:
        metadata['encoder']['encoded_chunk_policy'] = 'FFmpeg stdout reads of at most 224 bytes'
    report = {'reproducibility': metadata, 'source_wav_sha256': wav_hash, 'source_wav_bytes': wav_size,
              'source_pcm_reference': pcm_reference,
              'schema_version': 2,
              'samples': [], 'decoded_pcm_identical_across_samples': None, 'complete': False,
              'comparison_status': 'inconclusive', 'semantic_reference': None,
              'semantic_policy': _helpers.SemanticHistory.policy,
              'semantic_policy_note': 'Compare every ordered recognition-state-1 hypothesis set, including empty sets and repeats. Provisional observations and confidence scores are separate.',
              'notes': ['FFmpeg is an optional test source and verifier, not an engine dependency.',
                        'Native PyAV/Opus is an optional audio extra, not a core dependency.',
                        'One ready channel; sequential recorded schedule, no retry.',
                        'Audio producer timestamps are not network writes; source_exhausted_ms is not end-request yield.',
                        'Native source EOF is PCM reader exhaustion; FFmpeg source EOF is encoded stdout exhaustion; timings are not directly equivalent.',
                        'Last candidate or S3 end text is not necessarily final Rambler output.',
                        'Python process CPU includes native encoding/decoding but excludes FFmpeg child processes; it may have coarse platform resolution.',
                        'FFmpeg encoder and decoder CPU are not measured.',
                        'Decoded PCM hashes compare samples, not fidelity to the uncompressed WAV.',
                        'pipeline CPU/wall stop before verification; artifact write timings are an included subset, not independent totals.',
                        'Artifact write timings cover buffered file.write only; open/flush/close remain in pipeline totals.',
                        'Pipeline includes hashing, instrumentation and native codec; process CPU excludes FFmpeg children.',
                        'Report JSON saves and source rehash between samples are outside sample timings.',
                        'Native first header/audio markers observe complete muxer pages before upload, not completed network writes.',
                        'Import preparation and channel readiness precede samples; encoder setup is not separately observable.',
                        'Grid has equal repetition counts; positional balance is exact only over complete rotation cycles.']}
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
            row['scheduled_round'] = index // len(args.page_durations)
            row['position_in_round'] = index % len(args.page_durations)
            if row['success']:
                report['semantic_reference'] = _helpers.semantic_guard(row, report['semantic_reference'])
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
        report['comparison_status'] = 'semantic_match_observed_not_quality_proof'
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
    repetitions = p.add_mutually_exclusive_group()
    repetitions.add_argument('--pairs', type=int, default=3, help='legacy alternating schedule; exactly two durations, 1..3 pairs')
    repetitions.add_argument('--rounds', type=int, help='rotating grid repetitions per duration; at most 24 sessions total')
    p.add_argument('--native-chunk-policy', choices=('muxer', 'legacy224'), default='muxer',
                   help='muxer matches the PCM CLI; legacy224 reproduces historical fragmentation')
    p.add_argument('--page-durations', type=page_durations, default=[1000000, 20000])
    p.add_argument('--include-results', action='store_true', help='synthetic corpus only')
    p.add_argument('--language', default='en-US')
    p.add_argument('--timeout', type=float, default=30)
    return p


def main(argv=None):
    p = parser()
    args = p.parse_args(argv)
    try:
        experiment_schedule(args)
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
