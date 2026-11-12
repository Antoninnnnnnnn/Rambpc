"""Sequential engine benchmark; cold means a new channel, NOT a cold process/DNS/server.

Each round rotates chunk sizes and alternates fresh/reused order. A separately
recorded warmup precedes all reused samples. All timings are client-observed;
network, service and client processing are not separately measurable. Payload
sizes are protobuf sizes, not network bytes. Real endpoints require --live.
"""
from __future__ import annotations

import argparse
import asyncio
from contextlib import aclosing
import ctypes
from dataclasses import asdict
from datetime import datetime, timezone
import hashlib
import importlib
import ipaddress
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

import grpc

from rambler.credentials import EnvironmentCredentials
from rambler.engine import RamblerEngine
from rambler.protocol import EditingContext, ProtocolConfig
from rambler.transport import DEFAULT_TARGET, GrpcTransport

MAX_SESSIONS = 41


def loopback(target: str) -> bool:
    if target.startswith('['):
        host, sep, port = target[1:].partition(']:')
    else:
        host, sep, port = target.rpartition(':')
    if not sep or not port.isdecimal() or not 0 < int(port) < 65536:
        return False
    if host.lower() == 'localhost':
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def chunks_arg(value: str) -> list[int]:
    try:
        values = [int(x) for x in value.split(',')]
    except ValueError as exc:
        raise argparse.ArgumentTypeError('chunk sizes must be comma-separated integers') from exc
    if not values or len(set(values)) != len(values) or any(x <= 0 or x > 8 * 1024 * 1024 for x in values):
        raise argparse.ArgumentTypeError('chunk sizes must be unique positive integers at most 8388608')
    return values


def schedule(samples: int, chunks: list[int]) -> list[tuple[str, int]]:
    if not 1 <= samples <= 10 or not chunks or 1 + 2 * samples * len(chunks) > MAX_SESSIONS:
        raise ValueError('samples must be 1..10 and total sessions including warmup at most 41')
    result = []
    for round_number in range(samples):
        rotated = chunks[round_number % len(chunks):] + chunks[:round_number % len(chunks)]
        for index, chunk in enumerate(rotated):
            modes = ('fresh_channel', 'reused_channel') if (round_number + index) % 2 == 0 else ('reused_channel', 'fresh_channel')
            result.extend((mode, chunk) for mode in modes)
    return result


def percentile95(values: list[float]) -> float | None:
    if not values:
        return None
    return sorted(values)[math.ceil(.95 * len(values)) - 1]


def summarize(samples: list[dict]) -> list[dict]:
    groups = sorted({(s['mode'], s['chunk_bytes']) for s in samples if not s['warmup']})
    result = []
    for mode, chunk in groups:
        rows = [s for s in samples if not s['warmup'] and s['mode'] == mode and s['chunk_bytes'] == chunk]
        successful = [s for s in rows if s['success']]
        metrics = {}
        for key in ('open_ms', 'first_any_response_ms', 'first_text_ms', 's3_end_ms', 'eof_ms', 'cpu_ms', 'request_protobuf_bytes', 'response_protobuf_bytes', 'request_protobuf_overhead_bytes'):
            values = [s[key] for s in successful if s.get(key) is not None]
            metrics[key] = {'median': statistics.median(values) if values else None, 'p95_nearest_rank': percentile95(values)}
        result.append({'mode': mode, 'chunk_bytes': chunk, 'count': len(rows), 'successes': len(successful), 'metrics': metrics})
    return result


def rss_highwater_bytes() -> int | None:
    try:
        import resource
        value = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        import sys
        return int(value if sys.platform == 'darwin' else value * 1024)
    except ImportError:
        pass
    try:
        from ctypes import wintypes
        class Counters(ctypes.Structure):
            _fields_ = [('cb', wintypes.DWORD), ('PageFaultCount', wintypes.DWORD),
                        ('PeakWorkingSetSize', ctypes.c_size_t), ('WorkingSetSize', ctypes.c_size_t),
                        ('QuotaPeakPagedPoolUsage', ctypes.c_size_t), ('QuotaPagedPoolUsage', ctypes.c_size_t),
                        ('QuotaPeakNonPagedPoolUsage', ctypes.c_size_t), ('QuotaNonPagedPoolUsage', ctypes.c_size_t),
                        ('PagefileUsage', ctypes.c_size_t), ('PeakPagefileUsage', ctypes.c_size_t)]
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        psapi = ctypes.WinDLL('psapi', use_last_error=True)
        kernel.GetCurrentProcess.restype = wintypes.HANDLE
        psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
        psapi.GetProcessMemoryInfo.restype = wintypes.BOOL
        counters = Counters()
        counters.cb = ctypes.sizeof(counters)
        if psapi.GetProcessMemoryInfo(kernel.GetCurrentProcess(), ctypes.byref(counters), counters.cb):
            return counters.PeakWorkingSetSize
    except (AttributeError, OSError):
        pass
    return None


def reproducibility(args) -> dict:
    """Capture local run configuration, without environment variables or git paths."""
    root = Path(__file__).resolve().parent.parent
    files = {'harness': Path(__file__).resolve()}
    for name in ('engine', 'protocol', 'transport', 'wire'):
        files[name] = Path(importlib.import_module('rambler.' + name).__file__).resolve()
    hashes = {}
    for name, path in files.items():
        try:
            hashes[name] = hashlib.sha256(path.read_bytes()).hexdigest()
        except OSError:
            hashes[name] = None
    head = dirty = None
    try:
        def git(*arguments):
            return subprocess.run(('git', '-C', str(root), *arguments), check=True,
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                  timeout=5).stdout
        candidate = git('rev-parse', '--verify', 'HEAD').decode('ascii').strip()
        if len(candidate) in (40, 64) and all(c in '0123456789abcdef' for c in candidate):
            head = candidate
        dirty = bool(git('status', '--porcelain', '--untracked-files=normal'))
    except (OSError, subprocess.SubprocessError, UnicodeError):
        pass
    return {
        'captured_utc': datetime.now(timezone.utc).isoformat(),
        'os': {'system': platform.system(), 'release': platform.release(), 'machine': platform.machine()},
        'python': platform.python_version(), 'grpcio': grpc.__version__,
        'cpu': {'processor': platform.processor() or None, 'logical_count': os.cpu_count()},
        'code_sha256': hashes, 'git_head': head, 'git_dirty': dirty,
        'git_dirty_note': 'Snapshot includes untracked files, potentially including the new output.',
        'config': asdict(ProtocolConfig(language=args.language)),
        'timeout_seconds': args.timeout, 'chunk_sizes': args.chunk_sizes,
        'samples_per_mode_and_chunk': args.samples, 'concurrency': 1,
        'pacing': 'accelerated',
    }


def inspect_audio(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    count = 0
    with path.open('rb') as stream:
        header = stream.read(65536)
        if not header.startswith(b'OggS') or b'OpusHead' not in header:
            raise ValueError('input does not have an Ogg/Opus signature')
        digest.update(header)
        count += len(header)
        while block := stream.read(65536):
            digest.update(block)
            count += len(block)
    return digest.hexdigest(), count


def texts(response) -> list[dict]:
    results = []
    for item in response.hypotheses:
        edit = item.voice_edit
        record = {'text': item.text, 'intent': None if edit is None else edit.intent}
        if edit is not None:
            context = edit.edited_context
            record.update(transcript=edit.transcript, alternative_text=edit.alternative_text, edited_text=edit.edited_text,
                          edited_context=None if context is None else {
                              'before': context.before, 'selected': context.selected,
                              'after': context.after, 'cursor_aware': context.cursor_aware})
        if any(record.get(k) for k in ('text', 'transcript', 'alternative_text', 'edited_text', 'edited_context')):
            results.append(record)
    return results


def safe_results(results: list[dict], include: bool) -> list[dict]:
    if include:
        return results
    return [{'sha256': hashlib.sha256(json.dumps(item, sort_keys=True, ensure_ascii=False, separators=(',', ':')).encode('utf-8')).hexdigest(), 'intent': item['intent']} for item in results]


async def sample(args, mode: str, chunk: int, expected: tuple[str, int], *, transport=None, warmup=False, index=0) -> dict:
    start = time.monotonic()
    cpu = time.process_time()
    fresh = transport is None
    if fresh:
        transport = GrpcTransport(args.target, secure=not args.insecure)
    engine = RamblerEngine(transport, EnvironmentCredentials())
    row = {'mode': mode, 'chunk_bytes': chunk, 'warmup': warmup, 'channel_reused': not fresh and not warmup,
           'open_ms': None, 'first_any_response_ms': None, 'first_text_ms': None, 's3_end_ms': None,
           'eof_ms': None, 'last_audio_yield_ms': None, 'source_exhausted_ms': None,
           'cpu_ms': None, 'request_protobuf_bytes': None, 'response_protobuf_bytes': None,
           'request_protobuf_overhead_bytes': None, 'grpc_status': None, 's3_statuses': [],
           'requests_produced': None, 'responses_received': None,
           'audio_bytes_consumed': 0, 'audio_sha256_consumed': None, 'success': False,
           's3_error_codes': [], 'results': [], 'error_type': None,
           'channel_closed_after_sample': False}
    clock = lambda: (time.monotonic() - start) * 1000
    digest = hashlib.sha256()
    async def audio():
        with args.audio.open('rb') as stream:
            while block := stream.read(chunk):
                digest.update(block)
                row['audio_bytes_consumed'] += len(block)
                row['last_audio_yield_ms'] = clock()
                yield block
            row['source_exhausted_ms'] = clock()
    found = []
    ended = False
    failed_s3 = False
    eof = False
    try:
        if fresh or warmup:
            opened = time.monotonic()
            await engine.open(timeout=args.timeout)
            row['open_ms'] = (time.monotonic() - opened) * 1000
        async with aclosing(engine.stream(audio(), config=ProtocolConfig(language=args.language),
                                          context=EditingContext(), timeout=args.timeout)) as events:
            async for event in events:
                metrics = event.metrics
                row.update(request_protobuf_bytes=metrics.request_protobuf_bytes,
                           response_protobuf_bytes=metrics.response_protobuf_bytes,
                           requests_produced=metrics.requests_produced,
                           responses_received=metrics.responses_received)
                response = event.response
                now = clock()
                if response is None:
                    eof = True
                    row['grpc_status'] = 'OK'
                    row['eof_ms'] = now
                    continue
                row['s3_statuses'].append(response.status)
                if args.raw_dir is not None:
                    with (args.raw_dir / f'{index:03d}-{metrics.responses_received:04d}.pb').open('xb') as raw:
                        raw.write(response.raw)
                if row['first_any_response_ms'] is None:
                    row['first_any_response_ms'] = now
                candidates = texts(response)
                if candidates and row['first_text_ms'] is None:
                    row['first_text_ms'] = now
                found.extend(candidates)
                if response.status == 1:
                    ended = True
                    if row['s3_end_ms'] is None:
                        row['s3_end_ms'] = now
                if response.status == 2 or response.error_code not in (None, 0):
                    failed_s3 = True
                    row['s3_error_codes'].append(response.error_code)
        row['audio_sha256_consumed'] = digest.hexdigest()
        if row['request_protobuf_bytes'] is not None:
            row['request_protobuf_overhead_bytes'] = row['request_protobuf_bytes'] - row['audio_bytes_consumed']
        integrity = (row['audio_sha256_consumed'], row['audio_bytes_consumed']) == expected and row['source_exhausted_ms'] is not None
        row['results'] = safe_results(found, args.include_results)
        row['success'] = bool(integrity and eof and ended and found and not failed_s3)
        if not row['success']:
            row['error_type'] = 'AudioIntegrityError' if not integrity else 'S3Error' if failed_s3 else 'IncompleteSession'
    except Exception as exc:
        row['audio_sha256_consumed'] = digest.hexdigest()
        row['error_type'] = type(exc).__name__
        if isinstance(exc, grpc.RpcError):
            code = exc.code()
            row['grpc_status'] = code.name if code is not None else None
    finally:
        # Preserve partial observations even when the terminal gRPC status fails.
        row['results'] = safe_results(found, args.include_results)
        row['cpu_ms'] = (time.process_time() - cpu) * 1000
        row['process_rss_highwater_bytes'] = rss_highwater_bytes()
        if fresh:
            await transport.close()
            row['channel_closed_after_sample'] = True
    return row


async def run(args, output):
    expected = inspect_audio(args.audio)
    plan = schedule(args.samples, args.chunk_sizes)
    report = {'version': 1, 'reproducibility': reproducibility(args),
              'source': {'sha256': expected[0], 'bytes': expected[1], 'validation': 'signature only; not a full Ogg/Opus validation'},
              'target_kind': 'loopback' if loopback(args.target) else 'live', 'samples': [],
              'notes': ['Sequential concurrency 1. Warmup excluded from aggregates.',
                        'Fresh channel is not a cold process, DNS cache or server; gRPC may reuse lower-level resources.',
                        'Timings are client-observed from sample start, except open_ms and cpu_ms durations; no server-compute estimate.',
                        'Audio yield times are producer observations, not completed network writes; source_exhausted_ms is source EOF, not end-request yield.',
                        'CPU process_time can have coarse platform-dependent resolution; do not infer CPU gains from quantized values.',
                        'Sizes are protobuf payload sizes, not exact network bytes.',
                        'RSS highwater is process-wide, not a per-session peak.',
                        'Small n: medians and nearest-rank p95 are descriptive, not reliable tail estimates.'],
              'summary': [], 'complete': False}
    def save():
        output.seek(0)
        json.dump(report, output, ensure_ascii=False, indent=2, allow_nan=False)
        output.write('\n')
        output.truncate()
        output.flush()
    save()
    shared = GrpcTransport(args.target, secure=not args.insecure)
    try:
        warm = await sample(args, 'reused_channel', args.chunk_sizes[0], expected, transport=shared, warmup=True, index=0)
        report['samples'].append(warm)
        save()
        if not warm['success']:
            return False
        for index, (mode, chunk) in enumerate(plan, 1):
            row = await sample(args, mode, chunk, expected, transport=shared if mode == 'reused_channel' else None, index=index)
            report['samples'].append(row)
            report['summary'] = summarize(report['samples'])
            save()
            if not row['success']:
                return False
        report['complete'] = True
        save()
        return True
    finally:
        await shared.close()


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument('--live', action='store_true', help='authorize non-loopback endpoint')
    result.add_argument('--target', default=DEFAULT_TARGET)
    result.add_argument('--insecure', action='store_true', help='loopback tests only')
    result.add_argument('--audio', type=Path, required=True)
    result.add_argument('--output', type=Path, required=True)
    result.add_argument('--samples', type=int, default=5)
    result.add_argument('--chunk-sizes', type=chunks_arg, default=[224, 8192])
    result.add_argument('--timeout', type=float, default=30)
    result.add_argument('--language', default='fr-FR')
    result.add_argument('--include-results', action='store_true', help='include plaintext results; use only synthetic corpus')
    result.add_argument('--raw-dir', type=Path, help='existing directory for sensitive raw protobuf responses')
    return result


def main(argv=None):
    p = parser()
    args = p.parse_args(argv)
    try:
        schedule(args.samples, args.chunk_sizes)
        if not math.isfinite(args.timeout) or args.timeout <= 0:
            raise ValueError('timeout must be finite and positive')
        if not loopback(args.target) and not args.live:
            raise ValueError('--live is mandatory for non-loopback endpoints')
        if args.insecure and not loopback(args.target):
            raise ValueError('--insecure is restricted to loopback')
        if args.raw_dir is not None and not args.raw_dir.is_dir():
            raise ValueError('raw directory must already exist')
        with args.output.open('x', encoding='utf-8') as output:
            return 0 if asyncio.run(run(args, output)) else 1
    except Exception as exc:
        # Never include exception messages: RPC details may contain metadata.
        print(json.dumps({'error_type': type(exc).__name__, 'message': 'benchmark failed'}))
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
