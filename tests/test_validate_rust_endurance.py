import json
from pathlib import Path
import sys
from types import SimpleNamespace
import wave

import pytest
from tools import validate_rust_endurance as bench


def wav(path, seconds=1):
    with wave.open(str(path), 'wb') as f:
        f.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
        f.writeframes(b'\x00\x00' * 16000 * seconds)


def markers():
    return {'beginning': 'first token', 'middle': 'middle token', 'end': 'last token'}


def args(**kw):
    v = dict(allow_live=True, sessions=1, interval=1, timeout=140, stage='120', language='en-US',
             previous_report=None, marker_beginning='first token', marker_middle='middle token', marker_end='last token')
    v.update(kw)
    return SimpleNamespace(**v)


def sample(t):
    return {'elapsed_seconds': t, 'rss_bytes': 100, 'cpu_seconds': t / 2}


def test_source_header_hash_and_duration(tmp_path):
    path = tmp_path/'x.wav'
    wav(path)
    info = bench.inspect_wav(path)
    assert info['samples'] == 16000 and info['pcm']['bytes'] == 32000
    assert info['duration_seconds'] == 1 and info['file'] == bench.file_digest(path, bench.MAX_AUDIO)
    with wave.open(str(path), 'wb') as f:
        f.setparams((2, 2, 16000, 0, 'NONE', 'not compressed'))
        f.writeframes(b'\0' * 100)
    with pytest.raises(ValueError): bench.inspect_wav(path)


def test_gate_before_child_and_previous_required(tmp_path, monkeypatch):
    called = []
    monkeypatch.setattr(bench, 'run_child', lambda *a: called.append(True))
    binfile, audio = tmp_path/'exe', tmp_path/'audio.wav'
    binfile.write_bytes(b'x')
    wav(audio)
    base = ['--executable', str(binfile), '--audio', str(audio), '--output', str(tmp_path/'r.json'),
            '--stage', '120', '--timeout', '140', '--compiler-version', 'rustc test', '--build-version', 'test',
            '--marker-beginning', 'first token', '--marker-middle', 'middle token', '--marker-end', 'last token']
    with pytest.raises(ValueError, match='allow-live'): bench.main(base)
    assert called == [] and not (tmp_path/'r.json').exists()
    binary = {'sha256': 'a'*64, 'bytes': 3}
    with pytest.raises(ValueError, match='predecessor'):
        bench.validate_plan(args(stage='300', timeout=320), {'duration_seconds': 290}, binary)
    for changed in ({'sessions': 4}, {'interval': float('nan')}, {'timeout': 120}, {'language': 'bad/private'}):
        with pytest.raises(ValueError): bench.validate_plan(args(**changed), {'duration_seconds': 120}, binary)


def test_coverage_and_predecessor(tmp_path):
    samples = [sample(i) for i in range(121)]
    assert bench.coverage(samples, 1, 120)['coverage_ok']
    assert not bench.coverage(samples[:5], 1, 120)['coverage_ok']
    assert not bench.coverage([sample(0), sample(120)], 1, 120)['coverage_ok']
    binary = {'sha256':'a'*64, 'bytes':5}
    cfg = {'stage':'120', 'sessions':1, 'binary':binary, 'language':'en-US',
           'sample_interval_seconds':1, 'markers': {k:bench.text_hash(v) for k,v in markers().items()}}
    row = {'status':'healthy', 'resources':{'coverage_ok':True, 'samples':samples}, 'child_wall_seconds':120,
           'ordered_finals_match_first': None,
           'public':{'rendered_markers_found':{k:True for k in markers()},
                     'ordered_finals_summary': {'sha256':'b'*64, 'bytes':123}}}
    prev = {'report_kind':'rust_cli_endurance_v1', 'config':cfg, 'summary':{'status':'healthy'},
            'audio':{'duration_seconds':120}, 'sessions':[row]}
    assert bench.healthy_predecessor(prev,binary,markers(),'en-US')
    row['public']['rendered_markers_found'] = {}
    assert not bench.healthy_predecessor(prev,binary,markers(),'en-US')


def test_strict_projection_excludes_unknown_and_private():
    raw = {'type':'result','text':'first token middle token last token',
           'final_updates':[{'hypotheses':[{'text':'first token middle token last token',
               'private_id':'SECRET_VALUE','voice_edit':None}]}],
           'metrics':{'eof_seconds': 12.0, 'unknown':'SECRET_VALUE'}, 'session_id':'SECRET_VALUE'}
    result = bench.project_result(raw, markers())
    assert all(result['markers_found'].values())
    assert 'SECRET_VALUE' not in json.dumps(result)
    assert 'first token' not in json.dumps(result)
    with pytest.raises(ValueError): bench.strict_json(b'{"a":1,"a":2}')
    with pytest.raises(ValueError): bench.strict_json(b'{"a":NaN}')
    with pytest.raises(ValueError): bench.strict_json(b'{"a":1e999}')
    raw['metrics']['eof_seconds'] = None
    with pytest.raises(ValueError): bench.project_result(raw, markers())


def test_child_timeout_cleanup_and_bounded_output(monkeypatch):
    result = bench.run_child([sys.executable, '-c', 'import time; time.sleep(3)'], .15, 1)
    assert result['reason'] == 'timeout' and result['exit_code'] != 0 and result['elapsed_seconds'] < 2
    monkeypatch.setattr(bench, 'MAX_OUTPUT', 1024)
    result = bench.run_child([sys.executable, '-c', 'import sys; sys.stdout.write("x"*1000000)'], 5, 1)
    assert result['reason'] == 'output_limit' and len(result['stdout']) == 1024


def test_failure_stops_and_public_projection_does_not_leak(tmp_path, monkeypatch):
    binary, audio = tmp_path/'bin', tmp_path/'audio.wav'
    binary.write_bytes(b'not real')
    wav(audio)
    monkeypatch.setattr(bench, 'inspect_wav', lambda p: {'duration_seconds':120, 'file':bench.file_digest(p,bench.MAX_AUDIO)})
    monkeypatch.setattr(bench, 'source_metadata', lambda: {'cargo_lock':{}, 'rust_sources':{}, 'harness':{}})
    called = []
    def fail(*a):
        called.append(1)
        return {'exit_code':1, 'reason':None, 'stdout':b'SECRET_SERVICE_ID', 'stderr':b'SECRET_ERROR',
                'elapsed_seconds':1, 'samples':[sample(0),sample(1)]}
    monkeypatch.setattr(bench, 'run_child', fail)
    private, public = tmp_path/'private.json', tmp_path/'public.json'
    rc = bench.main(['--executable',str(binary),'--audio',str(audio),'--output',str(private),
                     '--public-output',str(public),'--stage','120','--timeout','140','--sessions','3',
                     '--compiler-version','test','--build-version','test','--allow-live',
                     '--marker-beginning','first token','--marker-middle','middle token','--marker-end','last token'])
    assert rc == 1 and len(called) == 1
    assert json.loads(private.read_text())['sessions'][0]['private_output']
    public_text = public.read_text()
    assert 'SECRET_SERVICE_ID' not in public_text and 'SECRET_ERROR' not in public_text
    assert 'private_output' not in public_text


def test_marker_matching_does_not_accept_word_prefixes():
    assert bench.contains_marker('The first token, then.', 'first token')
    assert not bench.contains_marker('The first tokens arrive.', 'first token')


def test_session_inconclusive_stops_without_retry(tmp_path, monkeypatch):
    binary, audio = tmp_path/'bin', tmp_path/'audio.wav'
    binary.write_bytes(b'x')
    wav(audio)
    monkeypatch.setattr(bench, 'inspect_wav', lambda p: {'duration_seconds':120, 'file':bench.file_digest(p,bench.MAX_AUDIO)})
    monkeypatch.setattr(bench, 'source_metadata', lambda: {'cargo_lock':{}, 'rust_sources':{}, 'harness':{}})
    calls = []
    payload = {'type':'result', 'text':'first token middle token last token',
               'final_updates':[{'hypotheses':[{'text':'first token middle token last token'}]}],
               'metrics':{'eof_seconds':120}}
    def incomplete(*a):
        calls.append(1)
        return {'exit_code':0, 'reason':None, 'stdout':json.dumps(payload).encode(), 'stderr':b'',
                'elapsed_seconds':120, 'samples':[sample(0)]}
    monkeypatch.setattr(bench, 'run_child', incomplete)
    private = tmp_path/'private.json'
    result = bench.main(['--executable',str(binary),'--audio',str(audio),'--output',str(private),
                         '--stage','120','--timeout','140','--sessions','2','--compiler-version','test',
                         '--build-version','test','--allow-live','--marker-beginning','first token',
                         '--marker-middle','middle token','--marker-end','last token'])
    assert result == 1 and len(calls) == 1
    assert json.loads(private.read_text())['sessions'][0]['status'] == 'inconclusive'


@pytest.mark.parametrize('different_finals', [False, True])
def test_rendered_markers_and_ordered_finals_gate(tmp_path, monkeypatch, different_finals):
    binary, audio = tmp_path/'bin', tmp_path/'audio.wav'
    binary.write_bytes(b'x')
    wav(audio)
    monkeypatch.setattr(bench, 'inspect_wav', lambda p: {'duration_seconds':120, 'file':bench.file_digest(p,bench.MAX_AUDIO)})
    monkeypatch.setattr(bench, 'source_metadata', lambda: {'cargo_lock':{}, 'rust_sources':{}, 'harness':{}})
    complete = 'first token middle token last token'
    calls = []
    def child(*a):
        calls.append(1)
        text = complete if different_finals else 'first token middle token'
        hypothesis = complete + (' extra' if different_finals and len(calls) == 2 else '')
        payload = {'type':'result', 'text':text, 'metrics':{'eof_seconds':120},
                   'final_updates':[{'hypotheses':[{'text':hypothesis,
                       'voice_edit':{'edited_text':text, 'intent':2}}]}]}
        return {'exit_code':0, 'reason':None, 'stdout':json.dumps(payload).encode(), 'stderr':b'',
                'elapsed_seconds':120, 'samples':[sample(i) for i in range(121)]}
    monkeypatch.setattr(bench, 'run_child', child)
    private, public = tmp_path/'private.json', tmp_path/'public.json'
    rc = bench.main(['--executable',str(binary),'--audio',str(audio),'--output',str(private),
                     '--public-output',str(public),'--stage','120','--timeout','140','--sessions','3',
                     '--compiler-version','test','--build-version','test','--allow-live',
                     '--marker-beginning','first token','--marker-middle','middle token','--marker-end','last token'])
    rows = json.loads(private.read_text())['sessions']
    assert rc == 1 and len(calls) == (2 if different_finals else 1)
    assert rows[-1]['status'] == 'inconclusive'
    if different_finals:
        assert rows[0]['status'] == 'healthy' and rows[0]['ordered_finals_match_first'] is None
        assert rows[1]['ordered_finals_match_first'] is False
        assert all(rows[1]['public']['rendered_markers_found'].values())
        assert json.loads(public.read_text())['sessions'][1]['ordered_finals_match_first'] is False
    else:
        assert all(rows[0]['public']['markers_found'].values())
        assert not rows[0]['public']['rendered_markers_found']['end']
