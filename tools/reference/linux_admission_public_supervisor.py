"""SOURCE ONLY; R2 is NOT qualified or authorized for native execution.

Dedicated stdlib supervisor, Python -I -S -B, Linux x86_64, non-root.
--unit performs only pure public-data checks; MAIN may run it after review.
--build-only: exactly 2 locked/offline Cargo no-run builds, freeze independent
single-link ELF copies, run only the frozen 17-test Fake harness (3 direct
children). Never launch the example main during build.
--observe-only: require externally pinned successful FINAL build report AND
receipt; consume one permanent reservation, then at most 11 target launches:
silent noargs, 2 pre-READY safety negatives, 8 hardened public-frame cases.
Never launch single-session mode, accept credentials, retry, resume or replay.
No service, device, socket, crash experiment, environment-content observation.

Not a sandbox. Assumes trusted existing tools/libraries/public Cargo sources,
normal procfs/pidfds, one thread/reaper, no concurrent same-UID mutation,
no build delegation to preexisting daemons and public-only interpreter startup.
SIGKILL of supervisor, kernel failure, uninterruptible children cannot guarantee
cleanup. Cleanup=true requires actual ECHILD, not merely a kill attempt.
"""
from types import SimpleNamespace
import contextlib
import ctypes
import errno
import hashlib
import json
import os
import re
import select
import signal
import stat
import sys
import time
import uuid

if sys.platform == 'linux':
    import resource
else:
    resource = None

PUBLIC = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
CAP = 512 * 1024 * 1024
IMAGE_CAP = 128 * 1024 * 1024
PCM_BYTES = 117440
PCM_SHA256 = 'cea1bc1614f738048b1a4f1bcad83f70aee42b9621b8b754f8aacbe1395b08c9'
READY = b'REAL_ADMISSION_READY_V1\n'
MAGIC = b'RLA1\0\r\n\x1a'
PUBLIC_FIELDS = (b'invented-public-frame-key', b'invalid.public.frame', b'invented-public-frame-cert')
EXAMPLE = 'crates/rambler-cli/examples/linux_real_admission.rs'
FAKE_TESTS = 'crates/rambler-cli/examples/support/linux_real_admission_tests.rs'
WORKSPACE_OWNER_NAME = 'owner-public-v1.json'
OWNER_NAME = 'admission-public-owner-v1.json'
OWNER_VALUE = {'schema': 'linux-admission-public-owner-v1', 'public_only': True}
MANIFEST_SCHEMA = 'linux-admission-public-source-v1'
CACHE_SCHEMA = 'linux-admission-public-cache-v1'
BUILD_SCHEMA = 'linux-admission-public-build-r2-v1'
REPORT_SCHEMA = 'linux-admission-public-supervisor-r2-v1'
FAKE_COUNTS = {'passed': 17, 'failed': 0, 'ignored': 0, 'measured': 0, 'filtered': 0, 'suites': 1}
CANCELLED = False
LIBC = None
REPORT = {'schema': REPORT_SCHEMA, 'ok': False, 'qualified_public_only': False,
          'cleanup': False, 'cancelled': False, 'stage': 'preflight', 'runs': [],
          'cases': [], 'service_calls': 0, 'credentials_sent': False,
          'live_go': False, 'single_session_entries': 0}


class Block(Exception):
    pass


def require(ok):
    if not ok:
        raise Block()


def cancel(_sig, _frame):
    global CANCELLED
    CANCELLED = True


def check_cancel():
    require(not CANCELLED)


# BEGIN unchanged reviewed helper functions.
def prctl(op, value):
    require(LIBC.prctl(ctypes.c_int(op), ctypes.c_ulong(value),
                       ctypes.c_ulong(0), ctypes.c_ulong(0), ctypes.c_ulong(0)) == 0)


def absolute(path):
    require(os.path.isabs(path) and path == os.path.normpath(path)
            and not path.startswith("//"))
    return path


def open_path(path, directory=False):
    """Walk every component with O_NOFOLLOW, not just the final component."""
    absolute(path)
    current = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        parts = path.split("/")[1:]
        require(all(p not in ("", ".", "..") for p in parts))
        for index, part in enumerate(parts):
            flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK
            if index < len(parts) - 1 or directory:
                flags |= os.O_DIRECTORY
            new = os.open(part, flags, dir_fd=current)
            os.close(current)
            current = new
        result, current = current, None
        return result
    finally:
        if current is not None:
            os.close(current)


def directory(path):
    fd = open_path(path, True)
    try:
        info = os.fstat(fd)
        require(info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700)
    finally:
        os.close(fd)


def pinned(path, digest, cap):
    fd = open_path(path)
    try:
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                and info.st_nlink == 1 and not info.st_mode & 0o6022
                and 0 < info.st_size <= cap)
        with os.fdopen(os.dup(fd), "rb") as stream:
            data = stream.read(cap + 1)
        require(len(data) == info.st_size and hashlib.sha256(data).hexdigest() == digest)
        return data
    finally:
        os.close(fd)


def exclusive(path):
    return os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL
                   | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)


def empty_children():
    try:
        os.waitid(os.P_ALL, 0, os.WEXITED | os.WNOHANG | os.WNOWAIT)
    except ChildProcessError:
        return True
    return False


def ownership(fd):
    # P_PIDFD wait is permitted only for a direct, unreaped child. ECHILD fails.
    return os.waitid(os.P_PIDFD, fd, os.WEXITED | os.WNOHANG | os.WNOWAIT)


def drain(seconds=6):
    """Return true ONLY after ECHILD, killing only proven direct children."""
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if empty_children():
            return True
        with open("/proc/self/task/%d/children" % os.getpid(), "rb") as stream:
            raw = stream.read(131073)
        require(len(raw) <= 131072)
        for item in raw.split():
            require(item.isdigit())
            pid = int(item)
            # No child is reaped between this procfs snapshot and pidfd_open;
            # zombies reserve their PIDs. No generic numeric-PID signal occurs.
            fd = os.pidfd_open(pid, 0)
            try:
                ownership(fd)
                try:
                    signal.pidfd_send_signal(fd, signal.SIGKILL, None, 0)
                except ProcessLookupError:
                    pass  # Still wait/reap the owned zombie.
                os.waitid(os.P_PIDFD, fd, os.WEXITED | os.WNOHANG)
            finally:
                os.close(fd)
            if time.monotonic() >= end:
                return empty_children()
        select.select([], [], [], 0.01)
    return empty_children()


def child_exec(argv, cwd, log, gate, parent, seconds):
    try:
        for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM, signal.SIGALRM,
                    signal.SIGCHLD, signal.SIGPIPE):
            signal.signal(sig, signal.SIG_DFL)
        signal.pthread_sigmask(signal.SIG_SETMASK, [])
        signal.alarm(3)  # A failed pidfd acquisition never releases this gate.
        prctl(1, signal.SIGKILL)
        require(os.getppid() == parent)
        require(os.read(gate, 1) == b"G")
        signal.alarm(seconds + 10)
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        resource.setrlimit(resource.RLIMIT_FSIZE, (CAP, CAP))
        resource.setrlimit(resource.RLIMIT_CPU, (seconds, seconds))
        os.chdir(cwd)
        null = os.open("/dev/null", os.O_RDONLY | os.O_CLOEXEC)
        for old, new in ((null, 0), (log, 1), (log, 2)):
            os.dup2(old, new)
        for name in os.listdir("/proc/self/fd"):
            fd = int(name)
            if fd > 2:
                with contextlib.suppress(OSError):
                    os.close(fd)
        os.execve(argv[0], argv, PUBLIC)
    except BaseException:
        os._exit(126)


def run(argv, cwd, log_path, seconds):
    require(empty_children())
    check_cancel()
    entry = {"program": os.path.basename(argv[0]), "exit": None, "cleanup": False}
    REPORT["runs"].append(entry)
    REPORT["cleanup"] = False  # A discovery/pidfd failure must not keep an old proof.
    log = read_gate = write_gate = fd = None
    launched = False
    try:
        log = exclusive(log_path)
        read_gate, write_gate = os.pipe2(os.O_CLOEXEC)
        parent = os.getpid()
        pid = os.fork()
        if pid == 0:
            os.close(write_gate)
            child_exec(argv, cwd, log, read_gate, parent, seconds)
            os._exit(126)
        launched = True
        os.close(read_gate)
        read_gate = None
        fd = os.pidfd_open(pid, 0)
        ownership(fd)
        check_cancel()
        require(os.write(write_gate, b"G") == 1)
        os.close(write_gate)
        write_gate = None
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            check_cancel()
            info = ownership(fd)
            if info is not None:
                # WNOWAIT keeps the direct child unreaped until drain owns it.
                entry["exit"] = {"code": info.si_code, "status": info.si_status}
                require(info.si_code == os.CLD_EXITED and info.si_status == 0)
                return
            select.select([fd], [], [], min(0.05, max(0, end - time.monotonic())))
        entry["timeout"] = True
        raise Block()
    finally:
        # EOF also makes a still-gated launcher exit even before drain begins.
        for handle in (read_gate, write_gate, log):
            if handle is not None:
                os.close(handle)
        try:
            entry["cleanup"] = drain() if launched else empty_children()
            REPORT["cleanup"] = entry["cleanup"]
            require(entry["cleanup"])
        finally:
            if fd is not None:
                os.close(fd)


def read_public(path, cap=65536):
    with open(path, "rb") as stream:
        raw = stream.read(cap + 1)
    require(len(raw) <= cap)
    return raw
# END unchanged reviewed helper functions.


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def hash_ok(value):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value) is not None


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'),
                      ensure_ascii=True, allow_nan=False).encode('ascii')


def strict_json(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result)
            result[key] = value
        return result
    def constant(_):
        raise Block()
    try:
        return json.loads(raw, object_pairs_hook=pairs, parse_constant=constant)
    except (ValueError, UnicodeError, RecursionError):
        raise Block() from None


def write_all(fd, raw):
    while raw:
        n = os.write(fd, raw)
        require(n > 0)
        raw = raw[n:]


def save_new(path, value):
    fd = exclusive(path)
    try:
        write_all(fd, canonical(value) + b'\n')
        os.fsync(fd)
    finally:
        os.close(fd)


def owned_bytes(path, cap, empty=False):
    fd = open_path(path)
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                and before.st_nlink == 1 and not before.st_mode & 0o6022
                and (0 if empty else 1) <= before.st_size <= cap)
        with os.fdopen(os.dup(fd), 'rb') as stream:
            raw = stream.read(cap + 1)
        after = os.fstat(fd)
        require(len(raw) == before.st_size and before.st_size == after.st_size
                and before.st_mtime_ns == after.st_mtime_ns
                and before.st_ctime_ns == after.st_ctime_ns)
        return raw
    finally:
        os.close(fd)


def safe_parts(name):
    if not isinstance(name, str) or not name or '\\' in name or ':' in name or '\0' in name:
        return False
    forbidden = {'target', 'secrets', 'private', 'captures', 'privatecaptures',
                 '.claude', '.git', '.ssh', 'keys'}
    return all(p not in ('', '.', '..') and p.lower() not in forbidden
               and not p.lower().startswith('.env') for p in name.split('/'))


def source_allowed(name):
    if not safe_parts(name):
        return False
    return (name in {'Cargo.toml', 'Cargo.lock', '.cargo/config.toml',
                     'tests/fixtures/live_synthetic_responses.json',
                     'tests/fixtures/live_rewrite_response.json',
                     'tests/fixtures/result_policy_conformance.json'}
            or (name.startswith('crates/') and
                (name.endswith('.rs') or name.endswith('/Cargo.toml')
                 or name.endswith('/THIRD_PARTY_LICENSES')
                 or name == 'crates/rambler-audio/tests/support/independent_decode.py')))


def cache_allowed(name):
    if not safe_parts(name):
        return False
    parts = name.split('/')
    return (len(parts) >= 3 and parts[:2] in
            (['registry', 'src'], ['registry', 'cache'], ['registry', 'index'])
            and all(p.lower() not in {'credentials', 'credentials.toml'} for p in parts))


def parse_entries(raw, schema, allowed, count_cap, file_cap, total_cap):
    value = strict_json(raw)
    require(isinstance(value, dict) and set(value) == {'schema', 'files'}
            and value['schema'] == schema and isinstance(value['files'], list)
            and 1 <= len(value['files']) <= count_cap)
    result, total = {}, 0
    for item in value['files']:
        require(isinstance(item, dict) and set(item) == {'path', 'size', 'sha256'})
        name, size, digest = item['path'], item['size'], item['sha256']
        require(allowed(name) and name not in result and type(size) is int
                and 0 <= size <= file_cap and hash_ok(digest))
        result[name] = item
        total += size
    require(total <= total_cap)
    for name in result:
        parts = name.split('/')
        require(all('/'.join(parts[:i]) not in result for i in range(1, len(parts))))
    return result


def parse_manifest(raw, example_sha, tests_sha):
    expected = parse_entries(raw, MANIFEST_SCHEMA, source_allowed, 1000,
                             8 * 1024 * 1024, 32 * 1024 * 1024)
    require({'Cargo.toml', 'Cargo.lock', '.cargo/config.toml', EXAMPLE, FAKE_TESTS}
            <= expected.keys())
    require(expected[EXAMPLE]['sha256'] == example_sha
            and expected[FAKE_TESTS]['sha256'] == tests_sha)
    return expected


def tree_gate(source, expected, directory_cap, file_cap):
    directory(source)
    allowed_dirs = set()
    for name in expected:
        parts = name.split('/')
        allowed_dirs.update('/'.join(parts[:i]) for i in range(1, len(parts)))
    require(len(allowed_dirs) <= directory_cap)
    observed, seen_dirs = set(), set()
    def walk_error(_):
        raise Block()
    for root, dirs, files in os.walk(source, followlinks=False, onerror=walk_error):
        for name in dirs:
            path = os.path.join(root, name)
            relative = os.path.relpath(path, source)
            require(relative in allowed_dirs and relative not in seen_dirs)
            seen_dirs.add(relative)
            directory(path)
        for name in files:
            relative = os.path.relpath(os.path.join(root, name), source)
            require(relative in expected and relative not in observed)
            observed.add(relative)
    require(observed == expected.keys() and seen_dirs == allowed_dirs)
    actual = []
    for name in sorted(expected):
        check_cancel()
        raw = owned_bytes(os.path.join(source, name), file_cap, empty=True)
        item = {'path': name, 'size': len(raw), 'sha256': sha(raw)}
        require(item == expected[name])
        actual.append(item)
    return sha(canonical(actual))


def source_gate(source, expected):
    result = tree_gate(source, expected, 1000, 8 * 1024 * 1024)
    raw = owned_bytes(os.path.join(source, '.cargo/config.toml'), 1024)
    require(raw.replace(b'\r\n', b'\n') ==
            b'[env]\nCMAKE_GENERATOR_x86_64_pc_windows_msvc = { value = "Visual Studio 17 2022", force = false }\n')
    return result


def copy_tree(source, destination, expected, file_cap):
    os.mkdir(destination, 0o700)
    for name in sorted(expected):
        check_cancel()
        raw = owned_bytes(os.path.join(source, name), file_cap, empty=True)
        require(len(raw) == expected[name]['size'] and sha(raw) == expected[name]['sha256'])
        path = os.path.join(destination, name)
        os.makedirs(os.path.dirname(path), mode=0o700, exist_ok=True)
        fd = exclusive(path)
        try:
            write_all(fd, raw)
            os.fsync(fd)
        finally:
            os.close(fd)


def public_cache_gate(args):
    inputs = os.path.join(args.workspace, 'admission-input-v1')
    raw = pinned(os.path.join(inputs, 'cargo-cache-manifest.json'),
                 args.cache_manifest_sha256, 16 * 1024 * 1024)
    expected = parse_entries(raw, CACHE_SCHEMA, cache_allowed, 100000,
                             32 * 1024 * 1024, 2 * 1024 * 1024 * 1024)
    source = os.path.join(inputs, 'cargo-cache')
    tree_gate(source, expected, 100000, 32 * 1024 * 1024)
    return source, expected


def no_external_cargo_config(source, cache):
    # Names only. Never open any external config or credential file.
    for name in ('config', 'config.toml', 'credentials', 'credentials.toml'):
        require(not os.path.lexists(os.path.join(cache, name)))
    parent = os.path.dirname(source)
    while True:
        for name in ('config', 'config.toml'):
            require(not os.path.lexists(os.path.join(parent, '.cargo', name)))
        next_parent = os.path.dirname(parent)
        if next_parent == parent:
            break
        parent = next_parent


def elf(raw):
    return (len(raw) >= 20 and raw[:6] == b'\x7fELF\x02\x01'
            and raw[16:18] in (b'\x02\0', b'\x03\0') and raw[18:20] == b'\x3e\0')


def native_tool(path, digest):
    require(hash_ok(digest))
    fd = open_path(path)
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode) and before.st_uid in (0, os.getuid())
                and before.st_nlink >= 1 and not before.st_mode & 0o6022
                and before.st_mode & stat.S_IXUSR and 20 <= before.st_size <= CAP)
        value, header, total = hashlib.sha256(), b'', 0
        while True:
            raw = os.read(fd, 1024 * 1024)
            if not raw:
                break
            if not header:
                header = raw[:20]
            value.update(raw)
            total += len(raw)
            require(total <= CAP)
        after = os.fstat(fd)
        require(total == before.st_size == after.st_size and elf(header)
                and before.st_mtime_ns == after.st_mtime_ns
                and before.st_ctime_ns == after.st_ctime_ns and value.hexdigest() == digest)
        try:
            os.getxattr(fd, 'security.capability')
        except OSError as exc:
            require(exc.errno == errno.ENODATA)
        else:
            raise Block()
    finally:
        os.close(fd)


def frozen_tool(path, digest):
    raw = pinned(path, digest, IMAGE_CAP)
    fd = open_path(path)
    try:
        require(stat.S_IMODE(os.fstat(fd).st_mode) == 0o500)
    finally:
        os.close(fd)
    require(elf(raw))
    native_tool(path, digest)


def freeze(source, destination):
    # Only the selected Cargo output may have 2 links, NEVER the executed copy.
    fd = open_path(source)
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                and 1 <= before.st_nlink <= 2 and not before.st_mode & 0o6022
                and before.st_mode & stat.S_IXUSR and 20 <= before.st_size <= IMAGE_CAP)
        with os.fdopen(os.dup(fd), 'rb') as stream:
            raw = stream.read(IMAGE_CAP + 1)
        after = os.fstat(fd)
        require(len(raw) == before.st_size == after.st_size and elf(raw)
                and before.st_mtime_ns == after.st_mtime_ns
                and before.st_ctime_ns == after.st_ctime_ns
                and before.st_nlink == after.st_nlink)
    finally:
        os.close(fd)
    out = exclusive(destination)
    try:
        write_all(out, raw)
        os.fchmod(out, 0o500)
        os.fsync(out)
    finally:
        os.close(out)
    frozen_tool(destination, sha(raw))
    return sha(raw)


def artifact(raw, test):
    matches = []
    for line in raw.split(b'\n'):
        if not line.startswith(b'{'):
            continue
        record = strict_json(line)
        require(isinstance(record, dict) and isinstance(record.get('reason'), str))
        if record['reason'] != 'compiler-artifact':
            continue
        target, profile = record.get('target'), record.get('profile')
        require(isinstance(target, dict) and isinstance(profile, dict))
        if target.get('name') != 'linux_real_admission':
            continue
        require(target.get('kind') == ['example'] and type(profile.get('test')) is bool)
        if profile['test'] is test and record.get('executable') is not None:
            path = record['executable']
            require(isinstance(path, str) and path and '\0' not in path)
            matches.append(path)
    require(len(matches) == 1)
    return matches[0]


def fake_counts(raw):
    pattern = rb'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;[^\r\n]*$'
    matches = re.findall(pattern, raw, re.MULTILINE)
    require(len(matches) == 1 and matches[0] == (b'17', b'0', b'0', b'0', b'0'))
    return dict(FAKE_COUNTS)


def frame():
    return MAGIC + b''.join(len(p).to_bytes(2, 'big') + p for p in PUBLIC_FIELDS)


def public_cases():
    valid = frame()
    return (('public-valid', valid, 'PUBLIC_FRAME_CHECKED', 0),
            ('empty-eof', b'', 'REJECT_FRAME', 1),
            ('short-magic', MAGIC[:4], 'REJECT_FRAME', 1),
            ('bad-magic', b'X' + valid[1:], 'REJECT_FRAME', 1),
            ('zero-length', MAGIC + b'\0\0', 'REJECT_FRAME', 1),
            ('over-cap', MAGIC + b'\x10\x01', 'REJECT_FRAME', 1),
            ('non-utf8', MAGIC + b'\0\1\xff', 'REJECT_FRAME', 1),
            ('trailing-byte', valid + b'\0', 'REJECT_FRAME', 1))


def terminal(category):
    require(category in {'PUBLIC_FRAME_CHECKED', 'REJECT_FRAME', 'BLOCKED_SAFETY'})
    errors = 0 if category == 'PUBLIC_FRAME_CHECKED' else 1
    return (f'REAL_ADMISSION_TERMINAL_V1 {category} primary_errors={errors} '
            'cleanup_errors=0 sessions=0 healthy=false pcm_bytes=0 '
            'pcm_eof=false source_closed=false\n').encode('ascii')


def fields(raw):
    result = {}
    for line in raw.decode('ascii').splitlines():
        key, sep, value = line.partition(':')
        if sep:
            require(key not in result)
            result[key] = value.strip()
    return result


def status_ok(raw, uid, gid, parent):
    value = fields(raw)
    masks = []
    for name in ('SigIgn', 'SigCgt'):
        text = value.get(name, '')
        if re.fullmatch('[0-9a-fA-F]{16}', text) is None:
            return False
        masks.append(int(text, 16))
    return (uid > 0 and gid > 0 and value.get('Uid', '').split() == [str(uid)] * 4
            and value.get('Gid', '').split() == [str(gid)] * 4
            and value.get('PPid') == str(parent) and value.get('Threads') == '1'
            and value.get('TracerPid') == '0' and value.get('NoNewPrivs') == '1'
            and all(value.get(k) == '0000000000000000' for k in
                    ('SigBlk', 'SigPnd', 'ShdPnd', 'CapInh', 'CapPrm', 'CapEff', 'CapAmb'))
            and masks[0] & ~0x1000 == 0 and masks[1] & ~0x440 == 0)


def limits_ok(raw):
    lines = raw.decode('ascii').splitlines()
    for prefix, expected in (('Max core file size', ['0', '0', 'bytes']),
                             ('Max file size', [str(CAP), str(CAP), 'bytes']),
                             ('Max cpu time', ['10', '10', 'seconds'])):
        matches = [line.split() for line in lines if line.startswith(prefix + ' ')]
        if len(matches) != 1 or matches[0][-3:] != expected:
            return False
    return True


def stat_identity(raw, pid, parent):
    first, sep, rest = raw.decode('ascii').strip().rpartition(') ')
    require(sep and first.startswith(str(pid) + ' ('))
    words = rest.split()
    require(len(words) >= 20 and words[0] not in ('Z', 'X', 'x')
            and words[1] == str(parent) and words[19].isdigit())
    return words[19]


def policy():
    paths = ('/proc/sys/kernel/core_pattern', '/proc/sys/kernel/core_uses_pid',
             '/proc/sys/fs/suid_dumpable', '/proc/sys/kernel/yama/ptrace_scope')
    require([read_public(p, 128) for p in paths] == [b'core\n', b'1\n', b'0\n', b'0\n'])


def observe_before_write(pid, fd):
    check_cancel()
    require(ownership(fd) is None)
    parent = os.getpid()
    identity = stat_identity(read_public(f'/proc/{pid}/stat'), pid, parent)
    require(status_ok(read_public(f'/proc/{pid}/status'), os.getuid(), os.getgid(), parent))
    require(limits_ok(read_public(f'/proc/{pid}/limits')))
    try:
        handle = os.open(f'/proc/{pid}/environ', os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    except OSError as exc:
        require(exc.errno == errno.EACCES)
    else:
        os.close(handle)  # NEVER read environment content, even on unexpected success.
        raise Block()
    require(stat_identity(read_public(f'/proc/{pid}/stat'), pid, parent) == identity)
    require(status_ok(read_public(f'/proc/{pid}/status'), os.getuid(), os.getgid(), parent))
    require(limits_ok(read_public(f'/proc/{pid}/limits')))
    policy()
    require(ownership(fd) is None)
    check_cancel()


def restore_target_signals(sigops):
    # CPython ignores SIGPIPE/SIGXFZ/SIGXFSZ; SIG_IGN survives execve.
    # Restore every catchable signal, not only the fixed launcher handlers.
    for sig in sorted(sigops.valid_signals()):
        if sig in (sigops.SIGKILL, sigops.SIGSTOP):
            continue
        try:
            sigops.signal(sig, sigops.SIG_DFL)
        except (OSError, ValueError):
            # glibc reserves 32/33. Any other reset failure blocks exec.
            if int(sig) not in (32, 33):
                raise


def target_exec(binary, cwd, input_fd, output_fd, pcm_fd, gate, parent, no_args):
    try:
        restore_target_signals(signal)
        signal.pthread_sigmask(signal.SIG_SETMASK, [])
        signal.alarm(3)
        prctl(1, signal.SIGKILL)
        require(os.getppid() == parent)
        require(os.read(gate, 1) == b'G')
        signal.alarm(20)
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        resource.setrlimit(resource.RLIMIT_FSIZE, (CAP, CAP))
        resource.setrlimit(resource.RLIMIT_CPU, (10, 10))
        os.chdir(cwd)
        null = os.open('/dev/null', os.O_WRONLY | os.O_CLOEXEC)
        # Preserve all source descriptors above 3 BEFORE any remapping.
        import fcntl
        sources = [fcntl.fcntl(item, fcntl.F_DUPFD_CLOEXEC, 10)
                   for item in (input_fd, output_fd, null, pcm_fd)]
        for new, old in enumerate(sources):
            os.dup2(old, new, inheritable=True)
        for name in os.listdir('/proc/self/fd'):
            fd = int(name)
            if fd > 3:
                with contextlib.suppress(OSError):
                    os.close(fd)
        require(os.getppid() == parent)
        argv = [binary] if no_args else [binary, '--public-frame-check-v1']
        os.execve(binary, argv, {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
    except BaseException:
        os._exit(126)


def read_exact_pipe(fd, expected, end):
    raw = b''
    while len(raw) < len(expected):
        check_cancel()
        require(time.monotonic() < end)
        if select.select([fd], [], [], min(0.05, max(0, end - time.monotonic())))[0]:
            chunk = os.read(fd, len(expected) - len(raw))
            require(chunk)
            raw += chunk
            require(expected.startswith(raw))
    require(raw == expected)


def target_case(binary, binary_sha, pcm, cwd, label, payload, category, code,
                no_args=False, bad_fd=False, nonblocking_fd=False):
    require(empty_children())
    check_cancel()
    require(sum((no_args, bad_fd, nonblocking_fd)) <= 1)
    policy()
    frozen_tool(binary, binary_sha)
    require(len(pinned(pcm, PCM_SHA256, PCM_BYTES)) == PCM_BYTES)
    os.mkdir(cwd, 0o700)
    entry = {'case': label, 'ok': False, 'cleanup': False,
             'environ_denied_before_input': False, 'frame_bytes_written': 0}
    REPORT['cases'].append(entry)
    REPORT['cleanup'] = False
    handles, fd, launched = set(), None, False
    try:
        read_in, write_in = os.pipe2(os.O_CLOEXEC)
        handles.update((read_in, write_in))
        read_out, write_out = os.pipe2(os.O_CLOEXEC)
        handles.update((read_out, write_out))
        gate_in, gate_out = os.pipe2(os.O_CLOEXEC)
        handles.update((gate_in, gate_out))
        pcm_fd = open_path(pcm)
        handles.add(pcm_fd)
        os.set_blocking(pcm_fd, True)  # open_path deliberately adds O_NONBLOCK.
        input_fd = read_in
        if nonblocking_fd:
            os.set_blocking(input_fd, False)
        if bad_fd:
            input_fd = os.open('/dev/null', os.O_RDONLY | os.O_CLOEXEC)
            handles.add(input_fd)
        parent = os.getpid()
        pid = os.fork()
        if pid == 0:
            os.close(gate_out)
            target_exec(binary, cwd, input_fd, write_out, pcm_fd, gate_in, parent, no_args)
            os._exit(126)
        launched = True
        for item in (read_in, write_out, gate_in, pcm_fd):
            os.close(item)
            handles.remove(item)
        if bad_fd:
            os.close(input_fd)
            handles.remove(input_fd)
        fd = os.pidfd_open(pid, 0)
        ownership(fd)
        check_cancel()
        require(os.write(gate_out, b'G') == 1)
        os.close(gate_out)
        handles.remove(gate_out)
        os.set_blocking(read_out, False)
        end = time.monotonic() + 10
        if not no_args and not bad_fd and not nonblocking_fd:
            read_exact_pipe(read_out, READY, end)
            require(not select.select([read_out], [], [], 0)[0])
            observe_before_write(pid, fd)
            entry['environ_denied_before_input'] = True
            require(isinstance(payload, bytes) and len(payload) <= 512)
            os.set_blocking(write_in, False)
            if payload:
                check_cancel()
                require(ownership(fd) is None)
                require(os.write(write_in, payload) == len(payload))
                entry['frame_bytes_written'] = len(payload)
            if label == 'public-valid':
                require(not select.select([read_out], [], [], 0.05)[0])
                require(ownership(fd) is None)
                entry['withheld_eof_no_terminal'] = True
        else:
            require(payload is None)
        os.close(write_in)  # Mandatory EOF, including empty input and negatives.
        handles.remove(write_in)
        expected = b'' if no_args else terminal(category)
        read_exact_pipe(read_out, expected, end)
        eof, info = False, None
        while time.monotonic() < end:
            check_cancel()
            if not eof and select.select([read_out], [], [], 0.02)[0]:
                require(os.read(read_out, 4097) == b'')
                eof = True
            info = ownership(fd)
            if eof and info is not None:
                break
            select.select([fd] if info is None else [], [], [], 0.01)
        require(eof and info is not None and info.si_code == os.CLD_EXITED and info.si_status == code)
        require(not os.listdir(cwd))
        policy()
        entry.update(ok=True, terminal_category=None if no_args else category,
                     exit_status=code, no_arg_not_hardened=no_args)
    finally:
        # A descriptor close error must not skip ownership cleanup.
        try:
            for handle in handles:
                with contextlib.suppress(OSError):
                    os.close(handle)
        finally:
            try:
                entry['cleanup'] = drain() if launched else empty_children()
                REPORT['cleanup'] = entry['cleanup']
                require(entry['cleanup'])
            finally:
                if fd is not None:
                    os.close(fd)


def initialize():
    global LIBC
    require(sys.flags.isolated == 1 and sys.flags.no_site == 1 and sys.dont_write_bytecode)
    require(resource is not None and os.uname().sysname == 'Linux' and os.uname().machine == 'x86_64')
    require(os.getuid() > 0 and os.getgid() > 0 and os.getuid() == os.geteuid()
            and os.getgid() == os.getegid())
    require(len(os.listdir('/proc/self/task')) == 1)
    for fd in range(3):
        os.fstat(fd)
    require(all(hasattr(os, name) for name in ('pidfd_open', 'P_PIDFD', 'WNOWAIT')))
    require(hasattr(signal, 'pidfd_send_signal'))
    require(signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL)
    require(empty_children())
    require(read_public('/proc/self/task/%d/children' % os.getpid(), 128) == b'')
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM, signal.SIGALRM,
                signal.SIGQUIT, signal.SIGUSR1, signal.SIGUSR2, signal.SIGPWR):
        signal.signal(sig, cancel)
    LIBC = ctypes.CDLL(None, use_errno=True)
    LIBC.prctl.restype = ctypes.c_int
    parent = os.getppid()
    require(parent > 1)
    prctl(4, 0)
    prctl(1, signal.SIGTERM)
    require(os.getppid() == parent)
    prctl(36, 1)
    require(empty_children())
    REPORT['cleanup'] = True


def arguments(argv=None):
    # Deliberately no argparse/gettext/terminal-width lookup: --unit may exercise
    # this parser without consulting environment variables or locale files.
    argv = list(sys.argv[1:] if argv is None else argv)
    require(len(argv) <= 64 and all(isinstance(v, str) for v in argv))
    required = ('workspace', 'owner-sha256', 'workspace-owner-sha256', 'supervisor-sha256',
                'source-manifest-sha256', 'example-sha256', 'fake-tests-sha256')
    optional = ('cache-manifest-sha256', 'cargo', 'cargo-sha256', 'rustc', 'rustc-sha256',
                'cmake', 'cmake-sha256', 'build-id', 'build-receipt-sha256', 'build-final-sha256',
                'example-binary-sha256', 'fake-binary-sha256')
    switches = {'--build-only': 'build_only', '--observe-only': 'observe_only'}
    values = {name.replace('-', '_'): None for name in required + optional}
    values.update(build_only=False, observe_only=False)
    options = {'--' + name: name.replace('-', '_') for name in required + optional}
    seen, i = set(), 0
    while i < len(argv):
        flag, sep, attached = argv[i].partition('=')
        require(flag not in seen and flag in (options.keys() | switches.keys()))
        seen.add(flag)
        if flag in switches:
            require(not sep)
            values[switches[flag]] = True
        else:
            if sep:
                value = attached
            else:
                i += 1
                require(i < len(argv))
                value = argv[i]
            require(value and not value.startswith('--'))
            values[options[flag]] = value
        i += 1
    require(values['build_only'] != values['observe_only'])
    require(all(values[name.replace('-', '_')] is not None for name in required))
    args = SimpleNamespace(**values)
    absolute(args.workspace)
    for name, value in vars(args).items():
        if name.endswith('_sha256') and value is not None:
            require(hash_ok(value))
    if args.build_only:
        require(hash_ok(args.cache_manifest_sha256))
        require(all(getattr(args, name) is None for name in
                    ('build_id', 'build_receipt_sha256', 'build_final_sha256',
                     'example_binary_sha256', 'fake_binary_sha256')))
        for name in ('cargo', 'rustc', 'cmake'):
            require(getattr(args, name) is not None and hash_ok(getattr(args, name + '_sha256')))
            absolute(getattr(args, name))
    else:
        require(args.cache_manifest_sha256 is None)
        require(all(getattr(args, name) is None for name in
                    ('cargo', 'cargo_sha256', 'rustc', 'rustc_sha256', 'cmake', 'cmake_sha256')))
        require(isinstance(args.build_id, str) and
                re.fullmatch('admission-public-build-[0-9a-f]{32}', args.build_id) is not None)
        require(all(hash_ok(getattr(args, name)) for name in
                    ('build_receipt_sha256', 'build_final_sha256',
                     'example_binary_sha256', 'fake_binary_sha256')))
    return args


def marker_gate(args):
    directory(args.workspace)
    expected = {'schema': 1, 'purpose': 'public-native-rambler-qualification',
                'name': os.path.basename(args.workspace), 'contains_credentials': False}
    value = strict_json(pinned(os.path.join(args.workspace, WORKSPACE_OWNER_NAME),
                               args.workspace_owner_sha256, 4096))
    require(canonical(value) == canonical(expected))
    value = strict_json(pinned(os.path.join(args.workspace, OWNER_NAME), args.owner_sha256, 4096))
    require(canonical(value) == canonical(OWNER_VALUE))


def input_gate(args):
    marker_gate(args)
    for name in ('build', 'admission-input-v1'):
        directory(os.path.join(args.workspace, name))
    inputs = os.path.join(args.workspace, 'admission-input-v1')
    raw = pinned(os.path.join(inputs, 'source-manifest.json'), args.source_manifest_sha256,
                 2 * 1024 * 1024)
    expected = parse_manifest(raw, args.example_sha256, args.fake_tests_sha256)
    source = os.path.join(inputs, 'source')
    fingerprint = source_gate(source, expected)
    require(len(pinned(os.path.join(inputs, 'public.pcm'), PCM_SHA256, PCM_BYTES)) == PCM_BYTES)
    return source, expected, fingerprint


def receipt_value(args, build_id, source_sha, pins, cache_sha):
    return {'schema': BUILD_SCHEMA, 'ok': True, 'cleanup': True,
            'build_id': build_id, 'supervisor_sha256': args.supervisor_sha256,
            'owner_sha256': args.owner_sha256,
            'workspace_owner_sha256': args.workspace_owner_sha256,
            'manifest_sha256': args.source_manifest_sha256, 'cache_manifest_sha256': cache_sha,
            'source_sha256': source_sha, 'example_source_sha256': args.example_sha256,
            'fake_source_sha256': args.fake_tests_sha256,
            'example_binary_sha256': pins['example'], 'fake_binary_sha256': pins['fake-tests'],
            'fake_counts': dict(FAKE_COUNTS), 'example_main_executions': 0,
            'service_calls': 0, 'live_go': False}


def build_final_value(args, build_id, source_sha, pins, receipt_sha, cache_sha):
    return {'schema': REPORT_SCHEMA, 'ok': True, 'qualified_public_only': False,
            'cleanup': True, 'cancelled': False, 'stage': 'complete', 'cases': [],
            'runs': [{'program': name, 'exit': {'code': 1, 'status': 0}, 'cleanup': True}
                     for name in ('cargo', 'cargo', 'fake-tests')],
            'service_calls': 0, 'credentials_sent': False, 'live_go': False,
            'single_session_entries': 0, 'phase': 'build-only', 'run_id': build_id,
            'source_sha256': source_sha, 'example_source_sha256': args.example_sha256,
            'fake_source_sha256': args.fake_tests_sha256,
            'source_manifest_sha256': args.source_manifest_sha256,
            'supervisor_sha256': args.supervisor_sha256,
            'owner_sha256': args.owner_sha256,
            'workspace_owner_sha256': args.workspace_owner_sha256,
            'cache_manifest_sha256': cache_sha, 'cache_unchanged': True,
            'binary_pins': pins, 'fake_counts': dict(FAKE_COUNTS),
            'build_receipt_sha256': receipt_sha, 'source_unchanged': True}


def validate_build_admission(args, source_sha, receipt, final):
    require(isinstance(receipt, dict) and isinstance(final, dict))
    cache_sha = receipt.get('cache_manifest_sha256')
    require(hash_ok(cache_sha))
    pins = {'example': args.example_binary_sha256, 'fake-tests': args.fake_binary_sha256}
    require(pins['example'] != pins['fake-tests'])
    expected_receipt = receipt_value(args, args.build_id, source_sha, pins, cache_sha)
    require(canonical(receipt) == canonical(expected_receipt))
    require(sha(canonical(receipt) + b'\n') == args.build_receipt_sha256)
    expected_final = build_final_value(args, args.build_id, source_sha, pins,
                                       args.build_receipt_sha256, cache_sha)
    require(canonical(final) == canonical(expected_final))


def build(args, owned, source, expected, source_sha):
    global PUBLIC
    REPORT['stage'] = 'build-preflight'
    cache_source, cache_expected = public_cache_gate(args)
    cache = os.path.join(owned, 'cargo')
    copy_tree(cache_source, cache, cache_expected, 32 * 1024 * 1024)
    tree_gate(cache, cache_expected, 100000, 32 * 1024 * 1024)
    REPORT['cache_manifest_sha256'] = args.cache_manifest_sha256
    require(os.path.dirname(args.cargo) == os.path.dirname(args.rustc)
            and '/toolchains/' in args.rustc and args.rustc.endswith('/bin/rustc')
            and args.cargo.endswith('/bin/cargo')
            and args.cmake == os.path.join(args.workspace, 'tools/cmake-portable-3.31.10/bin/cmake'))
    for name in ('cargo', 'rustc', 'cmake'):
        native_tool(getattr(args, name), getattr(args, name + '_sha256'))
    no_external_cargo_config(source, cache)
    copied = os.path.join(owned, 'source')
    copy_tree(source, copied, expected, 8 * 1024 * 1024)
    require(source_gate(copied, expected) == source_sha)
    no_external_cargo_config(copied, cache)
    for name in ('home', 'tmp', 'target', 'logs', 'bin'):
        os.mkdir(os.path.join(owned, name), 0o700)
    PUBLIC = {'PATH': os.path.dirname(args.rustc) + ':' + os.path.dirname(args.cmake) + ':/usr/bin:/bin',
              'LC_ALL': 'C', 'HOME': os.path.join(owned, 'home'), 'TMPDIR': os.path.join(owned, 'tmp'),
              'CARGO_HOME': cache, 'CARGO_TARGET_DIR': os.path.join(owned, 'target'),
              'RUSTC': args.rustc, 'CMAKE': args.cmake, 'CARGO_NET_OFFLINE': 'true'}
    common = ['-p', 'rambler-cli', '--features', 'audio', '--example', 'linux_real_admission',
              '--locked', '--offline', '--jobs', '2', '--message-format=json']
    pins = {}
    for test, label in ((False, 'example'), (True, 'fake-tests')):
        REPORT['stage'] = 'compile-' + label
        for name in ('cargo', 'rustc', 'cmake'):
            native_tool(getattr(args, name), getattr(args, name + '_sha256'))
        log = os.path.join(owned, 'logs', label + '.log')
        command = [args.cargo, 'test', '--no-run', *common] if test else [args.cargo, 'build', *common]
        run(command, copied, log, 600)
        path = absolute(artifact(owned_bytes(log, 8 * 1024 * 1024), test))
        require(os.path.commonpath((path, os.path.join(owned, 'target'))) == os.path.join(owned, 'target'))
        pins[label] = freeze(path, os.path.join(owned, 'bin', label))
    require(pins['example'] != pins['fake-tests'])
    REPORT['stage'] = 'frozen-fake-tests'
    PUBLIC = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
    fake = os.path.join(owned, 'bin/fake-tests')
    frozen_tool(fake, pins['fake-tests'])
    log = os.path.join(owned, 'logs/fake-run.log')
    run([fake, '--test-threads=1', '--format=terse'], owned, log, 30)
    counts = fake_counts(owned_bytes(log, 65536))
    for label in pins:
        frozen_tool(os.path.join(owned, 'bin', label), pins[label])
    require(source_gate(copied, expected) == source_sha and source_gate(source, expected) == source_sha)
    require(empty_children())
    receipt = receipt_value(args, REPORT['run_id'], source_sha, pins, args.cache_manifest_sha256)
    save_new(os.path.join(owned, 'build-receipt.json'), receipt)
    REPORT.update(binary_pins=pins, fake_counts=counts,
                  build_receipt_sha256=sha(canonical(receipt) + b'\n'))


def observe(args, owned, source, expected, source_sha):
    REPORT['stage'] = 'observe-preflight'
    built = os.path.join(args.workspace, 'build', args.build_id)
    directory(built)
    final = strict_json(pinned(os.path.join(built, 'final.json'), args.build_final_sha256, 65536))
    receipt = strict_json(pinned(os.path.join(built, 'build-receipt.json'), args.build_receipt_sha256, 65536))
    validate_build_admission(args, source_sha, receipt, final)
    binary, fake = os.path.join(built, 'bin/example'), os.path.join(built, 'bin/fake-tests')
    frozen_tool(binary, args.example_binary_sha256)
    frozen_tool(fake, args.fake_binary_sha256)  # pin only, NEVER execute Fake here
    pcm = os.path.join(args.workspace, 'admission-input-v1/public.pcm')
    require(len(pinned(pcm, PCM_SHA256, PCM_BYTES)) == PCM_BYTES)
    policy()
    save_new(os.path.join(built, 'observe-once.json'),
             {'schema': 'linux-admission-public-reservation-v1', 'run_id': REPORT['run_id']})
    REPORT['stage'] = 'no-args-not-hardened'
    target_case(binary, args.example_binary_sha256, pcm, os.path.join(owned, 'no-args'),
                'no-args-not-hardened', None, None, 0, no_args=True)
    REPORT['stage'] = 'pre-ready-fd-negative'
    target_case(binary, args.example_binary_sha256, pcm, os.path.join(owned, 'fd-negative'),
                'pre-ready-fd-negative', None, 'BLOCKED_SAFETY', 1, bad_fd=True)
    REPORT['stage'] = 'pre-ready-nonblocking-negative'
    target_case(binary, args.example_binary_sha256, pcm, os.path.join(owned, 'nonblocking-negative'),
                'pre-ready-nonblocking-negative', None, 'BLOCKED_SAFETY', 1, nonblocking_fd=True)
    for label, payload, category, code in public_cases():
        REPORT['stage'] = label
        target_case(binary, args.example_binary_sha256, pcm, os.path.join(owned, label),
                    label, payload, category, code)
    require(len(REPORT['cases']) == 11 and all(c['ok'] and c['cleanup'] for c in REPORT['cases']))
    require(sum(c['environ_denied_before_input'] for c in REPORT['cases']) == 8)
    frozen_tool(binary, args.example_binary_sha256)
    frozen_tool(fake, args.fake_binary_sha256)
    pinned(os.path.join(built, 'final.json'), args.build_final_sha256, 65536)
    pinned(os.path.join(built, 'build-receipt.json'), args.build_receipt_sha256, 65536)
    require(source_gate(source, expected) == source_sha)
    REPORT.update(qualified_public_only=True, malformed_public_frame_cases=7,
                  native_case_count=11, hardened_frame_cases=8, safety_negative_cases=2,
                  pcm_sha256=PCM_SHA256, pcm_bytes=PCM_BYTES,
                  terminal_counts={'PUBLIC_FRAME_CHECKED': 1, 'REJECT_FRAME': 7,
                                   'BLOCKED_SAFETY': 2, 'silent_no_arg': 1})


def native_main():
    # Never inspect inherited credential-like names/values.
    os.environ.clear()
    os.umask(0o077)
    args = arguments()
    initialized, final_fd = False, None
    source = expected = source_sha = None
    try:
        # Reject root/identity changes BEFORE opening any workspace input.
        require(resource is not None and sys.platform == 'linux'
                and os.getuid() > 0 and os.getgid() > 0
                and os.getuid() == os.geteuid() and os.getgid() == os.getegid())
        source, expected, source_sha = input_gate(args)
        pinned(absolute(os.path.abspath(__file__)), args.supervisor_sha256, 128 * 1024)
        REPORT.update(source_sha256=source_sha, example_source_sha256=args.example_sha256,
                      fake_source_sha256=args.fake_tests_sha256,
                      source_manifest_sha256=args.source_manifest_sha256,
                      supervisor_sha256=args.supervisor_sha256, owner_sha256=args.owner_sha256,
                      workspace_owner_sha256=args.workspace_owner_sha256,
                      phase='build-only' if args.build_only else 'observe-only')
        prefix = 'admission-public-build-' if args.build_only else 'admission-public-observe-'
        name = prefix + uuid.uuid4().hex
        owned = os.path.join(args.workspace, 'build', name)
        os.mkdir(owned, 0o700)  # Collision is terminal; NEVER retry/adopt.
        REPORT['run_id'] = name
        final_fd = exclusive(os.path.join(owned, 'final.json'))
        initialize()
        initialized = True
        signal.alarm(1800 if args.build_only else 240)
        if args.build_only:
            build(args, owned, source, expected, source_sha)
        else:
            observe(args, owned, source, expected, source_sha)
        check_cancel()
        require(empty_children())
        REPORT.update(ok=True, stage='complete')
    except BaseException:
        REPORT['ok'] = False
    finally:
        try:
            REPORT['cleanup'] = drain() if initialized else empty_children()
        except BaseException:
            REPORT['cleanup'] = False
        unchanged, cache_unchanged = False, None
        if source is not None and expected is not None and source_sha is not None:
            try:
                unchanged = source_gate(source, expected) == source_sha
                pinned(os.path.join(args.workspace, 'admission-input-v1/source-manifest.json'),
                       args.source_manifest_sha256, 2 * 1024 * 1024)
                marker_gate(args)
                pinned(absolute(os.path.abspath(__file__)), args.supervisor_sha256, 128 * 1024)
                pinned(os.path.join(args.workspace, 'admission-input-v1/public.pcm'), PCM_SHA256, PCM_BYTES)
            except BaseException:
                unchanged = False
        if args.build_only:
            cache_unchanged = False
            try:
                public_cache_gate(args)
                cache_unchanged = True
            except BaseException:
                pass
        REPORT.update(source_unchanged=unchanged, cache_unchanged=cache_unchanged, cancelled=CANCELLED)
        REPORT['ok'] = (REPORT['ok'] and REPORT['cleanup'] and unchanged
                        and (not args.build_only or cache_unchanged is True) and not CANCELLED)
        REPORT['qualified_public_only'] = REPORT['qualified_public_only'] and REPORT['ok']
        if final_fd is not None:
            try:
                write_all(final_fd, canonical(REPORT) + b'\n')
                os.fsync(final_fd)
            except BaseException:
                REPORT['ok'] = REPORT['qualified_public_only'] = False
                REPORT['report_persistence_failed'] = True
            finally:
                os.close(final_fd)


def pure_unit():
    """No input-file/environment/native/process access in these test bodies."""
    import unittest
    from types import SimpleNamespace

    class Tests(unittest.TestCase):
        def test_frame_encoding_and_cases(self):
            raw, offset = frame(), 8
            self.assertEqual(raw[:8], MAGIC)
            for value in PUBLIC_FIELDS:
                size = int.from_bytes(raw[offset:offset + 2], 'big')
                self.assertEqual(raw[offset + 2:offset + 2 + size], value)
                offset += 2 + size
            self.assertEqual(offset, len(raw))
            cases = public_cases()
            self.assertEqual(len(cases), 8)
            self.assertEqual(len({c[0] for c in cases}), 8)
            self.assertEqual(sum(c[2] == 'REJECT_FRAME' for c in cases), 7)
            self.assertTrue(all(len(c[1]) <= 512 for c in cases))
            self.assertEqual(dict((c[0], c[1]) for c in cases)['over-cap'], MAGIC + b'\x10\x01')

        def test_terminals(self):
            self.assertEqual(terminal('REJECT_FRAME'), b'REAL_ADMISSION_TERMINAL_V1 REJECT_FRAME primary_errors=1 cleanup_errors=0 sessions=0 healthy=false pcm_bytes=0 pcm_eof=false source_closed=false\n')
            self.assertIn(b'primary_errors=0', terminal('PUBLIC_FRAME_CHECKED'))
            self.assertIn(b'primary_errors=1', terminal('BLOCKED_SAFETY'))
            with self.assertRaises(Block):
                terminal('SESSION_HEALTHY')

        def test_strict_json_and_owner_types(self):
            for raw in (b'{"x":1,"x":1}', b'{"x":NaN}', b'{"x":Infinity}'):
                with self.assertRaises(Block):
                    strict_json(raw)
            expected = {'schema': 1, 'purpose': 'public-native-rambler-qualification',
                        'name': 'workspace', 'contains_credentials': False}
            self.assertNotEqual(canonical(expected), canonical(dict(expected, schema=True)))
            self.assertNotEqual(canonical(OWNER_VALUE), canonical(dict(OWNER_VALUE, public_only=1)))

        def test_public_allowlists(self):
            self.assertTrue(source_allowed(EXAMPLE))
            self.assertTrue(source_allowed(FAKE_TESTS))
            self.assertTrue(cache_allowed('registry/cache/public/a.crate'))
            self.assertTrue(cache_allowed('registry/index/public/.cache/aa/bb'))
            for part in ('private', 'captures', 'keys', '.ssh', '.git', '.claude', '.env.example'):
                self.assertFalse(source_allowed('crates/' + part + '/x.rs'))
                self.assertFalse(cache_allowed('registry/src/public/' + part + '/x'))
            for name in ('/Cargo.toml', '../Cargo.toml', 'crates/a/x\\y.rs', 'Cargo.toml/x.rs'):
                self.assertFalse(source_allowed(name))
            self.assertFalse(cache_allowed('registry/cache/../credentials'))

        def test_manifest_pins_and_shapes(self):
            entries = [{'path': n, 'size': 1, 'sha256': 'a' * 64}
                       for n in ('Cargo.toml', 'Cargo.lock', '.cargo/config.toml', EXAMPLE, FAKE_TESTS)]
            raw = canonical({'schema': MANIFEST_SCHEMA, 'files': entries})
            self.assertEqual(len(parse_manifest(raw, 'a' * 64, 'a' * 64)), 5)
            with self.assertRaises(Block):
                parse_manifest(raw, 'b' * 64, 'a' * 64)
            entries[0]['size'] = True
            with self.assertRaises(Block):
                parse_manifest(canonical({'schema': MANIFEST_SCHEMA, 'files': entries}), 'a' * 64, 'a' * 64)
            entries[0]['size'] = 1
            entries.append(entries[0])
            with self.assertRaises(Block):
                parse_manifest(canonical({'schema': MANIFEST_SCHEMA, 'files': entries}), 'a' * 64, 'a' * 64)
            entries = [{'path': 'registry/src/p/x', 'size': 1, 'sha256': 'a' * 64},
                       {'path': 'registry/src/p/x/y', 'size': 1, 'sha256': 'a' * 64}]
            with self.assertRaises(Block):
                parse_entries(canonical({'schema': CACHE_SCHEMA, 'files': entries}),
                              CACHE_SCHEMA, cache_allowed, 10, 10, 100)

        def test_artifact_and_fake_counts(self):
            def record(test, path):
                return canonical({'reason': 'compiler-artifact',
                    'target': {'name': 'linux_real_admission', 'kind': ['example']},
                    'profile': {'test': test}, 'executable': path})
            raw = record(False, '/public/example') + b'\n' + record(True, '/public/fake')
            self.assertEqual(artifact(raw, False), '/public/example')
            self.assertEqual(artifact(raw, True), '/public/fake')
            with self.assertRaises(Block):
                artifact(raw + b'\n' + record(False, '/public/other'), False)
            summary = b'test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'
            self.assertEqual(fake_counts(summary), FAKE_COUNTS)
            for bad in (summary + summary, summary.replace(b'17 passed', b'16 passed')):
                with self.assertRaises(Block):
                    fake_counts(bad)

        def test_status_limits_identity_elf(self):
            raw = (b'Uid: 7 7 7 7\nGid: 8 8 8 8\nPPid: 9\nThreads: 1\nTracerPid: 0\nNoNewPrivs: 1\n'
                   b'SigBlk: 0000000000000000\nSigPnd: 0000000000000000\nShdPnd: 0000000000000000\n'
                   b'SigIgn: 0000000000001000\nSigCgt: 0000000000000440\n'
                   b'CapInh: 0000000000000000\nCapPrm: 0000000000000000\n'
                   b'CapEff: 0000000000000000\nCapAmb: 0000000000000000\n')
            self.assertTrue(status_ok(raw, 7, 8, 9))
            self.assertFalse(status_ok(raw.replace(b'0000000000001000', b'0000000001001000'), 7, 8, 9))
            self.assertFalse(status_ok(raw, 7, 8, 10))
            self.assertFalse(status_ok(raw.replace(b'TracerPid: 0', b'TracerPid: 1'), 7, 8, 9))
            with self.assertRaises(Block):
                status_ok(raw + b'PPid: 9\n', 7, 8, 9)
            limits = (f'Max core file size 0 0 bytes\nMax file size {CAP} {CAP} bytes\n'
                      'Max cpu time 10 10 seconds\n').encode()
            self.assertTrue(limits_ok(limits))
            self.assertFalse(limits_ok(limits.replace(b'0 0 bytes', b'0 unlimited bytes')))
            self.assertFalse(limits_ok(limits + b'Max core file size 0 0 bytes\n'))
            words = ['S', '9'] + ['0'] * 17 + ['12345']
            sample = ('7 (public name) ' + ' '.join(words)).encode()
            self.assertEqual(stat_identity(sample, 7, 9), '12345')
            with self.assertRaises(Block):
                stat_identity(sample.replace(b') S ', b') Z '), 7, 9)
            self.assertTrue(elf(b'\x7fELF\x02\x01' + b'\0' * 10 + b'\x03\0\x3e\0'))
            self.assertFalse(hash_ok('A' * 64))

        def test_target_signal_reset_policy(self):
            calls = []
            def reset(sig, handler):
                calls.append((sig, handler))
                if sig == 32:
                    raise OSError('reserved')
                if sig == 33:
                    raise ValueError('reserved')
            fake = SimpleNamespace(SIGKILL=9, SIGSTOP=19, SIG_DFL=0,
                valid_signals=lambda: {1, 6, 9, 13, 19, 25, 32, 33, 40}, signal=reset)
            restore_target_signals(fake)
            self.assertEqual(calls, [(n, 0) for n in (1, 6, 13, 25, 32, 33, 40)])
            def failed_reset(sig, handler):
                raise OSError('non-reserved reset failure')
            fake.signal = failed_reset
            fake.valid_signals = lambda: {25}
            with self.assertRaises(OSError):
                restore_target_signals(fake)

        def test_cli_unique_exact_and_phase_bound(self):
            public = os.path.normpath('C:/public') if sys.platform == 'win32' else '/public'
            base = ['--build-only', '--workspace', os.path.join(public, 'workspace')]
            for name in ('owner', 'workspace-owner', 'supervisor', 'source-manifest', 'example', 'fake-tests'):
                base += ['--' + name + '-sha256', 'a' * 64]
            base += ['--cache-manifest-sha256', 'a' * 64]
            for name in ('cargo', 'rustc', 'cmake'):
                base += ['--' + name, os.path.join(public, name), '--' + name + '-sha256', 'a' * 64]
            self.assertTrue(arguments(base).build_only)
            for extra in (['--owner-sha256', 'a' * 64], ['--build-final-sha256', 'a' * 64],
                          ['--single-session-v1'], ['--observe-only'], ['--build']):
                with self.assertRaises(Block):
                    arguments(base + extra)

        def test_closed_build_admission(self):
            args = SimpleNamespace(**{name: 'a' * 64 for name in
                ('supervisor_sha256', 'owner_sha256', 'workspace_owner_sha256',
                 'source_manifest_sha256', 'example_sha256', 'fake_tests_sha256')})
            args.build_id = 'admission-public-build-' + '1' * 32
            args.example_binary_sha256, args.fake_binary_sha256 = 'b' * 64, 'c' * 64
            pins = {'example': args.example_binary_sha256, 'fake-tests': args.fake_binary_sha256}
            receipt = receipt_value(args, args.build_id, 'd' * 64, pins, 'e' * 64)
            args.build_receipt_sha256 = sha(canonical(receipt) + b'\n')
            final = build_final_value(args, args.build_id, 'd' * 64, pins,
                                       args.build_receipt_sha256, 'e' * 64)
            validate_build_admission(args, 'd' * 64, receipt, final)
            for key, value in (('ok', False), ('source_unchanged', False), ('cache_unchanged', False),
                               ('cleanup', False), ('cancelled', True), ('ok', 1),
                               ('qualified_public_only', True), ('runs', []), ('extra', False)):
                with self.assertRaises(Block):
                    validate_build_admission(args, 'd' * 64, receipt, dict(final, **{key: value}))
            for key, value in (('ok', False), ('cleanup', 1), ('service_calls', 1),
                               ('example_main_executions', 1), ('extra', False)):
                with self.assertRaises(Block):
                    validate_build_admission(args, 'd' * 64, dict(receipt, **{key: value}), final)

    suite = unittest.defaultTestLoader.loadTestsFromTestCase(Tests)
    result = unittest.TestResult()
    suite.run(result)
    ok = result.wasSuccessful() and result.testsRun == 10
    sys.stdout.write(json.dumps({'unit_ok': ok, 'unit_tests': result.testsRun,
                                 'failures': len(result.failures), 'errors': len(result.errors),
                                 'qualified_public_only': False, 'processes_launched': 0}) + '\n')
    return 0 if ok else 1


if __name__ == '__main__':
    if sys.argv[1:] == ['--unit']:
        sys.exit(pure_unit())
    try:
        native_main()
    except BaseException:
        REPORT['ok'] = REPORT['qualified_public_only'] = False
        REPORT['cancelled'] = CANCELLED
    try:
        sys.stdout.write(canonical(REPORT).decode('ascii') + '\n')
        sys.stdout.flush()
    except BaseException:
        os._exit(1)
    sys.exit(0 if REPORT['ok'] else 1)
