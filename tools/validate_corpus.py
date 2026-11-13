"""Bounded, opt-in validation of local corpus fixtures."""
from __future__ import annotations

import argparse
import asyncio
from contextlib import aclosing
from dataclasses import asdict, is_dataclass
import hashlib
import importlib.metadata
import ipaddress
import json
import math
from pathlib import Path
import platform
import subprocess
import sys
import time
import unicodedata
import wave

import grpc
from rambler.audio import WavAudio
from rambler.credentials import EnvironmentCredentials
from rambler.encoding import pcm16_to_ogg
from rambler.engine import RamblerEngine
from rambler.protocol import EditingContext, ProtocolConfig
from rambler.results import (AmbiguousResultError, ContextConflictError,
                             collect_dictation, replacement_proposal)
from rambler.transport import DEFAULT_TARGET, GrpcTransport

ROOT = Path(__file__).resolve().parents[1]
MAX_FILE = 32 * 1024 * 1024
MAX_SECONDS = 120
MAX_CASES = 10


def normalize(text):
    return ' '.join(''.join(' ' if unicodedata.category(c).startswith('P') else c
                            for c in text.casefold()).split())


def word_error_rate(reference, hypothesis):
    ref, hyp = normalize(reference).split(), normalize(hypothesis).split()
    if not ref:
        return None
    row = list(range(len(hyp) + 1))
    for i, a in enumerate(ref, 1):
        new = [i]
        for j, b in enumerate(hyp, 1):
            new.append(min(new[-1] + 1, row[j] + 1, row[j-1] + (a != b)))
        row = new
    return {'errors': row[-1], 'reference_words': len(ref), 'rate': row[-1] / len(ref),
            'limitation': 'Intelligent dictation may clean or rewrite spoken text.'}


def file_hash(path):
    digest = hashlib.sha256()
    total = 0
    with path.open('rb') as f:
        while block := f.read(65536):
            total += len(block)
            if total > MAX_FILE:
                raise ValueError('audio exceeds byte limit')
            digest.update(block)
    if not total:
        raise ValueError('empty audio')
    return digest.hexdigest()


def inspect_audio(path):
    suffix = path.suffix.lower()
    if suffix not in ('.wav', '.ogg'):
        raise ValueError('only WAV and Ogg input supported')
    initial = file_hash(path)
    if suffix == '.wav':
        audio = WavAudio(path, chunk_ms=20)
        if audio.format.sample_rate != 16000 or not 0 < audio.duration_s <= MAX_SECONDS:
            raise ValueError('WAV rate or duration outside limits')
        digest = hashlib.sha256()
        with wave.open(str(path), 'rb') as f:
            while data := f.readframes(32768):
                digest.update(data)
        return {'file_sha256': initial, 'source_sha256': digest.hexdigest(),
                'source_kind': 'pcm16le', 'duration_s': audio.duration_s}
    import av
    with av.open(str(path), mode='r') as container:
        streams = container.streams.audio
        if len(streams) != 1 or streams[0].codec_context.name != 'opus':
            raise ValueError('Ogg input must contain one Opus audio stream')
        duration = 0
        for frame in container.decode(streams[0]):
            duration += frame.samples / frame.sample_rate
            if duration > MAX_SECONDS:
                raise ValueError('audio duration exceeds limit')
        if not duration:
            raise ValueError('empty audio')
    return {'file_sha256': initial, 'source_sha256': initial,
            'source_kind': 'ogg', 'duration_s': duration}


def load_cases(path, ids=None):
    if path.stat().st_size > 1024 * 1024:
        raise ValueError('manifest too large')
    data = json.loads(path.read_text(encoding='utf-8'))
    cases = data['cases']
    if not isinstance(cases, list) or not 1 <= len(cases) <= MAX_CASES:
        raise ValueError('invalid case count')
    names = [c['id'] for c in cases]
    if any(not isinstance(n, str) or not n for n in names) or len(set(names)) != len(names):
        raise ValueError('invalid case ids')
    if ids is not None:
        requested = ids.split(',')
        if not all(requested) or len(set(requested)) != len(requested) or not set(requested) <= set(names):
            raise ValueError('invalid selected ids')
        cases = [c for c in cases if c['id'] in requested]
    return cases


def loopback(target):
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


def json_value(value):
    # JSON has no non-finite numbers. Preserve their identity explicitly.
    if isinstance(value, float) and not math.isfinite(value):
        return {'nonfinite_float': repr(value)}
    if is_dataclass(value):
        return json_value(asdict(value))
    if isinstance(value, bytes):
        return {'hex': value.hex()}
    if isinstance(value, dict):
        return {k: json_value(v) for k, v in value.items()}
    if isinstance(value, (tuple, list)):
        return [json_value(v) for v in value]
    return value


async def chunks(path, info, realtime, tracker):
    digest = hashlib.sha256()
    try:
        if info['source_kind'] == 'pcm16le':
            source = WavAudio(path, chunk_ms=20, realtime=realtime).chunks()
            async with aclosing(source):
                async for part in source:
                    digest.update(part)
                    tracker['bytes'] += len(part)
                    yield part
        else:
            with path.open('rb') as f:
                sizes = (224, 8192)
                i = 0
                while part := f.read(sizes[i % 2]):
                    i += 1
                    digest.update(part)
                    tracker['bytes'] += len(part)
                    yield part
                    await asyncio.sleep(0)
        tracker['sha256'] = digest.hexdigest()
        tracker['complete'] = True
    finally:
        tracker.setdefault('complete', False)


async def encoded_source(path, info, realtime, tracker, encoded):
    source = chunks(path, info, realtime, tracker)
    stream = pcm16_to_ogg(source, page_duration_ms=20) if info['source_kind'] == 'pcm16le' else source
    digest = hashlib.sha256()
    try:
        async with aclosing(stream):
            async for block in stream:
                encoded['bytes'] += len(block)
                if encoded['bytes'] > MAX_FILE:
                    raise ValueError('encoded audio exceeds limit')
                digest.update(block)
                yield block
        encoded['sha256'] = digest.hexdigest()
        encoded['complete'] = True
    finally:
        encoded.setdefault('complete', False)


def render(result, case, include):
    out = {'final_update_count': len(result.final_updates), 'rendering': 'ambiguous'}
    if include:
        out['final_updates'] = json_value(result.final_updates)
        out['context'] = json_value(result.context)
    try:
        text = result.text
    except AmbiguousResultError:
        return out
    out['rendering'] = 'unique'
    if include:
        out['rendered_text'] = text
    if 'reference_text' in case:
        out['wer'] = word_error_rate(case['reference_text'], text)
    if 'required_terms' in case:
        normalized = normalize(text)
        out['required_terms'] = {term: normalize(term) in normalized for term in case['required_terms']}
    h = result.single_hypothesis
    if h.voice_edit is not None and h.voice_edit.intent == 2:
        proposal = replacement_proposal(result, expected_revision='fixture-v1')
        proposal.validate_current(result.context, revision='fixture-v1')
        changed = EditingContext(before=result.context.before + 'x', selected=result.context.selected,
                                 after=result.context.after, cursor_aware=result.context.cursor_aware)
        for label, current, revision in (('context', changed, 'fixture-v1'),
                                         ('revision', result.context, 'fixture-v2')):
            try:
                proposal.validate_current(current, revision=revision)
            except ContextConflictError:
                out[f'{label}_conflict_detected'] = True
            else:
                out[f'{label}_conflict_detected'] = False
        out['replacement_verified'] = True
        if include:
            out['replacement_proposal'] = json_value(proposal)
    return out


def versions():
    libs = {}
    for name in ('rambpc-engine', 'grpcio', 'av'):
        try:
            libs[name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            libs[name] = None
    try:
        git = subprocess.run(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], capture_output=True,
                             text=True, timeout=5, check=True).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        git = None
    hashes = {}
    for path in sorted((ROOT / 'src' / 'rambler').glob('*.py')):
        hashes[path.name] = hashlib.sha256(path.read_bytes()).hexdigest()
    return {'python': sys.version, 'os': platform.platform(), 'libraries': libs, 'git_head': git,
            'rambler_source_sha256': hashes,
            'validator_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}


async def run_case(case, args, engine):
    path = ROOT / case['audio']
    info = inspect_audio(path)
    tracker, encoded = {'bytes': 0}, {'bytes': 0}
    context = EditingContext(**case.get('context', {}))
    started = time.monotonic()
    source = encoded_source(path, info, args.realtime, tracker, encoded)
    async with aclosing(source):
        result = await collect_dictation(engine, source, config=ProtocolConfig(language=case['language']),
                                         context=context, timeout=args.timeout)
    collection_seconds = time.monotonic() - started
    if not tracker.get('complete') or not encoded.get('complete') or tracker.get('sha256') != info['source_sha256'] or file_hash(path) != info['file_sha256']:
        raise ValueError('audio source not fully consumed or changed')
    report = {'id': case['id'], 'transport': 'success', 'audio': info,
              'consumed_source': tracker, 'sent_ogg': encoded,
              'effective_pacing': ('wav_20ms_realtime' if args.realtime and info['source_kind'] == 'pcm16le'
                                   else 'unpaced'),
              'timing': {'collection_seconds': collection_seconds,
                         'scope': 'monotonic wall time for collection including encoding, streaming and EOF; excludes pre-inspection and rendering',
                         'engine_metrics_at_eof': json_value(result.metrics)}}
    try:
        report.update(render(result, case, args.include_results))
    except Exception as exc:
        report.update({'rendering': 'error', 'render_error_class': type(exc).__name__})
        if args.include_results:
            report['final_updates'] = json_value(result.final_updates)
            report['context'] = json_value(result.context)
    return report


async def run(args, cases):
    transport = GrpcTransport(args.target, secure=not args.insecure)
    engine = RamblerEngine(transport, EnvironmentCredentials())
    outputs = []
    try:
        for case in cases:
            try:
                await engine.open(timeout=min(args.timeout, 10))
                outputs.append(await run_case(case, args, engine))
            except Exception as exc:
                code = exc.code() if isinstance(exc, grpc.RpcError) else None
                code_name = code.name if isinstance(code, grpc.StatusCode) else None
                outputs.append({'id': case['id'], 'transport': 'failed_or_unverified',
                                'effective_pacing': 'not_verified',
                                'error_class': type(exc).__name__, 'grpc_code': code_name})
                if code in (grpc.StatusCode.PERMISSION_DENIED, grpc.StatusCode.UNAUTHENTICATED):
                    break
    finally:
        await transport.close()
    return outputs


def outcome(cases, reports):
    succeeded = sum(item.get('transport') == 'success' and item.get('rendering') == 'unique'
                    for item in reports)
    failed = len(reports) - succeeded
    not_run = len(cases) - len(reports)
    return {'requested': len(cases), 'completed_successfully': succeeded, 'failed': failed,
            'not_run': not_run, 'status': 'success' if not failed and not not_run else 'failed',
            'scope': 'transport, source integrity and unambiguous rendering; excludes quality observations'}


def parser():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--manifest', required=True, type=Path)
    p.add_argument('--output', required=True, type=Path)
    p.add_argument('--ids')
    p.add_argument('--live', action='store_true')
    p.add_argument('--realtime', action='store_true')
    p.add_argument('--include-results', action='store_true')
    p.add_argument('--timeout', type=float, default=90)
    p.add_argument('--target', default=DEFAULT_TARGET)
    p.add_argument('--insecure', action='store_true')
    return p


def main(argv=None):
    p = parser()
    args = p.parse_args(argv)
    if not 0 < args.timeout <= 300:
        p.error('timeout must be within 0..300 seconds')
    if not loopback(args.target) and not args.live:
        p.error('non-loopback target requires --live')
    if args.insecure and not loopback(args.target):
        p.error('insecure transport is restricted to loopback')
    cases = load_cases(args.manifest, args.ids)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x', encoding='utf-8') as out:
        reports = asyncio.run(run(args, cases))
        summary = outcome(cases, reports)
        report = {'schema_version': 1, 'versions': versions(), 'requested_realtime': args.realtime,
                  'include_results': args.include_results, 'summary': summary, 'cases': reports}
        json.dump(json_value(report), out, ensure_ascii=False, indent=2, allow_nan=False)
        out.write('\n')
    return 0 if summary['status'] == 'success' else 1


if __name__ == '__main__':
    raise SystemExit(main())
