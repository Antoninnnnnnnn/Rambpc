"""Check the CLI's optional audio boundary using offline production Cargo graphs.

Only normal/build dependencies count: dev dependencies intentionally include Opus
and cannot prove that an audio-disabled production build excludes native audio.
No compilation is performed.
"""

import argparse
from pathlib import Path
import re
import subprocess
import sys


AUDIO_PACKAGES = frozenset({'rambler-audio', 'opus', 'opusic-sys'})
# Cargo's {p} is a package name/version, optionally followed by its source path.
# Repeated packages can carry a trailing (*); source paths may contain spaces
# and parentheses (including Windows drive letters).
PACKAGE_LINE = re.compile(
    r'(?P<name>[A-Za-z0-9_][A-Za-z0-9_-]*) '
    r'v[0-9]+\.[0-9]+\.[0-9]+'
    r'(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?'
    r'(?: \([^\r\n]*\))?'
)


class BoundaryError(Exception):
    """A safe-to-print failure without Cargo output or environment details."""


def parse_packages(output, label):
    """Reject empty/malformed graphs instead of interpreting them as no audio."""
    names = []
    for line in output.splitlines():
        if not line.strip():
            continue
        match = PACKAGE_LINE.fullmatch(line)
        if match is None:
            raise BoundaryError(f'{label}: unparseable Cargo graph (output withheld)')
        names.append(match.group('name'))
    if not names or names[0] != 'rambler-cli':
        raise BoundaryError(f'{label}: missing or empty CLI graph (output withheld)')
    return set(names)


def cargo_packages(repo, *, audio):
    label = 'audio-on' if audio else 'audio-off'
    command = [
        'cargo', 'tree', '-p', 'rambler-cli', '--locked', '--offline',
        '--edges', 'normal,build', '--target', 'all', '--prefix', 'none',
        '--format', '{p}', '--color', 'never',
    ]
    command += ['--features', 'audio'] if audio else ['--no-default-features']
    try:
        result = subprocess.run(
            command, cwd=repo, capture_output=True, text=True,
            encoding='utf-8', errors='strict', timeout=120, check=False,
        )
    except (OSError, subprocess.SubprocessError, UnicodeError):
        # Exceptions and Cargo stderr may contain registry credentials or paths.
        raise BoundaryError(f'{label}: Cargo command failed (details withheld)') from None
    if result.returncode != 0:
        raise BoundaryError(
            f'{label}: Cargo command failed (exit {result.returncode}; output withheld)'
        )
    if not isinstance(result.stdout, str):
        raise BoundaryError(f'{label}: missing Cargo graph (output withheld)')
    return parse_packages(result.stdout, label)


def check_boundary(repo):
    if not (repo / 'Cargo.toml').is_file():
        raise BoundaryError('repository must contain Cargo.toml')
    without_audio = cargo_packages(repo, audio=False)
    with_audio = cargo_packages(repo, audio=True)
    unexpected = AUDIO_PACKAGES & without_audio
    missing = AUDIO_PACKAGES - with_audio
    problems = []
    if unexpected:
        problems.append('audio-off includes ' + ', '.join(sorted(unexpected)))
    if missing:
        problems.append('audio-on positive control missing ' + ', '.join(sorted(missing)))
    if problems:
        raise BoundaryError('; '.join(problems))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        '--repo', type=Path, default=Path(__file__).resolve().parents[1],
        help='workspace root (defaults to the repository containing this script)',
    )
    args = parser.parse_args(argv)
    try:
        check_boundary(args.repo.resolve())
    except BoundaryError as error:
        print(f'Rust audio boundary FAILED: {error}', file=sys.stderr)
        return 1
    except OSError:
        print('Rust audio boundary FAILED: repository unavailable (details withheld)', file=sys.stderr)
        return 1
    print('Rust audio boundary OK: normal/build edges exclude audio when off; positive control passed')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
