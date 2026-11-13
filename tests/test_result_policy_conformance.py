"""Portable result-policy oracle for native ports; no RPC/session claims."""
import hashlib
import json
from pathlib import Path

import pytest

from rambler.protocol import EditingContext, decode_response
from rambler.results import DictationResult, replacement_proposal

FIXTURE = json.loads((Path(__file__).parent / 'fixtures/result_policy_conformance.json').read_text(encoding='utf-8'))


def context(value):
    return None if value is None else EditingContext(**value)


def revision(value):
    if value is None:
        return None
    if value['kind'] == 'string':
        return value['value']
    assert value['kind'] == 'integer'
    return int(value['decimal'])


def text_context(value):
    return None if value is None else {key: getattr(value, key) for key in ('before', 'selected', 'after', 'cursor_aware')}


@pytest.mark.parametrize('case', FIXTURE['cases'], ids=lambda case: case['id'])
def test_reference_result_policy_oracle(case):
    payloads = [bytes.fromhex(item['hex']) for item in case['response_payloads']]
    assert [hashlib.sha256(payload).hexdigest() for payload in payloads] == [item['sha256'] for item in case['response_payloads']]
    result = DictationResult(tuple(decode_response(payload) for payload in payloads), context(case['context']))
    observed = {}
    try:
        observed['text'] = {'value': result.text}
    except ValueError as error:
        observed['text'] = {'error': type(error).__name__}
    try:
        proposal = replacement_proposal(result, expected_revision=revision(case['expected_revision']))
    except ValueError as error:
        observed['proposal'] = {'error': type(error).__name__}
    else:
        selected = proposal.selection_utf16_range
        observed['proposal'] = {
            'replacement_text': proposal.replacement_text,
            'replacement_context': text_context(proposal.replacement_context),
            'selection_utf16_range': None if selected is None else list(selected),
            'validations': [],
        }
        for check in case['validations']:
            try:
                proposal.validate_current(context(check['context']), revision=revision(check['revision']))
            except ValueError as error:
                observed['proposal']['validations'].append({'error': type(error).__name__})
            else:
                observed['proposal']['validations'].append({'valid': True})
    assert observed == case['expected']


def test_result_policy_oracle_provenance_is_explicit():
    assert FIXTURE['fixture_kind'] == 'generated_result_policy_conformance_oracle'
    assert FIXTURE['reference_commit'] == 'ec39b5d'
    assert len({case['id'] for case in FIXTURE['cases']}) == len(FIXTURE['cases'])
    assert {case['origin'] for case in FIXTURE['cases']} == {
        'original_public_response', 'sanitized_derived_public_response', 'synthetic_wire'}
