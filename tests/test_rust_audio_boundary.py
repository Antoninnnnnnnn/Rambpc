"""Offline, stdlib-only tests for the production Cargo audio boundary check."""

from contextlib import redirect_stderr, redirect_stdout
import io
from pathlib import Path
import runpy
import subprocess
import sys
import unittest
from unittest.mock import patch

from tools import check_rust_audio_boundary as boundary


# Keep this oracle literal: deriving it from the checker would hide omissions.
REQUIRED_AUDIO_PACKAGES = frozenset({'rambler-audio', 'opus', 'opusic-sys'})
ROOT = 'rambler-cli v0.1.0 (C:\\synthetic workspace\\crates\\rambler-cli)\n'
OFF = ROOT + 'rambler-engine v0.1.0\nbytes v1.10.0\n'
ON = OFF + 'rambler-audio v0.1.0\nopus v0.3.1\nopusic-sys v0.5.0\n'
PRIVATE = 'https://synthetic-user:synthetic-password@invalid.example/private'


def completed(output, code=0, stderr=''):
    return subprocess.CompletedProcess([], code, output, stderr)


class AudioBoundaryTests(unittest.TestCase):
    def run_main(self, off=OFF, on=ON, *, responses=None, argv=None):
        stdout, stderr = io.StringIO(), io.StringIO()
        responses = responses if responses is not None else [completed(off), completed(on)]
        with patch.object(boundary.subprocess, 'run', side_effect=responses) as run:
            with patch.object(Path, 'is_file', return_value=True):
                with redirect_stdout(stdout), redirect_stderr(stderr):
                    code = boundary.main([] if argv is None else argv)
        return code, stdout.getvalue(), stderr.getvalue(), run

    def assert_failed(self, result):
        code, stdout, stderr, _ = result
        self.assertEqual(code, 1)
        self.assertEqual(stdout, '')
        self.assertIn('FAILED', stderr)
        self.assertNotIn(PRIVATE, stderr)

    def test_controls_and_exact_offline_production_commands(self):
        repo = (Path.cwd() / 'synthetic workspace').resolve()
        code, stdout, stderr, run = self.run_main(argv=['--repo', str(repo)])
        self.assertEqual(code, 0)
        self.assertIn('positive control passed', stdout)
        self.assertEqual(stderr, '')
        self.assertEqual(run.call_count, 2)
        base = [
            'cargo', 'tree', '-p', 'rambler-cli', '--locked', '--offline',
            '--edges', 'normal,build', '--target', 'all', '--prefix', 'none',
            '--format', '{p}', '--color', 'never',
        ]
        self.assertEqual(run.call_args_list[0].args[0], base + ['--no-default-features'])
        self.assertEqual(run.call_args_list[1].args[0], base + ['--features', 'audio'])
        for call in run.call_args_list:
            self.assertEqual(call.kwargs['cwd'], repo)
            self.assertTrue(call.kwargs['capture_output'])
            self.assertFalse(call.kwargs.get('shell', False))

    def test_required_audio_packages_are_exact(self):
        self.assertEqual(boundary.AUDIO_PACKAGES, REQUIRED_AUDIO_PACKAGES)

    def test_each_forbidden_package_in_audio_off_fails(self):
        for name in sorted(REQUIRED_AUDIO_PACKAGES):
            with self.subTest(package=name):
                result = self.run_main(off=OFF + f'{name} v1.2.3\n')
                self.assert_failed(result)
                self.assertIn('audio-off includes ' + name, result[2])

    def test_each_missing_positive_control_package_fails(self):
        for name in sorted(REQUIRED_AUDIO_PACKAGES):
            with self.subTest(package=name):
                on = OFF + ''.join(
                    f'{other} v1.2.3\n' for other in sorted(REQUIRED_AUDIO_PACKAGES - {name})
                )
                result = self.run_main(on=on)
                self.assert_failed(result)
                self.assertIn('positive control missing ' + name, result[2])

    def test_audio_off_cannot_substitute_for_positive_control(self):
        self.assert_failed(self.run_main(on=OFF))

    def test_neighboring_names_and_source_paths_do_not_trigger(self):
        neighbors = ''.join(
            f'{prefix}{name}{suffix} v1.2.3\n'
            for name in sorted(REQUIRED_AUDIO_PACKAGES)
            for prefix, suffix in [('not-', ''), ('', '-helper'), ('', '_sys')]
        )
        off = OFF + neighbors + 'other v1.2.3 (/synthetic/opus/rambler-audio/opusic-sys)\n'
        result = self.run_main(off=off)
        self.assertEqual(result[0], 0)

    def test_neighboring_names_do_not_satisfy_positive_control(self):
        on = OFF + ''.join(f'{name}-helper v1.2.3\n' for name in REQUIRED_AUDIO_PACKAGES)
        self.assert_failed(self.run_main(on=on))

    def test_cargo_package_format_sources_versions_and_repeats(self):
        graph = (
            ROOT
            + 'opus v0.3.1 (*)\n'
            + 'opusic-sys v0.5.0-rc.1+synthetic.2 (C:\\workspace (synthetic)\\opusic-sys) (*)\n'
            + 'rambler-audio v0.1.0 (/synthetic path (copy)/rambler-audio)\n'
            + 'opus v0.3.1\n'
            + 'other_name v1.2.3 (https://invalid.example/synthetic?branch=test#1234)\n'
        )
        self.assertEqual(
            boundary.parse_packages(graph, 'synthetic'),
            {'rambler-cli', 'opus', 'opusic-sys', 'rambler-audio', 'other_name'},
        )
        self.assertEqual(self.run_main(on=graph)[0], 0)

    def test_empty_missing_unparseable_and_wrong_root_graphs_fail_closed(self):
        invalid = [
            None, '', '\n \t\n', 'not a Cargo graph\n',
            'opus v0.3.1\n',
            ROOT + 'unparseable ' + PRIVATE + '\n',
            ROOT + 'opus-helper v1.2.3 trailing garbage\n',
            ROOT + 'opus not-a-version\n',
            ROOT + '├── opus v0.3.1\n',
        ]
        for graph in invalid:
            for position in ['off', 'on']:
                with self.subTest(graph=graph, position=position):
                    kwargs = {position: graph}
                    self.assert_failed(self.run_main(**kwargs))

    def test_nonzero_commands_cannot_pass_even_with_valid_stdout(self):
        for position in [0, 1]:
            with self.subTest(position=position):
                responses = [completed(OFF), completed(ON)]
                responses[position] = completed(ON, 101, PRIVATE)
                result = self.run_main(responses=responses)
                self.assert_failed(result)
                self.assertIn('exit 101', result[2])

    def test_spawn_timeout_and_decode_failures_are_redacted(self):
        errors = [
            FileNotFoundError(PRIVATE),
            PermissionError(PRIVATE),
            subprocess.TimeoutExpired(PRIVATE, 120, output=PRIVATE, stderr=PRIVATE),
            UnicodeDecodeError('utf-8', b'\xff', 0, 1, PRIVATE),
        ]
        for error in errors:
            for position in [0, 1]:
                with self.subTest(error=type(error).__name__, position=position):
                    responses = [error] if position == 0 else [completed(OFF), error]
                    self.assert_failed(self.run_main(responses=responses))

    def test_successful_cargo_stderr_is_not_echoed(self):
        result = self.run_main(responses=[completed(OFF, stderr=PRIVATE), completed(ON, stderr=PRIVATE)])
        self.assertEqual(result[0], 0)
        self.assertNotIn(PRIVATE, result[1] + result[2])

    def test_default_repository_is_script_relative_not_working_directory(self):
        with patch.object(boundary, 'check_boundary') as check:
            with redirect_stdout(io.StringIO()):
                self.assertEqual(boundary.main([]), 0)
        check.assert_called_once_with(Path(boundary.__file__).resolve().parents[1])

    def test_missing_manifest_fails_before_cargo(self):
        with patch.object(Path, 'is_file', return_value=False):
            with patch.object(boundary.subprocess, 'run') as run:
                with redirect_stderr(io.StringIO()) as stderr:
                    self.assertEqual(boundary.main([]), 1)
        run.assert_not_called()
        self.assertIn('Cargo.toml', stderr.getvalue())

    def test_repository_errors_are_redacted(self):
        with patch.object(boundary, 'check_boundary', side_effect=OSError(PRIVATE)):
            with redirect_stderr(io.StringIO()) as stderr:
                self.assertEqual(boundary.main([]), 1)
        self.assertNotIn(PRIVATE, stderr.getvalue())

    def test_help_is_available_without_cargo(self):
        with patch.object(boundary.subprocess, 'run') as run:
            with redirect_stdout(io.StringIO()) as stdout:
                with self.assertRaises(SystemExit) as exit_info:
                    boundary.main(['--help'])
        self.assertEqual(exit_info.exception.code, 0)
        self.assertIn('--repo', stdout.getvalue())
        run.assert_not_called()

    def test_script_entrypoint_exits_nonzero_on_violation(self):
        with patch.object(sys, 'argv', [boundary.__file__]):
            with patch.object(subprocess, 'run', side_effect=[completed(ON), completed(ON)]):
                with patch.object(Path, 'is_file', return_value=True):
                    with redirect_stderr(io.StringIO()):
                        with self.assertRaises(SystemExit) as exit_info:
                            runpy.run_path(boundary.__file__, run_name='__main__')
        self.assertEqual(exit_info.exception.code, 1)


if __name__ == '__main__':
    unittest.main()
