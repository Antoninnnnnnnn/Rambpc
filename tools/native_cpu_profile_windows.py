"""Bounded Windows runner for the release native CPU example, no external network.

Use Python -I -B. The allowlisted build environment and private log directory
are explicit caller inputs. Nothing invokes Cargo during a timed phase.
"""
from __future__ import annotations

import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
import time

REPO = Path(__file__).resolve().parents[1]
JOB_SHA = '3ac53d5e3f202512b18cb3e4bafdc27c8b87add5c4d9afb70a3dd07462fd76aa'
BUILD_NAMES = {'cargo.exe', 'rustc.exe', 'cl.exe', 'link.exe', 'msbuild.exe', 'cmake.exe'}
LIBTEST = r'(?:test [A-Za-z0-9_:]+ \.\.\. )?'
PREFIX = re.compile(LIBTEST + r'native_cpu_profile (\{[^\r\n]+\})')
OGG_PREFIX = re.compile(LIBTEST + r'native_cpu_ogg ((?:[0-9a-f]{2})+)')
OWNER = 'rambler-native-cpu-profile-v1\n'
LIMIT = 3 * 1024**3


def parse_row(raw: bytes):
    lines = raw.decode('utf-8').splitlines()
    rows = [json.loads(m[1]) for line in lines if (m := PREFIX.fullmatch(line))]
    if len(rows) != 1 or not isinstance(rows[0], dict):
        raise ValueError('expected one profiling row')
    return rows[0]


def parse_ogg(raw: bytes) -> bytes:
    lines = raw.decode('utf-8').splitlines()
    items = [bytes.fromhex(m[1]) for line in lines if (m := OGG_PREFIX.fullmatch(line))]
    if len(items) != 1 or not items[0].startswith(b'OggS'):
        raise ValueError('expected one raw Ogg proof line')
    return items[0]


def test_parser():
    raw = b'native_cpu_profile {"value":123}'
    ogg = b'native_cpu_ogg 4f67675300'
    if parse_ogg(b'test module::raw ... ' + ogg + b'\r\nok') != b'OggS\0':
        raise AssertionError('could not parse raw Ogg fixture')
    for fixture in [ogg + b'0', ogg + b'\n' + ogg, b'native_cpu_ogg ffff']:
        try:
            parse_ogg(fixture)
        except ValueError:
            continue
        raise AssertionError('accepted malformed raw Ogg fixture')
    for fixture in [raw + b'\r\n', b'running 1 test\r\ntest module::profile_row ... ' + raw
                    + b'\r\nok\r\n\r\ntest result: ok.\r\n']:
        if parse_row(fixture) != {'value': 123}:
            raise AssertionError('could not parse profiling fixture')
    for fixture in [b'debug ' + raw, b'test module::t ...  ' + raw, raw + b' ok',
                    raw + b'\n' + raw, b'native_cpu_profile {broken}', b'ok']:
        try:
            parse_row(fixture)
        except ValueError:
            continue
        raise AssertionError('accepted malformed profiling fixture')


def snapshot_builders():
    # Read executable basenames only. Never open any process or read command lines.
    class Entry(c.Structure):
        _fields_ = [('size', w.DWORD), ('usage', w.DWORD), ('pid', w.DWORD),
                    ('heap', c.c_size_t), ('module', w.DWORD), ('threads', w.DWORD),
                    ('parent', w.DWORD), ('priority', w.LONG), ('flags', w.DWORD),
                    ('name', w.WCHAR * 260)]

    k = c.WinDLL('kernel32', use_last_error=True)
    k.CreateToolhelp32Snapshot.argtypes = (w.DWORD, w.DWORD)
    k.CreateToolhelp32Snapshot.restype = w.HANDLE
    k.CloseHandle.argtypes = (w.HANDLE,)
    k.CloseHandle.restype = w.BOOL
    for fn in (k.Process32FirstW, k.Process32NextW):
        fn.argtypes = (w.HANDLE, c.POINTER(Entry))
        fn.restype = w.BOOL
    handle = k.CreateToolhelp32Snapshot(2, 0)
    if handle == c.c_void_p(-1).value:
        raise c.WinError(c.get_last_error())
    try:
        entry = Entry()
        entry.size = c.sizeof(entry)
        names = []
        if not k.Process32FirstW(handle, c.byref(entry)):
            raise c.WinError(c.get_last_error())
        while True:
            name = entry.name.lower()
            if name in BUILD_NAMES:
                names.append(name)
            if not k.Process32NextW(handle, c.byref(entry)):
                if c.get_last_error() != 18:  # ERROR_NO_MORE_FILES
                    raise c.WinError(c.get_last_error())
                return sorted(names)
    finally:
        if not k.CloseHandle(handle):
            raise c.WinError(c.get_last_error())


def gate(log):
    for attempt in range(61):
        names = snapshot_builders()
        with log.open('a', encoding='utf-8') as out:
            out.write(json.dumps({'attempt': attempt, 'builders': names}) + '\n')
        if not names:
            return attempt
        if attempt < 60:
            time.sleep(5)
    raise RuntimeError('build gate busy for five minutes')


def owned_job():
    path = REPO / 'tools/windows_owned_job.py'
    if hashlib.sha256(path.read_bytes()).hexdigest() != JOB_SHA:
        raise ValueError('Windows job helper does not match pinned file')
    spec = importlib.util.spec_from_file_location('windows_owned_job', path)
    job = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(job)
    return job


def target_size(path):
    return sum(p.stat().st_size for p in path.rglob('*') if p.is_file())


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--self-test', action='store_true')
    p.add_argument('--env', type=Path)
    p.add_argument('--target', type=Path)
    p.add_argument('--logs', type=Path)
    p.add_argument('phase', nargs='?', choices=['warm-build', 'build', 'attribution',
        'build-pair-baseline', 'build-candidate', 'proof', 'pairs', 'test', 'clippy', 'fmt', 'clean'])
    args = p.parse_args()
    test_parser()
    if args.self_test and args.phase is None:
        print('profiling parser fixtures passed')
        return
    if not (args.env and args.target and args.logs and args.phase):
        p.error('phase requires --env, --target and --logs')
    target = args.target.resolve()
    if target != args.target.absolute() or target.parent.name != 'target' or not re.fullmatch(r'cpu48[a-z0-9-]*', target.name):
        p.error('requires physically shallow cpu48* target, not an alias')
    marker = target / '.native-cpu-profile-owner'
    if args.phase == 'warm-build':
        target.mkdir(exist_ok=False)
        marker.write_text(OWNER, encoding='ascii', newline='\n')
    if marker.is_symlink() or marker.read_text(encoding='ascii') != OWNER:
        p.error('target has no native CPU profile owner marker')
    if args.phase == 'clean':
        print(f'removing {target_size(target)} owned target bytes')
        shutil.rmtree(target)
        return
    print('profiling parser fixtures passed', flush=True)
    logs = args.logs.resolve()
    logs.mkdir(exist_ok=True)
    env = json.loads(args.env.read_text(encoding='utf-8'))
    if env.get('CARGO_BUILD_JOBS') != '2' or env.get('CARGO_NET_OFFLINE') != 'true':
        raise ValueError('offline two-job build environment required')
    env['CARGO_TARGET_DIR'] = str(target)
    # MSVC linker temporaries must be writable and physically shallow too. The
    # allowlisted environment may have been extracted on a different job.
    temp = target / 'tmp'
    temp.mkdir(exist_ok=True)
    env['TEMP'] = env['TMP'] = str(temp)
    job = owned_job()

    def capture(label, cmd, *, timed=False):
        waits = gate(logs / f'{label}.gate.jsonl') if timed else None
        result, raw = job.capture_windows(cmd, logs / f'{label}.log', timeout=900, limit=2 * 1024**2,
                                          cwd=REPO, env=env)
        if timed:
            builders_after = snapshot_builders()
            with (logs / f'{label}.gate.jsonl').open('a', encoding='utf-8') as out:
                out.write(json.dumps({'after': builders_after}) + '\n')
            if builders_after:
                raise RuntimeError(f'build process observed at end of {label}; batch invalid')
        if not job.capture_ok(result):
            raise RuntimeError(f'{label} failed: exit={result["actual_exit"]}; see private log')
        size = target_size(target) if not timed else None
        if size is not None and size > LIMIT:
            raise RuntimeError('owned target exceeded 3 GiB')
        return raw, waits, size

    cargo = shutil.which('cargo.exe', path=env['PATH'])
    if not cargo:
        raise RuntimeError('Cargo missing from allowlisted PATH')
    locked = ['--release', '--offline', '--locked']
    cli = ['-p', 'rambler-cli', '--features', 'audio']
    phase = args.phase
    exe = target / 'release/examples/native_cpu_profile.exe'
    baseline = logs / 'native_cpu_profile_baseline.exe'
    pair_baseline = logs / 'native_cpu_profile_pair_baseline.exe'
    if phase in ('warm-build', 'build', 'build-pair-baseline', 'build-candidate', 'test', 'clippy', 'fmt'):
        if phase == 'warm-build':
            cmd = ['build', *locked, *cli, '--example', 'native_qualification']
        elif phase in ('build', 'build-pair-baseline', 'build-candidate'):
            cmd = ['build', *locked, *cli, '--example', 'native_cpu_profile']
        elif phase == 'test':
            cmd = ['test', *locked, '-p', 'rambler-audio']
            _, _, size = capture('test-profile', [cargo, 'test', *locked, *cli, '--example', 'native_cpu_profile'])
            print(f'test-profile passed; target logical bytes: {size}')
        elif phase == 'clippy':
            cmd = ['clippy', *locked, *cli, '-p', 'rambler-audio', '--all-targets', '--', '-D', 'warnings']
        else:
            cmd = ['fmt', '--all', '--', '--check']
        _, _, size = capture(phase, [cargo, *cmd])
        if phase == 'build-pair-baseline':
            with pair_baseline.open('xb') as dest, exe.open('rb') as source:
                shutil.copyfileobj(source, dest)
        print(f'{phase} passed; target logical bytes: {size}')
        return

    def verify_row(row, mode, measurement, identity):
        if row.get('mode') != mode or row.get('measurement') != measurement:
            raise ValueError('profiling row does not match its phase')
        if identity is not None and any(row[key] != identity[key] for key in ('input_sha256', 'identity')):
            raise ValueError('profiling output identity changed')

    identity = None
    if phase != 'attribution':
        with (logs / 'attribution.jsonl').open(encoding='ascii') as rows:
            identity = json.loads(rows.readline())
    if phase == 'proof':
        with (logs / 'proof.json').open('x', encoding='ascii') as proof:
            results = {}
            for mode in ('ready', 'channel'):
                streams = []
                for version, path in [('baseline', pair_baseline), ('candidate', exe)]:
                    raw, waits, _ = capture(f'proof-{version}-{mode}', [path, mode, 'control', '--emit-ogg'], timed=True)
                    row = parse_row(raw)
                    verify_row(row, mode, 'control', identity)
                    stream = parse_ogg(raw)
                    if hashlib.sha256(stream).hexdigest() != row['identity']['ogg_sha256']:
                        raise ValueError('raw Ogg proof digest mismatch')
                    streams.append(stream)
                if streams[0] != streams[1]:
                    raise ValueError('raw Ogg is not byte identical')
                results[mode] = {'byte_equal': True, **row['identity']}
            proof.write(json.dumps(results, sort_keys=True) + '\n')
        print('both modes proved raw Ogg byte equality')
        return
    if phase == 'pairs' and not all(json.loads((logs / 'proof.json').read_text())[mode]['byte_equal']
                                     for mode in ('ready', 'channel')):
        raise RuntimeError('raw Ogg proof required before paired batch')
    rows_path = logs / f'{phase}.jsonl'
    # Opening exclusively enforces the one-batch rule, including partial failures.
    with rows_path.open('x', encoding='ascii') as rows:
        if phase == 'attribution':
            with baseline.open('xb') as dest, exe.open('rb') as source:
                shutil.copyfileobj(source, dest)
            plan = [(i, 'baseline', mode, meter) for i in range(7)
                    for mode in (('ready', 'channel') if i % 2 == 0 else ('channel', 'ready'))
                    for meter in (('metered', 'control') if i % 2 == 0 else ('control', 'metered'))]
        else:
            plan = [(i, version, mode, 'control') for i in range(10)
                    for mode in (('ready', 'channel') if i % 2 == 0 else ('channel', 'ready'))
                    for version in (('baseline', 'candidate') if i % 2 == 0 else ('candidate', 'baseline'))]
        for index, version, mode, meter in plan:
            label = f'{phase}-{index}-{version}-{mode}-{meter}'
            old = baseline if phase == 'attribution' else pair_baseline
            raw, waits, _ = capture(label, [old if version == 'baseline' else exe, mode, meter], timed=True)
            row = parse_row(raw)
            verify_row(row, mode, meter, identity)
            if identity is None:
                identity = row
            row.update(index=index, version=version, waits=waits)
            rows.write(json.dumps(row, sort_keys=True) + '\n')
            rows.flush()
        print(f'{phase} complete: {len(plan)} timed sessions')


if __name__ == '__main__':
    main()
