"""Offline privacy projection tests; fake service identifiers, no credentials."""
import copy
import hashlib
import json
from pathlib import Path

import pytest

from tools import sanitize_corpus_report as sanitizer
from tools.research.inspect_s3_capture import fields, scalar, text, values

ROOT = Path(__file__).resolve().parents[1]
PRIVATE_ID = 'call_PRIVATE_SERVER_IDENTIFIER_87654321'
PRIVATE_ERROR = 'private-server-diagnostic-value'
CONTROLLED_TEXT = 'Réécris cette phrase, s’il te plaît. 😀\nLigne deux.'


def report():
    raw = b'\x0a' + bytes([len(PRIVATE_ID)]) + PRIVATE_ID.encode()
    voice = {'transcript': CONTROLLED_TEXT, 'alternative_text': None,
             'edited_text': 'Texte poli. 😀', 'edited_context': {
                 'before': 'Avant', 'selected': 'é😀', 'after': ' après', 'cursor_aware': True,
                 'raw': {'hex': raw.hex()}, 'unknown_fields': []},
             'intent': 3, 'active_field': 3, 'raw': {'hex': raw.hex()},
             'unknown_fields': [
                 {'number': 74956225, 'wire_type': 2, 'value': {'hex': raw.hex()}},
                 {'number': 91, 'wire_type': 0, 'value': 777777777},
                 {'number': 92, 'wire_type': 2, 'value': {'nested': {'call_id': PRIVATE_ID}}}]}
    hypothesis = {'text': CONTROLLED_TEXT, 'confidence': {'nonfinite_float': 'nan'},
                  'voice_edit': voice, 'raw': {'hex': raw.hex()}, 'unknown_fields': []}
    update = {'status': None, 'error_code': None, 'error_message': PRIVATE_ERROR,
              'raw': {'hex': raw.hex()}, 'unknown_fields': [],
              'result': {'recognition': {'state': 1, 'hypothesis_result': {
                  'hypotheses': [hypothesis], 'raw': {'hex': raw.hex()}, 'unknown_fields': []},
                  'partial_result': None, 'raw': {'hex': raw.hex()}, 'unknown_fields': []},
                  'speech': None, 'result_id': PRIVATE_ID, 'alternative_text': None,
                  'active_field': 1, 'active_raw': {'hex': raw.hex()},
                  'raw': {'hex': raw.hex()}, 'unknown_fields': []}}
    return {'schema_version': 1, 'versions': {'python': '3.14.5', 'os': 'test OS',
                'cpu': {'model': 'test CPU', 'logical_cores': 8, 'unexpected': PRIVATE_ID},
                'libraries': {'sambler-engine': '0.3.0', 'rambpc-engine': '0.3.0',
                              'grpcio': '1.85.1', 'unrecognized': PRIVATE_ID},
                'git_head': 'a' * 40, 'rambler_source_sha256': {'protocol.py': 'b' * 64,
                'unrecognized.py': PRIVATE_ID}, 'validator_sha256': 'c' * 64},
            'requested_realtime': False, 'include_results': True,
            'summary': {'requested': 1, 'completed_successfully': 1, 'failed': 0,
                        'not_run': 0, 'status': 'success', 'unrecognized': PRIVATE_ID},
            'unknown_root': {'authorization': PRIVATE_ID},
            'cases': [{'id': 'controlled-rewrite', 'transport': 'success',
                       'audio': {'file_sha256': 'd' * 64, 'source_sha256': 'e' * 64,
                                 'source_kind': 'pcm16le', 'duration_s': 1.5},
                       'timing': {'collection_seconds': 0.5, 'engine_metrics_at_eof': {
                           'session_id': PRIVATE_ID, 'requests_produced': 4,
                           'eof_seconds': 0.5, 'unknown_metric': PRIVATE_ID}},
                       'rendering': 'unique', 'final_updates': [update],
                       'context': {'before': CONTROLLED_TEXT, 'selected': '', 'after': '',
                                   'cursor_aware': False, 'unknown': PRIVATE_ID},
                       'rendered_text': CONTROLLED_TEXT,
                       'required_terms': {'é😀': True},
                       'replacement_proposal': {'snapshot': {}, 'expected_revision': PRIVATE_ID,
                                                'replacement_text': CONTROLLED_TEXT,
                                                'replacement_context': None},
                       'tool_metadata': {'call_id': PRIVATE_ID}}]}


def walk_keys(value):
    if isinstance(value, dict):
        for key, child in value.items():
            yield key
            yield from walk_keys(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk_keys(child)


def first_hypothesis(sanitized):
    return sanitized['cases'][0]['final_updates'][0]['result']['recognition']['hypothesis_result']['hypotheses'][0]


@pytest.mark.parametrize('include_text', [False, True])
def test_allowlist_withholds_nested_unknown_bytes_values_and_service_ids(include_text):
    original = report()
    before = copy.deepcopy(original)
    sanitized = sanitizer.sanitize_report(original, include_text=include_text)
    serialized = json.dumps(sanitized, ensure_ascii=False, allow_nan=False)
    assert original == before
    assert PRIVATE_ID not in serialized and PRIVATE_ID.encode().hex() not in serialized
    assert PRIVATE_ERROR not in serialized
    assert '777777777' not in serialized
    assert not {'raw', 'active_raw', 'hex', 'value', 'result_id', 'session_id',
                'expected_revision', 'tool_metadata', 'call_id', 'unknown_root'} & set(walk_keys(sanitized))
    voice = first_hypothesis(sanitized)['voice_edit']
    unknown = voice['unknown_field_summaries']
    assert [(x['number'], x['wire_type']) for x in unknown] == [(74956225, 2), (91, 0), (92, 2)]
    assert all(x['value_withheld'] for x in unknown)
    raw = bytes.fromhex(original['cases'][0]['final_updates'][0]['raw']['hex'])
    summary = sanitized['cases'][0]['final_updates'][0]['raw_summary']
    assert summary == {'sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw),
                       'top_level_fields': [{'number': 1, 'wire_type': 2}]}
    assert sanitized['versions']['rambler_source_sha256'] == {'protocol.py': 'b' * 64}
    assert sanitized['versions']['cpu'] == {'model': 'test CPU', 'logical_cores': 8}
    assert sanitized['versions']['libraries'] == {
        'sambler-engine': '0.3.0', 'rambpc-engine': '0.3.0', 'grpcio': '1.85.1',
    }
    assert sanitized['privacy_review_required'] is True
    assert 'not proof' in sanitized['publication_warning']


def test_known_unicode_text_is_hashed_by_default_and_only_exposed_by_opt_in():
    private = sanitizer.sanitize_report(report())
    public = sanitizer.sanitize_report(report(), include_text=True)
    expected = {'sha256': hashlib.sha256(CONTROLLED_TEXT.encode()).hexdigest(),
                'bytes': len(CONTROLLED_TEXT.encode())}
    assert first_hypothesis(private)['text'] == expected
    assert private['cases'][0]['context']['before'] == expected
    assert CONTROLLED_TEXT not in json.dumps(private, ensure_ascii=False)
    assert first_hypothesis(public)['text'] == CONTROLLED_TEXT
    assert public['cases'][0]['context']['before'] == CONTROLLED_TEXT
    assert public['cases'][0]['required_terms'] == [{'term': 'é😀', 'present': True}]
    assert public['text_included'] is True and private['text_included'] is False
    assert first_hypothesis(public)['voice_edit']['edited_context']['selected'] == 'é😀'


@pytest.mark.parametrize('value,label', [(float('nan'), 'nan'), (float('inf'), 'inf'),
                                        (-float('inf'), '-inf'), ({'nonfinite_float': 'nan'}, 'nan')])
def test_nonfinite_known_scores_have_strict_json_representation(value, label):
    original = report()
    hypothesis = original['cases'][0]['final_updates'][0]['result']['recognition']['hypothesis_result']['hypotheses'][0]
    hypothesis['confidence'] = value
    sanitized = sanitizer.sanitize_report(original)
    assert first_hypothesis(sanitized)['confidence'] == {'nonfinite_float': label}
    json.dumps(sanitized, allow_nan=False)


@pytest.mark.parametrize('constant', ['NaN', 'Infinity', '-Infinity'])
def test_cli_rejects_nonstandard_json_before_creating_output(tmp_path, constant):
    source, output = tmp_path / 'source.json', tmp_path / 'public.json'
    original = ('{"cases":[],"versions":{"python":' + constant + '}}').encode()
    source.write_bytes(original)
    with pytest.raises(ValueError, match='non-finite JSON'):
        sanitizer.main(['--input', str(source), '--output', str(output)])
    assert not output.exists() and source.read_bytes() == original


def test_cli_new_file_source_hash_and_source_unchanged(tmp_path):
    source, output = tmp_path / 'source.json', tmp_path / 'public.json'
    original = (json.dumps(report(), ensure_ascii=False, indent=1) + '\n').encode()
    source.write_bytes(original)
    assert sanitizer.main(['--input', str(source), '--output', str(output)]) == 0
    result = json.loads(output.read_text(encoding='utf-8'))
    assert result['source_report_summary'] == {'sha256': hashlib.sha256(original).hexdigest(),
                                               'bytes': len(original)}
    assert result['sanitizer_source_sha256'] == hashlib.sha256(Path(sanitizer.__file__).read_bytes()).hexdigest()
    assert source.read_bytes() == original
    saved = output.read_bytes()
    with pytest.raises(FileExistsError):
        sanitizer.main(['--input', str(source), '--output', str(output), '--include-text'])
    assert output.read_bytes() == saved and source.read_bytes() == original
    with pytest.raises(FileExistsError):
        sanitizer.main(['--input', str(source), '--output', str(source)])
    assert source.read_bytes() == original


def test_cli_include_text_remains_an_explicit_opt_in(tmp_path):
    source, output = tmp_path / 'source.json', tmp_path / 'public.json'
    source.write_text(json.dumps(report()), encoding='utf-8')
    sanitizer.main(['--input', str(source), '--output', str(output), '--include-text'])
    result = json.loads(output.read_text(encoding='utf-8'))
    assert first_hypothesis(result)['text'] == CONTROLLED_TEXT
    assert PRIVATE_ID not in output.read_text(encoding='utf-8')


def test_duplicate_keys_and_invalid_unicode_leave_no_output(tmp_path):
    source, output = tmp_path / 'source.json', tmp_path / 'public.json'
    source.write_text('{"cases":[],"cases":[]}', encoding='utf-8')
    with pytest.raises(ValueError, match='duplicate JSON'):
        sanitizer.main(['--input', str(source), '--output', str(output)])
    assert not output.exists()
    original = report()
    original['cases'][0]['rendered_text'] = '\ud800'
    source.write_text(json.dumps(original), encoding='utf-8')
    with pytest.raises(UnicodeError):
        sanitizer.main(['--input', str(source), '--output', str(output), '--include-text'])
    assert not output.exists()


@pytest.mark.parametrize('include_text', [False, True])
def test_manifest_and_hints_allowlist_only_nontext_metadata(include_text):
    original = report()
    original['manifest_summary'] = {'sha256': 'a' * 64, 'bytes': 321, 'hidden': PRIVATE_ID}
    case = original['cases'][0]
    case['language'] = 'fr-FR'
    case['initial_hints'] = {'bias_terms': ['fake-sensitive-hint']}
    case['initial_hints_summary'] = {
        'bias_terms': {'count': 1, 'sha256': 'b' * 64, 'raw': 'fake-sensitive-hint'},
        'preferences': {'count': 0, 'sha256': 'c' * 64, 'other': PRIVATE_ID},
        'secondary_languages': {'count': 1, 'sha256': 'd' * 64},
        'unknown': {'count': 1, 'sha256': 'e' * 64, 'raw': PRIVATE_ID}}
    projected = sanitizer.sanitize_report(original, include_text=include_text)
    assert projected['manifest_summary'] == {'sha256': 'a' * 64, 'bytes': 321}
    assert projected['cases'][0]['language'] == 'fr-FR'
    hints = projected['cases'][0]['initial_hints_summary']
    assert hints == {'bias_terms': {'count': 1, 'sha256': 'b' * 64},
                     'preferences': {'count': 0, 'sha256': 'c' * 64},
                     'secondary_languages': {'count': 1, 'sha256': 'd' * 64}}
    text = json.dumps(projected, ensure_ascii=False)
    assert 'fake-sensitive-hint' not in text and PRIVATE_ID not in text


def test_malformed_new_metadata_is_not_projected():
    original = report()
    original['manifest_summary'] = {'sha256': PRIVATE_ID, 'bytes': -1}
    original['cases'][0]['language'] = PRIVATE_ID
    original['cases'][0]['initial_hints_summary'] = {'bias_terms': {'count': -3, 'sha256': PRIVATE_ID}}
    projected = sanitizer.sanitize_report(original)
    assert projected['manifest_summary'] == {}
    assert 'language' not in projected['cases'][0]
    assert projected['cases'][0]['initial_hints_summary'] == {'bias_terms': {}}


def test_sanitized_derived_fixture_has_only_the_reviewed_known_components():
    fixture = json.loads((ROOT / 'tests/fixtures/live_rewrite_response.json').read_text(encoding='utf-8'))
    assert fixture['fixture_kind'] == 'sanitized_derived_server_response'
    assert fixture['payload_modified'] is True
    assert fixture['observed_at'] == '2026-11-13'
    assert fixture['omitted_extensions'] == [74956225]
    assert fixture['source_payload_sha256'] == '7a8c1217362395a706d0626aa69860413a82bed82cfc868f369750f185f4ff92'
    case = fixture['cases'][0]
    payload = bytes.fromhex(case['payload_hex'])
    assert len(payload) == case['payload_bytes'] == 261
    assert hashlib.sha256(payload).hexdigest() == case['payload_sha256']
    assert case['payload_sha256'] != fixture['source_payload_sha256']
    top = fields(payload)
    assert [(n,w) for n,w,_ in top] == [(1253625,2)]
    result = fields(values(top,1253625,2)[0])
    assert [(n,w) for n,w,_ in result] == [(1,2)]
    recognition = fields(values(result,1,2)[0])
    assert [(n,w) for n,w,_ in recognition] == [(1,0),(3,2)]
    assert scalar(recognition,1) == 1
    final = fields(values(recognition,3,2)[0])
    assert [(n,w) for n,w,_ in final] == [(3,2)]
    hypothesis = fields(values(final,3,2)[0])
    assert [(n,w) for n,w,_ in hypothesis] == [(1,2),(74956224,2)]
    hypothesis_text = values(hypothesis,1,2)[0]
    voice_payload = values(hypothesis,74956224,2)[0]
    voice = fields(voice_payload)
    assert [(n,w) for n,w,_ in voice] == [(1,2),(3,2),(4,0)]
    assert text(hypothesis,1) == text(voice,1) == case['expected']['hypothesis_text']
    assert text(voice,3) == case['expected']['edited_text']
    assert scalar(voice,4) == case['expected']['intent'] == 3
    assert hashlib.sha256(hypothesis_text).hexdigest() == fixture['source_known_component_sha256']['hypothesis_text_utf8']
    assert hashlib.sha256(voice_payload).hexdigest() == fixture['source_known_component_sha256']['voice_editing_result_payload']
    serialized = json.dumps(fixture)
    assert 'call_' not in serialized and 'tool_metadata' not in serialized
