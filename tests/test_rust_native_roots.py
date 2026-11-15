"""Offline, stdlib-only coverage for the native-root test launcher contract.

Load the helper by its absolute file path so discovery needs neither an
installed project nor the repository root on Python's import search path.
All Cargo messages and environment mappings below are synthetic.
"""

import importlib.util
import json
from pathlib import Path
import sys
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
HELPER_PATH = REPOSITORY_ROOT / 'tools' / 'check_rust_native_roots.py'
SPEC = importlib.util.spec_from_file_location('_native_roots_helper_under_test', HELPER_PATH)
if SPEC is None or SPEC.loader is None:
    raise ImportError('Cannot load native-root helper from its repository path')
native_roots = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = native_roots
SPEC.loader.exec_module(native_roots)


# Independent oracle: changing both the helper and the test name must not
# silently select a different (or nonexistent) Rust test.
NATIVE_ROOT_TEST = 'connection::tests::tls_setup_native_public_roots_smoke'
EXECUTABLE = '/synthetic workspace/target/debug/deps/rambler_transport-0123456789abcdef'


def artifact(**changes):
    """Return a fresh, representative Cargo compiler-artifact JSON object."""
    message = {
        'reason': 'compiler-artifact',
        'package_id': 'path+file:///synthetic/crates/rambler-transport#0.1.0',
        'manifest_path': '/synthetic/crates/rambler-transport/Cargo.toml',
        'target': {
            'kind': ['lib'],
            'crate_types': ['lib'],
            'name': 'rambler_transport',
            'src_path': '/synthetic/crates/rambler-transport/src/lib.rs',
            'edition': '2021',
            'doc': True,
            'doctest': True,
            'test': True,
        },
        'profile': {
            'opt_level': '0',
            'debuginfo': 2,
            'debug_assertions': True,
            'overflow_checks': True,
            'test': True,
        },
        'features': [],
        'filenames': [EXECUTABLE],
        'executable': EXECUTABLE,
        'fresh': False,
    }
    message.update(changes)
    return message


def cargo_output(*messages):
    return '\n'.join(json.dumps(message) for message in messages) + '\n'


class TransportExecutableSelectionTests(unittest.TestCase):
    def assert_rejected(self, output):
        with self.assertRaises(ValueError):
            native_roots.select_transport_test_executable(output)

    def test_exact_matching_artifact_returns_path(self):
        selected = native_roots.select_transport_test_executable(cargo_output(artifact()))
        self.assertIsInstance(selected, Path)
        self.assertEqual(selected, Path(EXECUTABLE))

    def test_literal_unicode_line_separators_in_json_paths_are_preserved(self):
        for separator in [chr(0x85), chr(0x2028), chr(0x2029)]:
            with self.subTest(codepoint=ord(separator)):
                executable = EXECUTABLE + separator + 'unicode'
                output = json.dumps(artifact(executable=executable), ensure_ascii=False) + '\n'
                self.assertEqual(
                    native_roots.select_transport_test_executable(output), Path(executable)
                )

    def test_empty_streams_fail_closed(self):
        for output in ['', '\n', ' \t\r\n']:
            with self.subTest(output=output):
                self.assert_rejected(output)

    def test_zero_matching_artifacts_is_rejected(self):
        self.assert_rejected(cargo_output({'reason': 'build-finished', 'success': True}))

    def test_duplicate_artifacts_even_with_same_path_are_rejected(self):
        self.assert_rejected(cargo_output(artifact(), artifact()))

    def test_multiple_distinct_matching_executables_are_rejected(self):
        second = artifact(executable=EXECUTABLE + '-second')
        self.assert_rejected(cargo_output(artifact(), second))

    def test_non_test_profiles_are_excluded(self):
        excluded = artifact(profile=dict(artifact()['profile'], test=False))
        self.assert_rejected(cargo_output(excluded))
        self.assertEqual(
            native_roots.select_transport_test_executable(cargo_output(excluded, artifact())),
            Path(EXECUTABLE),
        )

    def test_malformed_test_profile_flags_fail_closed(self):
        for flag in [None, 0, 1, 'true', 'false', [], {}]:
            with self.subTest(flag=flag):
                malformed = artifact(profile=dict(artifact()['profile'], test=flag))
                self.assert_rejected(cargo_output(malformed))
                self.assert_rejected(cargo_output(malformed, artifact()))

    def test_null_executable_is_excluded(self):
        excluded = artifact(executable=None)
        self.assert_rejected(cargo_output(excluded))
        self.assertEqual(
            native_roots.select_transport_test_executable(cargo_output(excluded, artifact())),
            Path(EXECUTABLE),
        )

    def test_examples_integration_tests_and_binaries_are_excluded(self):
        for kind in ['example', 'test', 'bin', 'bench', 'custom-build']:
            with self.subTest(kind=kind):
                target = dict(artifact()['target'], kind=[kind])
                excluded = artifact(target=target)
                self.assert_rejected(cargo_output(excluded))
                self.assertEqual(
                    native_roots.select_transport_test_executable(cargo_output(excluded, artifact())),
                    Path(EXECUTABLE),
                )

    def test_wrong_target_names_are_excluded(self):
        for name in ['rambler_engine', 'rambler_transport_helper', 'not_rambler_transport']:
            with self.subTest(name=name):
                target = dict(artifact()['target'], name=name)
                excluded = artifact(target=target)
                self.assert_rejected(cargo_output(excluded))
                self.assertEqual(
                    native_roots.select_transport_test_executable(cargo_output(excluded, artifact())),
                    Path(EXECUTABLE),
                )

    def test_wrong_message_reasons_are_excluded(self):
        for reason in ['compiler-message', 'build-script-executed', 'build-finished']:
            with self.subTest(reason=reason):
                excluded = artifact(reason=reason)
                self.assert_rejected(cargo_output(excluded))
                self.assertEqual(
                    native_roots.select_transport_test_executable(cargo_output(excluded, artifact())),
                    Path(EXECUTABLE),
                )

    def test_malformed_json_anywhere_fails_closed(self):
        good = cargo_output(artifact())
        for invalid in ['not JSON\n', '{\n', '{"reason":}\n', '{} trailing\n']:
            for output in [invalid, invalid + good, good + invalid]:
                with self.subTest(output=output):
                    self.assert_rejected(output)

    def test_nonobject_json_anywhere_fails_closed(self):
        good = cargo_output(artifact())
        for value in [None, True, 3, 'compiler-artifact', [], [artifact()]]:
            invalid = cargo_output(value)
            for output in [invalid, invalid + good, good + invalid]:
                with self.subTest(value=value, output=output):
                    self.assert_rejected(output)

    def test_package_id_format_variations_do_not_change_selection(self):
        for package_id in [
            'rambler-transport 0.1.0 (path+file:///synthetic/crates/rambler-transport)',
            'path+file:///synthetic/crates/rambler-transport#rambler-transport@0.1.0',
            'path+file:///synthetic/crates/rambler-transport#0.1.0',
        ]:
            with self.subTest(package_id=package_id):
                self.assertEqual(
                    native_roots.select_transport_test_executable(
                        cargo_output(artifact(package_id=package_id))
                    ),
                    Path(EXECUTABLE),
                )

    def test_unrelated_valid_cargo_messages_are_accepted(self):
        unrelated = artifact(target=dict(artifact()['target'], name='rambler_engine'))
        output = cargo_output(
            {'reason': 'compiler-message', 'message': {'rendered': 'synthetic warning'}},
            unrelated,
            artifact(),
            {'reason': 'build-script-executed', 'package_id': 'synthetic-build-script'},
            {'reason': 'build-finished', 'success': True},
        )
        self.assertEqual(native_roots.select_transport_test_executable(output), Path(EXECUTABLE))


class ChildEnvironmentTests(unittest.TestCase):
    def test_both_certificate_overrides_are_removed_from_a_copy(self):
        original = {
            'SSL_CERT_FILE': '/synthetic/no-certificate-file',
            'SSL_CERT_DIR': '/synthetic/no-certificate-directory',
            'PATH': '/synthetic/bin',
            'LANG': 'C.UTF-8',
        }
        snapshot = original.copy()
        cleaned = native_roots.clean_child_environment(original)
        self.assertIsInstance(cleaned, dict)
        self.assertIsNot(cleaned, original)
        self.assertEqual(cleaned, {'PATH': '/synthetic/bin', 'LANG': 'C.UTF-8'})
        self.assertEqual(original, snapshot)

    def test_case_variants_of_both_overrides_are_removed(self):
        original = {
            'SSL_CERT_FILE': 'synthetic-file',
            'ssl_cert_file': 'synthetic-file',
            'Ssl_Cert_File': 'synthetic-file',
            'SSL_CERT_DIR': 'synthetic-directory',
            'ssl_cert_dir': 'synthetic-directory',
            'Ssl_Cert_Dir': 'synthetic-directory',
            'OTHER': 'preserved',
        }
        snapshot = original.copy()
        self.assertEqual(native_roots.clean_child_environment(original), {'OTHER': 'preserved'})
        self.assertEqual(original, snapshot)

    def test_absent_overrides_preserve_all_other_keys_and_return_a_copy(self):
        for original in [{}, {
            'PATH': '/synthetic/bin',
            'SSL_CERT_FILE_EXTRA': 'preserved',
            'PREFIX_SSL_CERT_DIR': 'preserved',
            'SSL_CERT_DIRTY': 'preserved',
            'EMPTY': '',
        }]:
            with self.subTest(original=original):
                snapshot = original.copy()
                cleaned = native_roots.clean_child_environment(original)
                self.assertIsInstance(cleaned, dict)
                self.assertIsNot(cleaned, original)
                self.assertEqual(cleaned, snapshot)
                self.assertEqual(original, snapshot)
                cleaned['ADDED_IN_CHILD'] = 'synthetic'
                self.assertEqual(original, snapshot)

    def test_either_override_alone_is_removed(self):
        for name in ['SSL_CERT_FILE', 'SSL_CERT_DIR']:
            with self.subTest(name=name):
                original = {name: 'synthetic', 'KEEP': 'unchanged'}
                self.assertEqual(
                    native_roots.clean_child_environment(original), {'KEEP': 'unchanged'}
                )
                self.assertEqual(original, {name: 'synthetic', 'KEEP': 'unchanged'})


class IgnoredTestListingTests(unittest.TestCase):
    def test_exported_test_name_matches_the_required_literal(self):
        self.assertEqual(native_roots.NATIVE_ROOT_TEST, NATIVE_ROOT_TEST)

    def test_exact_listing_accepts_whitespace_and_returns_none(self):
        for output in [
            NATIVE_ROOT_TEST + ': test',
            NATIVE_ROOT_TEST + ': test\n',
            '\n \t' + NATIVE_ROOT_TEST + ':\t test\r\n\n',
        ]:
            with self.subTest(output=output):
                self.assertIsNone(native_roots.validate_ignored_test_listing(output))

    def test_empty_duplicate_wrong_name_and_extra_tokens_are_rejected(self):
        valid = NATIVE_ROOT_TEST + ': test'
        invalid_outputs = [
            '',
            ' \t\r\n',
            valid + '\n' + valid,
            'connection::tests::different_test: test',
            NATIVE_ROOT_TEST + '_extra: test',
            NATIVE_ROOT_TEST + ': benchmark',
            NATIVE_ROOT_TEST + ' test',
            NATIVE_ROOT_TEST + ':',
            'prefix ' + valid,
            valid + ' extra',
            valid + '\n1 test, 0 benchmarks',
            valid + '\nconnection::tests::different_test: test',
        ]
        for output in invalid_outputs:
            with self.subTest(output=output):
                with self.assertRaises(ValueError):
                    native_roots.validate_ignored_test_listing(output)


if __name__ == '__main__':
    unittest.main()
