import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'tools' / 'compare_native_qualification.py'
spec = importlib.util.spec_from_file_location('native_comparator', SCRIPT)
comparator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparator)


def specimen(repeats=2):
    return {'schema': 'rambler-native-qualification-v1', 'scope': 'synthetic-localhost-only',
            'repeats': repeats, 'rows': [
                {'case': case, 'repeat': repeat, 'pcm_samples': 16000,
                 'pcm_sha256': 'a' * 64, 'wire_responses_sha256': 'b' * 64,
                 'rendered_sha256': 'c' * 64, 'request_count': 1, 'response_count': 2,
                 'ogg_bytes': 42, 'first_response_us': 10 + repeat, 'session_us': 20 + repeat}
                for repeat in range(repeats) for case in comparator.CASES]}


class ComparatorTests(unittest.TestCase):
    def test_valid_pairing_and_all_samples(self):
        a = specimen(3)
        b = copy.deepcopy(a)
        for i, delta in enumerate((5, -3, 0)):
            b['rows'][i * 4]['first_response_us'] += delta
            b['rows'][i * 4]['session_us'] += delta * 2
        result = comparator.compare_reports(a, b)
        self.assertEqual(result['status'], 'accepted')
        row = result['cases'][0]
        self.assertEqual(row['sample_count'], 3)
        self.assertEqual(row['first_response_us_delta'],
                         {'samples': [5, -3, 0], 'median': 0, 'min': -3, 'max': 5})
        self.assertEqual(row['session_us_delta']['samples'], [10, -6, 0])

    def test_shape_order_types_and_bounds(self):
        mutations = [lambda x: x.update(extra='private'), lambda x: x.pop('scope'),
                     lambda x: x.update(repeats=True), lambda x: x.update(repeats=0),
                     lambda x: x['rows'].pop(), lambda x: x['rows'].reverse(),
                     lambda x: x['rows'][0].update(case='private text'),
                     lambda x: x['rows'][0].update(repeat=False),
                     lambda x: x['rows'][0].update(pcm_samples=160001),
                     lambda x: x['rows'][0].update(request_count=0),
                     lambda x: x['rows'][0].update(response_count=True),
                     lambda x: x['rows'][0].update(ogg_bytes=0),
                     lambda x: x['rows'][0].update(session_us=-1),
                     lambda x: x['rows'][0].update(first_response_us=21),
                     lambda x: x['rows'][0].update(pcm_sha256='A' * 64),
                     lambda x: x['rows'][0].update(extra='private')]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                report = specimen()
                mutation(report)
                with self.assertRaises(comparator.InvalidReport):
                    comparator.validate_report(report)
        report = specimen(1)
        report['rows'][0].update(first_response_us=0, session_us=0)
        comparator.validate_report(report)

    def test_semantic_mismatch_refuses_every_timing(self):
        for field, value in [('pcm_samples', 1), ('pcm_sha256', 'd'*64),
                             ('wire_responses_sha256', 'd'*64), ('rendered_sha256', 'd'*64),
                             ('request_count', 2), ('response_count', 3), ('ogg_bytes', 43)]:
            with self.subTest(field=field):
                b = specimen()
                b['rows'][4][field] = value
                self.assertEqual(comparator.compare_reports(specimen(), b),
                                 {'status': 'refused', 'reason': 'semantic_mismatch'})
        self.assertEqual(comparator.compare_reports(specimen(1), specimen(2))['reason'], 'semantic_mismatch')

    def test_json_input_rejections(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'report'
            for data in [b'{"schema":1,"schema":2}', b'{"x":NaN}', b'{"x":1.0}',
                         b'\xff', b' ' * (comparator.MAX_REPORT_BYTES + 1)]:
                with self.subTest(data=data[:20]):
                    path.write_bytes(data)
                    with self.assertRaises(comparator.InvalidReport):
                        comparator.read_report(path)
            path.write_text(json.dumps(specimen()), encoding='utf-8')
            self.assertEqual(comparator.read_report(path), specimen())
            link = Path(folder) / 'link'
            try:
                link.symlink_to(path)
            except (OSError, NotImplementedError):
                pass
            else:
                with self.assertRaises(comparator.InvalidReport):
                    comparator.read_report(link)
            with self.assertRaises(comparator.InvalidReport):
                comparator.read_report(folder)

    def test_cli_json_only_and_no_path_or_input_leaks(self):
        with tempfile.TemporaryDirectory() as folder:
            a = Path(folder) / 'private-name-a'
            b = Path(folder) / 'private-name-b'
            a.write_text(json.dumps(specimen()), encoding='utf-8')
            b.write_text(json.dumps(specimen()), encoding='utf-8')
            def run(*args):
                return subprocess.run([sys.executable, '-I', str(SCRIPT), *map(str, args)],
                                      capture_output=True, text=True, check=False)
            good = run(a, b)
            self.assertEqual(good.returncode, 0)
            self.assertEqual(good.stderr, '')
            self.assertEqual(json.loads(good.stdout)['status'], 'accepted')
            b.write_text('{"private-secret": 1}', encoding='utf-8')
            for proc in (run(a, b), run(a, Path(folder) / 'missing-private'), run()):
                self.assertNotEqual(proc.returncode, 0)
                self.assertEqual(proc.stderr, '')
                self.assertEqual(json.loads(proc.stdout)['status'], 'refused')
                self.assertNotIn('private', proc.stdout)
                self.assertNotIn(folder, proc.stdout)


if __name__ == '__main__':
    unittest.main()
