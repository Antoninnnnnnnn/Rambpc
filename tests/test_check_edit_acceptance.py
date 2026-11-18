"""Synthetic, offline regression cases for the exact editing contract."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

TOOL = Path(__file__).resolve().parents[1] / 'tools' / 'check_edit_acceptance.py'


def expectations():
    # An invented oracle, authored independently of the observation fixture.
    return {
        'schema': 'rambpc-edit-expectations-v1',
        'artifact_id': 'declared-plan',
        'provenance': {'corpus_id': 'toy-corpus', 'run_id': 'planned-run',
                       'candidate_id': 'candidate-build', 'policy': 'strict'},
        'cases': [
            {'case_id': 'structured', 'input_id': 'toy-01', 'intent': 2,
             'outcome': {'kind': 'edit', 'window': '🦔: bleu!', 'selection_utf16': [4, 8]}},
            {'case_id': 'rewrite', 'input_id': 'toy-02', 'intent': 3,
             'outcome': {'kind': 'no_edit'}},
            {'case_id': 'absent', 'input_id': 'toy-03', 'intent': None,
             'outcome': {'kind': 'no_edit'}},
            {'case_id': 'deletion', 'input_id': 'toy-04', 'intent': 2,
             'outcome': {'kind': 'edit', 'window': '', 'selection_utf16': None}},
        ],
    }


def observations():
    # A distinct invented candidate record, deliberately in another order.
    return {
        'schema': 'rambpc-edit-observations-v1',
        'artifact_id': 'separate-observation',
        'provenance': {'corpus_id': 'toy-corpus', 'run_id': 'planned-run',
                       'candidate_id': 'candidate-build', 'policy': 'strict'},
        'cases': [
            {'case_id': 'deletion', 'input_id': 'toy-04', 'status': 'observed', 'intent': 2,
             'outcome': {'kind': 'edit', 'window': '', 'selection_utf16': None}},
            {'case_id': 'structured', 'input_id': 'toy-01', 'status': 'observed', 'intent': 2,
             'outcome': {'kind': 'edit', 'window': '🦔: bleu!', 'selection_utf16': [4, 8]}},
            {'case_id': 'rewrite', 'input_id': 'toy-02', 'status': 'observed', 'intent': 3,
             'outcome': {'kind': 'no_edit'}},
            {'case_id': 'absent', 'input_id': 'toy-03', 'status': 'observed', 'intent': None,
             'outcome': {'kind': 'no_edit'}},
        ],
    }


class EditAcceptanceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.expected_path = Path(temporary.name) / 'expect-filename-sentinel.json'
        self.observed_path = Path(temporary.name) / 'observe-filename-sentinel.json'

    def cli(self, *args):
        run = subprocess.run([sys.executable, '-I', '-S', '-B', str(TOOL), *args],
                             capture_output=True, text=True, encoding='utf-8', timeout=10)
        self.assertEqual(run.stderr, '')
        self.assertLess(len(run.stdout), 512)
        self.assertNotIn('sentinel', run.stdout)
        self.assertEqual(run.stdout.count('\n'), 1)
        result = json.loads(run.stdout)
        self.assertEqual(result['schema'], 'rambpc-edit-acceptance-result-v1')
        self.assertEqual(result['scope'], 'offline-strict-proposal-only')
        return run.returncode, result

    def check_pair(self, expected, observed):
        for path, value in ((self.expected_path, expected), (self.observed_path, observed)):
            raw = value if type(value) is bytes else json.dumps(value).encode('utf-8')
            path.write_bytes(raw)
        return self.cli(self.expected_path, self.observed_path)

    def assert_verdict(self, actual, verdict, **counts):
        code, result = actual
        self.assertEqual(result['verdict'], verdict)
        self.assertEqual(code, {'conforming': 0, 'mismatch': 1, 'inconclusive': 2}[verdict])
        self.assertEqual(set(result), {'schema', 'scope', 'verdict', 'counts'})
        self.assertEqual(result['counts'], dict(expected=4, conforming_no_edit=0,
                                               conforming_edit=0, mismatch=0, missing=0,
                                               unqualified=0, unexpected=0) | counts)

    def assert_invalid(self, actual, reason):
        code, result = actual
        self.assertEqual(code, 3)
        self.assertEqual(result['verdict'], 'invalid')
        self.assertEqual(set(result), {'schema', 'scope', 'verdict', 'reason'})
        self.assertEqual(result['reason'], reason)

    def test_qualified_edit_and_no_edit_are_distinct_aggregate_counts(self):
        self.assert_verdict(self.check_pair(expectations(), observations()), 'conforming',
                            conforming_edit=2, conforming_no_edit=2)

    def test_mismatch_checks_exact_window_selection_intent_and_proposal_presence(self):
        for key, value in (
            ('window', '🦔: bleu?'),  # same selected text, changed surrounding window
            ('selection_utf16', [4, 7]),  # legal UTF-16, but not the expected range
            ('selection_utf16', None),
            ('selection_utf16', [4, 4]),  # collapsed is present, not absent
        ):
            with self.subTest(key=key, value=value):
                observed = observations()
                observed['cases'][1]['outcome'][key] = value
                self.assert_verdict(self.check_pair(expectations(), observed), 'mismatch',
                                    mismatch=1, conforming_edit=1, conforming_no_edit=2)
        observed = observations()
        observed['cases'][2]['intent'] = 2  # same no_edit, different supported intent
        observed['cases'][3]['outcome'] = {'kind': 'edit', 'window': '', 'selection_utf16': None}
        self.assert_verdict(self.check_pair(expectations(), observed), 'mismatch',
                            mismatch=2, conforming_edit=2)
        observed = observations()
        observed['cases'][0]['outcome'] = {'kind': 'no_edit'}  # deletion is still an edit
        self.assert_verdict(self.check_pair(expectations(), observed), 'mismatch',
                            mismatch=1, conforming_edit=1, conforming_no_edit=2)

    def test_no_normalization_and_valid_zero_length_selection(self):
        expected, observed = expectations(), observations()
        expected['cases'][0]['outcome'] = {'kind': 'edit', 'window': 'é', 'selection_utf16': [0, 0]}
        observed['cases'][1]['outcome'] = {'kind': 'edit', 'window': 'é', 'selection_utf16': [0, 0]}
        self.assert_verdict(self.check_pair(expected, observed), 'mismatch', mismatch=1,
                            conforming_edit=1, conforming_no_edit=2)
        observed['cases'][1]['outcome']['window'] = 'é'
        self.assert_verdict(self.check_pair(expected, observed), 'conforming',
                            conforming_edit=2, conforming_no_edit=2)

    def test_missing_unqualified_wrong_input_and_unexpected_are_not_no_edit(self):
        observed = observations()
        observed['cases'][0]['input_id'] = 'other-input'
        observed['cases'][2] = {'case_id': 'rewrite', 'input_id': 'toy-02', 'status': 'unqualified'}
        observed['cases'][3]['case_id'] = 'not-predeclared'
        self.assert_verdict(self.check_pair(expectations(), observed), 'inconclusive',
                            conforming_edit=1, unqualified=2, missing=1, unexpected=1)
        observed['cases'] = []
        self.assert_verdict(self.check_pair(expectations(), observed), 'inconclusive', missing=4)

    def test_an_unexpected_case_alone_blocks_conformance(self):
        observed = observations()
        observed['cases'].append({'case_id': 'extra', 'input_id': 'toy-extra', 'status': 'unqualified'})
        self.assert_verdict(self.check_pair(expectations(), observed), 'inconclusive',
                            conforming_edit=2, conforming_no_edit=2, unexpected=1)

    def test_mismatch_remains_visible_with_incomplete_evidence(self):
        observed = observations()
        observed['cases'][0]['outcome'] = {'kind': 'no_edit'}
        observed['cases'].pop()
        self.assert_verdict(self.check_pair(expectations(), observed), 'mismatch',
                            conforming_edit=1, conforming_no_edit=1, mismatch=1, missing=1)

    def test_provenance_and_independent_artifact_gate_all_comparison(self):
        for key in ('corpus_id', 'run_id', 'candidate_id'):
            with self.subTest(key=key):
                observed = observations()
                observed['provenance'][key] = 'unmatched'
                self.assert_verdict(self.check_pair(expectations(), observed), 'inconclusive', unqualified=4)
        observed = observations()
        observed['artifact_id'] = 'declared-plan'
        self.assert_verdict(self.check_pair(expectations(), observed), 'inconclusive', unqualified=4)

    def test_unestablished_intents_are_not_evaluated_or_declared_as_expectations(self):
        for intent in (0, 1, 4, 5, 6, 99):
            with self.subTest(intent=intent):
                observed = observations()
                observed['cases'][3]['intent'] = intent
                self.assert_verdict(self.check_pair(expectations(), observed), 'inconclusive',
                                    conforming_edit=2, conforming_no_edit=1, unqualified=1)
                expected = expectations()
                expected['cases'][2]['intent'] = intent
                self.assert_invalid(self.check_pair(expected, observed), 'invalid_expectations')

    def test_malformed_observations_are_invalid_not_partial_results(self):
        for field, value in (('status', 'unqualified'), ('case_id', 'structured'),
                             ('intent', True), ('intent', 2.0), ('input_id', 'a/b'),
                             ('intent', 2**31), ('input_id', None), ('extra', 'hidden-sentinel')):
            with self.subTest(field=field, value=value):
                observed = observations()
                observed['cases'][0][field] = value
                self.assert_invalid(self.check_pair(expectations(), observed), 'invalid_observations')
        for value in ([1, 8], [4, 10], [8, 4], [4, True], [4], '4:8'):
            with self.subTest(selection=value):
                observed = observations()
                observed['cases'][1]['outcome']['selection_utf16'] = value
                self.assert_invalid(self.check_pair(expectations(), observed), 'invalid_observations')
        observed = observations()
        del observed['cases'][1]['outcome']['selection_utf16']
        self.assert_invalid(self.check_pair(expectations(), observed), 'invalid_observations')

    def test_invalid_expectations_and_roles(self):
        expected = expectations()
        expected['cases'][0]['intent'] = 3  # not a strict proposal target
        self.assert_invalid(self.check_pair(expected, observations()), 'invalid_expectations')
        self.assert_invalid(self.check_pair(expectations(), expectations()), 'invalid_observations')
        self.assert_invalid(self.check_pair(expectations(), {'schema_version': 1, 'cases': []}),
                            'invalid_observations')
        expected = expectations()
        expected['cases'] = []
        self.assert_invalid(self.check_pair(expected, observations()), 'invalid_expectations')
        expected = expectations()
        del expected['provenance']['policy']
        self.assert_invalid(self.check_pair(expected, observations()), 'invalid_expectations')
        observed = observations()
        observed['provenance']['policy'] = 'compatible'
        self.assert_invalid(self.check_pair(expectations(), observed), 'invalid_observations')

    def test_invalid_serialization_and_size(self):
        raw = json.dumps(observations()).encode('utf-8')
        duplicate_nested_key = raw.replace(b'"intent": 2', b'"intent": 2, "intent": 2', 1)
        for payload in (duplicate_nested_key,
                        raw.replace(b'"intent": 2', b'"intent": NaN', 1),
                        raw.replace(b'"intent": 2', b'"intent": 12345678901234567890', 1),
                        b'\xff', raw + b' trailing-sentinel', b'[' * 4096, b' ' * (1024 * 1024 + 1)):
            with self.subTest(prefix=payload[:12]):
                self.assert_invalid(self.check_pair(expectations(), payload), 'invalid_observations')
        for window in ('\ud800', '🦔' * (32768 + 1)):
            observed = observations()
            observed['cases'][1]['outcome']['window'] = window
            self.assert_invalid(self.check_pair(expectations(), observed), 'invalid_observations')
        observed = observations()
        observed['cases'] = [dict(observed['cases'][0], case_id=f'case-{i}') for i in range(101)]
        self.assert_invalid(self.check_pair(expectations(), observed), 'invalid_observations')

    def test_only_fixed_fields_and_counts_are_emitted_even_with_private_like_values(self):
        expected, observed = expectations(), observations()
        test_text = 'synthetic window sentinel'
        expected['cases'][0]['case_id'] = observed['cases'][1]['case_id'] = 'case-sentinel'
        expected['cases'][0]['outcome'] = {'kind': 'edit', 'window': test_text, 'selection_utf16': None}
        code, result = self.check_pair(expected, observed)
        self.assert_verdict((code, result), 'mismatch', mismatch=1, conforming_edit=1, conforming_no_edit=2)
        self.assertNotIn(hashlib.sha256(test_text.encode()).hexdigest(), json.dumps(result))
        self.assert_invalid(self.cli('arg-sentinel'), 'invalid_arguments')
        self.assert_invalid(self.cli(self.observed_path.with_name('missing-sentinel'), self.observed_path),
                            'invalid_expectations')
        self.assert_invalid(self.cli(self.expected_path, self.observed_path.parent), 'invalid_observations')


if __name__ == '__main__':
    unittest.main()
