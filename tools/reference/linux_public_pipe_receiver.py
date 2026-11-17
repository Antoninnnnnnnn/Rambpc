"""SOURCE ONLY: developer PUBLIC-ONLY pipe bridge, not a secret/session channel.

No import-time process, file, signal, resource, environment or kernel operations.
The only accepted stdin message is PUBLIC_FRAME followed by EOF. See the separate
contract in docs/research/linux-public-pipe-receiver.md before any future use.
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
SUCCESS = (b'REAL_ADMISSION_TERMINAL_V1 PUBLIC_FRAME_CHECKED primary_errors=0 '
           b'cleanup_errors=0 sessions=0 healthy=false pcm_bytes=0 '
           b'pcm_eof=false source_closed=false\n')
CODES = frozenset(('STARTUP', 'NATIVE', 'INPUT', 'TIMEOUT', 'CANCELLED',
                   'CLEANUP', 'PINS', 'RECORD', 'OUTPUT', 'INTERNAL'))
SCHEMA = 'linux-public-pipe-receiver-result-v1'
# All twelve functions in the reviewed helper block, plus the exact additional
# APIs used below. No guessed names, fallback implementation or helper CLI call.
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


def arguments(argv):
    """Four mandatory, unique options; no modes, frame, endpoints or overrides."""
    names = ('public-root', 'owner-sha256', 'receiver-sha256', 'helper')
    require(len(argv) == 2 * len(names))
    result = {}
    for index in range(0, len(argv), 2):
        flag, value = argv[index:index + 2]
        require(isinstance(flag, str) and flag.startswith('--'))
        name = flag[2:]
        require(name in names and name not in result and isinstance(value, str) and value)
        result[name] = value
    require(set(result) == set(names))
    for name in ('owner-sha256', 'receiver-sha256'):
        require(re.fullmatch('[0-9a-f]{64}', result[name]) is not None)
    for name in ('public-root', 'helper'):
        public_path(result[name])
    return types.SimpleNamespace(**{k.replace('-', '_'): v for k, v in result.items()})


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


def result_record():
    return {'schema': SCHEMA, 'public_only': True, 'ok': False, 'cleanup': False,
            'source_unchanged': False, 'pins_unchanged': False, 'external_checks': 0,
            'public_frames_forwarded': 0, 'sessions': 0, 'healthy': False,
            'service_calls': 0, 'error': 'STARTUP'}


def receive_once(port):
    """Protocol state machine; tests supply a completely fake kernel/I/O port."""
    report = result_record()
    code, stage, native_ok = None, 'STARTUP', False
    try:
        port.prepare()
        stage = 'NATIVE'
        port.start()
        port.native_ready()
        port.observe()
        report['external_checks'] = 1
        stage = 'OUTPUT'
        port.emit_ready()
        stage = 'INPUT'
        port.read_public_frame()
        stage = 'NATIVE'
        port.observe()  # Second EACCES/status/policy proof immediately before write.
        report['external_checks'] = 2
        port.forward_public()  # No caller-supplied byte argument exists.
        report['public_frames_forwarded'] = 1
        port.native_terminal()
        native_ok = True
    except ClosedFailure as exc:
        code = exc.code
    except BaseException:
        code = stage
    finally:
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
            code = 'PINS' if code is None else code
        try:
            port.final_guard()
        except ClosedFailure as exc:
            code = exc.code
        except BaseException:
            code = 'CANCELLED'
    report['ok'] = bool(native_ok and code is None and report['cleanup']
                        and report['source_unchanged'] and report['pins_unchanged'])
    report['error'] = None if report['ok'] else (code or 'INTERNAL')
    try:
        port.persist(report)
    except BaseException:
        report['ok'], report['error'] = False, 'RECORD'
    try:
        port.final_guard()
        if report['ok']:
            port.emit_success()
            return 0
    except ClosedFailure as exc:
        report['error'] = exc.code
    except BaseException:
        report['error'] = 'OUTPUT'
    # No native stdout/stderr, exception text, argv, env or frame data escapes.
    with contextlib.suppress(BaseException):
        port.emit_error(report['error'] or 'INTERNAL')
    return 1


class LinuxPort:
    """Single-threaded Linux implementation; construction itself has no effects."""
    def __init__(self, args):
        self.args = args
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
        self.initialized = True
        self.parent = os.getppid()
        self.h.prctl(38, 1)  # PR_SET_NO_NEW_PRIVS, inherited by the target
        self.final_guard()
        signal.alarm(30)
        require(stat.S_ISFIFO(os.fstat(0).st_mode) and os.get_blocking(0))
        require(stat.S_ISFIFO(os.fstat(1).st_mode) and os.get_blocking(1))
        null = os.stat('/dev/null')
        stderr = os.fstat(2)
        require(stat.S_ISCHR(stderr.st_mode) and stderr.st_rdev == null.st_rdev)
        self.receiver = public_path(os.path.abspath(__file__))
        public_bytes(self.receiver, self.args.receiver_sha256, 128 * 1024)
        self.root = self.args.public_root
        public_directory(self.root)
        require(set(os.listdir(self.root)) == {'owner.json', 'inputs', 'cases'})
        owner = public_bytes(self.root + '/owner.json', self.args.owner_sha256, 4096)
        expected_owner = {'schema': 'linux-public-pipe-receiver-owner-v1',
                          'name': os.path.basename(self.root), 'public_only': True}
        require(self.h.canonical(self.h.strict_json(owner)) == self.h.canonical(expected_owner))
        self.inputs, self.cases = self.root + '/inputs', self.root + '/cases'
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
        self.owned = self.cases + '/public-pipe-case-' + os.urandom(16).hex()
        os.mkdir(self.owned, 0o700)
        public_directory(self.owned)
        self.case_owner = {'schema': 'linux-public-pipe-case-owner-v1', 'public_only': True}
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
        public_bytes(self.receiver, self.args.receiver_sha256, 128 * 1024)
        public_bytes(self.root + '/owner.json', self.args.owner_sha256, 4096)
        public_bytes(self.inputs + '/public.pcm', PCM_SHA, PCM_BYTES, size=PCM_BYTES)
        public_bytes(self.inputs + '/example-input', BINARY_SHA, BINARY_BYTES,
                     mode=0o500, size=BINARY_BYTES)
        self.h.frozen_tool(self.inputs + '/example-input', BINARY_SHA)

    def case_pins(self):
        public_directory(self.owned)
        public_directory(self.cwd)
        require(set(os.listdir(self.owned)) ==
                {'owner.json', 'result.json', 'example', 'public.pcm', 'target-cwd'})
        owner_sha = hashlib.sha256(self.h.canonical(self.case_owner) + b'\n').hexdigest()
        public_bytes(self.owned + '/owner.json', owner_sha, 4096)
        self.check_record()
        public_bytes(self.binary, BINARY_SHA, BINARY_BYTES, mode=0o500, size=BINARY_BYTES)
        self.h.frozen_tool(self.binary, BINARY_SHA)
        public_bytes(self.pcm, PCM_SHA, PCM_BYTES, size=PCM_BYTES)
        require(not os.listdir(self.cwd))

    def start(self):
        require(self.prepared and self.h.empty_children(), 'NATIVE')
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
        parent = os.getpid()
        self.pid = os.fork()
        if self.pid == 0:
            try:
                os.close(gate_out)
                # Fixed False: ALWAYS --public-frame-check-v1. The reviewed function
                # resets ALL catchable signals via restore_target_signals before exec.
                self.h.target_exec(self.binary, self.cwd, read_in, write_out, pcm,
                                   gate_in, parent, False)
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
        self.end = time.monotonic() + 15

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
        self.emit(READY)  # os.write: unbuffered pipe; nothing remains to flush.

    def input_ready(self, end):
        return self.wait(0, end)

    def input_read(self, cap):
        return os.read(0, cap)

    def read_public_frame(self):
        self.guard()
        os.set_blocking(0, False)  # Parent only; native stdin is a DIFFERENT pipe.
        bounded_public_input(self, min(self.end, self.now() + 5))

    def forward_public(self):
        self.guard()
        require(os.write(self.write_in, PUBLIC_FRAME) == len(PUBLIC_FRAME), 'NATIVE')
        self.close_owned(self.write_in)  # Mandatory EOF, not just a flush.

    def native_terminal(self):
        self.read_native_exact(SUCCESS, False)
        eof, info = False, None
        while self.now() < self.end:
            self.final_guard()
            if not eof and self.wait(self.read_out, self.end, False):
                try:
                    chunk = os.read(self.read_out, 1)
                except BlockingIOError:
                    continue
                require(chunk == b'', 'NATIVE')
                eof = True
            info = self.h.ownership(self.pidfd)
            if eof and info is not None:
                break
            # Bounded waiting even after EOF, without busy spinning on a pipe.
            select.select([self.pidfd] if info is None else [], [], [], 0.01)
        require(eof and info is not None and info.si_code == os.CLD_EXITED
                and info.si_status == 0, 'NATIVE')
        require(not os.listdir(self.cwd), 'NATIVE')
        self.h.policy()

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
        require(set(os.listdir(self.root)) == {'owner.json', 'inputs', 'cases'})
        require(set(os.listdir(self.inputs)) ==
                {'source-manifest.json', 'source', 'public.pcm', 'example-input'})
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

    def emit_success(self):
        self.emit(SUCCESS)

    def emit_error(self, code):
        require(code in CODES, 'INTERNAL')
        self.emit(b'REAL_ADMISSION_ERROR_V1 ' + code.encode('ascii') + b'\n')
        with contextlib.suppress(BaseException):
            os.write(2, b'PUBLIC_PIPE_RECEIVER_FAILED_V1\n')


def main(argv=None):
    port = None
    try:
        args = arguments(list(sys.argv[1:] if argv is None else argv))
        port = LinuxPort(args)
        return receive_once(port)
    except BaseException:
        # Parser/bootstrap failures must not invoke traceback, repr or argparse.
        with contextlib.suppress(BaseException):
            os.set_blocking(1, False)
            os.write(1, b'REAL_ADMISSION_ERROR_V1 STARTUP\n')
        with contextlib.suppress(BaseException):
            os.write(2, b'PUBLIC_PIPE_RECEIVER_FAILED_V1\n')
        return 1


if __name__ == '__main__':
    sys.exit(main())
