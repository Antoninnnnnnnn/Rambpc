"""Finite invented-fake QA only; NOT RUN until independent source review.

No native, receiver, channel, actual source input, readiness integration or opt-in.
"""

import ast
import importlib.util
import os
import socket
import stat
import threading
import traceback
import types
import unittest
from unittest import mock


SOURCE_PATH = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                           'tools', 'credential_frame_source.py')
NAMES = (b'RAMBLER_API_KEY', b'RAMBLER_ANDROID_PACKAGE', b'RAMBLER_ANDROID_CERT')
MAGIC = b'RLA1\0\r\n\x1a'
VALUES = (b'fake-alpha', b'fake.beta', b'fake-gamma')
PUBLIC = (b'invented-public-frame-key', b'invalid.public.frame',
          b'invented-public-frame-cert')
PRIVATE_SENTINEL = 'invented-error-sentinel-NOT-A-CREDENTIAL'
PATH = '/invented/source.txt'  # Fake POSIX ops, on every host.


def content(values=VALUES, newline=b'\n'):
    return newline.join(key + b'=' + value for key, value in zip(NAMES, values))


def expected(values=VALUES):
    return MAGIC + b''.join(len(value).to_bytes(2, 'big') + value for value in values)


def metadata(size, *, leaf=True, ino=13, ctime=100):
    return types.SimpleNamespace(
        st_dev=7, st_ino=ino, st_size=size if leaf else 0,
        st_mode=(stat.S_IFREG | 0o600) if leaf else (stat.S_IFDIR | 0o700),
        st_nlink=1 if leaf else 2, st_mtime_ns=20, st_mtime=0.000000020,
        st_ctime_ns=ctime, st_ctime=ctime / 1_000_000_000,
        st_birthtime_ns=10, st_birthtime=0.000000010, st_file_attributes=0)


class FakeHandle:
    def __init__(self, ops):
        self.ops = ops
        self.position = 0
        self.closed = False
        self.reads = 0

    def readinto(self, target):
        self.reads += 1
        self.ops.events.append('read')
        if self.ops.enter_read is not None:
            self.ops.enter_read.set()
            if not self.ops.release_read.wait(3):
                raise RuntimeError('finite fake wait expired')
        if self.ops.read_error:
            raise RuntimeError(PRIVATE_SENTINEL)
        if self.ops.read_result is not None:
            return self.ops.read_result
        count = min(len(target), self.ops.chunk, len(self.ops.data) - self.position)
        target[:count] = self.ops.data[self.position:self.position + count]
        self.position += count
        return count

    def close(self):
        self.ops.events.append('close')
        self.closed = True
        if self.ops.close_error:
            raise RuntimeError(PRIVATE_SENTINEL)


class FakeOps:
    windows = False

    def __init__(self, data=None):
        self.data = content() if data is None else data
        self.size = len(self.data)
        self.chunk = 1_000_000
        self.events = []
        self.visits = {}
        self.fstats = 0
        self.opens = 0
        self.handles = []
        self.read_error = False
        self.close_error = False
        self.open_error = False
        self.read_result = None
        self.on_lstat = None
        self.on_fstat = None
        self.enter_read = None
        self.release_read = None

    def lstat(self, path):
        self.events.append('lstat')
        visit = self.visits.get(path, 0) + 1
        self.visits[path] = visit
        leaf = path == PATH or (self.windows and path == 'C:/invented/source.txt')
        info = metadata(self.size, leaf=leaf, ino=13 if leaf else len(path) + 100)
        if self.on_lstat is not None:
            self.on_lstat(path, visit, info)
        return info

    def open(self, path):
        self.events.append('open')
        self.opens += 1
        if self.open_error:
            raise RuntimeError(PRIVATE_SENTINEL)
        handle = FakeHandle(self)
        self.handles.append(handle)
        return handle

    def fstat(self, handle):
        self.events.append('fstat')
        self.fstats += 1
        # Deliberately different ctime from lstat, stable within fstat.
        info = metadata(self.size, ctime=900)
        if self.on_fstat is not None:
            self.on_fstat(self.fstats, info)
        return info


class CredentialFrameSourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Eventual execution must follow an independent source review.
        spec = importlib.util.spec_from_file_location('credential_frame_source_fake_qa', SOURCE_PATH)
        module = importlib.util.module_from_spec(spec)
        with mock.patch('os.open', side_effect=AssertionError('actual open forbidden')), \
                mock.patch('os.lstat', side_effect=AssertionError('actual lstat forbidden')), \
                mock.patch.object(socket, 'socket', side_effect=AssertionError('network forbidden')):
            spec.loader.exec_module(module)
        cls.source = module

    def setUp(self):
        self.ops = FakeOps()
        self.buffers = []
        self.real_bytearray = bytearray

        def allocate(size):
            value = self.real_bytearray(size)
            self.buffers.append(value)
            return value

        self.patcher = mock.patch.object(self.source, '_allocate', side_effect=allocate)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)

    def producer(self, data=None):
        if data is not None:
            self.ops.data = data
            self.ops.size = len(data)
        return self.source.make_producer(PATH, _ops=self.ops)

    def assert_wiped(self):
        for buffer in self.buffers:
            # Check every allocated byte, without one Python iteration per byte
            # or including any buffer contents in a failure message.
            self.assertEqual(buffer.count(0), len(buffer))

    def assert_closed(self, action):
        error = None
        try:
            action()
        except BaseException as caught:
            error = caught
        self.assertIs(type(error), self.source.SourceClosed)
        self.assertEqual(error.args, ())
        self.assertEqual(str(error), '')
        self.assertIsNone(error.__cause__)
        self.assertIsNone(error.__context__)
        rendered = ''.join(traceback.format_exception(type(error), error, error.__traceback__))
        for text in (repr(error), str(error), rendered):
            self.assertNotIn(PRIVATE_SENTINEL, text)
        return error

    def reject(self, raw):
        producer, state = self.producer(raw)
        self.assert_closed(producer)
        self.assertTrue(state.called)
        self.assertIsNone(state.length)
        self.assert_wiped()

    def test_import_and_constructor_are_inert(self):
        # setUpClass imports with actual OS input APIs forbidden.
        producer, state = self.producer()
        self.assertEqual(self.ops.events, [])
        self.assertFalse(state.called)
        self.assertFalse(state.read_attempted)
        self.assertIsNone(state.length)
        self.assertEqual(self.buffers, [])
        self.assertTrue(callable(producer))

    def test_constructor_does_not_even_validate_path(self):
        producer, state = self.source.make_producer(None, _ops=self.ops)
        self.assertEqual(self.ops.events, [])
        self.assert_closed(producer)
        self.assertTrue(state.called)
        self.assertFalse(state.read_attempted)

    def test_probe_is_metadata_only(self):
        self.assertIsNone(self.source.probe(PATH, _ops=self.ops))
        self.assertEqual(self.ops.events, ['lstat'] * 3)
        self.assertEqual(self.ops.opens, 0)
        self.assertEqual(self.buffers, [])

    def test_probe_does_not_reserve_or_supply_producer_metadata(self):
        self.source.probe(PATH, _ops=self.ops)
        self.ops.size = 0
        producer, state = self.producer()
        self.assert_closed(producer)
        self.assertFalse(state.read_attempted)
        self.assertEqual(self.ops.opens, 0)

    def test_public_constants_independent_crosscheck(self):
        self.assertEqual(self.source.NAMES, NAMES)
        self.assertEqual(self.source.MAGIC, MAGIC)
        self.assertEqual(self.source.PUBLIC_FRAME, expected(PUBLIC))
        self.assertEqual(len(self.source.PUBLIC_FRAME), 85)
        self.assertEqual((self.source.FIELD_CAP, self.source.MAX_FILE, self.source.MAX_FRAME),
                         (4096, 65536, 12302))

    def test_lf_exact_frame_and_state(self):
        producer, state = self.producer(content() + b'\n')
        frame = producer()
        self.assertIs(type(frame), bytes)
        self.assertEqual(frame, expected())
        self.assertTrue(state.called)
        self.assertTrue(state.read_attempted)
        self.assertEqual(state.length, len(frame))
        self.assertEqual(repr(state), '<SourceState>')
        self.assertNotIn(str(state.length), repr(state))
        self.assert_wiped()

    def test_crlf_comments_blank_unrelated_and_reordered(self):
        raw = (b'# invented comment\r\n\r\nOTHER=ignored with spaces\r\n'
               + b'\r\n'.join(key + b'=' + value for key, value in reversed(list(zip(NAMES, VALUES))))
               + b'\r\n')
        producer, _ = self.producer(raw)
        self.assertEqual(producer(), expected())

    def test_matching_single_and_double_quotes(self):
        raw = content((b'"' + VALUES[0] + b'"', b"'" + VALUES[1] + b"'", VALUES[2]))
        producer, _ = self.producer(raw)
        self.assertEqual(producer(), expected())

    def test_minimum_frame_17(self):
        producer, state = self.producer(content((b'a', b'b', b'c')))
        self.assertEqual(producer(), expected((b'a', b'b', b'c')))
        self.assertEqual(state.length, 17)
        self.assert_wiped()

    def test_maximum_frame_12302(self):
        values = (b'a' * 4096, b'b' * 4096, b'c' * 4096)
        producer, state = self.producer(content(values))
        self.assertEqual(producer(), expected(values))
        self.assertEqual(state.length, 12302)
        self.assert_wiped()

    def test_file_size_exact_cap(self):
        base = content() + b'\n#'
        raw = base + b'x' * (65536 - len(base))
        producer, _ = self.producer(raw)
        self.assertEqual(producer(), expected())
        self.assertEqual(self.ops.handles[0].reads, 2)  # Explicit EOF read.
        self.assert_wiped()

    def test_missing_each_field(self):
        for index in range(3):
            with self.subTest(index=index):
                self.reject(b'\n'.join(key + b'=x' for i, key in enumerate(NAMES) if i != index))

    def test_duplicate_each_field(self):
        for key in NAMES:
            with self.subTest(key=key):
                self.reject(content() + b'\n' + key + b'=x')

    def test_empty_quoted_and_unquoted(self):
        for value in (b'', b'""', b"''"):
            with self.subTest(value=value):
                self.reject(content((value, b'b', b'c')))

    def test_overlong_values_quoted_and_unquoted(self):
        for value in (b'x' * 4097, b'"' + b'x' * 4097 + b'"'):
            with self.subTest(size=len(value)):
                self.reject(content((value, b'b', b'c')))

    def test_malformed_designated_key_spellings(self):
        for prefix, suffix in ((b'export ', b''), (b'export\t', b''), (b' ', b''),
                               (b'\t', b''), (b'', b' '), (b'', b'\t')):
            with self.subTest(prefix=prefix, suffix=suffix):
                # Include otherwise-valid fields: malformed extras must not be ignored.
                self.reject(content() + b'\n' + prefix + NAMES[0] + suffix + b'=x')

    def test_designated_missing_equals(self):
        self.reject(content() + b'\n' + NAMES[0])

    def test_value_spaces_are_not_trimmed(self):
        for value in (b' x', b'x ', b' x ', b'" x"', b'"x "'):
            with self.subTest(value=value):
                self.reject(content((value, b'b', b'c')))

    def test_bad_quotes_and_inner_quotes(self):
        for value in (b'"x', b"'x", b'"x\'', b'x"', b"x'", b'"a\'b"', b"'a\"b'"):
            with self.subTest(value=value):
                self.reject(content((value, b'b', b'c')))

    def test_disallowed_value_bytes_finite(self):
        invalid = list(range(33)) + [34, 35, 39, 92, 127] + list(range(128, 256))
        for number in invalid:
            with self.subTest(number=number):
                self.reject(content((b'a' + bytes([number]) + b'z', b'b', b'c')))

    def test_all_allowed_ascii_bytes(self):
        value = bytes(number for number in range(33, 127) if number not in (34, 35, 39, 92))
        producer, _ = self.producer(content((value, b'b', b'c')))
        self.assertEqual(producer(), expected((value, b'b', b'c')))

    def test_no_expansion_or_unescaping(self):
        values = (b'${FAKE}', b'$(FAKE)', b'%FAKE%')
        producer, _ = self.producer(content(values))
        self.assertEqual(producer(), expected(values))
        self.reject(content((b'a\\nb', b'b', b'c')))

    def test_bom_nul_and_lone_cr_anywhere(self):
        for raw in (b'\xef\xbb\xbf' + content(), b'#\0comment\n' + content(),
                    b'#bad\rcomment\n' + content(), content() + b'\r'):
            with self.subTest(length=len(raw)):
                self.reject(raw)

    def test_public_tuple_rejected_and_frame_wiped(self):
        self.reject(content(PUBLIC))
        self.assertTrue(any(len(buffer) == 85 for buffer in self.buffers))

    def test_unrelated_bare_line_rejected(self):
        self.reject(content() + b'\nnot-an-assignment')

    def test_path_lexical_rejections_before_metadata(self):
        for path in ('relative', '', '/', '//server/share', '/invented/../source',
                     '/invented/./source', '/invented//source', '/invented/source/',
                     '/invented/\0source', None):
            with self.subTest(path=path):
                self.ops.events.clear()
                self.assert_closed(lambda: self.source.probe(path, _ops=self.ops))
                self.assertEqual(self.ops.events, [])

    def test_windows_device_unc_ads_and_reserved_rejections(self):
        self.ops.windows = True
        for path in ('C:relative', '\\relative', '//host/share/source',
                     '\\\\?\\C:\\source', '\\\\.\\NUL', 'C:/invented/source:stream',
                     'C:/NUL', 'C:/con.txt', 'C:/COM1', 'C:/COM¹', 'C:/CON .txt',
                     'C:/bad./source', 'C:/bad /source', 'C:/a/../source',
                     'C:/a//source', 'C:/source?', 'C:/source*', 'C:/source\x01'):
            with self.subTest(path=path):
                self.ops.events.clear()
                self.assert_closed(lambda: self.source.probe(path, _ops=self.ops))
                self.assertEqual(self.ops.events, [])

    def test_windows_absolute_drive_path_with_fake_ops(self):
        self.ops.windows = True
        producer, _ = self.source.make_producer('C:\\invented\\source.txt', _ops=self.ops)
        self.assertEqual(producer(), expected())

    def test_symlink_and_reparse_each_component(self):
        for target in ('/', '/invented', PATH):
            for reparse in (False, True):
                with self.subTest(target=target, reparse=reparse):
                    def change(path, visit, info):
                        if path == target:
                            if reparse:
                                info.st_file_attributes = 0x400
                            else:
                                info.st_mode = stat.S_IFLNK | 0o777
                    self.ops.on_lstat = change
                    producer, _ = self.producer()
                    self.assert_closed(producer)
        self.assertEqual(self.ops.opens, 0)

    def test_leaf_regular_link_and_metadata_size_gate(self):
        for field, value in (('st_mode', stat.S_IFDIR), ('st_mode', stat.S_IFIFO),
                             ('st_nlink', 2), ('st_nlink', 0), ('st_size', 0),
                             ('st_size', 65537)):
            with self.subTest(field=field, value=value):
                def change(path, visit, info):
                    if path == PATH:
                        setattr(info, field, value)
                self.ops.on_lstat = change
                producer, _ = self.producer()
                self.assert_closed(producer)
        self.assertEqual(self.ops.opens, 0)

    def test_parent_must_be_directory(self):
        def change(path, visit, info):
            if path == '/invented':
                info.st_mode = stat.S_IFREG | 0o600
        self.ops.on_lstat = change
        producer, _ = self.producer()
        self.assert_closed(producer)
        self.assertEqual(self.ops.opens, 0)

    def test_lstat_fstat_common_fields_mismatch(self):
        for field in ('st_dev', 'st_ino', 'st_size', 'st_mtime_ns', 'st_mtime',
                      'st_mode', 'st_nlink', 'st_birthtime_ns', 'st_birthtime'):
            with self.subTest(field=field):
                def change(visit, info):
                    setattr(info, field, getattr(info, field) + 1)
                self.ops.on_fstat = change
                producer, state = self.producer()
                self.assert_closed(producer)
                self.assertFalse(state.read_attempted)
                self.assertTrue(self.ops.handles[-1].closed)

    def test_handle_changes_after_read_including_ctime(self):
        for field in ('st_dev', 'st_ino', 'st_size', 'st_mtime_ns', 'st_mode',
                      'st_nlink', 'st_birthtime_ns', 'st_ctime_ns', 'st_ctime'):
            with self.subTest(field=field):
                self.ops.fstats = 0
                def change(visit, info):
                    if visit == 2:
                        setattr(info, field, getattr(info, field) + 1)
                self.ops.on_fstat = change
                producer, _ = self.producer()
                self.assert_closed(producer)
                self.assert_wiped()

    def test_named_leaf_changes_after_read_including_ctime(self):
        for field in ('st_dev', 'st_ino', 'st_size', 'st_mtime_ns', 'st_mode',
                      'st_nlink', 'st_birthtime_ns', 'st_ctime_ns', 'st_ctime'):
            with self.subTest(field=field):
                self.ops.visits.clear()
                def change(path, visit, info):
                    if path == PATH and visit == 2:
                        setattr(info, field, getattr(info, field) + 1)
                self.ops.on_lstat = change
                producer, _ = self.producer()
                self.assert_closed(producer)
                self.assert_wiped()

    def test_parent_changes_and_postread_reparse(self):
        for target in ('/', '/invented', PATH):
            for field in ('st_ino', 'st_ctime_ns', 'st_file_attributes'):
                with self.subTest(target=target, field=field):
                    self.ops.visits.clear()
                    def change(path, visit, info):
                        if path == target and visit == 2:
                            setattr(info, field, 0x400 if field == 'st_file_attributes' else 999)
                    self.ops.on_lstat = change
                    producer, _ = self.producer()
                    self.assert_closed(producer)
                    self.assert_wiped()

    def test_cross_api_ctime_difference_is_accepted(self):
        producer, _ = self.producer()
        self.assertEqual(producer(), expected())
        self.assertEqual(self.ops.fstats, 2)

    def test_optional_birthtime_absent_on_one_api(self):
        def change(visit, info):
            del info.st_birthtime
            del info.st_birthtime_ns
        self.ops.on_fstat = change
        producer, _ = self.producer()
        self.assertEqual(producer(), expected())

    def test_partial_reads_and_explicit_eof(self):
        self.ops.chunk = 1
        producer, _ = self.producer()
        self.assertEqual(producer(), expected())
        self.assertEqual(self.ops.handles[0].reads, len(self.ops.data) + 1)
        self.assert_wiped()

    def test_short_read_size_mismatch(self):
        self.ops.size += 1
        producer, _ = self.producer()
        self.assert_closed(producer)
        self.assert_wiped()

    def test_growth_oversize_bounded_at_max_plus_one(self):
        self.ops.data = b'x' * 65537
        self.ops.size = 65536
        producer, _ = self.producer()
        self.assert_closed(producer)
        self.assertEqual(self.ops.handles[0].position, 65537)
        self.assertEqual(self.ops.handles[0].reads, 1)
        self.assertEqual(len(self.buffers[-1]), 65537)
        self.assert_wiped()

    def test_invalid_readinto_results(self):
        for result in (-1, 65538, True, 'invented', 1.5):
            with self.subTest(result=result):
                self.ops.read_result = result
                producer, _ = self.producer()
                self.assert_closed(producer)
                self.assert_wiped()

    def test_none_readinto_result(self):
        class NoneHandle(FakeHandle):
            def readinto(self, target):
                return None
        def fake_open(path):
            handle = NoneHandle(self.ops)
            self.ops.handles.append(handle)
            return handle
        with mock.patch.object(self.ops, 'open', side_effect=fake_open):
            producer, _ = self.producer()
            self.assert_closed(producer)
        self.assert_wiped()

    def test_open_failure_privacy_and_consumption(self):
        self.ops.open_error = True
        producer, state = self.producer()
        self.assert_closed(producer)
        self.assertFalse(state.read_attempted)
        self.assert_closed(producer)
        self.assertEqual(self.ops.opens, 1)
        self.assertIsNone(state.length)

    def test_read_failure_close_and_wipe(self):
        self.ops.read_error = True
        producer, state = self.producer()
        self.assert_closed(producer)
        self.assertTrue(state.read_attempted)
        self.assertTrue(self.ops.handles[0].closed)
        self.assert_wiped()

    def test_close_failure_after_successful_read(self):
        self.ops.close_error = True
        producer, state = self.producer()
        self.assert_closed(producer)
        self.assertIsNone(state.length)
        self.assert_wiped()

    def test_read_and_close_failure_no_retained_context(self):
        self.ops.read_error = self.ops.close_error = True
        producer, state = self.producer()
        error = self.assert_closed(producer)
        self.assertIsNone(error.__context__)
        self.assertEqual(repr(state), '<SourceState>')
        self.assert_wiped()

    def test_parse_failure_wipes_populated_read_buffer(self):
        self.reject(content((b'invented\\bad', b'b', b'c')))
        self.assertEqual(len(self.buffers[-1]), 65537)

    def test_frame_allocation_failure_wipes_source(self):
        original = self.source._allocate
        def allocate(size):
            if size != 65537:
                raise RuntimeError(PRIVATE_SENTINEL)
            return original(size)
        with mock.patch.object(self.source, '_allocate', side_effect=allocate):
            producer, _ = self.producer()
            self.assert_closed(producer)
        self.assert_wiped()

    def test_single_use_success_cannot_reopen(self):
        producer, state = self.producer()
        self.assertEqual(producer(), expected())
        events = list(self.ops.events)
        self.assert_closed(producer)
        self.assertEqual(self.ops.events, events)
        self.assertEqual(self.ops.opens, 1)
        self.assertEqual(state.length, len(expected()))

    def test_single_use_failed_parser_cannot_reopen(self):
        producer, _ = self.producer(b'wrong')
        self.assert_closed(producer)
        events = list(self.ops.events)
        self.assert_closed(producer)
        self.assertEqual(self.ops.events, events)
        self.assertEqual(self.ops.opens, 1)

    def test_concurrent_call_while_read_blocked(self):
        self.ops.enter_read = threading.Event()
        self.ops.release_read = threading.Event()
        producer, state = self.producer()
        results = []
        errors = []
        def first():
            try:
                results.append(producer())
            except BaseException as error:
                errors.append(type(error))
        worker = threading.Thread(target=first, daemon=True)
        worker.start()
        try:
            self.assertTrue(self.ops.enter_read.wait(3))
            self.assert_closed(producer)
            self.assertTrue(state.called)
            self.assertEqual(self.ops.opens, 1)
        finally:
            self.ops.release_read.set()
            worker.join(3)
        self.assertFalse(worker.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(results, [expected()])
        self.assert_wiped()

    def test_simultaneous_call_race_success_or_failure(self):
        for fail in (False, True):
            with self.subTest(fail=fail):
                ops = FakeOps()
                ops.read_error = fail
                producer, _ = self.source.make_producer(PATH, _ops=ops)
                barrier = threading.Barrier(5, timeout=3)
                outcomes = []
                lock = threading.Lock()
                def invoke():
                    try:
                        barrier.wait()
                        value = producer()
                        outcome = ('ok', value)
                    except BaseException as error:
                        outcome = ('error', type(error))
                    with lock:
                        outcomes.append(outcome)
                workers = [threading.Thread(target=invoke, daemon=True) for _ in range(4)]
                for worker in workers:
                    worker.start()
                barrier.wait()
                for worker in workers:
                    worker.join(3)
                    self.assertFalse(worker.is_alive())
                self.assertEqual(ops.opens, 1)
                self.assertEqual(sum(item[0] == 'ok' for item in outcomes), 0 if fail else 1)
                self.assertTrue(all(kind == 'ok' or value is self.source.SourceClosed
                                    for kind, value in outcomes))
                self.assert_wiped()

    def test_state_has_no_error_or_content_slots(self):
        producer, state = self.producer(b'bad')
        self.assert_closed(producer)
        self.assertEqual(state.__slots__, ('_lock', '_called', '_read_attempted', '_length'))
        self.assertFalse(hasattr(state, '__dict__'))
        self.assertIsNone(state.length)
        self.assertEqual(str(state), '<SourceState>')

    def test_default_open_flags_and_unbuffered_handle_with_only_fakes(self):
        marker = object()
        with mock.patch.object(self.source.os, 'open', return_value=76543) as opened, \
                mock.patch.object(self.source.io, 'FileIO', return_value=marker) as fileio:
            self.assertIs(self.source._open(PATH), marker)
        flags = opened.call_args.args[1]
        self.assertEqual(flags & getattr(os, 'O_ACCMODE', os.O_WRONLY | os.O_RDWR), os.O_RDONLY)
        for name in ('O_BINARY', 'O_NOINHERIT', 'O_CLOEXEC', 'O_NOFOLLOW',
                     'O_NONBLOCK', 'O_NOCTTY'):
            value = getattr(os, name, 0)
            self.assertEqual(flags & value, value)
        fileio.assert_called_once_with(76543, mode='rb', closefd=True)
        # Exercise the optional POSIX bits even on Windows, with no real open.
        with mock.patch.object(self.source.os, 'open', return_value=76543) as opened, \
                mock.patch.object(self.source.io, 'FileIO', return_value=marker), \
                mock.patch.object(self.source.os, 'O_NONBLOCK', 0x40000000, create=True), \
                mock.patch.object(self.source.os, 'O_NOCTTY', 0x20000000, create=True):
            self.assertIs(self.source._open(PATH), marker)
            self.assertEqual(opened.call_args.args[1] & 0x60000000, 0x60000000)

    def test_default_open_closes_fake_descriptor_if_wrapping_fails(self):
        with mock.patch.object(self.source.os, 'open', return_value=76543), \
                mock.patch.object(self.source.io, 'FileIO', side_effect=RuntimeError(PRIVATE_SENTINEL)), \
                mock.patch.object(self.source.os, 'close') as closed:
            self.ops.open = self.source._open
            producer, _ = self.producer()
            self.assert_closed(producer)
        closed.assert_called_once_with(76543)

    def test_lstat_failure_privacy(self):
        def fail(path):
            raise RuntimeError(PRIVATE_SENTINEL)
        self.ops.lstat = fail
        self.assert_closed(lambda: self.source.probe(PATH, _ops=self.ops))
        producer, state = self.producer()
        self.assert_closed(producer)
        self.assertFalse(state.read_attempted)
        self.assertEqual(self.ops.opens, 0)

    def test_fstat_failure_closes_and_is_private(self):
        def fail(handle):
            raise RuntimeError(PRIVATE_SENTINEL)
        self.ops.fstat = fail
        producer, state = self.producer()
        self.assert_closed(producer)
        self.assertTrue(self.ops.handles[0].closed)
        self.assertFalse(state.read_attempted)
        self.assert_wiped()

    def test_postread_lstat_failure_is_private_and_wipes(self):
        def change(path, visit, info):
            if visit == 2:
                raise RuntimeError(PRIVATE_SENTINEL)
        self.ops.on_lstat = change
        producer, _ = self.producer()
        self.assert_closed(producer)
        self.assert_wiped()

    def test_partially_built_frame_is_wiped_on_failure(self):
        original = self.source._allocate
        class FailingFrame(bytearray):
            failed = False
            def __setitem__(self, key, value):
                if key == 12 and value != 0 and not self.failed:
                    self.failed = True
                    raise RuntimeError(PRIVATE_SENTINEL)
                super().__setitem__(key, value)
        def allocate(size):
            if size == 65537:
                return original(size)
            buffer = FailingFrame(size)
            self.buffers.append(buffer)
            return buffer
        with mock.patch.object(self.source, '_allocate', side_effect=allocate):
            producer, state = self.producer()
            self.assert_closed(producer)
            self.assertIsNone(state.length)
        self.assertTrue(any(isinstance(buffer, FailingFrame) for buffer in self.buffers))
        self.assert_wiped()

    def test_static_source_restrictions_and_sole_frame_copy(self):
        # The sole explicit public source-file read in this suite.
        with open(SOURCE_PATH, 'rb') as handle:
            raw = handle.read()
        self.assertNotIn(b'\r', raw)
        tree = ast.parse(raw, filename=SOURCE_PATH)
        imports = {alias.name.split('.')[0] for node in ast.walk(tree)
                   if isinstance(node, ast.Import) for alias in node.names}
        self.assertEqual(imports, {'io', 'ntpath', 'os', 'posixpath', 'stat', 'threading'})
        self.assertFalse(any(isinstance(node, ast.ImportFrom) for node in ast.walk(tree)))
        for forbidden in (b'.env', b'environ', b'logging', b'hashlib', b'socket', b'subprocess'):
            self.assertNotIn(forbidden, raw)
        copies = [node for node in ast.walk(tree) if isinstance(node, ast.Call)
                  and isinstance(node.func, ast.Name) and node.func.id == 'bytes']
        self.assertEqual(len(copies), 1)
        self.assertEqual(ast.unparse(copies[0]), 'bytes(frame)')
        self.assertFalse(any(isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
                             and node.func.id == 'print' for node in ast.walk(tree)))
