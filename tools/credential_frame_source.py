"""SOURCE_UNQUALIFIED: inert, bounded source; FIRST_CONTENT_READ_ONLY_WHEN_CALLED.

No caller or READY enforcement lives here. Private seams are for invented fake QA.
"""

import io
import ntpath
import os
import posixpath
import stat
import threading

NAMES = (b'RAMBLER_API_KEY', b'RAMBLER_ANDROID_PACKAGE', b'RAMBLER_ANDROID_CERT')
MAGIC = b'RLA1\0\r\n\x1a'
FIELD_CAP = 4096
MAX_FILE = 65536
MAX_FRAME = 12302
PUBLIC_FRAME = (MAGIC + b'\0\x19invented-public-frame-key'
                + b'\0\x14invalid.public.frame'
                + b'\0\x1ainvented-public-frame-cert')
_allocate = bytearray  # Private mutable-buffer observation seam for fake QA.


class SourceClosed(Exception):
    """Fixed, empty operational failure; never carries a source exception."""

    def __init__(self):
        super().__init__()


class _Invalid(Exception):
    pass


class SourceState:
    """Read-only observations; length is private-workflow, secret-derived data."""

    __slots__ = ('_lock', '_called', '_read_attempted', '_length')

    def __init__(self):
        self._lock = threading.Lock()
        self._called = False
        self._read_attempted = False
        self._length = None

    @property
    def called(self):
        with self._lock:
            return self._called

    @property
    def read_attempted(self):
        with self._lock:
            return self._read_attempted

    @property
    def length(self):
        with self._lock:
            return self._length

    def __repr__(self):
        return '<SourceState>'


def _open(path):
    flags = os.O_RDONLY
    for name in ('O_BINARY', 'O_NOINHERIT', 'O_CLOEXEC', 'O_NOFOLLOW',
                 'O_NONBLOCK', 'O_NOCTTY'):
        flags |= getattr(os, name, 0)
    fd = os.open(path, flags)
    try:
        return io.FileIO(fd, mode='rb', closefd=True)
    except BaseException:
        os.close(fd)
        raise


class _FileOps:
    windows = os.name == 'nt'

    def lstat(self, path):
        return os.lstat(path)

    def open(self, path):
        return _open(path)

    def fstat(self, handle):
        return os.fstat(handle.fileno())


def _components(path, windows):
    if type(path) is not str or not path or '\0' in path:
        raise _Invalid()
    if windows:
        normalized = path.replace('\\', '/')
        drive, tail = ntpath.splitdrive(normalized)
        if (len(drive) != 2 or drive[1] != ':'
                or drive[0] not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ'
                or not tail.startswith('/')):
            raise _Invalid()
        parts = tail[1:].split('/')
        root = drive + '/'
        devices = {'CON', 'PRN', 'AUX', 'NUL', 'CONIN$', 'CONOUT$'}
        devices.update('COM' + str(i) for i in range(1, 10))
        devices.update('LPT' + str(i) for i in range(1, 10))
        for part in parts:
            if (not part or part in ('.', '..') or part.endswith((' ', '.'))
                    or any(ord(c) < 32 or c in ':<>"|?*' for c in part)
                    or part.split('.')[0].rstrip(' ').upper() in devices
                    or part.split('.')[0].rstrip(' ').upper() in
                    {'COM¹', 'COM²', 'COM³', 'LPT¹', 'LPT²', 'LPT³'}):
                raise _Invalid()
    else:
        if not posixpath.isabs(path) or path.startswith('//'):
            raise _Invalid()
        parts = path[1:].split('/')
        root = '/'
        if any(not part or part in ('.', '..') for part in parts):
            raise _Invalid()
    paths = [root]
    for part in parts:
        paths.append(paths[-1].rstrip('/') + '/' + part)
    return paths


def _snapshot(info, leaf):
    if (stat.S_ISLNK(info.st_mode)
            or getattr(info, 'st_file_attributes', 0) & 0x400):
        raise _Invalid()
    if leaf:
        if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
                or not 1 <= info.st_size <= MAX_FILE):
            raise _Invalid()
    elif not stat.S_ISDIR(info.st_mode):
        raise _Invalid()
    fields = ('st_dev', 'st_ino', 'st_size', 'st_mode', 'st_nlink',
              'st_mtime_ns', 'st_mtime', 'st_ctime_ns', 'st_ctime',
              'st_birthtime_ns', 'st_birthtime')
    return {key: getattr(info, key) for key in fields if hasattr(info, key)}


def _scan(path, ops):
    paths = _components(path, ops.windows)
    return [(part, _snapshot(ops.lstat(part), i == len(paths) - 1))
            for i, part in enumerate(paths)]


def _same_common(left, right):
    # Windows lstat/fstat ctime can differ: compare ctime only within each API.
    return all(left[key] == right[key] for key in left.keys() & right.keys()
               if key not in ('st_ctime', 'st_ctime_ns'))


def probe(path, *, _ops=None):
    """Metadata only; return None, or the fixed SourceClosed error."""
    failed = False
    try:
        _scan(path, _FileOps() if _ops is None else _ops)
    except BaseException:
        failed = True
    if failed:
        raise SourceClosed() from None


def _spans(data, used):
    if used >= 3 and data[0] == 239 and data[1] == 187 and data[2] == 191:
        raise _Invalid()
    for i in range(used):
        if data[i] == 0 or (data[i] == 13 and (i + 1 == used or data[i + 1] != 10)):
            raise _Invalid()
    found = [None, None, None]
    start = 0
    while start < used:
        end = start
        while end < used and data[end] != 10:
            end += 1
        stop = end - 1 if end > start and data[end - 1] == 13 else end
        if stop > start and data[start] != 35:
            # Inspect key syntax only; never slice/copy a private value.
            eq = start
            while eq < stop and data[eq] != 61:
                eq += 1
            key_end = eq
            while key_end > start and data[key_end - 1] in (9, 32):
                key_end -= 1
            key_start = start
            while key_start < key_end and data[key_start] in (9, 32):
                key_start += 1
            if (key_end - key_start >= 7
                    and all(data[key_start + j] == c for j, c in enumerate(b'export'))
                    and data[key_start + 6] in (9, 32)):
                key_start += 7
                while key_start < key_end and data[key_start] in (9, 32):
                    key_start += 1
            index = None
            for n, name in enumerate(NAMES):
                if key_end - key_start == len(name) and all(
                        data[key_start + j] == c for j, c in enumerate(name)):
                    index = n
                    break
            if index is not None:
                name = NAMES[index]
                if (key_start != start or eq != start + len(name)
                        or eq == stop or found[index] is not None):
                    raise _Invalid()
                lo, hi = eq + 1, stop
                if lo < hi and data[lo] in (34, 39):
                    if hi - lo < 2 or data[hi - 1] != data[lo]:
                        raise _Invalid()
                    lo, hi = lo + 1, hi - 1
                if not 1 <= hi - lo <= FIELD_CAP:
                    raise _Invalid()
                for i in range(lo, hi):
                    if not 33 <= data[i] <= 126 or data[i] in (34, 39, 92, 35):
                        raise _Invalid()
                found[index] = (lo, hi)
            else:
                # Unrelated assignments still have a column-0 ASCII identifier key.
                if eq == stop or eq == start:
                    raise _Invalid()
                for i in range(start, eq):
                    c = data[i]
                    if not (65 <= c <= 90 or 97 <= c <= 122 or c == 95
                            or (i > start and 48 <= c <= 57)):
                        raise _Invalid()
        start = end + 1
    if any(span is None for span in found):
        raise _Invalid()
    return found


def _wipe(buffer):
    if buffer is not None:
        try:
            for i in range(len(buffer)):
                buffer[i] = 0
        except BaseException:
            pass  # Best effort only; not secure erasure.


def _consume(path, ops, state):
    source = frame = None
    try:
        before_path = _scan(path, ops)
        handle = ops.open(path)
        try:
            before_handle = _snapshot(ops.fstat(handle), True)
            if not _same_common(before_path[-1][1], before_handle):
                raise _Invalid()
            source = _allocate(MAX_FILE + 1)
            used = 0
            while True:
                view = memoryview(source)[used:]
                try:
                    with state._lock:
                        state._read_attempted = True
                    count = handle.readinto(view)
                finally:
                    view.release()
                if type(count) is not int or count < 0 or count > len(source) - used:
                    raise _Invalid()
                if count == 0:
                    break
                used += count
                if used > MAX_FILE:
                    raise _Invalid()
            if used != before_handle['st_size']:
                raise _Invalid()
            after_handle = _snapshot(ops.fstat(handle), True)
            if before_handle != after_handle:
                raise _Invalid()
        finally:
            handle.close()
        if before_path != _scan(path, ops):
            raise _Invalid()
        spans = _spans(source, used)
        size = len(MAGIC) + sum(2 + hi - lo for lo, hi in spans)
        if not 17 <= size <= MAX_FRAME:
            raise _Invalid()
        frame = _allocate(size)
        frame[:len(MAGIC)] = MAGIC
        cursor = len(MAGIC)
        for lo, hi in spans:
            length = hi - lo
            frame[cursor] = length >> 8
            frame[cursor + 1] = length & 255
            cursor += 2
            for i in range(lo, hi):
                frame[cursor] = source[i]
                cursor += 1
        if frame == PUBLIC_FRAME:
            raise _Invalid()
        result = bytes(frame)  # The sole immutable private frame copy.
        with state._lock:
            state._length = size
        return result
    finally:
        _wipe(source)
        _wipe(frame)


def make_producer(path, *, _ops=None):
    """No I/O or path validation. Return (single-use callable, public state)."""
    state = SourceState()

    def producer():
        failed = False
        claimed = False
        try:
            with state._lock:
                if not state._called:
                    state._called = True
                    claimed = True
            if claimed:
                return _consume(path, _FileOps() if _ops is None else _ops, state)
        except BaseException:
            failed = True
        # Outside the except: from None alone would retain __context__.
        if failed or not claimed:
            raise SourceClosed() from None

    return producer, state
