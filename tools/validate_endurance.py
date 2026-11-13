"""Bounded progressive PCM endurance validation; real services require --live.

One selected WAV, 1..3 sequential sessions, no retry after a non-healthy session.
Stage 300 requires a healthy stage-120 report. Smoke is loopback-only. Markers
are observations in individually renderable final updates, NEVER concatenated
segments or a quality/WER proof. Reports contain no raw bytes or server IDs;
--include-text exposes controlled semantic text only and requires privacy review.
"""
from __future__ import annotations

import argparse
import asyncio
from contextlib import aclosing
import ctypes
import hashlib
import importlib
import importlib.metadata
import ipaddress
import json
import math
import os
from pathlib import Path
import platform
import re
import sys
import time
import unicodedata
import wave

import grpc
from rambler.audio import WavAudio
from rambler.credentials import EnvironmentCredentials
from rambler.encoding import pcm16_to_ogg
from rambler.engine import RamblerEngine
from rambler.protocol import EditingContext, ProtocolConfig, context_request
from rambler.results import AmbiguousResultError, DictationResult, ResultLimitError, collect_dictation
from rambler.transport import DEFAULT_TARGET, GrpcTransport

MAX_SESSIONS = 3
MAX_SECONDS = 300
MAX_TIMEOUT = 360
MAX_FILE_BYTES = 16 * 1024 * 1024
MAX_PCM_BYTES = MAX_SECONDS * 16000 * 2
MAX_FINAL_UPDATES = 256
MAX_FINAL_BYTES = 8 * 1024 * 1024
MAX_RESOURCE_SAMPLES = 400
MAX_REPORT_BYTES = 64 * 1024 * 1024
PHASES = ('beginning', 'middle', 'end')
ENCODING_PROFILE = {'codec': 'libopus', 'sample_rate': 16000, 'channels': 1,
                    'sample_width': 2, 'bitrate': 24000, 'application': 'voip',
                    'frame_duration_ms': 20, 'page_duration_ms': 20, 'pcm_chunk_ms': 20}
ROOT = Path(__file__).resolve().parents[1]


def normalize(text):
    return ' '.join(''.join(' ' if unicodedata.category(c).startswith('P') else c
                           for c in text.casefold()).split())


def loopback(target):
    host, separator, port = (target[1:].partition(']:') if target.startswith('[')
                             else target.rpartition(':'))
    if not separator or not port.isdecimal() or not 0 < int(port) < 65536:
        return False
    if host.lower() == 'localhost':
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def _unique(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError('duplicate JSON key')
        value[key] = item
    return value


def read_json(path, limit):
    with Path(path).open('rb') as source:
        data = source.read(limit + 1)
    if len(data) > limit:
        raise ValueError('JSON exceeds size limit')
    def reject(_):
        raise ValueError('nonfinite JSON constant')
    return json.loads(data.decode('utf-8'), object_pairs_hook=_unique, parse_constant=reject), data


def digest(data):
    return {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}


def file_digest(path):
    count = 0
    sha = hashlib.sha256()
    with Path(path).open('rb') as source:
        while block := source.read(65536):
            count += len(block)
            if count > MAX_FILE_BYTES:
                raise ValueError('audio exceeds file limit')
            sha.update(block)
    return {'sha256': sha.hexdigest(), 'bytes': count}


def load_case(path, selected):
    report, original = read_json(path, 64 * 1024)
    cases = report.get('cases')
    if not isinstance(cases, list) or not 1 <= len(cases) <= 2:
        raise ValueError('manifest must contain one or two cases')
    ids = [case.get('id') for case in cases]
    if any(not isinstance(name, str) or not 1 <= len(name) <= 64 for name in ids) or len(set(ids)) != len(ids):
        raise ValueError('invalid case IDs')
    if selected not in ids:
        raise ValueError('select one manifest case')
    case = dict(cases[ids.index(selected)])
    if not isinstance(case.get('language'), str) or not 1 <= len(case['language']) <= 32:
        raise ValueError('invalid language')
    markers = case.get('markers')
    if not isinstance(markers, dict) or set(markers) != set(PHASES):
        raise ValueError('three phase markers required')
    normalized = []
    for phase in PHASES:
        text = markers[phase]
        if not isinstance(text, str) or not 1 <= len(text.encode('utf-8')) <= 256 or not normalize(text):
            raise ValueError('invalid marker text')
        normalized.append(normalize(text))
    if len(set(normalized)) != 3:
        raise ValueError('markers must be distinct')
    case['audio'] = (Path(path).resolve().parent / case['audio']).resolve()
    context_request(EditingContext(**case.get('context', {})))
    return case, digest(original)


def inspect_wav(path):
    original = file_digest(path)
    audio = WavAudio(path, chunk_ms=20, realtime=True)
    if audio.format.sample_rate != 16000 or not 0 < audio.duration_s <= MAX_SECONDS:
        raise ValueError('only PCM16 mono 16000Hz WAV up to 300s')
    sha = hashlib.sha256()
    count = 0
    with wave.open(str(path), 'rb') as source:
        remaining = audio.frame_count
        while remaining:
            samples = min(32768, remaining)
            block = source.readframes(samples)
            if len(block) != samples * 2:
                raise ValueError('truncated PCM source')
            if sys.byteorder == 'big':
                swapped = bytearray(block)
                swapped[0::2], swapped[1::2] = block[1::2], block[0::2]
                block = bytes(swapped)
            count += len(block)
            if count > MAX_PCM_BYTES:
                raise ValueError('PCM byte limit exceeded')
            sha.update(block)
            remaining -= samples
    return {'file': original, 'pcm': {'sha256': sha.hexdigest(), 'bytes': count},
            'duration_seconds': audio.duration_s, 'sample_rate': 16000, 'channels': 1,
            'sample_width': 2}


def resource_coverage(samples, interval, pipeline_seconds):
    """Require both bounded gaps and a plausible number of timed observations."""
    valid = lambda value: type(value) in (int, float) and math.isfinite(value)
    gap_limit = max(3.0, 3 * interval) if valid(interval) and interval > 0 else None
    if (gap_limit is None or not 1 <= interval <= 10 or not valid(pipeline_seconds)
            or pipeline_seconds < 0 or not isinstance(samples, list)):
        return {'coverage_ok': False, 'coverage_error': 'invalid_sampling_metadata'}
    minimum = max(2, math.floor(.8 * pipeline_seconds / interval) + 1)
    times = [sample.get('elapsed_seconds') if isinstance(sample, dict) else None for sample in samples]
    if not times or not all(valid(value) and value >= 0 for value in times):
        return {'coverage_ok': False, 'coverage_error': 'invalid_sample_times',
                'minimum_sample_count': minimum, 'maximum_allowed_gap_seconds': gap_limit}
    gaps = [b - a for a, b in zip(times, times[1:])]
    covered = (minimum <= len(times) <= MAX_RESOURCE_SAMPLES
               and all(0 <= gap <= gap_limit for gap in gaps)
               and times[0] <= gap_limit and times[-1] >= pipeline_seconds - gap_limit
               and times[-1] <= pipeline_seconds + gap_limit)
    return {'coverage_ok': covered, 'minimum_sample_count': minimum,
            'maximum_allowed_gap_seconds': gap_limit,
            'max_observed_sample_gap_seconds': max(gaps) if gaps else None,
            'coverage_policy': 'at least floor(80% of duration/interval)+1 samples (minimum 2), bounded gaps and covered endpoints'}


def _valid_hash(value):
    return isinstance(value, str) and re.fullmatch(r'[0-9a-f]{64}', value) is not None


def healthy_predecessor(previous, args, case):
    """Cross-check local evidence consistency, NOT authenticity or anti-forgery."""
    try:
        config, summary, audio, rows = (previous[key] for key in ('config', 'summary', 'audio', 'sessions'))
        count = config['sessions']
        expected_target = 'loopback' if loopback(args.target) else 'live'
        if (previous['report_kind'] != 'rambler_endurance_v1' or previous['schema_version'] != 1
                or previous.get('cancelled') or any(key in previous for key in (
                    'error_class', 'engine_cleanup_error_class', 'transport_cleanup_error_class'))
                or config['stage'] != '120' or type(count) is not int or not 1 <= count <= MAX_SESSIONS
                or config['target_kind'] != expected_target or config['secure'] is not (not args.insecure)
                or config['language'] != case['language'] or config['pcm_chunk_ms'] != 20
                or config['ogg_page_ms'] != 20 or config['encoding_profile'] != ENCODING_PROFILE
                or summary['status'] != 'healthy' or type(summary['requested']) is not int
                or type(summary['completed']) is not int
                or summary['requested'] != count or summary['completed'] != count
                or not isinstance(rows, list) or len(rows) != count):
            return False
        duration = audio['duration_seconds']
        pcm, source_file = audio['pcm'], audio['file']
        if (type(duration) not in (int, float) or not math.isfinite(duration) or not 110 < duration <= 130
                or (audio['sample_rate'], audio['channels'], audio['sample_width']) != (16000, 1, 2)
                or type(pcm['bytes']) is not int or not 0 < pcm['bytes'] <= MAX_PCM_BYTES
                or pcm['bytes'] % 2 or abs(pcm['bytes'] / 32000 - duration) > 1 / 16000
                or type(source_file['bytes']) is not int or not pcm['bytes'] < source_file['bytes'] <= MAX_FILE_BYTES
                or not _valid_hash(pcm['sha256']) or not _valid_hash(source_file['sha256'])):
            return False
        for phase in PHASES:
            marker = config['markers'][phase]
            if not ((isinstance(marker, str) and normalize(marker) == normalize(case['markers'][phase]))
                    or marker == digest(case['markers'][phase].encode('utf-8'))):
                return False
        reference_signature = None
        for index, row in enumerate(rows):
            consumed, encoded = row['consumed_source'], row['sent_ogg']
            events, metrics, progress = row['events'], row['engine_metrics'], row['progress']
            finals, markers, resources = row['final_updates'], row['markers'], row['resources']
            if (row['index'] != index or row['status'] != 'healthy' or row['transport'] != 'success'
                    or row.get('cancelled') or 'error_class' in row
                    or row['source_integrity'] is not True or row['file_unchanged'] is not True
                    or consumed['complete'] is not True
                    or {key: consumed[key] for key in ('sha256', 'bytes')} != pcm
                    or encoded['complete'] is not True or not _valid_hash(encoded['sha256'])
                    or type(encoded['bytes']) is not int or not 0 < encoded['bytes'] <= MAX_FILE_BYTES
                    or not isinstance(finals, list) or not 1 <= len(finals) <= MAX_FINAL_UPDATES
                    or events['final_updates'] != len(finals)
                    or type(events['responses']) is not int or events['responses'] < len(finals)
                    or metrics['responses_received'] != events['responses']
                    or any(final['recognition_state'] != 1 or len(final['hypotheses']) != 1 for final in finals)
                    or markers['all_observed'] is not True or markers['ambiguous_update_indices'] != []
                    or resources['all_rss_available'] is not True or resources['sample_limit_exceeded'] is not False
                    or resources['coverage_ok'] is not True or resources['rss_error_classes'] != []
                    or row.get('profiling_inconclusive', False)
                    or type(row['pipeline_wall_seconds']) not in (int, float)
                    or not math.isfinite(row['pipeline_wall_seconds'])
                    or row['pipeline_wall_seconds'] < duration - .05
                    or not resource_coverage(resources['samples'], config['sample_interval_seconds'],
                                             row['pipeline_wall_seconds'])['coverage_ok']
                    or any(type(sample['current_rss_bytes']) is not int or sample['current_rss_bytes'] <= 0
                           for sample in resources['samples'])):
                return False
            times = [events['engine_eof_seconds'], events['s3_end_seconds'], metrics['eof_seconds'],
                     progress['pcm_source_eof_seconds'], progress['encoded_source_eof_seconds']]
            if (not all(type(value) in (int, float) and math.isfinite(value) and value >= 0 for value in times)
                    or times[0] <= 0 or times[2] <= 0 or times[1] > times[0]
                    or times[3] < duration - .05 or times[0] > row['pipeline_wall_seconds'] + .001
                    or not times[3] <= times[4] <= times[0]):
                return False
            signature = row['semantic_signature']
            if (not _valid_hash(signature['sha256'])
                    or signature['policy'] != 'all_ordered_state_1_sets_no_dedup_scores_excluded_v1'
                    or signature['update_count'] != len(finals)
                    or signature['hypothesis_counts'] != [len(final['hypotheses']) for final in finals]
                    or reference_signature is not None and signature != reference_signature):
                return False
            reference_signature = signature
            for phase in PHASES:
                matches = markers['observations'][phase]
                if not isinstance(matches, list) or not matches:
                    return False
                for match in matches:
                    span = match['normalized_codepoint_span']
                    if (type(match['update_index']) is not int or not 0 <= match['update_index'] < len(finals)
                            or not isinstance(span, list) or len(span) != 2
                            or any(type(value) is not int for value in span) or not 0 <= span[0] < span[1]):
                        return False
        return True
    except (KeyError, TypeError, ValueError, AttributeError, OverflowError):
        return False


def validate_plan(args, info, *, case=None):
    if type(args.sessions) is not int or not 1 <= args.sessions <= MAX_SESSIONS:
        raise ValueError('sessions must be 1..3')
    if not math.isfinite(args.timeout) or not info['duration_seconds'] + 10 <= args.timeout <= MAX_TIMEOUT:
        raise ValueError('timeout must cover audio plus 10s and be at most 360s')
    if not math.isfinite(args.sample_interval) or not 1 <= args.sample_interval <= 10:
        raise ValueError('resource sample interval must be 1..10s')
    if not loopback(args.target) and not args.live:
        raise ValueError('non-loopback target requires --live')
    if args.insecure and not loopback(args.target):
        raise ValueError('insecure is loopback-only')
    low, high = {'smoke': (0, 5), '120': (110, 130), '300': (280, 300)}[args.stage]
    if not low < info['duration_seconds'] <= high:
        raise ValueError('duration outside selected endurance stage')
    if args.stage == 'smoke' and not loopback(args.target):
        raise ValueError('smoke stage is loopback-only')
    predecessor = None
    if args.stage == '300':
        if args.previous_report is None:
            raise ValueError('stage 300 requires a healthy stage-120 report')
        previous, original = read_json(args.previous_report, MAX_REPORT_BYTES)
        if case is None or not healthy_predecessor(previous, args, case):
            raise ValueError('stage-120 predecessor evidence is incomplete or incompatible')
        predecessor = digest(original)
    return predecessor


class _WindowsCounters(ctypes.Structure):
    # Fixed Win32 DWORD widths; same layout as benchmark_engine's counters.
    _fields_ = [('cb', ctypes.c_uint32), ('PageFaultCount', ctypes.c_uint32),
                ('PeakWorkingSetSize', ctypes.c_size_t), ('WorkingSetSize', ctypes.c_size_t),
                ('QuotaPeakPagedPoolUsage', ctypes.c_size_t), ('QuotaPagedPoolUsage', ctypes.c_size_t),
                ('QuotaPeakNonPagedPoolUsage', ctypes.c_size_t), ('QuotaNonPagedPoolUsage', ctypes.c_size_t),
                ('PagefileUsage', ctypes.c_size_t), ('PeakPagefileUsage', ctypes.c_size_t)]


_WINDOWS_COUNTERS_POINTER = ctypes.POINTER(_WindowsCounters)
_WINDOWS_RSS_READER = None


def current_rss_bytes():
    """Current resident/working-set bytes, NEVER process high-water or Python heap."""
    global _WINDOWS_RSS_READER
    try:
        if sys.platform == 'win32':
            if _WINDOWS_RSS_READER is None:
                kernel = ctypes.WinDLL('kernel32', use_last_error=True)
                psapi = ctypes.WinDLL('psapi', use_last_error=True)
                kernel.GetCurrentProcess.restype = ctypes.c_void_p
                psapi.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, _WINDOWS_COUNTERS_POINTER, ctypes.c_uint32]
                psapi.GetProcessMemoryInfo.restype = ctypes.c_int
                _WINDOWS_RSS_READER = (psapi.GetProcessMemoryInfo, kernel.GetCurrentProcess())
            read, handle = _WINDOWS_RSS_READER
            value = _WindowsCounters()
            value.cb = ctypes.sizeof(value)
            if read(handle, ctypes.byref(value), value.cb):
                return int(value.WorkingSetSize)
        elif sys.platform.startswith('linux'):
            with open('/proc/self/statm', encoding='ascii') as source:
                resident = int(source.read(128).split()[1])
            return resident * os.sysconf('SC_PAGE_SIZE')
        elif sys.platform == 'darwin':
            lib = ctypes.CDLL('/usr/lib/libproc.dylib')
            lib.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
            lib.proc_pidinfo.restype = ctypes.c_int
            value = (ctypes.c_uint64 * 12)()
            if lib.proc_pidinfo(os.getpid(), 4, 0, value, ctypes.sizeof(value)) == ctypes.sizeof(value):
                return int(value[1])
    except (OSError, AttributeError, ValueError, IndexError):
        pass
    return None


class ResourceSampler:
    def __init__(self, interval):
        self.interval = interval
        self.samples = []
        self.stopped = asyncio.Event()
        self.limit_exceeded = False
        self.rss_error_classes = set()
        self.started = time.monotonic()
        self.cpu_started = time.process_time()

    def sample(self):
        if len(self.samples) >= MAX_RESOURCE_SAMPLES:
            self.limit_exceeded = True
            return
        try:
            rss = current_rss_bytes()
        except Exception as exc:
            # A broken sampler must neither leak the RPC nor mask its exception.
            self.rss_error_classes.add(type(exc).__name__)
            rss = None
        self.samples.append({'elapsed_seconds': time.monotonic() - self.started,
                             'process_cpu_seconds': time.process_time() - self.cpu_started,
                             'current_rss_bytes': rss})

    async def watch(self):
        while not self.stopped.is_set():
            try:
                await asyncio.wait_for(self.stopped.wait(), self.interval)
            except TimeoutError:
                self.sample()

    def summary(self, pipeline_seconds=None):
        rss = [item['current_rss_bytes'] for item in self.samples
               if type(item['current_rss_bytes']) is int and item['current_rss_bytes'] > 0]
        if pipeline_seconds is None:
            pipeline_seconds = self.samples[-1]['elapsed_seconds'] if self.samples else 0
        coverage = resource_coverage(self.samples, self.interval, pipeline_seconds)
        return {'samples': self.samples, 'sample_limit_exceeded': self.limit_exceeded,
                **coverage,
                'profiling_status': ('covered' if coverage['coverage_ok'] and not self.limit_exceeded
                                     and len(rss) == len(self.samples) else 'inconclusive'),
                'rss_error_classes': sorted(self.rss_error_classes),
                'all_rss_available': bool(self.samples) and len(rss) == len(self.samples),
                'rss_first_bytes': self.samples[0]['current_rss_bytes'] if self.samples else None,
                'rss_last_bytes': self.samples[-1]['current_rss_bytes'] if self.samples else None,
                'rss_sampled_max_bytes': max(rss) if rss else None,
                'scope': 'Current process resident/working-set bytes, including native libraries. Sampled maximum is NOT lifetime high-water or exact peak. Not Python heap or total system/virtual memory.'}


async def audio_source(path, info, consumed, encoded, progress, started):
    pcm_sha, ogg_sha = hashlib.sha256(), hashlib.sha256()
    async def pcm():
        try:
            async with aclosing(WavAudio(path, chunk_ms=20, realtime=True).chunks()) as source:
                async for block in source:
                    if consumed['bytes'] + len(block) > MAX_PCM_BYTES:
                        raise ValueError('PCM stream byte limit exceeded')
                    consumed['bytes'] += len(block)
                    pcm_sha.update(block)
                    now = time.monotonic() - started
                    progress.setdefault('first_pcm_yield_seconds', now)
                    if consumed['bytes'] >= info['pcm']['bytes'] / 2:
                        progress.setdefault('half_pcm_yield_seconds', now)
                    progress['last_pcm_yield_seconds'] = now
                    yield block
            consumed['complete'] = True
            progress['pcm_source_eof_seconds'] = time.monotonic() - started
        finally:
            consumed['sha256'] = pcm_sha.hexdigest()
    try:
        async with aclosing(pcm16_to_ogg(pcm(), page_duration_ms=20)) as source:
            async for block in source:
                if encoded['bytes'] + len(block) > MAX_FILE_BYTES:
                    raise ValueError('encoded stream byte limit exceeded')
                encoded['bytes'] += len(block)
                ogg_sha.update(block)
                progress.setdefault('first_ogg_yield_seconds', time.monotonic() - started)
                progress['last_ogg_yield_seconds'] = time.monotonic() - started
                yield block
        encoded['complete'] = True
        progress['encoded_source_eof_seconds'] = time.monotonic() - started
    finally:
        encoded['sha256'] = ogg_sha.hexdigest()


def _text(value, include):
    return value if value is None or include else digest(value.encode('utf-8'))


def summarize_update(response, include):
    hypotheses = []
    for hypothesis in response.hypotheses:
        edit = hypothesis.voice_edit
        item = {'text': _text(hypothesis.text, include), 'confidence': hypothesis.confidence,
                'voice_edit': None, 'raw_summary': digest(hypothesis.raw)}
        if isinstance(item['confidence'], float) and not math.isfinite(item['confidence']):
            item['confidence'] = {'nonfinite_float': repr(item['confidence'])}
        if edit is not None:
            context = edit.edited_context
            item['voice_edit'] = {'intent': edit.intent, 'active_field': edit.active_field,
                'transcript': _text(edit.transcript, include), 'edited_text': _text(edit.edited_text, include),
                'alternative_text': _text(edit.alternative_text, include), 'raw_summary': digest(edit.raw),
                'edited_context': None if context is None else {
                    **{key: _text(getattr(context, key), include) for key in ('before', 'selected', 'after')},
                    'cursor_aware': context.cursor_aware}}
        hypotheses.append(item)
    return {'status': response.status, 'error_code': response.error_code,
            'recognition_state': response.recognition_state, 'raw_summary': digest(response.raw),
            'hypotheses': hypotheses,
            'retention_note': 'Ordered known semantics; raw bytes/unknown values/server IDs retained only in bounded in-memory responses, not published in this report.'}


def semantic_signature(updates):
    """All ordered final sets, never scores, raw bytes or transport timing."""
    def project(value):
        if isinstance(value, dict):
            return {key: project(item) for key, item in value.items()
                    if key not in ('raw_summary', 'confidence', 'status', 'error_code', 'retention_note')}
        if isinstance(value, list):
            return [project(item) for item in value]
        return value
    payload = [project(summarize_update(response, False)) for response in updates]
    return {'sha256': digest(json.dumps(payload, sort_keys=True, separators=(',', ':'),
                                       allow_nan=False).encode('utf-8'))['sha256'],
            'policy': 'all_ordered_state_1_sets_no_dedup_scores_excluded_v1',
            'update_count': len(updates), 'hypothesis_counts': [len(response.hypotheses) for response in updates]}


def marker_observations(updates, context, markers):
    observations = {phase: [] for phase in PHASES}
    ambiguous = []
    for index, response in enumerate(updates):
        try:
            text = DictationResult((response,), context).text
        except AmbiguousResultError:
            ambiguous.append(index)
            continue
        normalized = normalize(text)
        for phase in PHASES:
            match = re.search(r'(?<!\w)' + re.escape(normalize(markers[phase])) + r'(?!\w)', normalized)
            if match is not None:
                observations[phase].append({'update_index': index, 'normalized_codepoint_span': list(match.span())})
    return {'observations': observations, 'all_observed': all(observations.values()),
            'ambiguous_update_indices': ambiguous,
            'scope': 'First normalized occurrence per marker per independently renderable final update. No update concatenation/dedup; spans are normalized Unicode codepoints, not original-text offsets or acoustic times. Coverage alone is not quality proof.'}


async def run_session(case, args, engine, info, *, row=None):
    row = {} if row is None else row
    started, cpu = time.monotonic(), time.process_time()
    consumed, encoded = {'bytes': 0, 'complete': False}, {'bytes': 0, 'complete': False}
    progress, events, updates = {}, {'responses': 0, 'final_updates': 0}, []
    row.update(status='failed', transport='failed_or_unverified', consumed_source=consumed,
               sent_ogg=encoded, progress=progress, events=events)
    context = EditingContext(**case.get('context', {}))
    retained_bytes = 0
    class ObservedEngine:
        async def stream(self, source, **kwargs):
            nonlocal retained_bytes
            async with aclosing(engine.stream(source, **kwargs)) as stream:
                async for event in stream:
                    now = time.monotonic() - started
                    response = event.response
                    if response is None:
                        events['engine_eof_seconds'] = now
                        row['engine_metrics'] = {
                            key: getattr(event.metrics, key) for key in (
                                'requests_produced', 'request_protobuf_bytes', 'responses_received',
                                'response_protobuf_bytes', 'elapsed_seconds', 'first_response_seconds', 'eof_seconds')}
                    else:
                        events['responses'] += 1
                        events.setdefault('first_response_seconds', now)
                        if response.status == 1:
                            events.setdefault('s3_end_seconds', now)
                        if response.recognition_state == 1:
                            if len(updates) >= MAX_FINAL_UPDATES or retained_bytes + len(response.raw) > MAX_FINAL_BYTES:
                                raise ResultLimitError('final response retention exceeded')
                            updates.append(response)
                            retained_bytes += len(response.raw)
                            events['final_updates'] += 1
                            events.setdefault('first_final_seconds', now)
                    yield event
    sampler = ResourceSampler(args.sample_interval)
    sampler.sample()
    watcher = asyncio.create_task(sampler.watch())
    try:
        source = audio_source(case['audio'], info, consumed, encoded, progress, started)
        async with asyncio.timeout(args.timeout), aclosing(source):
            result = await collect_dictation(ObservedEngine(), source,
                config=ProtocolConfig(language=case['language']), context=context, timeout=args.timeout,
                max_final_updates=MAX_FINAL_UPDATES, max_result_bytes=MAX_FINAL_BYTES)
        row['transport'] = 'success'
        row['source_integrity'] = (consumed.get('complete') and encoded.get('complete')
                                   and {key: consumed.get(key) for key in ('sha256', 'bytes')} == info['pcm'])
        row['markers'] = marker_observations(result.final_updates, result.context, case['markers'])
        row['status'] = 'healthy' if (row['source_integrity'] and updates
            and row['markers']['all_observed'] and not row['markers']['ambiguous_update_indices']) else 'inconclusive'
        if not row['source_integrity']:
            row.update(status='failed', error_class='AudioIntegrityError')
    except asyncio.CancelledError:
        row.update(error_class='CancelledError', cancelled=True)
        raise
    except Exception as exc:
        row['error_class'] = type(exc).__name__
        if isinstance(exc, grpc.RpcError):
            code = exc.code()
            row['grpc_code'] = code.name if isinstance(code, grpc.StatusCode) else None
    finally:
        sampler.stopped.set()
        await watcher
        sampler.sample()
        row['pipeline_wall_seconds'] = time.monotonic() - started
        row['pipeline_process_cpu_seconds'] = time.process_time() - cpu
        row['pipeline_scope'] = ('Collection, rolling source hashes, native encoding, engine, marker observations, resource sampling and session cleanup; excludes preinspection/import/channel open, post-file verification and report serialization. No isolated server-compute estimate.')
        row['resources'] = sampler.summary(row['pipeline_wall_seconds'])
        row['retained_final_protobuf_bytes'] = retained_bytes
        row['semantic_signature'] = semantic_signature(updates)
        row['final_updates'] = [summarize_update(response, args.include_text) for response in updates]
        row['profiling_inconclusive'] = row['resources']['profiling_status'] != 'covered'
        if row['status'] == 'healthy' and row['profiling_inconclusive']:
            row['status'] = 'inconclusive'
    # File rehash is verification, deliberately outside pipeline CPU/wall.
    verify_wall, verify_cpu = time.monotonic(), time.process_time()
    try:
        row['file_unchanged'] = file_digest(case['audio']) == info['file']
    except Exception as exc:
        row.update(file_unchanged=False, status='failed', error_class=type(exc).__name__)
    row['verification_wall_seconds'] = time.monotonic() - verify_wall
    row['verification_process_cpu_seconds'] = time.process_time() - verify_cpu
    if not row['file_unchanged']:
        row.update(status='failed', error_class='AudioFileChanged')
    return row


def provenance():
    modules = {}
    for name in ('audio', 'encoding', 'engine', 'protocol', 'results', 'transport', 'wire'):
        module = importlib.import_module('rambler.' + name)
        modules[name] = digest(Path(module.__file__).read_bytes())['sha256']
    return {'python': platform.python_version(), 'os': platform.platform(),
            'grpcio': grpc.__version__, 'pyav': importlib.metadata.version('av'),
            'module_sha256': modules, 'tool_sha256': digest(Path(__file__).read_bytes())['sha256']}


def save_report(output, report):
    text = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + '\n'
    if len(text.encode('utf-8')) > MAX_REPORT_BYTES:
        raise ValueError('report exceeds output byte limit')
    output.seek(0)
    output.write(text)
    output.truncate()
    output.flush()


async def run(args, case, info, manifest_summary, predecessor, output):
    prepared_wall, prepared_cpu = time.monotonic(), time.process_time()
    av = importlib.import_module('av')
    captured_context = EditingContext(**case.get('context', {}))
    report = {'report_kind': 'rambler_endurance_v1', 'schema_version': 1,
              'provenance': provenance(), 'manifest_summary': manifest_summary,
              'previous_report_summary': predecessor, 'audio': info,
              'config': {'stage': args.stage, 'sessions': args.sessions, 'concurrency': 1,
                         'target_kind': 'loopback' if loopback(args.target) else 'live',
                         'secure': not args.insecure,
                         'context': {**{key: _text(getattr(captured_context, key), args.include_text)
                                         for key in ('before', 'selected', 'after')},
                                     'cursor_aware': captured_context.cursor_aware},
                         'timeout_seconds': args.timeout, 'sample_interval_seconds': args.sample_interval,
                         'pcm_chunk_ms': 20, 'ogg_page_ms': 20, 'language': case['language'],
                         'encoding_profile': dict(ENCODING_PROFILE),
                         'case_id': case['id'], 'markers': {phase: _text(case['markers'][phase], args.include_text) for phase in PHASES},
                         'include_text': args.include_text, 'max_final_updates': MAX_FINAL_UPDATES,
                         'max_final_protobuf_bytes': MAX_FINAL_BYTES, 'max_resource_samples': MAX_RESOURCE_SAMPLES,
                         'max_pcm_bytes': MAX_PCM_BYTES, 'max_encoded_bytes': MAX_FILE_BYTES},
              'preparation': {'pyav_version': av.__version__,
                              'wall_seconds': time.monotonic() - prepared_wall,
                              'process_cpu_seconds': time.process_time() - prepared_cpu,
                              'scope': 'PyAV import and provenance only; WAV inspection precedes this. Encoder setup is per session, not separately isolated.'},
              'summary': {'status': 'inconclusive', 'requested': args.sessions, 'completed': 0},
              'sessions': [], 'notes': [
                  'Sequential sessions, no retries; stop after the first non-healthy outcome.',
                  'Stage gate checks complete local predecessor evidence and compatible target/profile, not an authenticated service attestation or anti-forgery proof.',
                  'RSS profiling requires bounded sample gaps, covered endpoints and at least 80% of nominal interval observations; otherwise profiling is inconclusive even if transport succeeds.',
                  'WAV pacing is at chunk starts, not microphone completion; EOF timestamps are producer observations, not network writes.',
                  'CPU includes codec, hashing, sampler and engine; no network/server compute isolation. RSS includes native memory, but not all address-space/remote memory.',
                  'Only bounded finals and resource samples retained; provisional events counted, not stored. All final updates remain ordered without deduplication.',
                  'Repeated sessions must have identical ordered known final semantics; confidence/raw IDs excluded. A difference is inconclusive, not a transport failure.',
                  'Preinspection reads the complete WAV and may warm OS file caches; this is not a cold-start benchmark.',
                  'Text opt-in requires controlled-corpus review. Raw protobuf/unknown values/server IDs are never included in this report.',
                  'Marker observation is not a quality score; repeated-session RSS observations do not prove absence of leaks.' ]}
    save_report(output, report)
    transport = GrpcTransport(args.target, secure=not args.insecure)
    engine = RamblerEngine(transport, None if loopback(args.target) else EnvironmentCredentials())
    try:
        opened = time.monotonic()
        await engine.open(timeout=min(args.timeout, 10))
        report['channel_open_seconds'] = time.monotonic() - opened
        for index in range(args.sessions):
            row = {'index': index}
            report['sessions'].append(row)
            await run_session(case, args, engine, info, row=row)
            if index and row['status'] == 'healthy':
                row['same_ordered_final_semantics_as_first'] = (
                    row['semantic_signature'] == report['sessions'][0]['semantic_signature'])
                if not row['same_ordered_final_semantics_as_first']:
                    row.update(status='inconclusive', semantic_error='OrderedFinalHistoryMismatch')
            report['summary']['completed'] += 1
            save_report(output, report)
            if row['status'] != 'healthy':
                break
        report['summary']['status'] = ('healthy' if len(report['sessions']) == args.sessions
            and all(row['status'] == 'healthy' for row in report['sessions'])
            else 'failed' if any(row['status'] == 'failed' for row in report['sessions']) else 'inconclusive')
    except asyncio.CancelledError:
        report['cancelled'] = True
        report['summary']['status'] = 'failed'
        raise
    except Exception as exc:
        report['error_class'] = type(exc).__name__
        report['summary']['status'] = 'failed'
        if isinstance(exc, grpc.RpcError):
            code = exc.code()
            report['grpc_code'] = code.name if isinstance(code, grpc.StatusCode) else None
    finally:
        try:
            await engine.close()
        except BaseException as exc:
            report['summary']['status'] = 'failed'
            report['engine_cleanup_error_class'] = type(exc).__name__
            raise
        finally:
            try:
                await transport.close()
            except BaseException as exc:
                report['summary']['status'] = 'failed'
                report['transport_cleanup_error_class'] = type(exc).__name__
                raise
            finally:
                save_report(output, report)
    return report['summary']['status'] == 'healthy'


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument('--manifest', type=Path, required=True)
    result.add_argument('--id', required=True)
    result.add_argument('--output', type=Path, required=True)
    result.add_argument('--stage', choices=('smoke', '120', '300'), default='120')
    result.add_argument('--previous-report', type=Path)
    result.add_argument('--sessions', type=int, default=1)
    result.add_argument('--timeout', type=float, default=180)
    result.add_argument('--sample-interval', type=float, default=1)
    result.add_argument('--include-text', action='store_true')
    result.add_argument('--target', default=DEFAULT_TARGET)
    result.add_argument('--live', action='store_true')
    result.add_argument('--insecure', action='store_true')
    return result


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        # Reject non-loopback permission mistakes before inspecting any audio.
        if not loopback(args.target) and not args.live:
            raise ValueError('non-loopback requires --live')
        case, manifest_summary = load_case(args.manifest, args.id)
        info = inspect_wav(case['audio'])
        predecessor = validate_plan(args, info, case=case)
        with args.output.open('x', encoding='utf-8', newline='\n') as output:
            return 0 if asyncio.run(run(args, case, info, manifest_summary, predecessor, output)) else 1
    except KeyboardInterrupt:
        return 130
    except Exception as exc:
        print(json.dumps({'error_class': type(exc).__name__, 'message': 'endurance validation failed'}))
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
