import asyncio
import hashlib
import json
from pathlib import Path
from types import SimpleNamespace
import wave

import grpc
import pytest

from rambler.protocol import EditingContext, decode_response
from rambler.results import DictationResult
from rambler.transport import DEFAULT_METHOD, GrpcTransport
from rambler.engine import RamblerEngine
from rambler.wire import bytes_field, string_field, varint_field
from tools import validate_corpus as validator


def test_normalization_and_wer():
    assert validator.normalize('ÉTÉ—là,  23!') == 'été là 23'
    score = validator.word_error_rate('One, TWO three', 'one too three four')
    assert score['errors'] == 2 and score['reference_words'] == 3
    assert validator.word_error_rate('!!!', 'x') is None


def test_manifest_selection_and_limit(tmp_path):
    path = tmp_path / 'cases.json'
    path.write_text(json.dumps({'cases': [{'id': 'a'}, {'id': 'b'}]}))
    assert [x['id'] for x in validator.load_cases(path, 'b')] == ['b']
    for ids in ('a,a', 'a,c', ''):
        with pytest.raises(ValueError):
            validator.load_cases(path, ids)
    path.write_text(json.dumps({'cases': [{'id': str(i)} for i in range(11)]}))
    with pytest.raises(ValueError):
        validator.load_cases(path)


def test_file_limits_and_unsupported(tmp_path):
    flac = tmp_path / 'test.flac'
    flac.write_bytes(b'fLaC')
    with pytest.raises(ValueError):
        validator.inspect_audio(flac)
    large = tmp_path / 'large.ogg'
    with large.open('wb') as f:
        f.truncate(validator.MAX_FILE + 1)
    with pytest.raises(ValueError):
        validator.inspect_audio(large)


async def test_wave_encoding_full_consumption(tmp_path):
    pytest.importorskip('av')
    path = tmp_path / 'input.wav'
    pcm = b'\x00\x00' * 1600
    with wave.open(str(path), 'wb') as out:
        out.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
        out.writeframes(pcm)
    info = validator.inspect_audio(path)
    source, encoded = {'bytes': 0}, {'bytes': 0}
    content = b''.join([chunk async for chunk in validator.encoded_source(path, info, False, source, encoded)])
    assert source['complete'] and encoded['complete']
    assert source['sha256'] == hashlib.sha256(pcm).hexdigest() == info['source_sha256']
    assert encoded['sha256'] == hashlib.sha256(content).hexdigest()
    assert content.startswith(b'OggS')


def response(count=1, edit=True):
    payload = string_field(1, 'original')
    if edit:
        payload += bytes_field(74956224, varint_field(4, 2) + string_field(3, 'edited'))
    hypotheses = b''.join(bytes_field(3, payload) for _ in range(count))
    recognition = varint_field(1, 1) + bytes_field(3, hypotheses)
    return decode_response(bytes_field(1253625, bytes_field(1, recognition)))


def test_render_ambiguity_and_verified_replacement():
    context = EditingContext('old')
    result = DictationResult((response(),), context)
    report = validator.render(result, {}, True)
    assert report['rendered_text'] == 'edited'
    assert report['replacement_verified'] and report['context_conflict_detected'] and report['revision_conflict_detected']
    assert report['replacement_proposal']['expected_revision'] == 'fixture-v1'
    assert report['final_updates'] and report['context']['before'] == 'old'
    ambiguous = validator.render(DictationResult((response(2),), context), {}, True)
    assert ambiguous['rendering'] == 'ambiguous' and 'rendered_text' not in ambiguous
    assert len(ambiguous['final_updates']) == 1
    private = validator.render(result, {}, False)
    assert 'final_updates' not in private and 'rendered_text' not in private


def test_guard_and_output_newness(tmp_path):
    manifest = tmp_path / 'cases.json'
    manifest.write_text('{"cases":[{"id":"x"}]}')
    output = tmp_path / 'out.json'
    output.write_text('unchanged')
    with pytest.raises(SystemExit):
        validator.main(['--manifest', str(manifest), '--output', str(output)])
    with pytest.raises(FileExistsError):
        validator.main(['--manifest', str(manifest), '--output', str(output), '--target', '127.0.0.1:12345'])
    assert output.read_text() == 'unchanged'
    assert validator.loopback('[::1]:1234') and not validator.loopback('localhost.evil:1234')


async def test_stop_on_permission_and_record_error(monkeypatch):
    class Failure(grpc.RpcError):
        def code(self):
            return grpc.StatusCode.PERMISSION_DENIED
    class Transport:
        def __init__(self, *args, **kwargs): pass
        async def close(self): pass
    class Engine:
        def __init__(self, *args): pass
        async def open(self, **kwargs): pass
    calls = []
    async def fail(case, args, engine):
        calls.append(case['id'])
        raise Failure('sensitive')
    monkeypatch.setattr(validator, 'GrpcTransport', Transport)
    monkeypatch.setattr(validator, 'RamblerEngine', Engine)
    monkeypatch.setattr(validator, 'run_case', fail)
    args = SimpleNamespace(target='localhost:1', insecure=True, timeout=1)
    result = await validator.run(args, [{'id': 'a', 'language': 'en-US'}, {'id': 'b', 'language': 'en-US'}])
    assert calls == ['a'] and result[0]['grpc_code'] == 'PERMISSION_DENIED'
    assert 'sensitive' not in json.dumps(result)


async def test_loopback_complete_collection(tmp_path):
    pytest.importorskip('av')
    path = tmp_path / 'sample.wav'
    with wave.open(str(path), 'wb') as out:
        out.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
        out.writeframes(b'\x00\x00' * 1600)
    async def handler(requests, context):
        async for _ in requests:
            pass
        yield response(edit=False).raw
        yield b'\x08\x01'
    service, method = DEFAULT_METHOD[1:].split('/')
    server = grpc.aio.server()
    server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler(service, {
        method: grpc.stream_stream_rpc_method_handler(handler, request_deserializer=lambda x: x,
                                                      response_serializer=lambda x: x)
    }),))
    port = server.add_insecure_port('127.0.0.1:0')
    await server.start()
    transport = GrpcTransport(f'127.0.0.1:{port}', secure=False)
    try:
        await transport.open()
        args = SimpleNamespace(realtime=False, timeout=10, include_results=True)
        case = {'id': 'local', 'audio': str(path), 'language': 'en-US', 'reference_text': 'original'}
        report = await validator.run_case(case, args, RamblerEngine(transport))
        assert report['transport'] == 'success' and report['rendered_text'] == 'original'
        assert report['wer']['errors'] == 0
        assert report['consumed_source']['complete'] and report['sent_ogg']['complete']
        assert report['timing']['collection_seconds'] >= 0
        assert report['timing']['engine_metrics_at_eof']['eof_seconds'] is not None
        assert report['effective_pacing'] == 'unpaced'
    finally:
        await transport.close()
        await server.stop(None)


@pytest.mark.parametrize('report', [
    {'id': 'x', 'transport': 'failed_or_unverified'},
    {'id': 'x', 'transport': 'success', 'rendering': 'ambiguous'},
    {'id': 'x', 'transport': 'success', 'rendering': 'error'},
])
def test_failed_outcomes_exit_nonzero(tmp_path, monkeypatch, report):
    manifest = tmp_path / 'manifest.json'
    manifest.write_text('{"cases":[{"id":"x"}]}')
    async def mock_run(args, cases):
        return [report]
    monkeypatch.setattr(validator, 'run', mock_run)
    output = tmp_path / 'nested' / 'report.json'
    code = validator.main(['--manifest', str(manifest), '--output', str(output),
                           '--target', 'localhost:1234'])
    assert code == 1
    assert json.loads(output.read_text())['summary']['status'] == 'failed'


def test_nonfinite_strict_json():
    value = validator.json_value({'positive': float('inf'), 'negative': -float('inf'),
                                  'nan': float('nan'), 'raw': b'\xff'})
    assert value['nan'] == {'nonfinite_float': 'nan'}
    assert value['positive'] == {'nonfinite_float': 'inf'}
    assert value['negative'] == {'nonfinite_float': '-inf'}
    assert value['raw'] == {'hex': 'ff'}
    json.dumps(value, allow_nan=False)


async def test_effective_pacing_is_per_input(tmp_path, monkeypatch):
    path = tmp_path / 'source.wav'
    with wave.open(str(path), 'wb') as out:
        out.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
        out.writeframes(b'\x00\x00' * 320)
    pytest.importorskip('av')
    async def collect(engine, source, **kwargs):
        async for _ in source:
            pass
        return DictationResult((response(edit=False),), kwargs['context'])
    monkeypatch.setattr(validator, 'collect_dictation', collect)
    args = SimpleNamespace(realtime=True, timeout=5, include_results=False)
    case = {'id': 'x', 'audio': str(path), 'language': 'en-US'}
    result = await validator.run_case(case, args, object())
    assert result['effective_pacing'] == 'wav_20ms_realtime'
    ogg = tmp_path / 'source.ogg'
    info = validator.inspect_audio(path)
    data = b''.join([x async for x in validator.encoded_source(path, info, False,
                           {'bytes': 0}, {'bytes': 0})])
    ogg.write_bytes(data)
    case['audio'] = str(ogg)
    result = await validator.run_case(case, args, object())
    assert result['effective_pacing'] == 'unpaced'


def test_manifest_hash_is_of_parsed_buffer(tmp_path):
    raw = b'{ "cases": [{"id":"a"},{"id":"b"}] }\n'
    path = tmp_path / 'manifest.json'
    path.write_bytes(raw)
    cases, summary = validator.load_manifest(path, 'b')
    assert [x['id'] for x in cases] == ['b']
    assert summary == {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest()}
    path.write_bytes(b' ' * (validator.MAX_MANIFEST + 1))
    with pytest.raises(ValueError, match='manifest too large'):
        validator.load_manifest(path)


def test_initial_hints_to_tuples_and_canonical_fingerprints(monkeypatch):
    received = []
    def fake_config(**kwargs):
        received.append(kwargs)
        return kwargs
    monkeypatch.setattr(validator, 'ProtocolConfig', fake_config)
    case = {'language': 'fr-FR', 'initial_hints': {'bias_terms': ['secret-é', 'two'],
            'preferences': ['secret-p'], 'secondary_languages': ['en-US']}}
    config = validator.case_config(case)
    assert config['bias_terms'] == ('secret-é', 'two') and config['preferences'] == ('secret-p',)
    assert config['secondary_languages'] == ('en-US',)
    digest = validator.hints_summary(validator.initial_hints(case))
    encoded = json.dumps(['secret-é', 'two'], ensure_ascii=False, separators=(',', ':')).encode('utf8')
    assert digest['bias_terms'] == {'count': 2, 'sha256': hashlib.sha256(encoded).hexdigest()}
    assert 'secret' not in json.dumps(digest)
    validator.case_config({'language': 'en-US'})
    validator.case_config({'language': 'en-US', 'initial_hints': {}})
    assert received[-2:] == [{'language': 'en-US'}, {'language': 'en-US'}]


@pytest.mark.parametrize('bad', [None, [], 'bad', {'bad': []}, {'bias_terms': 'text'},
                                 {'preferences': [1]}, {'secondary_languages': (x for x in [])}])
async def test_bad_hints_rejected_before_rpc(monkeypatch, bad):
    calls = []
    class Transport:
        def __init__(self, *args, **kwargs): pass
        async def close(self): pass
    class Engine:
        def __init__(self, *args): pass
        async def open(self, **kwargs): calls.append('open')
    monkeypatch.setattr(validator, 'GrpcTransport', Transport)
    monkeypatch.setattr(validator, 'RamblerEngine', Engine)
    args = SimpleNamespace(target='localhost:1', insecure=True, timeout=1)
    result = await validator.run(args, [{'id': 'a', 'language': 'en-US', 'initial_hints': bad}])
    assert calls == [] and result[0]['transport'] == 'failed_or_unverified'


async def test_invalid_hint_unicode_reports_safe_value_error_before_open(monkeypatch):
    from tools import sanitize_corpus_report
    calls = []
    class Transport:
        def __init__(self, *args, **kwargs): pass
        async def close(self): pass
    class Engine:
        def __init__(self, *args): pass
        async def open(self, **kwargs): calls.append('open')
    monkeypatch.setattr(validator, 'GrpcTransport', Transport)
    monkeypatch.setattr(validator, 'RamblerEngine', Engine)
    case = {'id': 'x', 'language': 'en-US', 'initial_hints': {'bias_terms': ['\ud800']}}
    with pytest.raises(ValueError) as error:
        validator.initial_hints(case)
    assert isinstance(error.value.__cause__, UnicodeEncodeError)
    assert '\ud800' not in str(error.value)
    args = SimpleNamespace(target='localhost:1', insecure=True, timeout=1)
    result = await validator.run(args, [case])
    assert not calls and result[0]['error_class'] == 'ValueError'
    projected = sanitize_corpus_report.sanitize_report({'cases': result})
    assert projected['cases'][0]['error_class'] == 'ValueError'


def test_old_sdk_never_silently_ignores_hints(monkeypatch):
    def old_config(*, language):
        return language
    monkeypatch.setattr(validator, 'ProtocolConfig', old_config)
    assert validator.case_config({'language': 'en-US'}) == 'en-US'
    with pytest.raises(validator.UnsupportedInitialHintsError):
        validator.case_config({'language': 'en-US', 'initial_hints': {'bias_terms': ['sensitive']}})


async def test_hint_config_reaches_collect(tmp_path, monkeypatch):
    pytest.importorskip('av')
    path = tmp_path / 'source.wav'
    with wave.open(str(path), 'wb') as out:
        out.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
        out.writeframes(b'\0\0' * 320)
    calls = []
    def fake_config(**kwargs): return kwargs
    async def collect(engine, source, **kwargs):
        calls.append(kwargs['config'])
        async for _ in source: pass
        return DictationResult((response(edit=False),), kwargs['context'])
    monkeypatch.setattr(validator, 'ProtocolConfig', fake_config)
    monkeypatch.setattr(validator, 'collect_dictation', collect)
    case = {'id': 'x', 'audio': str(path), 'language': 'fr-FR',
            'initial_hints': {'bias_terms': ['sensitive-phrase']}}
    result = await validator.run_case(case, SimpleNamespace(realtime=False, timeout=5, include_results=True), object())
    assert calls == [{'language': 'fr-FR', 'bias_terms': ('sensitive-phrase',)}]
    assert result['language'] == 'fr-FR' and result['initial_hints_summary']['bias_terms']['count'] == 1
    assert 'sensitive-phrase' not in json.dumps(result)
