"""SOURCE ONLY, NOT LIVE GO: reservation-bound single-session pipe reference.

Import is inert on Windows/Linux. No credential loader, endpoint override, retry,
subprocess fallback, private report reader or operational provisioning. MAIN alone
owns any later native qualification. See linux-session-pipe-receiver.md.
"""
import contextlib
import hashlib
import os
import re
import select
import signal
import stat
import sys
import time
import types

HELPER_SHA = 'bb711f7ce1ada062cee9ed47136ae0496c38d8095d8ced26229bb2f8328ec0d9'
BINARY_SHA = '1d183b7adece6c2a05ab15a0d8bd5faeaab7bc855cbb89b1091ced81f3883991'
BINARY_BYTES = 70095512
PCM_SHA = 'cea1bc1614f738048b1a4f1bcad83f70aee42b9621b8b754f8aacbe1395b08c9'
PCM_BYTES = 117440
MANIFEST_SHA = '50a8a6f2a81b0e9e3abe60d88f9078ee1ab437a98741cc8d5468b126036ed284'
EXAMPLE_SHA = 'fae7a6b088f3d1a7f6bd7b3f795ae99a3ec119ca6d029cb2ecf9fdf811c2c8c6'
FAKE_SOURCE_SHA = '269fc87b38482f1185595180039768721dbdcd7c7838f145add245e241cdeb0f'
TREE_SHA = '1e0fe2437674b6992351bf7e322129ccb64c3a0d7b552cb2188f9a8dbd5bec39'
PUBLIC_ENV = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
PUBLIC_FIELDS = (b'invented-public-frame-key', b'invalid.public.frame',
                 b'invented-public-frame-cert')
PUBLIC_FRAME = b'RLA1\0\r\n\x1a' + b''.join(len(p).to_bytes(2, 'big') + p for p in PUBLIC_FIELDS)
READY = b'REAL_ADMISSION_READY_V1\n'
MAX_FRAME = 12302
MODES = frozenset(('PUBLIC_GUARD', 'REAL'))
UPSTREAM_READY = {
    'PUBLIC_GUARD': b'RAMBLER_SESSION_READY_V1 PUBLIC_GUARD\n',
    'REAL': b'RAMBLER_SESSION_READY_V1 REAL\n',
}
UPSTREAM_SUCCESS = {
    'PUBLIC_GUARD': b'RAMBLER_SESSION_GUARD_TERMINAL_V1 PUBLIC_TUPLE_REJECTED_BEFORE_RUNTIME sessions=0\n',
    'REAL': b'RAMBLER_SESSION_REAL_TERMINAL_V1 SESSION_HEALTHY sessions=1\n',
}
GUARD_TERMINAL = (b'REAL_ADMISSION_TERMINAL_V1 REJECT_CREDENTIALS primary_errors=1 '
                  b'cleanup_errors=0 sessions=0 healthy=false pcm_bytes=0 '
                  b'pcm_eof=false source_closed=false\n')
REAL_TERMINAL = (b'REAL_ADMISSION_TERMINAL_V1 SESSION_HEALTHY primary_errors=0 '
                 b'cleanup_errors=0 sessions=1 healthy=true pcm_bytes=117440 '
                 b'pcm_eof=true source_closed=true\n')
CODES = frozenset(('STARTUP', 'RESERVATION', 'NATIVE', 'OBSERVE', 'INPUT',
    'PUBLIC_TUPLE', 'FORWARD', 'NATIVE_OUTPUT', 'NATIVE_REJECTED', 'CONNECT',
    'SESSION', 'NATIVE_CLEANUP', 'GUARD_BREACH', 'NATIVE_UNEXPECTED', 'TIMEOUT',
    'CANCELLED', 'CLEANUP', 'PINS', 'RECORD', 'OUTPUT', 'INTERNAL'))
SCHEMA = 'linux-session-pipe-controller-result-v1'
PRE_RUNTIME = frozenset(('BLOCKED_HARDEN', 'BLOCKED_SAFETY', 'BLOCKED_ARGS',
    'BLOCKED_PCM', 'REJECT_FRAME', 'REJECT_CREDENTIALS', 'RUNTIME_FAILURE'))
RUNTIME = frozenset(('CONNECT_FAILURE', 'SESSION_FAILURE',
    'SESSION_AND_CLEANUP_FAILURE', 'CLEANUP_FAILURE', 'SESSION_HEALTHY'))
CATEGORIES = PRE_RUNTIME | RUNTIME | {'IO_FAILURE', 'INTERNAL_FAILURE'}
READY_SECONDS = 10
INPUT_SECONDS = 5
FORWARD_SECONDS = 47
DRAIN_SECONDS = 6
GUARD_SECONDS = 15
PARENT_ALARM = {'PUBLIC_GUARD': 30, 'REAL': 110}

HELPER_APIS = (
    'prctl', 'absolute', 'open_path', 'directory', 'pinned', 'exclusive',
    'empty_children', 'ownership', 'drain', 'child_exec', 'run', 'read_public',
    'initialize', 'check_cancel', 'parse_manifest', 'source_gate', 'strict_json',
    'canonical', 'save_new', 'write_all', 'frozen_tool', 'freeze', 'policy',
    'observe_before_write', 'target_exec', 'restore_target_signals')


class ClosedFailure(Exception):
    def __init__(self, code):
        self.code = code if code in CODES else 'INTERNAL'
        super().__init__(self.code)


def require(ok, code='STARTUP'):
    if not ok:
        raise ClosedFailure(code)


def public_path(path):
    # Linux spelling even when imported by Windows unit tests. No home expansion.
    require(isinstance(path, str) and path.startswith('/') and not path.startswith('//')
            and '\\' not in path and ':' not in path and '\0' not in path
            and len(path) <= 4096)
    parts = path.split('/')[1:]
    forbidden = {'.claude', '.git', '.ssh', 'keys', 'private', 'captures', 'secrets',
                 'privatecaptures'}
    require(all(p and p not in ('.', '..') and p.lower() not in forbidden
                and not p.lower().startswith('.env')
                and re.fullmatch('[A-Za-z0-9_.+-]+', p) is not None for p in parts))
    return path


def trusted_open(path, directory=False):
    """Bootstrap no-follow walk; every ancestor is trusted, not merely the leaf."""
    public_path(path)
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        root = os.fstat(fd)
        require(root.st_uid == 0 and not root.st_mode & 0o022)
        parts = path.split('/')[1:]
        for index, part in enumerate(parts):
            is_dir = index < len(parts) - 1 or directory
            flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK
            if is_dir:
                flags |= os.O_DIRECTORY
            new = os.open(part, flags, dir_fd=fd)
            os.close(fd)
            fd = new
            info = os.fstat(fd)
            if is_dir:
                require(stat.S_ISDIR(info.st_mode) and info.st_uid in (0, os.getuid())
                        and not info.st_mode & 0o022)
        result, fd = fd, None
        return result
    finally:
        if fd is not None:
            os.close(fd)


def public_bytes(path, digest, cap, mode=0o600, size=None):
    """Strict single-link owned public input, stable across bounded hash read."""
    fd = trusted_open(path)
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                and before.st_nlink == 1 and stat.S_IMODE(before.st_mode) == mode
                and 0 <= before.st_size <= cap
                and (size is None or before.st_size == size))
        pieces, total = [], 0
        while True:
            chunk = os.read(fd, min(1024 * 1024, cap + 1 - total))
            if not chunk:
                break
            total += len(chunk)
            require(total <= cap)
            pieces.append(chunk)
        after = os.fstat(fd)
        require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                 before.st_ctime_ns, before.st_mode, before.st_nlink, before.st_uid) ==
                (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns,
                 after.st_ctime_ns, after.st_mode, after.st_nlink, after.st_uid))
        raw = b''.join(pieces)
        require(len(raw) == before.st_size and hashlib.sha256(raw).hexdigest() == digest)
        return raw
    finally:
        os.close(fd)


def public_directory(path):
    fd = trusted_open(path, True)
    try:
        info = os.fstat(fd)
        require(info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o700)
    finally:
        os.close(fd)


def load_helper(path):
    # Execute ONLY bytes already checked, not a second path-based import/open.
    raw = public_bytes(path, HELPER_SHA, 128 * 1024)
    module = types.ModuleType('_byte_pinned_public_admission_helper')
    module.__file__ = path
    exec(compile(raw, '<byte-pinned-public-helper>', 'exec'), module.__dict__)
    require(all(callable(getattr(module, name, None)) for name in HELPER_APIS))
    return module


def bounded_public_input(io, end):
    """I/O-injected bounded comparator. Never hashes, decodes or logs input.

    At most 86 bytes are read. Matching 85 bytes without EOF is NOT accepted.
    The return value deliberately carries no untrusted frame bytes.
    """
    raw = b''
    while True:
        io.guard()
        require(io.now() < end, 'TIMEOUT')
        if not io.input_ready(end):
            continue
        try:
            chunk = io.input_read(len(PUBLIC_FRAME) + 1 - len(raw))
        except BlockingIOError:
            continue
        if chunk == b'':
            require(raw == PUBLIC_FRAME, 'INPUT')
            return
        raw += chunk
        require(len(raw) <= len(PUBLIC_FRAME) and PUBLIC_FRAME.startswith(raw), 'INPUT')


def session_target_exec(binary, cwd, input_fd, output_fd, pcm_fd, gate, parent):
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
        argv = [binary, '--single-session-v1']
        os.execve(binary, argv, {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
    except BaseException:
        os._exit(126)


class LinuxPort:
    """Single-threaded Linux implementation; construction itself has no effects."""
    def __init__(self, args):
        self.args = args
        self.mode = args.mode
        self.buffer = None
        self.used = 0
        self.reservation_consumed = False
        self.terminal = None
        self.h = None
        self.handles = set()
        self.pidfd = None
        self.pid = None
        self.parent = None
        self.initialized = False
        self.prepared = False
        self.launched = False
        self.record_fd = None
        self.owned = None
        self.expected = None
        self.end = None

    def prepare(self):
        require(sys.platform == 'linux' and sys.flags.isolated == 1
                and sys.flags.no_site == 1 and sys.dont_write_bytecode)
        require(os.getuid() > 0 and os.getgid() > 0 and os.getuid() == os.geteuid()
                and os.getgid() == os.getegid())
        # Trusted env -i is a prerequisite, not a retrospective secret scrubber.
        require(dict(os.environ) == PUBLIC_ENV)
        os.umask(0o077)
        self.h = load_helper(self.args.helper)
        self.h.initialize()  # core0, dumpable0, stable-parent PDEATHSIG, subreaper
        signal.alarm(PARENT_ALARM[self.mode])
        self.initialized = True
        self.parent = os.getppid()
        self.h.prctl(38, 1)  # PR_SET_NO_NEW_PRIVS, inherited by the target
        self.final_guard()
        require(stat.S_ISFIFO(os.fstat(0).st_mode) and os.get_blocking(0))
        require(stat.S_ISFIFO(os.fstat(1).st_mode) and os.get_blocking(1))
        null = os.stat('/dev/null')
        stderr = os.fstat(2)
        require(stat.S_ISCHR(stderr.st_mode) and stderr.st_rdev == null.st_rdev)
        self.controller = public_path(os.path.abspath(__file__))
        public_bytes(self.controller, self.args.controller_sha256, 128 * 1024)
        self.root = self.args.root
        public_directory(self.root)
        require(set(os.listdir(self.root)) == {'owner.json', 'inputs', 'reservations', 'cases'})
        owner = public_bytes(self.root + '/owner.json', self.args.owner_sha256, 4096)
        expected_owner = {'schema': 'linux-session-pipe-controller-owner-v1',
                          'name': os.path.basename(self.root), 'public_inputs_only': True}
        require(self.h.canonical(self.h.strict_json(owner)) == self.h.canonical(expected_owner))
        self.inputs, self.cases = self.root + '/inputs', self.root + '/cases'
        self.reservations = self.root + '/reservations'
        public_directory(self.reservations)
        require(set(os.listdir(self.reservations)) == {self.args.reservation})
        self.reservation_check(self.reservations + '/' + self.args.reservation)
        public_directory(self.inputs)
        public_directory(self.cases)
        require(set(os.listdir(self.inputs)) ==
                {'source-manifest.json', 'source', 'public.pcm', 'example-input'})
        self.source = self.inputs + '/source'
        self.source_check()
        self.input_pins()
        self.h.policy()
        require(self.h.empty_children())
        # Fresh namespace, no retries, adoption, old reservation or old report IO.
        self.owned = self.cases + '/session-pipe-case-' + os.urandom(16).hex()
        os.mkdir(self.owned, 0o700)
        public_directory(self.owned)
        self.case_owner = {'schema': 'linux-session-pipe-case-owner-v1', 'public_inputs_only': True}
        self.h.save_new(self.owned + '/owner.json', self.case_owner)
        self.record_fd = self.h.exclusive(self.owned + '/result.json')
        self.binary = self.owned + '/example'
        require(self.h.freeze(self.inputs + '/example-input', self.binary) == BINARY_SHA)
        pcm = public_bytes(self.inputs + '/public.pcm', PCM_SHA, PCM_BYTES, size=PCM_BYTES)
        self.pcm = self.owned + '/public.pcm'
        fd = self.h.exclusive(self.pcm)
        try:
            self.h.write_all(fd, pcm)
            os.fsync(fd)
        finally:
            os.close(fd)
        self.cwd = self.owned + '/target-cwd'
        os.mkdir(self.cwd, 0o700)
        self.case_pins()
        self.prepared = True

    def source_check(self):
        raw = public_bytes(self.inputs + '/source-manifest.json', MANIFEST_SHA, 2 * 1024 * 1024)
        expected = self.h.parse_manifest(raw, EXAMPLE_SHA, FAKE_SOURCE_SHA)
        if self.expected is not None:
            require(expected == self.expected)
        self.expected = expected
        # Stronger explicit 0600/0700 file/directory contract than helper defaults.
        public_directory(self.source)
        for name, item in expected.items():
            parts = name.split('/')
            for index in range(1, len(parts)):
                public_directory(self.source + '/' + '/'.join(parts[:index]))
            public_bytes(self.source + '/' + name, item['sha256'], 8 * 1024 * 1024,
                         size=item['size'])
        require(self.h.source_gate(self.source, expected) == TREE_SHA)

    def input_pins(self):
        public_bytes(self.args.helper, HELPER_SHA, 128 * 1024)
        public_bytes(self.controller, self.args.controller_sha256, 128 * 1024)
        public_bytes(self.root + '/owner.json', self.args.owner_sha256, 4096)
        public_bytes(self.inputs + '/public.pcm', PCM_SHA, PCM_BYTES, size=PCM_BYTES)
        public_bytes(self.inputs + '/example-input', BINARY_SHA, BINARY_BYTES,
                     mode=0o500, size=BINARY_BYTES)
        self.h.frozen_tool(self.inputs + '/example-input', BINARY_SHA)

    def case_pins(self):
        public_directory(self.owned)
        public_directory(self.cwd)
        require(set(os.listdir(self.owned)) ==
                {'owner.json', 'result.json', 'example', 'public.pcm', 'target-cwd'} |
                ({'reservation.json'} if self.reservation_consumed else set()))
        owner_sha = hashlib.sha256(self.h.canonical(self.case_owner) + b'\n').hexdigest()
        public_bytes(self.owned + '/owner.json', owner_sha, 4096)
        self.check_record()
        public_bytes(self.binary, BINARY_SHA, BINARY_BYTES, mode=0o500, size=BINARY_BYTES)
        self.h.frozen_tool(self.binary, BINARY_SHA)
        public_bytes(self.pcm, PCM_SHA, PCM_BYTES, size=PCM_BYTES)
        require(not os.listdir(self.cwd))

    def start(self):
        require(self.prepared and self.reservation_consumed and self.h.empty_children(), 'NATIVE')
        self.final_guard()
        read_in, self.write_in = os.pipe2(os.O_CLOEXEC)
        self.handles.update((read_in, self.write_in))
        self.read_out, write_out = os.pipe2(os.O_CLOEXEC)
        self.handles.update((self.read_out, write_out))
        gate_in, gate_out = os.pipe2(os.O_CLOEXEC)
        self.handles.update((gate_in, gate_out))
        pcm = self.h.open_path(self.pcm)
        self.handles.add(pcm)
        os.set_blocking(pcm, True)
        if self.mode == 'REAL':
            import fcntl
            require(fcntl.fcntl(self.write_in, 1032) >= 16384, 'FORWARD')
        # Bind the AST-equivalent exec to ONLY the byte-pinned helper globals.
        target = types.FunctionType(session_target_exec.__code__, self.h.__dict__)
        parent = os.getpid()
        self.pid = os.fork()
        if self.pid == 0:
            try:
                os.close(gate_out)
                target(self.binary, self.cwd, read_in, write_out, pcm, gate_in, parent)
            except BaseException:
                pass
            # Never return a launcher failure into the parent's protocol/record
            # state machine, even if a pre-target descriptor operation fails.
            os._exit(126)
        self.launched = True
        for fd in (read_in, write_out, gate_in, pcm):
            self.close_owned(fd)
        self.pidfd = os.pidfd_open(self.pid, 0)
        require(self.h.ownership(self.pidfd) is None, 'NATIVE')
        self.guard()
        require(os.write(gate_out, b'G') == 1, 'NATIVE')
        self.close_owned(gate_out)
        os.set_blocking(self.read_out, False)
        os.set_blocking(self.write_in, False)  # Does not change the child read end.
        self.gate_time = time.monotonic()
        self.end = self.gate_time + (GUARD_SECONDS if self.mode == 'PUBLIC_GUARD'
                                          else READY_SECONDS)

    def close_owned(self, fd):
        os.close(fd)
        self.handles.remove(fd)

    def final_guard(self):
        require(self.initialized and self.h is not None, 'STARTUP')
        if self.h.CANCELLED:
            raise ClosedFailure('CANCELLED')
        require(os.getppid() == self.parent and self.parent > 1, 'CANCELLED')
        require(self.h.resource.getrlimit(self.h.resource.RLIMIT_CORE) == (0, 0), 'NATIVE')
        # Getter operations take no pointer argument. Never read process environ.
        require(self.h.LIBC.prctl(3, 0, 0, 0, 0) == 0, 'NATIVE')
        require(self.h.LIBC.prctl(39, 0, 0, 0, 0) == 1, 'NATIVE')

    def guard(self):
        self.final_guard()
        require(self.pidfd is not None and self.h.ownership(self.pidfd) is None, 'NATIVE')

    def now(self):
        return time.monotonic()

    def wait(self, fd, end, living=True):
        if living:
            self.guard()
        else:
            self.final_guard()
        remaining = end - self.now()
        require(remaining > 0, 'TIMEOUT')
        watched = [fd, self.pidfd] if living else [fd]
        ready = select.select(watched, [], [], min(0.05, remaining))[0]
        if living:
            self.guard()
        return fd in ready

    def read_native_exact(self, expected, living):
        raw = b''
        while len(raw) < len(expected):
            if not self.wait(self.read_out, self.end, living):
                continue
            try:
                chunk = os.read(self.read_out, len(expected) - len(raw))
            except BlockingIOError:
                continue
            require(chunk, 'NATIVE')
            raw += chunk
            require(expected.startswith(raw), 'NATIVE')
        require(raw == expected, 'NATIVE')

    def native_ready(self):
        self.read_native_exact(READY, True)
        require(not select.select([self.read_out], [], [], 0)[0], 'NATIVE')

    def observe(self):
        self.guard()
        require(not select.select([self.read_out], [], [], 0)[0], 'NATIVE')
        # Reviewed exact external living/stat/status/limits/policy and OPEN-only
        # EACCES check. The helper closes unexpected environ-open success unread.
        self.h.observe_before_write(self.pid, self.pidfd)
        self.guard()

    def emit_ready(self):
        self.guard()
        self.emit(UPSTREAM_READY[self.mode])  # os.write: unbuffered pipe; nothing remains to flush.

    def input_ready(self, end):
        return self.wait(0, end)

    def input_read(self, cap):
        return os.read(0, cap)







    def cleanup(self):
        closed, drained = True, False
        try:
            for fd in tuple(self.handles):
                try:
                    self.close_owned(fd)
                except BaseException:
                    closed = False
        finally:
            try:
                if self.initialized:
                    drained = self.h.drain() is True
                    drained = drained and self.h.empty_children() is True  # actual ECHILD
                elif self.h is not None:
                    drained = self.h.empty_children() is True
            finally:
                if self.pidfd is not None:
                    try:
                        os.close(self.pidfd)
                    except BaseException:
                        closed = False
                    self.pidfd = None
        return closed and drained

    def recheck(self):
        if not self.prepared:
            return False
        self.final_guard()
        public_directory(self.root)
        public_directory(self.inputs)
        public_directory(self.cases)
        public_directory(self.reservations)
        require(set(os.listdir(self.root)) == {'owner.json', 'inputs', 'reservations', 'cases'})
        require(set(os.listdir(self.inputs)) ==
                {'source-manifest.json', 'source', 'public.pcm', 'example-input'})
        require(not os.listdir(self.reservations), 'PINS')
        require(self.reservation_consumed, 'RESERVATION')
        self.reservation_check(self.owned + '/reservation.json')
        self.input_pins()
        self.source_check()
        self.case_pins()
        self.h.policy()
        require(self.h.empty_children(), 'CLEANUP')
        return True

    def check_record(self):
        require(self.record_fd is not None, 'RECORD')
        fd = trusted_open(self.owned + '/result.json')
        try:
            info, held = os.fstat(fd), os.fstat(self.record_fd)
            require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                    and stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1
                    and info.st_size == 0 and (info.st_dev, info.st_ino) ==
                    (held.st_dev, held.st_ino), 'RECORD')
        finally:
            os.close(fd)

    def persist(self, report):
        if self.record_fd is None:
            require(not report['ok'], 'RECORD')
            return
        fd = self.record_fd
        try:
            self.check_record()
            self.h.write_all(fd, self.h.canonical(report) + b'\n')
            os.fsync(fd)
        finally:
            self.record_fd = None
            os.close(fd)

    def emit(self, raw):
        # Fixed protocol bytes only. Nonblocking upstream avoids an unbounded
        # blocked write; each <= PIPE_BUF message is a single all-or-error write.
        os.set_blocking(1, False)
        require(os.write(1, raw) == len(raw), 'OUTPUT')




    def reservation_check(self, path):
        raw = public_bytes(path, self.args.reservation_sha256, 4096)
        value = self.h.strict_json(raw)
        expected = {'schema': 'linux-session-pipe-reservation-v1',
                    'mode': self.mode, 'id': self.args.reservation_id}
        require(self.h.canonical(value) == self.h.canonical(expected), 'RESERVATION')

    def consume_reservation(self):
        require(self.prepared and not self.reservation_consumed, 'RESERVATION')
        public_directory(self.reservations)
        require(set(os.listdir(self.reservations)) == {self.args.reservation}, 'RESERVATION')
        destination = self.owned + '/reservation.json'
        # Fresh case, trusted single-owner namespace: no adoption and no retry.
        require(not os.path.lexists(destination), 'RESERVATION')
        os.rename(self.reservations + '/' + self.args.reservation, destination)
        self.reservation_consumed = True  # A failed post-rename check still consumes it.
        self.reservation_check(destination)

    def input_readinto(self, view):
        return os.readv(0, [view])

    def read_frame(self):
        self.guard()
        os.set_blocking(0, False)
        end = self.now() + INPUT_SECONDS
        if self.mode == 'PUBLIC_GUARD':
            bounded_public_input(self, min(self.end, end))
        else:
            self.buffer = bytearray(MAX_FRAME)
            self.used = bounded_real_input(self, self.buffer, end)
            require(memoryview(self.buffer)[:self.used] != PUBLIC_FRAME, 'PUBLIC_TUPLE')

    def forward_guard(self):
        # No argument or mutable caller-provided frame can enter this path.
        self.guard()
        require(os.write(self.write_in, PUBLIC_FRAME) == len(PUBLIC_FRAME), 'FORWARD')
        self.close_owned(self.write_in)

    def forward_real(self):
        self.guard()
        require(self.buffer is not None and 17 <= self.used <= MAX_FRAME, 'FORWARD')
        try:
            # Empty >=16384-byte pipe, nonblocking, exactly one write. No continuation.
            require(os.write(self.write_in, memoryview(self.buffer)[:self.used]) == self.used,
                    'FORWARD')
            self.close_owned(self.write_in)
        finally:
            self.wipe()

    def forward(self):
        if self.mode == 'REAL':
            self.end = self.now() + FORWARD_SECONDS
            self.forward_real()
        else:
            self.forward_guard()

    def wipe(self):
        overwrite(self.buffer)
        self.buffer = None
        self.used = 0

    def terminal_read(self, cap):
        while True:
            if not self.wait(self.read_out, self.end, False):
                continue
            try:
                return os.read(self.read_out, cap)
            except BlockingIOError:
                continue

    def native_terminal(self):
        # Never relay raw output. No decoding: a closed bytes regex only.
        raw = bytearray()
        while len(raw) < 256:
            part = self.terminal_read(1)
            require(part, 'NATIVE_OUTPUT')
            raw += part
            if part == b'\n':
                break
        require(raw.endswith(b'\n'), 'NATIVE_OUTPUT')
        parsed = parse_terminal(raw, allow_public=self.mode == 'PUBLIC_GUARD')
        require(self.terminal_read(1) == b'', 'NATIVE_OUTPUT')
        # A terminal observation counts only after the closed line AND EOF.
        self.terminal = parsed
        info = None
        while self.now() < self.end:
            self.final_guard()
            info = self.h.ownership(self.pidfd)
            if info is not None:
                break
            select.select([self.pidfd], [], [], min(0.01, max(0, self.end - self.now())))
        require(info is not None, 'TIMEOUT')
        parsed['exit'] = (info.si_status if info.si_code == os.CLD_EXITED
                          and info.si_status in (0, 1) else 'OTHER')
        require(not os.listdir(self.cwd), 'NATIVE')
        self.h.policy()
        return parsed

    def emit_success(self):
        self.emit(UPSTREAM_SUCCESS[self.mode])

    def emit_error(self, code, rpc):
        require(code in CODES and rpc in ('ZERO', 'ONE', 'UNKNOWN'), 'INTERNAL')
        self.emit(error_line(self.mode, code, rpc))
        with contextlib.suppress(BaseException):
            os.write(2, b'SESSION_PIPE_RECEIVER_FAILED_V1\n')


def arguments(argv):
    """Exactly eight unique, separated pairs; public metadata selectors only."""
    names = ('root', 'owner-sha256', 'controller-sha256', 'helper', 'mode',
             'reservation', 'reservation-sha256', 'helper-sha256')
    require(len(argv) == 16)
    result = {}
    for index in range(0, len(argv), 2):
        flag, value = argv[index:index + 2]
        require(type(flag) is str and flag.startswith('--'))
        name = flag[2:]
        require(name in names and name not in result and type(value) is str
                and value and not value.startswith('--'))
        result[name] = value
    require(set(result) == set(names))
    for name in ('owner-sha256', 'controller-sha256', 'reservation-sha256', 'helper-sha256'):
        require(re.fullmatch('[0-9a-f]{64}', result[name]) is not None)
    require(result['helper-sha256'] == HELPER_SHA)
    for name in ('root', 'helper'):
        public_path(result[name])
    require(result['mode'] in ('public-guard', 'real'))
    match = re.fullmatch(r'(public-guard|real)-([0-9a-f]{32})\.json', result['reservation'])
    require(match is not None and match[1] == result['mode'])
    result['reservation-id'] = match[2]
    result['mode'] = {'public-guard': 'PUBLIC_GUARD', 'real': 'REAL'}[result['mode']]
    return types.SimpleNamespace(**{k.replace('-', '_'): v for k, v in result.items()})


def overwrite(buffer):
    """Best effort only; no claim about CPython, kernel, swap or other copies."""
    if buffer is not None:
        buffer[:] = b'\0' * len(buffer)


def bounded_real_input(io, buffer, end):
    """Segment-exact readv seam; never decode/hash/log/copy a secret field."""
    require(type(buffer) is bytearray and len(buffer) == MAX_FRAME, 'INPUT')
    view = memoryview(buffer)
    eof = bytearray(1)
    complete = False
    offset = 0

    def segment(size, magic=False):
        nonlocal offset
        stop = offset + size
        start = offset
        while offset < stop:
            io.guard()
            require(io.now() < end, 'TIMEOUT')
            if not io.input_ready(end):
                continue
            try:
                n = io.input_readinto(view[offset:stop])
            except BlockingIOError:
                continue
            require(type(n) is int and 0 < n <= stop - offset, 'INPUT')
            offset += n
            if magic:
                require(view[start:offset] == memoryview(b'RLA1\0\r\n\x1a')[:offset-start],
                        'INPUT')

    try:
        segment(8, True)
        for _ in range(3):
            segment(2)
            size = buffer[offset - 2] * 256 + buffer[offset - 1]
            require(1 <= size <= 4096, 'INPUT')
            segment(size)
        while True:
            io.guard()
            require(io.now() < end, 'TIMEOUT')
            if not io.input_ready(end):
                continue
            try:
                n = io.input_readinto(memoryview(eof))
            except BlockingIOError:
                continue
            require(n == 0, 'INPUT')
            break
        complete = True
        return offset
    finally:
        overwrite(eof)
        if not complete:
            overwrite(buffer)
        view.release()


def parse_terminal(raw, allow_public=False):
    """Closed category/counters/flags; raw bytes never leave this parser."""
    match = re.fullmatch(
        rb'REAL_ADMISSION_TERMINAL_V1 ([A-Z_]+) primary_errors=([01]) '
        rb'cleanup_errors=([0-3]) sessions=([01]) healthy=(true|false) '
        rb'pcm_bytes=(0|[1-9][0-9]{0,5}) pcm_eof=(true|false) '
        rb'source_closed=(true|false)\n', raw)
    require(match is not None and len(raw) <= 256, 'NATIVE_OUTPUT')
    category = next((c for c in CATEGORIES | ({'PUBLIC_FRAME_CHECKED'} if allow_public else set())
                     if match[1] == c.encode('ascii')), None)
    require(category is not None, 'NATIVE_OUTPUT')
    primary, cleanup, sessions, count = (int(match[i]) for i in (2, 3, 4, 6))
    require(count <= PCM_BYTES, 'NATIVE_OUTPUT')
    return {'category': category, 'sessions': sessions, 'healthy': match[5] == b'true',
            'guard_exact': raw == GUARD_TERMINAL, 'real_exact': raw == REAL_TERMINAL,
            'exit': None}


def classify(mode, terminal):
    category = terminal['category']
    if mode == 'PUBLIC_GUARD':
        if (terminal['exit'] == 0 or category == 'PUBLIC_FRAME_CHECKED'
                or category in RUNTIME or terminal['sessions'] == 1):
            return 'GUARD_BREACH'
        if terminal['guard_exact'] and terminal['exit'] == 1:
            return None
        if terminal['exit'] == 'OTHER':
            return 'NATIVE'
        return 'NATIVE_UNEXPECTED'
    if terminal['real_exact'] and terminal['exit'] == 0:
        return None
    if terminal['exit'] == 'OTHER':
        return 'NATIVE'
    if category in ('REJECT_FRAME', 'REJECT_CREDENTIALS'):
        return 'NATIVE_REJECTED'
    if category == 'CONNECT_FAILURE':
        return 'CONNECT'
    if category in ('SESSION_FAILURE', 'SESSION_AND_CLEANUP_FAILURE'):
        return 'SESSION'
    if category == 'CLEANUP_FAILURE':
        return 'NATIVE_CLEANUP'
    return 'NATIVE'


def observe_terminal(report, terminal):
    if terminal is None:
        return
    report['native_category'] = terminal['category']
    report['native_exit'] = terminal['exit']
    report['sessions'] = terminal['sessions']
    if terminal['category'] == 'SESSION_HEALTHY':
        report['rpc'] = 'ONE'
    elif (terminal['category'] in PRE_RUNTIME and terminal['sessions'] == 0
          and terminal['exit'] == 1):
        report['rpc'] = 'ZERO'  # Source-derived, NOT measured service telemetry.
    else:
        report['rpc'] = 'UNKNOWN'
    if (report['mode'] == 'PUBLIC_GUARD' and
            (terminal['exit'] == 0 or terminal['category'] == 'PUBLIC_FRAME_CHECKED'
             or terminal['category'] in RUNTIME or terminal['sessions'] == 1)):
        report['rpc'] = 'UNKNOWN'


def result_record(mode):
    require(mode in MODES)
    return {'schema': SCHEMA, 'mode': mode,
            'qualification': 'PUBLIC_GUARD_ONLY' if mode == 'PUBLIC_GUARD' else 'REAL_SINGLE_ATTEMPT',
            'ok': False, 'error': 'STARTUP', 'cleanup': False,
            'source_unchanged': False, 'pins_unchanged': False,
            'reservation_consumed': False, 'external_checks': 0,
            'frames_forwarded': 0, 'native_category': None, 'native_exit': None,
            'sessions': 0, 'rpc': 'ZERO', 'healthy': False}


def run_once(port, mode):
    """No loop/retry around launch or forward; fake port is the QA boundary."""
    report = result_record(mode)
    stage, code, native_ok, forwarding = 'STARTUP', None, False, False
    try:
        require(port.mode == mode)
        port.prepare()
        stage = 'RESERVATION'
        port.consume_reservation()
        report['reservation_consumed'] = True
        stage = 'NATIVE'
        port.start()  # The ONLY fork/exec occurs before any upstream input read.
        port.native_ready()
        stage = 'OBSERVE'
        port.observe()
        report['external_checks'] = 1
        stage = 'OUTPUT'
        port.emit_ready()
        stage = 'INPUT'
        port.read_frame()
        stage = 'OBSERVE'
        port.observe()  # Exact helper EACCES/status/limits/policy proof, then write.
        report['external_checks'] = 2
        stage = 'FORWARD'
        forwarding = True
        report['rpc'], report['sessions'] = 'UNKNOWN', 'UNKNOWN'
        port.forward()  # Exactly one call site; partial writes cannot resume.
        report['frames_forwarded'] = 1
        stage = 'NATIVE_OUTPUT'
        terminal = port.native_terminal()
        observe_terminal(report, terminal)
        code = classify(mode, terminal)
        native_ok = code is None
    except ClosedFailure as exc:
        code = exc.code
    except BaseException:
        code = stage
    finally:
        observe_terminal(report, port.terminal)
        report['reservation_consumed'] = port.reservation_consumed is True
        try:
            port.wipe()
        except BaseException:
            code = 'INTERNAL'
        try:
            report['cleanup'] = port.cleanup() is True
        except BaseException:
            report['cleanup'] = False
        if not report['cleanup']:
            code = 'CLEANUP'
        try:
            unchanged = port.recheck()
            report['source_unchanged'] = unchanged is True
            report['pins_unchanged'] = unchanged is True
        except BaseException:
            pass
        if not report['source_unchanged'] or not report['pins_unchanged']:
            code = code or 'PINS'
        try:
            port.final_guard()
        except ClosedFailure as exc:
            code = exc.code
        except BaseException:
            code = 'CANCELLED'
    if forwarding and port.terminal is None:
        report['rpc'], report['sessions'] = 'UNKNOWN', 'UNKNOWN'
    report['ok'] = bool(native_ok and code is None and report['cleanup']
                        and report['source_unchanged'] and report['pins_unchanged']
                        and report['reservation_consumed'])
    report['healthy'] = bool(report['ok'] and mode == 'REAL'
                             and report['native_category'] == 'SESSION_HEALTHY')
    report['error'] = None if report['ok'] else (code or 'INTERNAL')
    try:
        port.persist(report)
    except BaseException:
        report['ok'], report['healthy'], report['error'] = False, False, 'RECORD'
    try:
        port.final_guard()
        if report['ok']:
            port.emit_success()
            return 0
    except ClosedFailure as exc:
        report['error'] = exc.code
    except BaseException:
        report['error'] = 'OUTPUT'
    # Persisted OK is NOT independently trusted if the final write/guard failed.
    with contextlib.suppress(BaseException):
        port.emit_error(report['error'] or 'INTERNAL', report['rpc'])
    return 1


def error_line(mode, code, rpc):
    require(mode in MODES and code in CODES and rpc in ('ZERO', 'ONE', 'UNKNOWN'), 'INTERNAL')
    return ('RAMBLER_SESSION_ERROR_V1 ' + mode + ' ' + code + ' rpc=' + rpc + '\n').encode('ascii')


def main(argv=None):
    try:
        args = arguments(list(sys.argv[1:] if argv is None else argv))
    except BaseException:
        # Malformed CLI has no trusted mode. Never emit a guessed REAL READY.
        with contextlib.suppress(BaseException):
            os.write(2, b'SESSION_PIPE_RECEIVER_FAILED_V1\n')
        return 1
    return run_once(LinuxPort(args), args.mode)


if __name__ == '__main__':
    sys.exit(main())
