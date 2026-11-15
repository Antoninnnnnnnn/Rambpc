"""Run the ignored native public-root TLS smoke directly, without Cargo's env injection.

Cargo only builds and reports the unit-test executable; it never launches the test.
All child environments are copies with certificate overrides removed.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Mapping


NATIVE_ROOT_TEST = 'connection::tests::tls_setup_native_public_roots_smoke'


class NativeRootsError(Exception):
    """A safe-to-print failure without child output or private paths."""


def select_transport_test_executable(output: str) -> Path:
    """Select one Cargo lib-test artifact, rejecting malformed or ambiguous output."""
    invalid = 'invalid Cargo artifact metadata (output withheld)'
    if not isinstance(output, str):
        raise ValueError(invalid)
    matches = []
    # Cargo emits LF-delimited JSON; Unicode separators may occur inside paths.
    for line in output.split('\n'):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except (ValueError, RecursionError):
            raise ValueError(invalid) from None
        if not isinstance(record, dict) or not isinstance(record.get('reason'), str):
            raise ValueError(invalid)
        if not record['reason']:
            raise ValueError(invalid)
        if record['reason'] != 'compiler-artifact':
            continue
        target = record.get('target')
        profile = record.get('profile')
        if not isinstance(target, dict) or not isinstance(profile, dict):
            raise ValueError(invalid)
        name = target.get('name')
        kinds = target.get('kind')
        if (
            not isinstance(name, str) or not name
            or not isinstance(kinds, list) or not kinds
            or any(not isinstance(kind, str) or not kind for kind in kinds)
            or type(profile.get('test')) is not bool
            or 'executable' not in record
        ):
            raise ValueError(invalid)
        executable = record['executable']
        if executable is not None and (
            not isinstance(executable, str) or not executable.strip() or '\x00' in executable
        ):
            raise ValueError(invalid)
        if (
            name == 'rambler_transport' and 'lib' in kinds
            and profile['test'] is True and executable is not None
        ):
            matches.append(Path(executable))
    if len(matches) != 1:
        raise ValueError('expected exactly one transport lib-test artifact (output withheld)')
    return matches[0]


def clean_child_environment(mapping: Mapping[str, str]) -> dict[str, str]:
    """Copy the parent environment, removing certificate overrides case-insensitively."""
    return {
        key: value for key, value in mapping.items()
        if key.upper() not in {'SSL_CERT_FILE', 'SSL_CERT_DIR'}
    }


def validate_ignored_test_listing(output: str) -> None:
    """Require exactly the intended ignored unit test, with no extras."""
    if not isinstance(output, str) or output.split() != [NATIVE_ROOT_TEST + ':', 'test']:
        raise ValueError('expected exactly one ignored native-root test (output withheld)')


def run_command(command, *, repo, label, timeout):
    try:
        result = subprocess.run(
            command, cwd=repo, env=clean_child_environment(os.environ),
            capture_output=True, text=True, encoding='utf-8', errors='strict',
            timeout=timeout, check=False,
        )
    except subprocess.TimeoutExpired:
        # This is a process deadline, not a guarantee that a started blocking OS
        # root loader can be interrupted or that it had even started.
        raise NativeRootsError(f'{label}: command timed out (output withheld)') from None
    except (OSError, subprocess.SubprocessError, UnicodeError):
        raise NativeRootsError(f'{label}: command failed (details withheld)') from None
    if result.returncode != 0:
        raise NativeRootsError(
            f'{label}: command failed (exit {result.returncode}; output withheld)'
        )
    return result.stdout


def check_native_roots(repo):
    if not (repo / 'Cargo.toml').is_file():
        raise NativeRootsError('repository must contain Cargo.toml')
    output = run_command(
        [
            'cargo', 'test', '--workspace', '--lib', '--all-features', '--locked',
            '--offline', '--no-run', '--message-format=json',
        ],
        repo=repo, label='artifact-build', timeout=120,
    )
    try:
        executable = select_transport_test_executable(output)
    except ValueError:
        raise NativeRootsError('artifact-selection: invalid or ambiguous metadata (output withheld)') from None
    if not executable.is_absolute():
        executable = repo / executable
    if not executable.is_file():
        raise NativeRootsError('artifact-selection: test executable unavailable (path withheld)')
    test_cwd = repo / 'crates' / 'rambler-transport'
    listing = run_command(
        [str(executable), '--list', '--format', 'terse', '--ignored', '--exact', NATIVE_ROOT_TEST],
        repo=test_cwd, label='ignored-test-listing', timeout=20,
    )
    try:
        validate_ignored_test_listing(listing)
    except ValueError:
        raise NativeRootsError('ignored-test-listing: expected exactly one test (output withheld)') from None
    run_command(
        [str(executable), NATIVE_ROOT_TEST, '--ignored', '--exact'],
        repo=test_cwd, label='native-root-smoke', timeout=30,
    )


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        '--repo', type=Path, default=Path(__file__).resolve().parents[1],
        help='workspace root (defaults to the repository containing this script)',
    )
    args = parser.parse_args(argv)
    try:
        check_native_roots(args.repo.resolve())
    except NativeRootsError as error:
        print(f'Rust native roots FAILED: {error}', file=sys.stderr)
        return 1
    except OSError:
        print('Rust native roots FAILED: filesystem unavailable (details withheld)', file=sys.stderr)
        return 1
    print('Rust native roots OK: exact ignored smoke passed via direct test executable')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
