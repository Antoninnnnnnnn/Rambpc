"""SOURCE ONLY. Linux x86_64; Python -I; public cleared environment only.
--unit is pure: no processes, crashes, policy reads or native calls.
--observe ABS_BINARY ABS_RUST_SOURCE SOURCE_SHA256 BINARY_SHA256 DRIVER_SHA256
Main must independently pin all three source/binary artifacts before use.
Creates an exclusive 0700 directory under the caller's public working directory.
No core bytes are opened/read; no credential receiver, network or live mode.
"""
import contextlib
import ctypes
import errno
import hashlib
import json
import os
import re
import resource
import select
import signal
import stat
import sys
import tempfile
import time

CAP = 16 * 1024 * 1024
PUBLIC = {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}
READY = b"G1_PUBLIC_READY_V1\n"
PATTERN_SHA = "0d45f5fd462b8c70bffb10021ac1bcff3f58f29b1faf7568595095427d42812c"
REASONS = ("arguments", "unit", "policy", "pins", "launch", "ready", "observer",
           "positive", "protected", "cleanup", "cancelled", "internal")
RESULT = {k: False for k in ("qualified", "positive_core", "positive_core_bit",
          "protected_no_core", "protected_no_core_bit", "environ_denied",
          "cleanup", "unit_ok")}
RESULT.update({"launched": 0, "reaped": 0, "abort_statuses": 0, "unit_checks": 0})
RESULT.update({"blocked_" + k: False for k in REASONS})
CANCELLED = False
STAGE = "arguments"
LIBC = None

class Block(Exception):
    pass

def require(value):
    if not value:
        raise Block()

def cancellation():
    if CANCELLED:
        RESULT["blocked_cancelled"] = True
        raise Block()

def cancel(_sig, _frame):
    global CANCELLED
    CANCELLED = True

def bounded(path, cap=65536):
    with open(path, "rb") as f:
        data = f.read(cap + 1)
    require(len(data) <= cap)
    return data

def fields(raw):
    values = {}
    for line in raw.decode("ascii").splitlines():
        key, sep, value = line.partition(":")
        if sep:
            require(key not in values)
            values[key] = value.strip()
    return values

def status_ok(raw, uid, gid):
    v = fields(raw)
    return (uid > 0 and gid > 0 and v.get("Uid", "").split() == [str(uid)] * 4
            and v.get("Gid", "").split() == [str(gid)] * 4
            and v.get("TracerPid") == "0"
            and v.get("SigBlk") == "0000000000000000"
            and all(v.get(k) == "0000000000000000"
                    for k in ("CapInh", "CapPrm", "CapEff", "CapAmb")))

def policy_ok(parts):
    return (parts == [b"core\n", b"1\n", b"0\n", b"0\n"]
            and hashlib.sha256(parts[0][:-1]).hexdigest() == PATTERN_SHA)

def policy():
    require(os.uname().sysname == "Linux" and os.uname().machine == "x86_64")
    require(os.getuid() == os.geteuid() and os.getgid() == os.getegid())
    require(status_ok(bounded("/proc/self/status"), os.getuid(), os.getgid()))
    require(policy_ok([bounded(p, 128) for p in (
        "/proc/sys/kernel/core_pattern", "/proc/sys/kernel/core_uses_pid",
        "/proc/sys/fs/suid_dumpable", "/proc/sys/kernel/yama/ptrace_scope")]))
    require(resource.getrlimit(resource.RLIMIT_CORE) == (0, resource.RLIM_INFINITY))
    require(dict(os.environ) == PUBLIC)

def pin(path, digest, cap, binary=False):
    require(os.path.isabs(path) and re.fullmatch("[0-9a-f]{64}", digest))
    s = os.lstat(path)
    require(stat.S_ISREG(s.st_mode) and s.st_uid == os.getuid()
            and s.st_nlink == 1 and not s.st_mode & 0o6022 and 0 < s.st_size <= cap)
    data = bounded(path, cap)
    require(hashlib.sha256(data).hexdigest() == digest)
    if binary:
        require(s.st_mode & stat.S_IXUSR and data[:6] == b"\x7fELF\x02\x01"
                and data[16:18] in (b"\x02\x00", b"\x03\x00") and data[18:20] == b"\x3e\x00")
        try:
            os.getxattr(path, "security.capability", follow_symlinks=False)
        except OSError as e:
            require(e.errno == errno.ENODATA)
        else:
            raise Block()
    return data

def prctl(op, value):
    require(LIBC.prctl(ctypes.c_int(op), ctypes.c_ulong(value), ctypes.c_ulong(0),
                       ctypes.c_ulong(0), ctypes.c_ulong(0)) == 0)

class Owned:
    def __init__(self):
        self.pid = None
        self.fd = None
        self.out = None
        self.raw = None

    def start(self, binary, cwd, positive):
        r, w = os.pipe2(os.O_CLOEXEC)
        a = b = null = None
        try:
            a, b = os.pipe2(os.O_CLOEXEC)
            null = os.open("/dev/null", os.O_RDWR | os.O_CLOEXEC)
            parent = os.getpid()
            pid = os.fork()  # One child, later replaced by the exact pinned target.
            if pid == 0:
                try:
                    signal.signal(signal.SIGALRM, signal.SIG_DFL)
                    signal.alarm(3)  # Gate/loader watchdog; no core-producing signal.
                    prctl(1, signal.SIGKILL)
                    require(os.getppid() == parent)
                    os.close(b)
                    require(os.read(a, 1) == b"G")
                    os.chdir(cwd)
                    for old, new in ((null, 0), (w, 1), (null, 2)):
                        os.dup2(old, new)
                    for name in os.listdir("/proc/self/fd"):
                        fd = int(name)
                        if fd > 2:
                            with contextlib.suppress(OSError):
                                os.close(fd)
                    mode = "--public-positive-v1" if positive else "--public-protected-v1"
                    os.execve(binary, [binary, mode], PUBLIC)
                except BaseException:
                    os._exit(1)
            self.pid, self.out = pid, r
            RESULT["launched"] += 1
            r = None
            # Child remains behind the gate until pidfd and direct-child ownership
            # exist. On failure/parent death, EOF or its 3s alarm ends the launcher.
            self.fd = os.pidfd_open(pid, 0)
            self.ownership()
            cancellation()
            require(os.write(b, b"G") == 1)
            os.set_blocking(self.out, False)
        finally:
            for fd in (r, w, a, b, null):
                if fd is not None:
                    os.close(fd)

    def ownership(self):
        require(self.fd is not None and self.raw is None)
        return os.waitid(os.P_PIDFD, self.fd, os.WEXITED | os.WNOHANG | os.WNOWAIT)

    def send(self, sig):
        observed = self.ownership()  # ECHILD is fatal; never signal a numeric PID.
        if sig == signal.SIGABRT:
            require(observed is None)  # Do not credit a previously terminated child.
        signal.pidfd_send_signal(self.fd, sig, None, 0)

    def reap(self, seconds):
        end = time.monotonic() + seconds
        while self.raw is None and time.monotonic() < end:
            if self.fd is not None:
                self.ownership()
            pid, raw, _ = os.wait4(self.pid, os.WNOHANG)
            if pid:
                require(pid == self.pid)
                self.raw = raw  # Keep WCOREDUMP: no subprocess wait/poll conversion.
                RESULT["reaped"] += 1
                break
            select.select([], [], [], 0.025)
        require(self.raw is not None)

    def ready(self):
        data, end = b"", time.monotonic() + 4
        while data != READY and time.monotonic() < end:
            cancellation()
            if select.select([self.out], [], [], 0.025)[0]:
                chunk = os.read(self.out, len(READY) + 1 - len(data))
                require(chunk)
                data += chunk
                require(READY.startswith(data))
        require(data == READY)

    def close(self):
        for fd in (self.out, self.fd):
            if fd is not None:
                os.close(fd)
        self.out = self.fd = None

def finish_child(child):
    try:
        if child.pid is not None and child.raw is None:
            if child.fd is not None:
                try:
                    child.send(signal.SIGKILL)
                except ProcessLookupError:
                    pass  # An owned zombie still must be reaped below.
            child.reap(5)  # Gate fallback also expires without a pidfd.
    finally:
        child.close()

@contextlib.contextmanager
def child_guard(child):
    try:
        yield child
    finally:
        finish_child(child)

def limits_ok(raw, positive):
    limits = raw.decode("ascii").splitlines()
    for prefix, expected in (("Max core file size", CAP if positive else 0),
                             ("Max file size", CAP)):
        lines = [line.split() for line in limits if line.startswith(prefix + " ")]
        if len(lines) != 1 or lines[0][-3:] != [str(expected), str(expected), "bytes"]:
            return False
    return True

def observe(child, positive):
    raw = bounded(f"/proc/{child.pid}/status")
    require(status_ok(raw, os.getuid(), os.getgid()))
    require(fields(raw).get("PPid") == str(os.getpid()))
    require(limits_ok(bounded(f"/proc/{child.pid}/limits"), positive))
    path = f"/proc/{child.pid}/environ"
    require(os.stat(path).st_uid == (os.getuid() if positive else 0))
    try:
        fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    except OSError as e:
        require(not positive and e.errno == errno.EACCES)
        RESULT["environ_denied"] = True
    else:
        os.close(fd)  # NEVER read even one byte, including the positive control.
        require(positive)

def core_name(name, pid):
    return type(pid) is int and pid > 0 and name == "core." + str(pid)

def case_ok(positive, raw, size):
    return (os.WIFSIGNALED(raw) and os.WTERMSIG(raw) == signal.SIGABRT
            and bool(os.WCOREDUMP(raw)) == positive
            and ((size is not None and 0 < size <= CAP) if positive else size is None))

def unit():
    global CANCELLED
    checks = 0
    def check(value):
        nonlocal checks
        require(value)
        checks += 1
    fake_status = (b"Uid: 7 7 7 7\nGid: 8 8 8 8\nTracerPid: 0\nSigBlk: 0000000000000000\n" +
                   b"CapInh: 0000000000000000\nCapPrm: 0000000000000000\n" +
                   b"CapEff: 0000000000000000\nCapAmb: 0000000000000000\n")
    check(status_ok(fake_status, 7, 8))
    check(not status_ok(fake_status.replace(b"TracerPid: 0", b"TracerPid: 1"), 7, 8))
    check(not status_ok(fake_status.replace(b"SigBlk: 0000000000000000", b"SigBlk: 0000000000000001"), 7, 8))
    check(not status_ok(fake_status.replace(b"CapEff: 0000000000000000", b"CapEff: 0000000000000001"), 7, 8))
    try:
        fields(b"TracerPid: 0\nTracerPid: 0\n")
    except Block:
        check(True)
    else:
        raise Block()
    check(policy_ok([b"core\n", b"1\n", b"0\n", b"0\n"]))
    for bad in (b"|helper\n", b"/core\n", b"core.%p\n", b"core", b"core\n\n"):
        check(not policy_ok([bad, b"1\n", b"0\n", b"0\n"]))
    check(not policy_ok([b"core\n", b"0\n", b"0\n", b"0\n"]))
    check(not policy_ok([b"core\n", b"1\n", b"1\n", b"0\n"]))
    check(not policy_ok([b"core\n", b"1\n", b"0\n", b"1\n"]))
    fake_limits = f"Max core file size 0 0 bytes\nMax file size {CAP} {CAP} bytes\n".encode()
    check(limits_ok(fake_limits, False) and not limits_ok(fake_limits, True))
    check(not limits_ok(fake_limits.replace(b"0 0 bytes", b"0 unlimited bytes"), False))
    check(limits_ok(fake_limits.replace(b"0 0 bytes", f"{CAP} {CAP} bytes".encode()), True))
    check(core_name("core.7", 7))
    check(all(not core_name(n, 7) for n in ("core", "core.07", "../core.7", "core.8")))
    check(case_ok(True, 6 | 128, 1) and case_ok(False, 6, None))
    check(not case_ok(True, 6, 1) and not case_ok(True, 6 | 128, 0))
    check(not case_ok(False, 6 | 128, None) and not case_ok(False, 6, 0))
    class Fake:
        pid, fd, raw = 7, 9, None
        log = []
        def send(self, sig): self.log.append(sig)
        def reap(self, seconds): self.log.append("reap"); self.raw = 9
        def close(self): self.log.append("close")
    fake = Fake()
    CANCELLED = True
    try:
        with child_guard(fake):
            cancellation()
    except Block:
        pass
    finally:
        CANCELLED = False
        RESULT["blocked_cancelled"] = False
    check(fake.log == [signal.SIGKILL, "reap", "close"])
    gate = Fake()
    gate.fd, gate.log = None, []
    finish_child(gate)
    check(gate.log == ["reap", "close"])
    done = Fake()
    done.raw, done.log = 9, []
    finish_child(done)
    check(done.log == ["close"])
    RESULT.update(unit_ok=True, unit_checks=checks)

def main():
    global STAGE, LIBC
    require(sys.flags.isolated == 1)
    if sys.argv[1:] == ["--unit"]:
        STAGE = "unit"
        unit()
        return
    require(len(sys.argv) == 7 and sys.argv[1] == "--observe")
    binary, source, source_sha, binary_sha, driver_sha = sys.argv[2:]
    STAGE = "policy"
    policy()
    for fd in range(3):
        os.fstat(fd)
    require(len(os.listdir("/proc/self/task")) == 1)
    require(all(hasattr(os, k) for k in ("pidfd_open", "P_PIDFD", "WNOWAIT", "WCOREDUMP")))
    require(hasattr(signal, "pidfd_send_signal"))
    require(signal.getsignal(signal.SIGCHLD) == signal.SIG_DFL)
    for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
        signal.signal(sig, cancel)
    LIBC = ctypes.CDLL(None, use_errno=True)
    LIBC.prctl.restype = ctypes.c_int
    parent = os.getppid()
    require(parent > 1)
    prctl(4, 0)  # Driver never holds a credential; no driver core is useful.
    require(LIBC.prctl(ctypes.c_int(3), ctypes.c_ulong(0), ctypes.c_ulong(0),
                       ctypes.c_ulong(0), ctypes.c_ulong(0)) == 0)
    prctl(1, signal.SIGTERM)
    require(os.getppid() == parent)
    signal.alarm(35)
    fd = os.pidfd_open(os.getpid(), 0)
    os.close(fd)
    STAGE = "pins"
    pin(source, source_sha, 65536)
    pin(os.path.abspath(__file__), driver_sha, 65536)
    image = pin(binary, binary_sha, 8 * 1024 * 1024, True)
    root = target = None
    dirs, cores = [], []
    os.umask(0o077)
    try:
        root = os.path.abspath(tempfile.mkdtemp(prefix="n4_g1_public_20261116.", dir="."))
        os.chmod(root, 0o700)
        target = os.path.join(root, "target")
        with open(target, "xb") as f:
            f.write(image)
        os.chmod(target, 0o500)
        for positive in (True, False):
            cancellation()
            STAGE = "policy"
            policy()  # Before EACH launch, not a historical probe.
            STAGE = "pins"
            pin(source, source_sha, 65536)
            pin(os.path.abspath(__file__), driver_sha, 65536)
            pin(target, binary_sha, 8 * 1024 * 1024, True)
            cwd = os.path.join(root, "positive" if positive else "protected")
            os.mkdir(cwd, 0o700)
            dirs.append(cwd)
            require(not os.listdir(cwd))
            with child_guard(Owned()) as child:
                STAGE = "launch"
                child.start(target, cwd, positive)
                cores.append(os.path.join(cwd, "core." + str(child.pid)))
                STAGE = "ready"
                child.ready()
                STAGE = "observer"
                observe(child, positive)
                STAGE = "policy"
                policy()  # Recheck immediately before owned SIGABRT.
                cancellation()
                child.send(signal.SIGABRT)
                child.reap(5)
                policy()  # Detect a policy change during the observed crash too.
                STAGE = "positive" if positive else "protected"
                require(os.read(child.out, len(READY) + 1) == b"")
                names = os.listdir(cwd)
                require(all(core_name(n, child.pid) for n in names) and len(names) <= 1)
                size = None
                if names:
                    s = os.lstat(cores[-1])  # Metadata ONLY, never open a core.
                    require(stat.S_ISREG(s.st_mode) and s.st_uid == os.getuid()
                            and s.st_nlink == 1 and not s.st_mode & 0o077)
                    size = s.st_size
                if os.WIFSIGNALED(child.raw) and os.WTERMSIG(child.raw) == signal.SIGABRT:
                    RESULT["abort_statuses"] += 1
                if positive:
                    RESULT.update(positive_core=size is not None and 0 < size <= CAP,
                                  positive_core_bit=bool(os.WCOREDUMP(child.raw)))
                else:
                    RESULT.update(protected_no_core=size is None,
                                  protected_no_core_bit=not bool(os.WCOREDUMP(child.raw)))
                require(case_ok(positive, child.raw, size))
            if names:
                os.unlink(cores[-1])
            cores.pop()
    finally:
        # Only exact names in directories exclusively created above. Unknown
        # entries are NEVER recursively deleted; cleanup then fails closed.
        clean = True
        for path in cores + ([target] if target else []):
            try:
                os.unlink(path)
            except FileNotFoundError:
                pass
            except OSError:
                clean = False
        for path in list(reversed(dirs)) + ([root] if root else []):
            try:
                os.rmdir(path)
            except OSError:
                clean = False
        RESULT["cleanup"] = clean and RESULT["launched"] == RESULT["reaped"]
        if not RESULT["cleanup"]:
            RESULT["blocked_cleanup"] = True
    require(RESULT["cleanup"] and RESULT["launched"] == RESULT["reaped"]
            == RESULT["abort_statuses"] == 2)
    cancellation()
    RESULT["qualified"] = True  # This host/public experiment ONLY; never live GO.

if __name__ == "__main__":
    try:
        main()
    except BaseException:
        RESULT["qualified"] = False
        RESULT["blocked_" + (STAGE if STAGE in REASONS else "internal")] = True
    if CANCELLED:
        RESULT["qualified"] = False
        RESULT["blocked_cancelled"] = True
    try:
        sys.stdout.write(json.dumps(RESULT, sort_keys=True, separators=(",", ":")) + "\n")
        sys.stdout.flush()
    except BaseException:
        os._exit(1)
    sys.exit(0 if RESULT["qualified"] or RESULT["unit_ok"] else 1)
