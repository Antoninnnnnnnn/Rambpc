"""Short offline/loopback contracts, not long or real-service endurance runs."""
import asyncio
from contextlib import asynccontextmanager
from dataclasses import replace
import hashlib
import io
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace
import wave

import grpc
import pytest

SPEC = importlib.util.spec_from_file_location('validate_endurance', Path(__file__).parents[1] / 'tools/validate_endurance.py')
endurance = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(endurance)
from rambler.engine import RamblerEngine
from rambler.protocol import EditingContext, decode_response
from rambler.transport import DEFAULT_METHOD, GrpcTransport
from rambler.wire import bytes_field, string_field, varint_field

MARKERS = {'beginning': 'amber orchard', 'middle': 'cobalt lantern', 'end': 'violet harbor'}
TEXT = 'Amber orchard. Cobalt lantern! Violet harbor.'


def arguments(**overrides):
    values = dict(stage='smoke', sessions=1, timeout=15, sample_interval=1,
                  include_text=False, target='127.0.0.1:1', live=False,
                  insecure=True, previous_report=None)
    values.update(overrides)
    return SimpleNamespace(**values)


def wav_file(tmp_path, frames=1600):
    path = tmp_path / 'control.wav'
    with wave.open(str(path), 'wb') as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(16000)
        output.writeframes(b'\x02\x01' * frames)
    return path


def case_for(path):
    return {'id': 'control', 'audio': path, 'language': 'en-US', 'markers': dict(MARKERS)}


def final(text=TEXT, count=1):
    hypothesis = string_field(1, text)
    recognition = varint_field(1, 1) + bytes_field(3, b''.join(bytes_field(3, hypothesis) for _ in range(count)))
    # Unknown bytes intentionally contain a fake sensitive value.
    return bytes_field(1253625, bytes_field(1, recognition)) + string_field(99, 'fake-private-opaque')


@asynccontextmanager
async def server(handler):
    service, method = DEFAULT_METHOD[1:].split('/')
    instance = grpc.aio.server()
    instance.add_generic_rpc_handlers((grpc.method_handlers_generic_handler(service, {
        method: grpc.stream_stream_rpc_method_handler(handler, request_deserializer=lambda value: value,
                                                      response_serializer=lambda value: value)
    }),))
    port = instance.add_insecure_port('127.0.0.1:0')
    await instance.start()
    transport = GrpcTransport(f'127.0.0.1:{port}', secure=False)
    await transport.open()
    engine = RamblerEngine(transport)
    try:
        yield engine, transport, port
    finally:
        await engine.close()
        await transport.close()
        await instance.stop(None)


def predecessor_report(*, include_text=False):
    duration, pipeline = 118.0, 118.5
    sampler = endurance.ResourceSampler(1)
    sampler.samples = [{'elapsed_seconds': float(index), 'process_cpu_seconds': index / 1000,
                        'current_rss_bytes': 1000000 + index} for index in range(119)]
    sampler.samples.append({'elapsed_seconds': pipeline, 'process_cpu_seconds': .12, 'current_rss_bytes': 1000119})
    response = decode_response(final())
    pcm = {'bytes': 118 * 32000, 'sha256': '1' * 64}
    row = {'index': 0, 'status': 'healthy', 'transport': 'success', 'source_integrity': True,
           'file_unchanged': True, 'consumed_source': {**pcm, 'complete': True},
           'sent_ogg': {'bytes': 300000, 'sha256': '3' * 64, 'complete': True},
           'pipeline_wall_seconds': pipeline, 'pipeline_process_cpu_seconds': .12,
           'progress': {'pcm_source_eof_seconds': 117.98, 'encoded_source_eof_seconds': 118.0},
           'events': {'responses': 2, 'final_updates': 1, 'engine_eof_seconds': 118.4, 's3_end_seconds': 118.2},
           'engine_metrics': {'responses_received': 2, 'eof_seconds': 118.3},
           'final_updates': [endurance.summarize_update(response, include_text)],
           'semantic_signature': endurance.semantic_signature([response]),
           'markers': endurance.marker_observations([response], EditingContext(), MARKERS),
           'resources': sampler.summary(pipeline)}
    return {'report_kind': 'rambler_endurance_v1', 'schema_version': 1,
            'config': {'stage': '120', 'sessions': 1, 'target_kind': 'live', 'secure': True,
                       'language': 'en-US', 'pcm_chunk_ms': 20, 'ogg_page_ms': 20,
                       'encoding_profile': dict(endurance.ENCODING_PROFILE), 'sample_interval_seconds': 1,
                       'markers': {phase: endurance._text(text, include_text) for phase, text in MARKERS.items()}},
            'audio': {'duration_seconds': duration, 'sample_rate': 16000, 'channels': 1, 'sample_width': 2,
                      'pcm': pcm, 'file': {'bytes': pcm['bytes'] + 44, 'sha256': '2' * 64}},
            'summary': {'status': 'healthy', 'requested': 1, 'completed': 1}, 'sessions': [row]}


def test_manifest_relative_paths_markers_and_preinspection_hash(tmp_path):
    path = wav_file(tmp_path)
    manifest = tmp_path / 'manifest.json'
    original = json.dumps({'cases': [{**case_for('control.wav'), 'context': {'before': ''}}]}).encode()
    manifest.write_bytes(original)
    case, source = endurance.load_case(manifest, 'control')
    assert case['audio'] == path.resolve()
    assert source['sha256'] == hashlib.sha256(original).hexdigest()
    info = endurance.inspect_wav(path)
    assert info['pcm'] == {'bytes': 3200, 'sha256': hashlib.sha256(b'\x02\x01' * 1600).hexdigest()}
    assert info['duration_seconds'] == .1
    for markers in ({'beginning': 'a'}, {**MARKERS, 'middle': 'AMBER ORCHARD!'}):
        manifest.write_text(json.dumps({'cases': [{**case_for('control.wav'), 'markers': markers}]}))
        with pytest.raises(ValueError):
            endurance.load_case(manifest, 'control')
    manifest.write_text('{"cases":[],"cases":[]}')
    with pytest.raises(ValueError):
        endurance.load_case(manifest, 'control')


@pytest.mark.parametrize('changes', [dict(sessions=0), dict(sessions=4), dict(timeout=float('nan')),
    dict(timeout=361), dict(timeout=5), dict(sample_interval=.5), dict(sample_interval=11),
    dict(stage='smoke', target='example.invalid:443', live=True, insecure=False),
    dict(target='example.invalid:443'), dict(target='example.invalid:443', live=True)])
def test_plan_limits_before_streaming(changes):
    with pytest.raises(ValueError):
        endurance.validate_plan(arguments(**changes), {'duration_seconds': .1})


@pytest.mark.parametrize('include_text', [False, True])
def test_stage_gate_and_no_cut_or_padding(tmp_path, include_text):
    assert endurance.validate_plan(arguments(stage='120', timeout=180), {'duration_seconds': 118.682375}) is None
    args = arguments(stage='300', timeout=350, target='speech.invalid:443', live=True, insecure=False)
    case = case_for(tmp_path / 'unused.wav')
    with pytest.raises(ValueError):
        endurance.validate_plan(args, {'duration_seconds': 288.3515625}, case=case)
    predecessor = tmp_path / 'previous.json'
    report = predecessor_report(include_text=include_text)
    predecessor.write_text(json.dumps(report))
    args.previous_report = predecessor
    assert endurance.validate_plan(args, {'duration_seconds': 288.3515625}, case=case) == endurance.digest(predecessor.read_bytes())
    report['sessions'][0]['status'] = 'inconclusive'
    predecessor.write_text(json.dumps(report))
    with pytest.raises(ValueError):
        endurance.validate_plan(args, {'duration_seconds': 288.3515625}, case=case)
    with pytest.raises(ValueError):
        endurance.validate_plan(arguments(stage='120'), {'duration_seconds': 100})


@pytest.mark.parametrize('path,value', [
    (('config', 'target_kind'), 'loopback'),
    (('config', 'language'), 'fr-FR'),
    (('config', 'pcm_chunk_ms'), 40),
    (('config', 'ogg_page_ms'), 100),
    (('config', 'encoding_profile', 'bitrate'), 48000),
    (('config', 'encoding_profile'), '__delete__'),
    (('config', 'sessions'), 4),
    (('config', 'sessions'), True),
    (('summary', 'requested'), '__delete__'),
    (('summary', 'completed'), '__delete__'),
    (('summary', 'completed'), 0),
    (('sessions',), []),
    (('audio', 'duration_seconds'), 40),
    (('audio', 'pcm', 'sha256'), 'corrupted'),
    (('sessions', 0, 'consumed_source'), '__delete__'),
    (('sessions', 0, 'consumed_source', 'sha256'), '9' * 64),
    (('sessions', 0, 'consumed_source', 'bytes'), 123),
    (('sessions', 0, 'consumed_source', 'complete'), False),
    (('sessions', 0, 'sent_ogg', 'complete'), False),
    (('sessions', 0, 'file_unchanged'), False),
    (('sessions', 0, 'source_integrity'), False),
    (('sessions', 0, 'pipeline_wall_seconds'), .1),
    (('sessions', 0, 'progress', 'pcm_source_eof_seconds'), .1),
    (('sessions', 0, 'semantic_signature', 'update_count'), 2),
    (('sessions', 0, 'markers', 'all_observed'), False),
    (('sessions', 0, 'markers', 'observations', 'end'), []),
    (('sessions', 0, 'events', 'engine_eof_seconds'), '__delete__'),
    (('sessions', 0, 'events', 's3_end_seconds'), '__delete__'),
    (('sessions', 0, 'engine_metrics', 'eof_seconds'), None),
    (('sessions', 0, 'resources', 'samples'), [
        {'elapsed_seconds': 0, 'current_rss_bytes': 1000},
        {'elapsed_seconds': 118.5, 'current_rss_bytes': 1000}]),
    (('cancelled',), True),
    (('engine_cleanup_error_class',), 'RuntimeError'),
])
def test_incomplete_changed_or_corrupted_predecessor_rejected(path, value):
    report = predecessor_report()
    parent = report
    for key in path[:-1]:
        parent = parent[key]
    if value == '__delete__':
        del parent[path[-1]]
    else:
        parent[path[-1]] = value
    args = arguments(stage='300', timeout=350, target='speech.invalid:443', live=True, insecure=False)
    assert not endurance.healthy_predecessor(report, args, case_for(Path('unused.wav')))


def test_minimal_claimed_healthy_and_loopback_to_live_rejected():
    minimal = {'report_kind': 'rambler_endurance_v1', 'config': {'stage': '120'},
               'summary': {'status': 'healthy'}, 'sessions': [{'status': 'healthy'}]}
    live = arguments(stage='300', timeout=350, target='speech.invalid:443', live=True, insecure=False)
    assert not endurance.healthy_predecessor(minimal, live, case_for(Path('unused.wav')))
    report = predecessor_report()
    report['config'].update(target_kind='loopback', secure=False)
    assert endurance.healthy_predecessor(report, arguments(stage='300', timeout=350), case_for(Path('unused.wav')))
    assert not endurance.healthy_predecessor(report, live, case_for(Path('unused.wav')))


def test_predecessor_repetitions_all_must_have_completed_consistent_evidence():
    report = predecessor_report()
    report['config']['sessions'] = 3
    report['summary'].update(requested=3, completed=3)
    report['sessions'] = [json.loads(json.dumps(report['sessions'][0])) for _ in range(3)]
    for index, row in enumerate(report['sessions']):
        row['index'] = index
    live = arguments(stage='300', timeout=350, target='speech.invalid:443', live=True, insecure=False)
    case = case_for(Path('unused.wav'))
    assert endurance.healthy_predecessor(report, live, case)
    report['sessions'][2]['semantic_signature']['sha256'] = '9' * 64
    assert not endurance.healthy_predecessor(report, live, case)


@pytest.mark.parametrize('times,interval,duration,expected', [
    ([float(i) for i in range(119)], 1, 118, True),
    ([0, 118], 1, 118, False),  # Event loop blocked for the whole audio.
    ([float(i) for i in range(0, 119, 2)], 1, 118, False),  # Gaps <=3s, but half the samples missing.
    ([float(i) for i in range(10, 119)], 1, 118, False),
    ([float(i) for i in range(101)], 1, 118, False),
    ([0, 1, .5, 2], 1, 2, False),
    ([0, float('nan')], 1, 1, False),
    ([0, .1], 1, .1, True),
    ([float(i) for i in range(0, 111, 10)] + [118], 10, 118, True),
])
def test_rss_coverage_requires_time_gaps_counts_and_endpoints(times, interval, duration, expected):
    samples = [{'elapsed_seconds': value, 'process_cpu_seconds': 0, 'current_rss_bytes': 1000} for value in times]
    assert endurance.resource_coverage(samples, interval, duration)['coverage_ok'] is expected
    sampler = endurance.ResourceSampler(interval)
    sampler.samples = samples
    summary = sampler.summary(duration)
    assert summary['all_rss_available']
    assert summary['coverage_ok'] is expected
    assert summary['profiling_status'] == ('covered' if expected else 'inconclusive')


def test_markers_per_update_without_concatenation_or_alternative_selection():
    context = EditingContext()
    updates = [decode_response(final(MARKERS[phase])) for phase in endurance.PHASES]
    result = endurance.marker_observations(updates, context, MARKERS)
    assert result['all_observed'] and not result['ambiguous_update_indices']
    assert [result['observations'][phase][0]['update_index'] for phase in endurance.PHASES] == [0, 1, 2]
    assert result['observations']['beginning'][0]['normalized_codepoint_span'] == [0, 13]
    split = endurance.marker_observations([decode_response(final('amber')), decode_response(final('orchard'))], context, MARKERS)
    assert not split['observations']['beginning'] and not split['all_observed']
    alternatives = endurance.marker_observations([decode_response(final(count=2))], context, MARKERS)
    assert alternatives['ambiguous_update_indices'] == [0] and not alternatives['all_observed']
    assert not endurance.marker_observations([decode_response(final('unamber orchard'))], context, MARKERS)['all_observed']
    assert endurance.normalize(' AMBER,\nORCHARD! ') == 'amber orchard'


def test_ordered_final_signature_and_privacy_projection():
    a, b = decode_response(final('A')), decode_response(final('B'))
    assert endurance.semantic_signature([a, b]) != endurance.semantic_signature([b])
    assert endurance.semantic_signature([a, b]) != endurance.semantic_signature([b, a])
    assert endurance.semantic_signature([a, a]) != endurance.semantic_signature([a])
    result = a.result
    recognition = result.recognition
    hypotheses = recognition.hypothesis_result
    changed = replace(a, result=replace(result, recognition=replace(recognition,
        hypothesis_result=replace(hypotheses, hypotheses=(replace(a.hypotheses[0], confidence=.9),)))))
    assert endurance.semantic_signature([a]) == endurance.semantic_signature([changed])
    projected = endurance.summarize_update(decode_response(final(count=2)), False)
    assert len(projected['hypotheses']) == 2
    text = json.dumps(projected)
    assert TEXT not in text and 'fake-private-opaque' not in text and 'hex' not in text
    assert endurance.summarize_update(a, True)['hypotheses'][0]['text'] == 'A'


def test_rss_current_primitive_and_bounded_sampler(monkeypatch):
    actual = endurance.current_rss_bytes()
    assert actual is None or actual > 0
    monkeypatch.setattr(endurance, 'current_rss_bytes', lambda: 123456)
    sampler = endurance.ResourceSampler(1)
    for _ in range(endurance.MAX_RESOURCE_SAMPLES + 3):
        sampler.sample()
    summary = sampler.summary()
    assert len(summary['samples']) == endurance.MAX_RESOURCE_SAMPLES
    assert summary['sample_limit_exceeded'] and summary['rss_sampled_max_bytes'] == 123456
    assert summary['rss_first_bytes'] == summary['rss_last_bytes'] == 123456
    assert 'NOT lifetime high-water' in summary['scope']


def test_windows_current_working_set_is_not_peak(monkeypatch):
    class Function:
        def __init__(self, function): self.function = function
        def __call__(self, *args): return self.function(*args)
    def counters(handle, pointer, size):
        pointer._obj.PeakWorkingSetSize = 999999
        pointer._obj.WorkingSetSize = 123456
        return True
    kernel = SimpleNamespace(GetCurrentProcess=Function(lambda: 1))
    psapi = SimpleNamespace(GetProcessMemoryInfo=Function(counters))
    dll_calls = []
    def win_dll(name, **kwargs):
        dll_calls.append(name)
        return kernel if name == 'kernel32' else psapi
    monkeypatch.setattr(endurance.sys, 'platform', 'win32')
    monkeypatch.setattr(endurance, '_WINDOWS_RSS_READER', None)
    monkeypatch.setattr(endurance.ctypes, 'WinDLL', win_dll, raising=False)
    assert endurance.current_rss_bytes() == 123456
    assert endurance.current_rss_bytes() == 123456
    assert dll_calls == ['kernel32', 'psapi']


def test_report_budget_checked_before_overwriting_previous_observation(monkeypatch):
    output = io.StringIO('previous snapshot')
    monkeypatch.setattr(endurance, 'MAX_REPORT_BYTES', 8)
    with pytest.raises(ValueError):
        endurance.save_report(output, {'large': 'value'})
    assert output.getvalue() == 'previous snapshot'


async def test_native_loopback_progressive_integrity_and_cleanup(tmp_path, monkeypatch):
    pytest.importorskip('av')
    path = wav_file(tmp_path)
    info = endurance.inspect_wav(path)
    calls = []
    async def handler(requests, context):
        calls.append([request async for request in requests])
        yield final()
        yield b'\x08\x01'
    def forbidden(*args, **kwargs):
        raise AssertionError('encoder subprocess forbidden')
    monkeypatch.setattr(endurance.asyncio, 'create_subprocess_exec', forbidden)
    async with server(handler) as (engine, transport, _):
        row = await endurance.run_session(case_for(path), arguments(), engine, info)
        assert row['status'] == 'healthy' and row['transport'] == 'success'
        assert row['consumed_source']['sha256'] == info['pcm']['sha256']
        assert row['consumed_source']['bytes'] == info['pcm']['bytes']
        assert row['consumed_source']['complete'] and row['sent_ogg']['complete'] and row['file_unchanged']
        assert row['markers']['all_observed']
        assert row['semantic_signature']['update_count'] == 1
        assert len(row['resources']['samples']) >= 2 and row['resources']['all_rss_available']
        assert row['progress']['first_pcm_yield_seconds'] <= row['progress']['half_pcm_yield_seconds'] <= row['progress']['pcm_source_eof_seconds']
        assert row['pipeline_process_cpu_seconds'] >= 0 and row['verification_process_cpu_seconds'] >= 0
        assert row['progress']['pcm_source_eof_seconds'] <= row['progress']['encoded_source_eof_seconds'] <= row['events']['engine_eof_seconds']
        assert not engine._sessions and transport._channel is not None
        assert 'fake-private-opaque' not in json.dumps(row) and TEXT not in json.dumps(row)
    assert len(calls) == 1


async def test_missing_rss_coverage_is_profiling_inconclusive_not_transport_error(tmp_path, monkeypatch):
    pytest.importorskip('av')
    path = wav_file(tmp_path)
    original_summary = endurance.ResourceSampler.summary
    def blocked_loop(self, pipeline_seconds=None):
        self.samples = [{'elapsed_seconds': value, 'process_cpu_seconds': .1, 'current_rss_bytes': 1000000}
                        for value in (0, 118)]
        return original_summary(self, 118)
    monkeypatch.setattr(endurance.ResourceSampler, 'summary', blocked_loop)
    async def handler(requests, context):
        async for _ in requests:
            pass
        yield final()
        yield b'\x08\x01'
    async with server(handler) as (engine, _, _):
        row = await endurance.run_session(case_for(path), arguments(), engine, endurance.inspect_wav(path))
        assert row['transport'] == 'success' and row['status'] == 'inconclusive'
        assert row['resources']['all_rss_available'] and not row['resources']['coverage_ok']
        assert row['profiling_inconclusive'] and 'error_class' not in row
        assert not engine._sessions


@pytest.mark.parametrize('change', ['pcm_hash', 'file'])
async def test_integrity_failure_is_not_healthy(tmp_path, change):
    pytest.importorskip('av')
    path = wav_file(tmp_path)
    info = endurance.inspect_wav(path)
    if change == 'pcm_hash':
        info['pcm']['sha256'] = '0' * 64
    async def handler(requests, context):
        async for _ in requests:
            pass
        if change == 'file':
            with path.open('ab') as output:
                output.write(b'changed')
        yield final()
        yield b'\x08\x01'
    async with server(handler) as (engine, _, _):
        row = await endurance.run_session(case_for(path), arguments(), engine, info)
        assert row['transport'] == 'success' and row['status'] == 'failed'
        assert row['error_class'] == ('AudioIntegrityError' if change == 'pcm_hash' else 'AudioFileChanged')
        assert not engine._sessions


@pytest.mark.parametrize('mode', ['grpc', 's3', 'early', 'incomplete', 'ambiguous', 'missing', 'limit'])
async def test_late_errors_early_end_ambiguity_and_limits(tmp_path, monkeypatch, mode):
    pytest.importorskip('av')
    path = wav_file(tmp_path)
    if mode == 'limit':
        monkeypatch.setattr(endurance, 'MAX_FINAL_UPDATES', 1)
    async def handler(requests, context):
        if mode == 'early':
            await anext(requests)
            yield b'\x08\x01'
            return
        async for _ in requests:
            pass
        yield final('missing everything' if mode == 'missing' else TEXT, count=2 if mode == 'ambiguous' else 1)
        if mode == 'limit':
            yield final()
        if mode == 's3':
            yield b'\x08\x02'
        elif mode != 'incomplete':
            yield b'\x08\x01'
        if mode == 'grpc':
            await context.abort(grpc.StatusCode.PERMISSION_DENIED, 'fake-private-error')
    async with server(handler) as (engine, transport, _):
        row = await endurance.run_session(case_for(path), arguments(), engine, endurance.inspect_wav(path))
        assert row['status'] != 'healthy'
        if mode in ('ambiguous', 'missing'):
            assert row['transport'] == 'success' and row['status'] == 'inconclusive'
        else:
            assert row['transport'] == 'failed_or_unverified' and row['status'] == 'failed'
        assert not engine._sessions and transport._channel is not None
        assert len(row['final_updates']) == (0 if mode == 'early' else 1)
        if mode == 'grpc':
            assert row['grpc_code'] == 'PERMISSION_DENIED'
        if mode == 'limit':
            assert row['error_class'] == 'ResultLimitError'
        if mode == 'ambiguous':
            assert len(row['final_updates'][0]['hypotheses']) == 2
        assert 'fake-private-error' not in json.dumps(row)


@pytest.mark.parametrize('cancel', [False, True])
async def test_rss_failure_cannot_leak_session_or_mask_cancellation(tmp_path, monkeypatch, cancel):
    pytest.importorskip('av')
    path = wav_file(tmp_path, frames=4000)
    entered = asyncio.Event()
    async def handler(requests, context):
        async for _ in requests:
            entered.set()
        yield final()
        yield b'\x08\x01'
    def broken_rss():
        raise RuntimeError('fake-private-sampling-detail')
    monkeypatch.setattr(endurance, 'current_rss_bytes', broken_rss)
    row = {}
    async with server(handler) as (engine, _, _):
        task = asyncio.create_task(endurance.run_session(case_for(path), arguments(), engine,
            endurance.inspect_wav(path), row=row))
        if cancel:
            await asyncio.wait_for(entered.wait(), 2)
            task.cancel()
            with pytest.raises(asyncio.CancelledError):
                await task
            assert row['cancelled'] and row['error_class'] == 'CancelledError'
        else:
            await task
            assert row['transport'] == 'success' and row['status'] == 'inconclusive'
        assert not engine._sessions
        assert row['resources']['rss_error_classes'] == ['RuntimeError']
        assert 'fake-private-sampling-detail' not in json.dumps(row)


async def test_pcm_stream_bound(tmp_path, monkeypatch):
    pytest.importorskip('av')
    path = wav_file(tmp_path)
    info = endurance.inspect_wav(path)
    monkeypatch.setattr(endurance, 'MAX_PCM_BYTES', 640)
    async def handler(requests, context):
        async for _ in requests:
            pass
    async with server(handler) as (engine, _, _):
        row = await endurance.run_session(case_for(path), arguments(), engine, info)
        assert row['status'] == 'failed' and row['consumed_source']['bytes'] <= 640
        assert not row['consumed_source']['complete'] and not engine._sessions


@pytest.mark.parametrize('mode', ['healthy', 'failed', 'inconclusive', 'mismatch', 'cleanup'])
async def test_sequential_stop_no_retry_and_cleanup_report(tmp_path, monkeypatch, mode):
    pytest.importorskip('av')
    calls = []
    closed = []
    class Transport:
        def __init__(self, *args, **kwargs): pass
        async def close(self): closed.append('transport')
    class Engine:
        def __init__(self, *args, **kwargs): pass
        async def open(self, **kwargs): pass
        async def close(self):
            closed.append('engine')
            if mode == 'cleanup':
                raise RuntimeError('fake-private-cleanup')
    async def session(case, args, engine, info, *, row):
        calls.append(len(calls))
        row.update(status=mode if mode in ('failed', 'inconclusive') else 'healthy',
                   semantic_signature={'sha256': 'changed' if mode == 'mismatch' and len(calls) == 2 else 'same'})
        await asyncio.sleep(0)
    monkeypatch.setattr(endurance, 'GrpcTransport', Transport)
    monkeypatch.setattr(endurance, 'RamblerEngine', Engine)
    monkeypatch.setattr(endurance, 'run_session', session)
    monkeypatch.setattr(endurance, 'provenance', lambda: {})
    output = io.StringIO()
    coroutine = endurance.run(arguments(sessions=3), case_for(tmp_path / 'unused.wav'), {}, {}, None, output)
    if mode == 'cleanup':
        with pytest.raises(RuntimeError):
            await coroutine
    else:
        assert await coroutine == (mode == 'healthy')
    report = json.loads(output.getvalue())
    assert len(calls) == (1 if mode in ('failed', 'inconclusive') else 2 if mode == 'mismatch' else 3)
    assert closed == ['engine', 'transport']
    assert report['summary']['status'] == ('healthy' if mode == 'healthy' else 'failed' if mode in ('failed', 'cleanup') else 'inconclusive')
    if mode == 'mismatch':
        assert report['sessions'][-1]['semantic_error'] == 'OrderedFinalHistoryMismatch'
    if mode == 'cleanup':
        assert report['engine_cleanup_error_class'] == 'RuntimeError'
    assert 'fake-private-cleanup' not in output.getvalue()


async def test_cancelled_run_saves_partial_observations_and_closes_owned_resources(tmp_path, monkeypatch):
    pytest.importorskip('av')
    entered = asyncio.Event()
    closed = []
    class Transport:
        def __init__(self, *args, **kwargs): pass
        async def close(self): closed.append('transport')
    class Engine:
        def __init__(self, *args, **kwargs): pass
        async def open(self, **kwargs): pass
        async def close(self): closed.append('engine')
    async def session(case, args, engine, info, *, row):
        row.update(status='failed', events={'responses': 4})
        entered.set()
        await asyncio.Event().wait()
    monkeypatch.setattr(endurance, 'GrpcTransport', Transport)
    monkeypatch.setattr(endurance, 'RamblerEngine', Engine)
    monkeypatch.setattr(endurance, 'run_session', session)
    monkeypatch.setattr(endurance, 'provenance', lambda: {})
    output = io.StringIO()
    task = asyncio.create_task(endurance.run(arguments(), case_for(tmp_path / 'unused.wav'), {}, {}, None, output))
    await asyncio.wait_for(entered.wait(), 2)
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        await task
    report = json.loads(output.getvalue())
    assert report['cancelled'] and report['summary']['status'] == 'failed'
    assert report['sessions'][0]['events']['responses'] == 4
    assert closed == ['engine', 'transport']


def test_isolated_help_and_reject_live_without_credentials_or_overwrite(tmp_path):
    script = Path(endurance.__file__).resolve()
    result = subprocess.run([sys.executable, '-I', '-B', str(script), '--help'],
                            capture_output=True, text=True, timeout=10)
    assert result.returncode == 0 and '--previous-report' in result.stdout
    output = tmp_path / 'existing.json'
    output.write_text('preserve')
    assert endurance.main(['--manifest', str(tmp_path / 'missing.json'), '--id', 'control',
                           '--output', str(output)]) == 1
    assert output.read_text() == 'preserve'


async def test_isolated_cli_loopback(tmp_path):
    pytest.importorskip('av')
    path = wav_file(tmp_path)
    manifest = tmp_path / 'manifest.json'
    manifest.write_text(json.dumps({'cases': [case_for(path.name)]}))
    output = tmp_path / 'endurance.json'
    async def handler(requests, context):
        async for _ in requests:
            pass
        yield final()
        yield b'\x08\x01'
    async with server(handler) as (_, _, port):
        process = await asyncio.create_subprocess_exec(sys.executable, '-I', '-B', endurance.__file__,
            '--manifest', str(manifest), '--id', 'control', '--output', str(output),
            '--stage', 'smoke', '--target', f'127.0.0.1:{port}', '--insecure', '--timeout', '15',
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        stdout, stderr = await asyncio.wait_for(process.communicate(), 15)
        assert process.returncode == 0, (stdout.decode(errors='replace'), stderr.decode(errors='replace'))
    report = json.loads(output.read_text(encoding='utf-8'))
    assert report['summary']['status'] == 'healthy' and report['config']['concurrency'] == 1
    assert report['sessions'][0]['markers']['all_observed']
    assert report['sessions'][0]['consumed_source']['sha256'] == endurance.inspect_wav(path)['pcm']['sha256']
    assert 'fake-private-opaque' not in output.read_text(encoding='utf-8')
