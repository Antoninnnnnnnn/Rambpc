"""Private, opt-in Rust CLI endurance harness; one fresh child per session.

Never loads dotenv files. Child inherits the caller environment without inspecting
credentials, but PATH is restricted to OS system directories. On Unix those
directories may contain Python or FFmpeg; this is not proof they are absent. The report contains
bounded raw child output and MUST remain private. Public projection contains only
allowlisted hashes, counters and local observations; inspect before publishing.
Unlike the Python endurance bench, sessions do not reuse a channel or process.
"""
from __future__ import annotations

import argparse
import base64
from contextlib import ExitStack
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import struct
import subprocess
import sys
import threading
import time
import unicodedata
import wave

ROOT = Path(__file__).resolve().parents[1]
MAX_AUDIO = 16 * 1024 * 1024
MAX_OUTPUT = 8 * 1024 * 1024
MAX_REPORT = 64 * 1024 * 1024
MAX_SAMPLES = 400
PHASES = ('beginning', 'middle', 'end')
ERROR_CATEGORIES = frozenset(('grpc', 'timeout', 'credentials', 'transport', 's3', 'audio',
                              'cleanup', 'protocol', 'ambiguous', 'result_limit', 'incomplete',
                              'configuration', 'engine', 'arguments', 'cancelled', 'signal',
                              'safety_blocked', 'unrecognized_safety'))
GRPC_CODES = frozenset(('Ok', 'Cancelled', 'Unknown', 'InvalidArgument', 'DeadlineExceeded',
                        'NotFound', 'AlreadyExists', 'PermissionDenied', 'ResourceExhausted',
                        'FailedPrecondition', 'Aborted', 'OutOfRange', 'Unimplemented', 'Internal',
                        'Unavailable', 'DataLoss', 'Unauthenticated'))


def digest(data):
    return {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}


def file_digest(path, limit):
    size = 0
    sha = hashlib.sha256()
    with Path(path).open('rb') as source:
        while block := source.read(65536):
            size += len(block)
            if size > limit:
                raise ValueError('file exceeds limit')
            sha.update(block)
    return {'sha256': sha.hexdigest(), 'bytes': size}


class Pcm16WaveRead(wave.Wave_read):
    def _read_fmt_chunk(self, chunk):
        header = chunk.read(16)
        if len(header) < 16 or struct.unpack_from('<H', header, 14)[0] != 16:
            raise ValueError('WAV must contain 16-bit PCM')
        chunk.seek(0)
        super()._read_fmt_chunk(chunk)


def inspect_wav(path):
    file = file_digest(path, MAX_AUDIO)
    h = hashlib.sha256()
    with Pcm16WaveRead(str(path)) as source:
        if (source.getnchannels(), source.getsampwidth(), source.getframerate(), source.getcomptype()) != (1, 2, 16000, 'NONE'):
            raise ValueError('audio must be uncompressed mono PCM16 at 16000 Hz')
        frames = source.getnframes()
        if not 0 < frames <= 300 * 16000:
            raise ValueError('audio duration outside limits')
        remaining = frames
        while remaining:
            count = min(remaining, 32768)
            chunk = source.readframes(count)
            if len(chunk) != count * 2:
                raise ValueError('truncated WAV')
            if sys.byteorder == 'big':
                swapped = bytearray(chunk)
                swapped[0::2], swapped[1::2] = chunk[1::2], chunk[0::2]
                chunk = bytes(swapped)
            h.update(chunk)
            remaining -= count
    return {'file': file, 'pcm': {'sha256': h.hexdigest(), 'bytes': frames * 2},
            'duration_seconds': frames / 16000, 'samples': frames,
            'sample_rate': 16000, 'channels': 1, 'sample_width': 2}


def normalize(text):
    return ' '.join(''.join(' ' if unicodedata.category(c).startswith('P') else c
                            for c in text.casefold()).split())


def contains_marker(text, marker):
    return ' ' + normalize(marker) + ' ' in ' ' + normalize(text) + ' '


def strict_json(raw):
    def unique(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError('duplicate JSON key')
            result[key] = value
        return result
    def reject(_):
        raise ValueError('nonfinite JSON')
    def finite_float(value):
        number = float(value)
        if not math.isfinite(number):
            raise ValueError('nonfinite JSON')
        return number
    return json.loads(raw.decode('utf-8'), object_pairs_hook=unique, parse_constant=reject,
                      parse_float=finite_float)


def text_hash(text):
    return digest(text.encode('utf-8'))


def project_audio_progress(value):
    """Encoded-source counters only; no PCM/server-consumption claim."""
    if not isinstance(value, dict):
        return {}
    result = {}
    for key in ('chunks', 'bytes'):
        item = value.get(key)
        if type(item) is int and 0 <= item <= 2**64 - 1:
            result[key] = item
    for key in ('eof', 'cleanup_finished', 'cleanup_succeeded'):
        if type(value.get(key)) is bool:
            result[key] = value[key]
    return result


def project_result(value, markers):
    """Keep only known semantic field hashes, counts and EOF metrics."""
    if not isinstance(value, dict) or value.get('type') != 'result':
        raise ValueError('not a successful result')
    text = value.get('text')
    updates = value.get('final_updates')
    metrics = value.get('metrics')
    if not isinstance(text, str) or not isinstance(updates, list) or not 1 <= len(updates) <= 256 or not isinstance(metrics, dict):
        raise ValueError('missing result fields')
    known = []
    observations = {phase: False for phase in PHASES}
    for update in updates:
        if not isinstance(update, dict) or not isinstance(update.get('hypotheses'), list):
            raise ValueError('invalid final update')
        hypotheses = update['hypotheses']
        if len(hypotheses) > 256:
            raise ValueError('too many hypotheses')
        projected = []
        for h in hypotheses:
            if not isinstance(h, dict):
                raise ValueError('invalid hypothesis')
            fields = {}
            if h.get('safety_status') is not None:
                if type(h['safety_status']) is not int or h['safety_status'] != 0:
                    raise ValueError('unsafe final candidate')
                fields['safety_status'] = h['safety_status']
            confidence = h.get('confidence')
            if confidence is not None:
                if type(confidence) not in (int, float) or not math.isfinite(confidence):
                    raise ValueError('invalid confidence')
                fields['confidence'] = confidence
            if h.get('text') is not None:
                if not isinstance(h['text'], str):
                    raise ValueError('invalid hypothesis text')
                fields['text'] = text_hash(h['text'])
                for phase in PHASES:
                    observations[phase] |= contains_marker(h['text'], markers[phase])
            edit = h.get('voice_edit')
            if edit is not None:
                if not isinstance(edit, dict):
                    raise ValueError('invalid voice edit')
                edited = {}
                for name in ('transcript', 'alternative_text', 'edited_text'):
                    if edit.get(name) is not None:
                        if not isinstance(edit[name], str):
                            raise ValueError('invalid edit text')
                        edited[name] = text_hash(edit[name])
                        if name == 'edited_text':
                            for phase in PHASES:
                                observations[phase] |= contains_marker(edit[name], markers[phase])
                for name in ('intent', 'active_field'):
                    if type(edit.get(name)) is int:
                        edited[name] = edit[name]
                context = edit.get('edited_context')
                if context is not None:
                    if not isinstance(context, dict) or any(not isinstance(context.get(x), str) for x in ('before', 'selected', 'after')):
                        raise ValueError('invalid edited context')
                    edited['edited_context'] = {k: text_hash(context[k]) for k in ('before', 'selected', 'after')}
                    if type(context.get('cursor_aware')) is bool:
                        edited['edited_context']['cursor_aware'] = context['cursor_aware']
                    combined = ''.join(context[k] for k in ('before', 'selected', 'after'))
                    for phase in PHASES:
                        observations[phase] |= contains_marker(combined, markers[phase])
                fields['voice_edit'] = edited
            projected.append(fields)
        known.append({'hypotheses': projected})
    allowed_metrics = ('requests_produced', 'request_protobuf_bytes', 'responses_received',
                       'response_protobuf_bytes', 'elapsed_seconds', 'first_response_seconds', 'eof_seconds')
    clean_metrics = {}
    for k in allowed_metrics:
        v = metrics.get(k)
        if v is not None:
            if type(v) not in (int, float) or not math.isfinite(v) or v < 0:
                raise ValueError('invalid metric')
            clean_metrics[k] = v
    if 'eof_seconds' not in clean_metrics:
        raise ValueError('missing EOF metric')
    # Historical binaries do not emit this field. If present, require the full
    # successful-source observation rather than silently accepting false flags.
    if 'audio' in metrics:
        audio = project_audio_progress(metrics['audio'])
        if len(audio) != 5 or not all(audio[key] for key in ('eof', 'cleanup_finished', 'cleanup_succeeded')):
            raise ValueError('incomplete encoded audio metric')
        clean_metrics['audio'] = audio
    rendered_markers = {phase: contains_marker(text, markers[phase]) for phase in PHASES}
    canonical_finals = json.dumps(known, ensure_ascii=False, sort_keys=True, separators=(',', ':'),
                                  allow_nan=False).encode('utf-8')
    return {'rendered_text': text_hash(text), 'final_updates': known,
            'ordered_finals_summary': digest(canonical_finals),
            'final_update_count': len(known), 'markers_found': observations,
            'rendered_markers_found': rendered_markers, 'engine_metrics': clean_metrics}


def project_cli_error(value):
    """Allow only known error categories and explicitly typed diagnostics."""
    if not isinstance(value, dict) or value.get('type') != 'error':
        return None
    result = {}
    for key in ('category', 'close_error_category'):
        item = value.get(key)
        if isinstance(item, str) and item in ERROR_CATEGORIES:
            result[key] = item
    for key in ('grpc_code', 'close_grpc_code'):
        item = value.get(key)
        if isinstance(item, str) and item in GRPC_CODES:
            result[key] = item
    incomplete = value.get('incomplete')
    if isinstance(incomplete, dict):
        diagnostic = {}
        if type(incomplete.get('saw_s3_end')) is bool:
            diagnostic['saw_s3_end'] = incomplete['saw_s3_end']
        completion = incomplete.get('completion')
        if isinstance(completion, dict):
            clean = {}
            for key in ('requests_produced', 'requests_polled', 'responses_received'):
                if type(completion.get(key)) is int and completion[key] >= 0:
                    clean[key] = completion[key]
            for key in ('source_exhausted', 'upload_eof_polled', 'source_closed'):
                if type(completion.get(key)) is bool:
                    clean[key] = completion[key]
            code = completion.get('terminal_code')
            if isinstance(code, str) and code in GRPC_CODES:
                clean['terminal_code'] = code
            diagnostic['completion'] = clean
        metrics = incomplete.get('metrics')
        if isinstance(metrics, dict):
            clean = {}
            for key in ('requests_produced', 'request_protobuf_bytes', 'responses_received',
                        'response_protobuf_bytes'):
                if type(metrics.get(key)) is int and metrics[key] >= 0:
                    clean[key] = metrics[key]
            for key in ('elapsed_seconds', 'first_response_seconds', 'eof_seconds'):
                item = metrics.get(key)
                if ((type(item) is int and item >= 0)
                        or (type(item) is float and item >= 0 and math.isfinite(item))):
                    clean[key] = item
            if 'audio' in metrics:
                clean['audio'] = project_audio_progress(metrics['audio'])
            diagnostic['metrics'] = clean
        result['incomplete'] = diagnostic
    return result


def coverage(samples, interval, elapsed):
    gap = max(3.0, 3 * interval)
    minimum = max(2, math.floor(.8 * elapsed / interval) + 1)
    times = [s['elapsed_seconds'] for s in samples]
    gaps = [b-a for a,b in zip(times,times[1:])]
    valid = (minimum <= len(times) <= MAX_SAMPLES and bool(times) and times[0] <= gap
             and times[-1] >= elapsed-gap and all(0 <= x <= gap for x in gaps)
             and all(type(s.get('rss_bytes')) is int and s['rss_bytes'] > 0
                     and type(s.get('cpu_seconds')) in (float, int) and s['cpu_seconds'] >= 0 for s in samples))
    return {'coverage_ok': bool(valid), 'minimum_samples': minimum,
            'max_allowed_gap_seconds': gap, 'max_observed_gap_seconds': max(gaps) if gaps else None}


class WindowsCounters(ctypes.Structure):
    _fields_ = [('cb', ctypes.c_uint32), ('faults', ctypes.c_uint32),
                ('peak', ctypes.c_size_t), ('working_set', ctypes.c_size_t)] + [(f'extra{i}', ctypes.c_size_t) for i in range(6)]


def child_resources(pid):
    """Current working set/RSS and cumulative child process CPU seconds."""
    try:
        if sys.platform.startswith('linux'):
            with open(f'/proc/{pid}/statm', encoding='ascii') as f:
                rss = int(f.read(128).split()[1]) * os.sysconf('SC_PAGE_SIZE')
            with open(f'/proc/{pid}/stat', encoding='ascii') as f:
                stat = f.read(8192).rsplit(')', 1)[1].split()
            cpu = (int(stat[11]) + int(stat[12])) / os.sysconf('SC_CLK_TCK')
            return rss, cpu
        if sys.platform == 'win32':
            kernel = ctypes.WinDLL('kernel32', use_last_error=True)
            psapi = ctypes.WinDLL('psapi', use_last_error=True)
            kernel.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
            kernel.OpenProcess.restype = ctypes.c_void_p
            kernel.CloseHandle.argtypes = [ctypes.c_void_p]
            kernel.GetProcessTimes.argtypes = [ctypes.c_void_p] + [ctypes.POINTER(ctypes.c_uint64)] * 4
            psapi.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.POINTER(WindowsCounters), ctypes.c_uint32]
            handle = kernel.OpenProcess(0x1000 | 0x10, False, pid)
            if not handle:
                return None, None
            try:
                counters = WindowsCounters()
                counters.cb = ctypes.sizeof(counters)
                create, exit_, kernel_time, user_time = (ctypes.c_uint64() for _ in range(4))
                if not psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb):
                    return None, None
                if not kernel.GetProcessTimes(handle, ctypes.byref(create), ctypes.byref(exit_), ctypes.byref(kernel_time), ctypes.byref(user_time)):
                    return None, None
                return int(counters.working_set), (kernel_time.value + user_time.value) / 1e7
            finally:
                kernel.CloseHandle(handle)
    except (OSError, ValueError, IndexError, AttributeError):
        pass
    return None, None


def restricted_env():
    # Inherit caller's explicit environment, never inspect/log credential values.
    env = os.environ.copy()
    if os.name == 'nt':
        buf = ctypes.create_unicode_buffer(32768)
        length = ctypes.windll.kernel32.GetWindowsDirectoryW(buf, len(buf))
        if not 0 < length < len(buf):
            raise OSError('cannot locate Windows directory')
        root = buf.value
        env['PATH'] = os.pathsep.join((str(Path(root) / 'System32'), root))
    else:
        env['PATH'] = '/usr/bin:/bin'
    return env


def _drain(stream, buffer, exceeded):
    try:
        while block := stream.read(65536):
            remaining = MAX_OUTPUT - len(buffer)
            buffer.extend(block[:max(0, remaining)])
            if len(block) > remaining:
                exceeded.set()
    finally:
        stream.close()


def run_child(command, timeout, interval):
    """Bound both pipes, timeout, child lifetime and sampling; no shell or retry."""
    started = time.monotonic()
    child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, env=restricted_env(), shell=False)
    stdout, stderr, exceeded = bytearray(), bytearray(), threading.Event()
    threads = [threading.Thread(target=_drain, args=(pipe, buffer, exceeded), daemon=True)
               for pipe, buffer in ((child.stdout, stdout), (child.stderr, stderr))]
    for thread in threads:
        thread.start()
    samples = []
    reason = None
    next_sample = started
    try:
        while child.poll() is None:
            now = time.monotonic()
            if exceeded.is_set():
                reason = 'output_limit'
                break
            if now - started > timeout:
                reason = 'timeout'
                break
            if now >= next_sample:
                rss, cpu = child_resources(child.pid)
                if len(samples) < MAX_SAMPLES:
                    samples.append({'elapsed_seconds': now-started, 'rss_bytes': rss, 'cpu_seconds': cpu})
                else:
                    reason = 'sample_limit'
                    break
                next_sample += interval
            time.sleep(min(0.1, max(0.001, next_sample-now)))
    finally:
        if child.poll() is None:
            child.kill()
        child.wait()
        for thread in threads:
            thread.join()
    elapsed = time.monotonic() - started
    if exceeded.is_set():
        reason = 'output_limit'
    return {'exit_code': child.returncode, 'reason': reason, 'stdout': bytes(stdout), 'stderr': bytes(stderr),
            'elapsed_seconds': elapsed, 'samples': samples}


def healthy_predecessor(previous, binary, markers, language):
    try:
        cfg = previous['config']
        sessions = previous['sessions']
        return (previous['report_kind'] == 'rust_cli_endurance_v1' and cfg['stage'] == '120'
                and cfg['binary'] == binary and cfg['language'] == language
                and cfg['markers'] == {k: text_hash(markers[k]) for k in PHASES}
                and previous['summary']['status'] == 'healthy'
                and type(cfg['sessions']) is int and 1 <= cfg['sessions'] <= 3 and len(sessions) == cfg['sessions']
                and type(previous['audio']['duration_seconds']) in (int, float)
                and 110 < previous['audio']['duration_seconds'] <= 130
                and all(x['status'] == 'healthy' and x['resources']['coverage_ok']
                        and coverage(x['resources']['samples'], cfg['sample_interval_seconds'], x['child_wall_seconds'])['coverage_ok']
                        and set(x['public']['rendered_markers_found']) == set(PHASES)
                        and all(x['public']['rendered_markers_found'][k] is True for k in PHASES)
                        and x['public']['ordered_finals_summary'] == sessions[0]['public']['ordered_finals_summary']
                        and x['ordered_finals_match_first'] is (None if index == 0 else True)
                        for index, x in enumerate(sessions)))
    except (KeyError, TypeError, AttributeError, ValueError, ZeroDivisionError, OverflowError):
        return False


def validate_plan(args, audio, binary):
    if not args.allow_live:
        raise ValueError('live execution requires --allow-live')
    if not 1 <= args.sessions <= 3 or not 1 <= args.interval <= 10:
        raise ValueError('invalid session count or sampling interval')
    if not audio['duration_seconds'] + 10 <= args.timeout <= 360:
        raise ValueError('timeout must cover audio plus 10s and be at most 360s')
    lower, upper = {'120': (110, 130), '300': (280, 300)}[args.stage]
    if not lower < audio['duration_seconds'] <= upper:
        raise ValueError('audio duration outside selected stage')
    if not re.fullmatch(r'[A-Za-z]{2,3}(?:-[A-Za-z0-9]{2,8}){0,3}', args.language):
        raise ValueError('invalid language')
    markers = {phase: getattr(args, 'marker_' + phase) for phase in PHASES}
    if any(not m or len(m.encode('utf-8')) > 256 or not normalize(m) for m in markers.values()):
        raise ValueError('three nonempty bounded markers required')
    if len({normalize(m) for m in markers.values()}) != 3:
        raise ValueError('markers must be distinct')
    predecessor = None
    if args.stage == '300':
        if args.previous_report is None:
            raise ValueError('stage 300 requires healthy stage 120 predecessor')
        with args.previous_report.open('rb') as f:
            raw = f.read(MAX_REPORT + 1)
        if len(raw) > MAX_REPORT or not healthy_predecessor(strict_json(raw), binary, markers, args.language):
            raise ValueError('invalid stage 120 predecessor')
        predecessor = digest(raw)
    return markers, predecessor


def source_metadata():
    files = sorted((ROOT/'crates').glob('*/src/**/*.rs'))
    return {'cargo_lock': file_digest(ROOT/'Cargo.lock', 4*1024*1024),
            'rust_sources': {str(p.relative_to(ROOT)).replace('\\', '/'): file_digest(p, 4*1024*1024) for p in files},
            'harness': file_digest(Path(__file__), 4*1024*1024), 'os': platform.platform(),
            'python': platform.python_version(),
            'compiler_note': 'Compiler and version are caller-supplied build metadata, not verified by this harness.'}


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--executable', required=True, type=Path)
    p.add_argument('--audio', required=True, type=Path)
    p.add_argument('--output', required=True, type=Path)
    p.add_argument('--public-output', type=Path, help='optional NEW allowlisted projection; inspect before sharing')
    p.add_argument('--stage', choices=('120','300'), required=True)
    p.add_argument('--previous-report', type=Path)
    p.add_argument('--sessions', type=int, default=1)
    p.add_argument('--timeout', type=int, required=True)
    p.add_argument('--interval', type=float, default=1)
    p.add_argument('--language', default='en-US')
    p.add_argument('--compiler-version', required=True)
    p.add_argument('--build-version', required=True)
    p.add_argument('--allow-live', action='store_true')
    for phase in PHASES:
        p.add_argument('--marker-' + phase, required=True)
    args = p.parse_args(argv)
    executable = args.executable.resolve(strict=True)
    if not executable.is_file() or not args.compiler_version or not args.build_version:
        raise ValueError('local executable and build metadata required')
    binary = file_digest(executable, 256*1024*1024)
    audio = inspect_wav(args.audio)
    markers, predecessor = validate_plan(args, audio, binary)
    metadata = source_metadata()
    metadata.update({'binary': binary, 'compiler_version_claimed': args.compiler_version,
                     'build_version_claimed': args.build_version})
    report = {'report_kind':'rust_cli_endurance_v1', 'schema_version':1,
              'privacy_warning':'PRIVATE: contains raw subprocess outputs; inspect before sharing.',
              'config':{'stage':args.stage, 'sessions':args.sessions, 'binary':binary,
                        'language':args.language, 'markers':{k:text_hash(v) for k,v in markers.items()},
                        'sample_interval_seconds':args.interval, 'timeout_seconds':args.timeout,
                        'process_model':'fresh process and channel per session; no reuse',
                        'path_policy':'OS system directories only'},
              'audio':audio, 'provenance':metadata, 'predecessor':predecessor, 'sessions':[]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.public_output is not None:
        args.public_output.parent.mkdir(parents=True, exist_ok=True)
    with ExitStack() as stack:
        out = stack.enter_context(args.output.open('x', encoding='utf-8'))
        public_out = (stack.enter_context(args.public_output.open('x', encoding='utf-8'))
                      if args.public_output is not None else None)
        first_final_signature = None
        for _ in range(args.sessions):
            row = {'status':'inconclusive'}
            try:
                command = [str(executable), 'dictate-wav', str(args.audio.resolve()), '--realtime',
                           '--language', args.language, '--timeout', str(args.timeout)]
                result = run_child(command, args.timeout + 5, args.interval)
                row['private_output'] = {k:{'base64':base64.b64encode(result[k]).decode('ascii'), **digest(result[k])} for k in ('stdout','stderr')}
                row['exit_code'] = result['exit_code']
                row['reason'] = result['reason']
                row['child_wall_seconds'] = result['elapsed_seconds']
                row['resources'] = {'samples':result['samples'], **coverage(result['samples'],args.interval,result['elapsed_seconds']),
                                    'scope':'Child current resident/working-set and cumulative CPU, sampled; not exact peak. Wall clock includes process startup and cleanup.'}
                if result['reason'] is None and result['exit_code'] == 0:
                    row['public'] = project_result(strict_json(result['stdout']), markers)
                    signature = row['public']['ordered_finals_summary']
                    row['ordered_finals_match_first'] = (None if first_final_signature is None
                                                         else signature == first_final_signature)
                    if first_final_signature is None:
                        first_final_signature = signature
                    unchanged = file_digest(args.audio, MAX_AUDIO) == audio['file'] and file_digest(executable, 256*1024*1024) == binary
                    row['input_and_binary_unchanged'] = unchanged
                    row['status'] = ('healthy' if unchanged and row['resources']['coverage_ok']
                                     and all(row['public']['rendered_markers_found'].values())
                                     and row['ordered_finals_match_first'] is not False else 'inconclusive')
                else:
                    row['status'] = 'failed'
                    if result['exit_code'] != 0 and result['reason'] is None:
                        try:
                            diagnostic = project_cli_error(strict_json(result['stdout']))
                        except (ValueError, UnicodeError, RecursionError):
                            diagnostic = None
                        if diagnostic is not None:
                            row['cli_error'] = diagnostic
            except Exception as exc:
                row['error_class'] = type(exc).__name__
                row['status'] = 'failed'
            report['sessions'].append(row)
            if row['status'] != 'healthy':
                break
        good = len(report['sessions']) == args.sessions and all(row['status'] == 'healthy' for row in report['sessions'])
        report['summary'] = {'status':'healthy' if good else 'failed_or_inconclusive',
                             'requested':args.sessions, 'completed':len(report['sessions'])}
        json.dump(report, out, ensure_ascii=True, indent=2, allow_nan=False)
        out.write('\n')
        if public_out is not None:
            projected = {'report_kind': 'rust_cli_endurance_public_v1', 'config': report['config'],
                         'audio': audio, 'binary': binary, 'cargo_lock': metadata['cargo_lock'],
                         'rust_sources': metadata['rust_sources'], 'harness': metadata['harness'],
                         'predecessor': predecessor, 'summary': report['summary'], 'sessions': []}
            for row in report['sessions']:
                projected['sessions'].append({key: row[key] for key in (
                    'status', 'exit_code', 'reason', 'child_wall_seconds', 'resources',
                    'public', 'cli_error', 'ordered_finals_match_first', 'input_and_binary_unchanged', 'error_class') if key in row})
            json.dump(projected, public_out, ensure_ascii=True, indent=2, allow_nan=False)
            public_out.write('\n')
    return 0 if good else 1


if __name__ == '__main__':
    raise SystemExit(main())
