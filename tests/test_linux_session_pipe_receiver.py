"""Finite SOURCE-ONLY QA. Fakes only: never initialize/fork/harden/run Native.

Ordinary unittest/pytest discovery contains NO native/integration test or opt-in.
Only the explicitly named public source files below may be opened by these tests.
No actual PCM, binary, procfs, environment contents or private reports are read.
"""
import ast
import copy
import hashlib
import importlib.util
import itertools
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

BASE = Path(__file__).resolve().parents[1]
SOURCE = BASE / 'tools/reference/linux_session_pipe_receiver.py'
PUBLIC = BASE / 'tools/reference/linux_public_pipe_receiver.py'
HELPER = BASE / 'tools/reference/linux_admission_public_supervisor.py'
SPEC = importlib.util.spec_from_file_location('session_receiver_fake_qa', SOURCE)
sut = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sut)  # Inert import only, NEVER main/prepare/initialize.


def encoded(fields):
    return b'RLA1\0\r\n\x1a' + b''.join(len(p).to_bytes(2, 'big') + p for p in fields)


def terminal(category, sessions=0, primary=1, cleanup=0, healthy=False,
             count=0, eof=False, closed=False):
    return (f'REAL_ADMISSION_TERMINAL_V1 {category} primary_errors={primary} '
            f'cleanup_errors={cleanup} sessions={sessions} healthy={str(healthy).lower()} '
            f'pcm_bytes={count} pcm_eof={str(eof).lower()} '
            f'source_closed={str(closed).lower()}\n').encode('ascii')


def cli(mode='public-guard'):
    return ['--root', '/public/session-root', '--owner-sha256', 'a' * 64,
            '--controller-sha256', 'b' * 64, '--helper', '/public/helper.py',
            '--mode', mode, '--reservation', mode + '-' + '1' * 32 + '.json',
            '--reservation-sha256', 'c' * 64, '--helper-sha256', sut.HELPER_SHA]


def node(path, name, parent=None):
    tree = ast.parse(path.read_text(encoding='utf-8'))
    scope = tree.body if parent is None else next(n for n in tree.body
                                                 if getattr(n, 'name', None) == parent).body
    return copy.deepcopy(next(n for n in scope if getattr(n, 'name', None) == name))


def dump(value):
    return ast.dump(value, include_attributes=False)


class FakeIO:
    """All bytes are invented; no real fd, process, clock or signal operation."""
    def __init__(self, raw: bytes, *, eof: bool = True, chunk: int = 12303):
        self.raw = raw
        self.offset = 0
        self.eof = eof
        self.chunk = chunk
        self.clock = 0.0
        self.requests = []
        self.read_events = []
        self.fail = None
        self.blocks = 0

    def guard(self) -> None:
        if self.fail is not None:
            raise sut.ClosedFailure(self.fail)

    def now(self) -> float:
        self.clock += 0.001
        return self.clock

    def input_ready(self, end: float) -> bool:
        if self.offset == len(self.raw) and not self.eof:
            self.clock += 1.0
            return False
        return True

    def input_read(self, cap: int) -> bytes:
        self.requests.append((self.offset, cap))
        if self.blocks:
            self.blocks -= 1
            raise BlockingIOError()
        count = min(cap, self.chunk, len(self.raw) - self.offset)
        value = self.raw[self.offset:self.offset + count]
        self.offset += count
        self.read_events.append('input')
        return value

    def input_readinto(self, view: memoryview) -> int:
        # The fake alone uses invented bytes; production uses readv into the view.
        value = self.input_read(len(view))
        view[:len(value)] = value
        return len(value)


class FakePort(sut.LinuxPort):
    """Typed fake-kernel adapter; inherited OS methods run only under fake OS."""
    def __init__(self, mode='PUBLIC_GUARD', raw=None):
        super().__init__(SimpleNamespace(mode=mode))
        self.cwd = '/public/fake-cwd'
        self.io = FakeIO(sut.PUBLIC_FRAME if raw is None else raw)
        self.events = []
        self.output = []
        self.stderr = []
        self.report = None
        self.fail_at = None
        self.fail_code = None
        self.custom_exception = None
        self.cleanup_ok = True
        self.pins_ok = True
        self.empty_ok = True
        self.parent_ok = True
        self.forks = 0
        self.writes = 0
        self.wipe_calls = 0
        self.short_write = False
        self.reservation_exists = True
        self.bad_reservation = False
        self.native_raw = sut.GUARD_TERMINAL if mode == 'PUBLIC_GUARD' else sut.REAL_TERMINAL
        self.native_eof = True
        self.native_offset = 0
        self.native_status = 1 if mode == 'PUBLIC_GUARD' else 0
        self.native_si_code = 1
        self.h = SimpleNamespace(drain=lambda: self.cleanup_ok,
                                 empty_children=lambda: self.empty_ok,
                                 ownership=lambda _: SimpleNamespace(si_code=self.native_si_code,
                                                                     si_status=self.native_status),
                                 policy=lambda: None)
        self.fake_os = SimpleNamespace(set_blocking=lambda *a: None,
                                       write=self.fake_write, close=lambda fd: None,
                                       CLD_EXITED=1, listdir=lambda _: [])

    def event(self, name: str) -> None:
        self.events.append(name)
        if self.fail_at == name:
            if self.custom_exception is not None:
                raise self.custom_exception
            raise sut.ClosedFailure(self.fail_code or 'INTERNAL')

    def prepare(self) -> None:
        self.event('prepare')
        self.prepared = self.initialized = True

    def consume_reservation(self) -> None:
        self.event('reservation')
        sut.require(self.reservation_exists, 'RESERVATION')
        self.reservation_exists = False
        self.reservation_consumed = True
        sut.require(not self.bad_reservation, 'RESERVATION')

    def start(self) -> None:
        self.event('start')
        self.forks += 1
        self.pidfd = 99
        self.write_in = 10
        self.handles = {10, 11}
        self.end = self.now() + sut.GUARD_SECONDS

    def native_ready(self) -> None:
        self.event('native_ready')

    def observe(self) -> None:
        self.event('observe')
        # Model the unchanged helper's OPEN-only EACCES proof, never inspect environ.
        self.events.append('environ_open_EACCES')

    def guard(self) -> None:
        self.final_guard()

    def final_guard(self) -> None:
        sut.require(self.parent_ok, 'CANCELLED')
        if self.fail_at == 'final_guard':
            self.event('final_guard')

    def now(self) -> float:
        return self.io.now()

    def input_ready(self, end: float) -> bool:
        return self.io.input_ready(end)

    def input_read(self, cap: int) -> bytes:
        self.events.append('input')
        return self.io.input_read(cap)

    def input_readinto(self, view: memoryview) -> int:
        self.events.append('input')
        return self.io.input_readinto(view)

    def read_frame(self) -> None:
        self.event('read_frame')
        with mock.patch.object(sut, 'os', self.fake_os):
            super().read_frame()

    def forward(self) -> None:
        self.event('forward')
        with mock.patch.object(sut, 'os', self.fake_os):
            super().forward()

    def fake_write(self, fd: int, raw) -> int:
        self.writes += 1
        if self.mode == 'PUBLIC_GUARD':
            sut.require(raw == sut.PUBLIC_FRAME, 'INTERNAL')
        return len(raw) - 1 if self.short_write else len(raw)

    def wipe(self) -> None:
        self.wipe_calls += 1
        super().wipe()

    def terminal_read(self, cap: int) -> bytes:
        self.event('terminal_read')
        if self.native_offset == len(self.native_raw) and not self.native_eof:
            raise sut.ClosedFailure('TIMEOUT')
        part = self.native_raw[self.native_offset:self.native_offset + cap]
        self.native_offset += len(part)
        return part

    def native_terminal(self):
        self.event('native_terminal')
        with mock.patch.object(sut, 'os', self.fake_os):
            return super().native_terminal()

    def cleanup(self) -> bool:
        self.event('cleanup')
        with mock.patch.object(sut, 'os', self.fake_os):
            return super().cleanup()

    def recheck(self) -> bool:
        self.event('recheck')
        return self.pins_ok

    def persist(self, report) -> None:
        self.event('persist')
        self.report = copy.deepcopy(report)

    def emit(self, raw: bytes) -> None:
        self.event('emit')
        self.output.append(raw)

    def emit_ready(self) -> None:
        self.event('ready')
        self.emit(sut.UPSTREAM_READY[self.mode])

    def emit_success(self) -> None:
        self.event('success')
        super().emit_success()

    def emit_error(self, code: str, rpc: str) -> None:
        self.output.append(sut.error_line(self.mode, code, rpc))
        self.stderr.append(b'SESSION_PIPE_RECEIVER_FAILED_V1\n')


class StaticTests(unittest.TestCase):
    def test_import_is_inert(self):
        raw = SOURCE.read_text(encoding='utf-8')
        code = compile(raw, '<public-source-import-test>', 'exec')
        trap = mock.Mock(side_effect=AssertionError('OS operation during import'))
        with mock.patch.object(sut.os, 'open', trap), \
             mock.patch.object(sut.os, 'read', trap), \
             mock.patch.object(sut.os, 'write', trap), \
             mock.patch.object(sut.signal, 'alarm', trap, create=True), \
             mock.patch.object(sut.signal, 'signal', trap), \
             mock.patch('builtins.open', trap):
            scope = {'__name__': 'inert_qa', '__file__': str(SOURCE)}
            exec(code, scope)
        trap.assert_not_called()

    def test_qualified_source_hashes_unchanged(self):
        pins = {
            HELPER: 'bb711f7ce1ada062cee9ed47136ae0496c38d8095d8ced26229bb2f8328ec0d9',
            PUBLIC: 'fd70a045c606ef3bf9f9a0b8352707499ef1943e2056a307feb5bab2f98c89c0',
            BASE / 'tools/windows_owned_job.py': '3ac53d5e3f202512b18cb3e4bafdc27c8b87add5c4d9afb70a3dd07462fd76aa',
            BASE / 'tools/windows_owned_pipe_job.py': 'a9e6c5890e39461549d1abcf898f7d405c10ca2171715e3a26e52c469ae86ea5',
            BASE / 'crates/rambler-cli/examples/linux_real_admission.rs': sut.EXAMPLE_SHA,
            BASE / 'crates/rambler-cli/examples/support/linux_real_admission_tests.rs': sut.FAKE_SOURCE_SHA,
        }
        for path, expected in pins.items():
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), expected)

    def test_exec_ast_only_fixed_argv_and_signature_differ(self):
        expected = node(HELPER, 'target_exec')
        actual = node(SOURCE, 'session_target_exec')
        expected.name = actual.name
        self.assertEqual(expected.args.args.pop().arg, 'no_args')
        assignments = [n for n in ast.walk(expected) if isinstance(n, ast.Assign)
                       and any(isinstance(t, ast.Name) and t.id == 'argv' for t in n.targets)]
        self.assertEqual(len(assignments), 1)
        assignments[0].value = ast.parse("[binary, '--single-session-v1']", mode='eval').body
        self.assertEqual(dump(actual), dump(expected))

    def test_copied_free_functions_ast_equal(self):
        for name in ('public_path', 'trusted_open', 'public_bytes', 'public_directory',
                     'load_helper', 'bounded_public_input'):
            self.assertEqual(dump(node(SOURCE, name)), dump(node(PUBLIC, name)))

    def test_copied_port_functions_ast_equal(self):
        for name in ('source_check', 'close_owned', 'final_guard', 'guard', 'now', 'wait',
                     'read_native_exact', 'native_ready', 'observe', 'input_ready',
                     'input_read', 'cleanup', 'check_record', 'persist', 'emit'):
            self.assertEqual(dump(node(SOURCE, name, 'LinuxPort')),
                             dump(node(PUBLIC, name, 'LinuxPort')))

    def test_start_ast_has_only_required_session_deltas(self):
        expected = node(PUBLIC, 'start', 'LinuxPort')
        actual = node(SOURCE, 'start', 'LinuxPort')
        # Precisely allow the reservation proof, pipe-capacity check, helper-global
        # binding, fixed target call and mode budgets; retain every other statement.
        actual.body = [n for n in actual.body if not (
            (isinstance(n, ast.If) and ast.unparse(n.test) == "self.mode == 'REAL'")
            or (isinstance(n, ast.Assign) and ast.unparse(n.targets[0]) in ('target', 'self.gate_time')))]
        first = actual.body[0].value.args[0]
        first.values = [v for v in first.values if ast.unparse(v) != 'self.reservation_consumed']
        old_call = next(n for n in ast.walk(expected) if isinstance(n, ast.Call)
                        and ast.unparse(n.func) == 'self.h.target_exec')
        for n in ast.walk(actual):
            if isinstance(n, ast.Call) and ast.unparse(n.func) == 'target':
                n.func, n.args = old_call.func, old_call.args
        actual.body[-1] = expected.body[-1]
        self.assertEqual(dump(actual), dump(expected))

    def test_recheck_ast_only_new_root_and_reservation_deltas(self):
        expected = node(PUBLIC, 'recheck', 'LinuxPort')
        actual = node(SOURCE, 'recheck', 'LinuxPort')
        keep = []
        for n in actual.body:
            text = ast.unparse(n)
            if (text.startswith('require(not os.listdir(self.reservations)')
                    or text.startswith('require(self.reservation_consumed')
                    or text.startswith('self.reservation_check(')
                    or text == 'public_directory(self.reservations)'):
                continue
            keep.append(n)
        actual.body = keep
        for n in ast.walk(actual):
            if isinstance(n, ast.Set):
                n.elts = [e for e in n.elts if not (isinstance(e, ast.Constant)
                                                  and e.value == 'reservations')]
        self.assertEqual(dump(actual), dump(expected))

    def test_one_launch_and_forward_before_any_input(self):
        flow = node(SOURCE, 'run_once')
        calls = [n for n in ast.walk(flow) if isinstance(n, ast.Call)]
        names = [ast.unparse(n.func) for n in calls]
        self.assertEqual(names.count('port.start'), 1)
        self.assertEqual(names.count('port.forward'), 1)
        self.assertFalse(any(isinstance(n, (ast.For, ast.While)) for n in ast.walk(flow)))
        direct = [ast.unparse(n.value.func) for n in next(n for n in flow.body if isinstance(n, ast.Try)).body
                  if isinstance(n, ast.Expr) and isinstance(n.value, ast.Call)]
        self.assertLess(direct.index('port.start'), direct.index('port.read_frame'))
        tree = ast.parse(SOURCE.read_text(encoding='utf-8'))
        self.assertEqual(sum(isinstance(n, ast.Call) and ast.unparse(n.func) == 'os.fork'
                             for n in ast.walk(tree)), 1)
        self.assertNotIn('subprocess', [n.id for n in ast.walk(tree) if isinstance(n, ast.Name)])

    def test_hardening_precedes_input_and_ready(self):
        prepare = ast.unparse(node(SOURCE, 'prepare', 'LinuxPort'))
        for snippet in ('dict(os.environ) == PUBLIC_ENV', 'os.umask(63)', 'self.h.initialize()',
                        'self.h.prctl(38, 1)', 'os.get_blocking(0)', 'self.source_check()',
                        'self.input_pins()', 'self.h.freeze('):
            self.assertIn(snippet, prepare)
        self.assertNotIn('os.read(0', prepare)
        self.assertLess(prepare.index('self.h.initialize()'), prepare.index('signal.alarm('))
        self.assertIn('PARENT_ALARM[self.mode]', prepare)

    def test_fixed_native_sdk_no_anonymous_retry_or_logs(self):
        rust = (BASE / 'crates/rambler-cli/examples/linux_real_admission.rs').read_text()
        self.assertIn('GrpcTransport::connect(TransportConfig::default())', rust)
        self.assertIn('credentials: Some(Arc::new(admitted.credentials))', rust)
        self.assertIn('drop(runtime)', rust)
        self.assertIn('transport.close()', rust)
        self.assertIn('engine.close()', rust)
        self.assertNotIn('Anonymous', rust)
        exec_ast = ast.unparse(node(SOURCE, 'session_target_exec'))
        self.assertNotIn('--public-frame-check-v1', exec_ast)
        self.assertIn("[binary, '--single-session-v1']", exec_ast)
        for name in ('bounded_real_input',):
            text = ast.unparse(node(SOURCE, name))
            for forbidden in ('.decode(', 'hashlib', 'print(', 'logging', 'open('):
                self.assertNotIn(forbidden, text)

    def test_deadline_arithmetic_fake_clock(self):
        self.assertGreater(sut.PARENT_ALARM['REAL'], sut.READY_SECONDS + sut.INPUT_SECONDS
                           + sut.FORWARD_SECONDS + sut.DRAIN_SECONDS)
        self.assertGreater(sut.PARENT_ALARM['REAL'], 90)
        p = FakePort('REAL', encoded((b'a', b'b', b'c')))
        p.used, p.buffer = 17, bytearray(12302)
        before = p.now()
        p.write_in, p.handles = 10, {10}
        p.forward()
        self.assertAlmostEqual(p.end - before, 47.001, places=3)

    def test_exact_constants_lengths(self):
        self.assertEqual(len(sut.PUBLIC_FRAME), 85)
        self.assertEqual(len(sut.GUARD_TERMINAL), 151)
        self.assertEqual(len(sut.REAL_TERMINAL), 150)
        self.assertEqual(len(sut.READY), 24)
        self.assertEqual([len(sut.UPSTREAM_READY[m]) for m in ('PUBLIC_GUARD', 'REAL')], [38, 30])
        self.assertEqual([len(sut.UPSTREAM_SUCCESS[m]) for m in ('PUBLIC_GUARD', 'REAL')], [82, 60])
        for mode, code, rpc in itertools.product(sut.MODES, sut.CODES, ('ZERO', 'ONE', 'UNKNOWN')):
            line = sut.error_line(mode, code, rpc)
            self.assertLessEqual(len(line), 112)
            self.assertEqual(line.count(b'\n'), 1)


class ParserAndFrameTests(unittest.TestCase):
    def test_parser_exact_eight_pairs_and_reordering(self):
        value = sut.arguments(cli())
        self.assertEqual(value.mode, 'PUBLIC_GUARD')
        pairs = [cli()[i:i+2] for i in range(0, 16, 2)]
        self.assertEqual(sut.arguments(sum(reversed(pairs), [])).reservation_id, '1' * 32)
        self.assertEqual(sut.arguments(cli('real')).mode, 'REAL')

    def test_parser_duplicate_attached_unknown_missing(self):
        bad = [cli()[:-2], cli() + ['--mode', 'real'],
               ['--root=/public/session-root'] + cli()[2:],
               ['--unknown', '/public/session-root'] + cli()[2:],
               cli()[:2] + cli()[:2] + cli()[4:]]
        for value in bad:
            with self.assertRaises(sut.ClosedFailure):
                sut.arguments(value)

    def test_parser_bad_modes_hashes_and_private_paths(self):
        for index, value in ((1, '/public/.env'), (1, '/public/../root'), (9, 'bad'),
                             (15, 'd' * 64), (3, 'A' * 64), (7, '/private/helper.py')):
            args = cli()
            args[index] = value
            with self.assertRaises(sut.ClosedFailure):
                sut.arguments(args)

    def test_parser_reservation_mode_mismatch(self):
        args = cli()
        args[11] = 'real-' + '1' * 32 + '.json'
        with self.assertRaises(sut.ClosedFailure):
            sut.arguments(args)
        p = sut.LinuxPort(sut.arguments(cli()))
        p.h = SimpleNamespace(strict_json=lambda _: {'schema': 'linux-session-pipe-reservation-v1',
            'mode': 'REAL', 'id': '1' * 32}, canonical=lambda x: repr(sorted(x.items())).encode())
        with mock.patch.object(sut, 'public_bytes', return_value=b'public metadata'):
            with self.assertRaises(sut.ClosedFailure):
                p.reservation_check('/public/reservation.json')

    def test_guard_exact_frame_and_eof(self):
        io = FakeIO(sut.PUBLIC_FRAME, chunk=1)
        self.assertIsNone(sut.bounded_public_input(io, 5))
        self.assertEqual(io.offset, 85)
        self.assertEqual(len(io.requests), 86)

    def test_guard_all_85_truncations(self):
        for end in range(85):
            with self.assertRaises(sut.ClosedFailure):
                sut.bounded_public_input(FakeIO(sut.PUBLIC_FRAME[:end]), 5)

    def test_guard_every_single_byte_flip(self):
        for index in range(85):
            changed = bytearray(sut.PUBLIC_FRAME)
            changed[index] ^= 1
            with self.assertRaises(sut.ClosedFailure):
                sut.bounded_public_input(FakeIO(bytes(changed)), 5)

    def test_guard_trailing_and_missing_eof(self):
        for io in (FakeIO(sut.PUBLIC_FRAME + b'x'), FakeIO(sut.PUBLIC_FRAME, eof=False)):
            with self.assertRaises(sut.ClosedFailure):
                sut.bounded_public_input(io, 5)
            self.assertLessEqual(io.offset, 86)

    def test_real_independent_field_boundaries(self):
        for index, size in itertools.product(range(3), (1, 4096)):
            fields = [b'a', b'b', b'c']
            fields[index] = b'x' * size
            raw, buffer = encoded(fields), bytearray(12302)
            io = FakeIO(raw)
            self.assertEqual(sut.bounded_real_input(io, buffer, 5), len(raw))
            self.assertEqual(memoryview(buffer)[:len(raw)], raw)
            sut.overwrite(buffer)

    def test_real_maximum_frame(self):
        raw = encoded((b'a' * 4096, b'b' * 4096, b'c' * 4096))
        self.assertEqual(len(raw), 12302)
        io, buffer = FakeIO(raw), bytearray(12302)
        self.assertEqual(sut.bounded_real_input(io, buffer, 5), 12302)
        self.assertEqual(io.offset, 12302)
        self.assertEqual(io.requests[-1], (12302, 1))
        sut.overwrite(buffer)

    def test_real_bad_first_lengths_stop_at_ten(self):
        for size in (0, 4097, 65535):
            io = FakeIO(b'RLA1\0\r\n\x1a' + size.to_bytes(2, 'big') + b'UNREAD')
            buffer = bytearray(12302)
            with self.assertRaises(sut.ClosedFailure):
                sut.bounded_real_input(io, buffer, 5)
            self.assertEqual(io.offset, 10)
            self.assertFalse(any(buffer))

    def test_real_later_bad_lengths_do_not_read_body(self):
        for index, size in itertools.product(range(3), (0, 4097, 65535)):
            prefix = b'RLA1\0\r\n\x1a' + (b'\0\1x' * index)
            io = FakeIO(prefix + size.to_bytes(2, 'big') + b'UNREAD')
            with self.assertRaises(sut.ClosedFailure):
                sut.bounded_real_input(io, bytearray(12302), 5)
            self.assertEqual(io.offset, len(prefix) + 2)

    def test_real_truncation_in_each_segment(self):
        raw = encoded((b'alpha', b'beta', b'gamma'))
        for end in range(len(raw)):
            buffer = bytearray(12302)
            with self.assertRaises(sut.ClosedFailure):
                sut.bounded_real_input(FakeIO(raw[:end], chunk=1), buffer, 5)
            self.assertFalse(any(buffer))

    def test_real_trailing_and_missing_eof(self):
        raw = encoded((b'a', b'b', b'c'))
        for io in (FakeIO(raw + b'!'), FakeIO(raw, eof=False)):
            with self.assertRaises(sut.ClosedFailure):
                sut.bounded_real_input(io, bytearray(12302), 5)
            self.assertLessEqual(io.offset, len(raw) + 1)

    def test_real_no_prefetch_and_magic_prefix_rejection(self):
        raw = encoded((b'abc', b'de', b'f'))
        io = FakeIO(raw, chunk=1)
        io.blocks = 1
        self.assertEqual(sut.bounded_real_input(io, bytearray(12302), 5), len(raw))
        boundaries = (8, 10, 13, 15, 17, 19, 20, 21)
        for offset, cap in io.requests:
            boundary = next(end for end in boundaries if end > offset)
            self.assertEqual(cap, boundary - offset)
        bad = FakeIO(b'X' + raw[1:], chunk=1)
        with self.assertRaises(sut.ClosedFailure):
            sut.bounded_real_input(bad, bytearray(12302), 5)
        self.assertEqual(bad.offset, 1)

    def test_real_invalid_utf8_delegated_without_decode(self):
        raw = encoded((b'\xff', b'\x80', b'\xc0\x80'))
        self.assertEqual(sut.bounded_real_input(FakeIO(raw), bytearray(12302), 5), len(raw))

    def test_real_public_tuple_rejected_locally(self):
        p = FakePort('REAL', sut.PUBLIC_FRAME)
        self.assertEqual(sut.run_once(p, p.mode), 1)
        self.assertEqual((p.report['error'], p.report['rpc'], p.writes), ('PUBLIC_TUPLE', 'ZERO', 0))

    def test_guard_never_forwards_nonpublic(self):
        for raw in (encoded((b'not-public', b'pkg', b'cert')), sut.PUBLIC_FRAME + b'!', b''):
            p = FakePort(raw=raw)
            self.assertEqual(sut.run_once(p, p.mode), 1)
            self.assertEqual(p.writes, 0)
            self.assertEqual(p.report['rpc'], 'ZERO')


class FlowTests(unittest.TestCase):
    def run_port(self, p):
        status = sut.run_once(p, p.mode)
        self.assertLessEqual(p.forks, 1)
        self.assertLessEqual(p.writes, 1)
        self.assertGreaterEqual(p.wipe_calls, 1)
        self.assertIsNone(p.buffer)
        return status

    def test_guard_happy_order_and_exact_native_exit_one(self):
        p = FakePort()
        self.assertEqual(self.run_port(p), 0)
        self.assertTrue(p.report['ok'])
        self.assertEqual(p.report['native_exit'], 1)
        self.assertEqual(p.report['rpc'], 'ZERO')
        self.assertFalse(p.report['healthy'])
        self.assertEqual(p.output, [sut.UPSTREAM_READY[p.mode], sut.UPSTREAM_SUCCESS[p.mode]])
        for first, second in (('prepare', 'reservation'), ('reservation', 'start'),
                              ('start', 'input'), ('native_ready', 'observe'),
                              ('observe', 'ready'), ('forward', 'native_terminal'),
                              ('cleanup', 'recheck'), ('recheck', 'persist'), ('persist', 'success')):
            self.assertLess(p.events.index(first), p.events.index(second))
        self.assertEqual(p.events.count('observe'), 2)
        self.assertEqual(p.events.count('environ_open_EACCES'), 2)
        forward = p.events.index('forward')
        self.assertEqual(p.events[forward-2:forward], ['observe', 'environ_open_EACCES'])
        self.assertEqual((p.forks, p.writes), (1, 1))

    def test_guard_exit_zero_is_breach_unknown(self):
        p = FakePort()
        p.native_status = 0
        self.assertEqual(self.run_port(p), 1)
        self.assertEqual((p.report['error'], p.report['rpc']), ('GUARD_BREACH', 'UNKNOWN'))

    def test_guard_runtime_or_public_check_breaches(self):
        for category in sut.RUNTIME | {'PUBLIC_FRAME_CHECKED'}:
            p = FakePort()
            p.native_raw = terminal(category)
            self.assertEqual(self.run_port(p), 1)
            self.assertEqual((p.report['error'], p.report['rpc']), ('GUARD_BREACH', 'UNKNOWN'))

    def test_guard_other_preruntime_unexpected_zero(self):
        for category in sut.PRE_RUNTIME - {'REJECT_CREDENTIALS'}:
            p = FakePort()
            p.native_raw = terminal(category)
            self.assertEqual(self.run_port(p), 1)
            self.assertEqual((p.report['error'], p.report['rpc']), ('NATIVE_UNEXPECTED', 'ZERO'))

    def test_guard_malformed_extra_missing_eof_and_signal(self):
        cases = [(b'bad\n', True, 1, 'NATIVE_OUTPUT'),
                 (sut.GUARD_TERMINAL + b'x', True, 1, 'NATIVE_OUTPUT'),
                 (sut.GUARD_TERMINAL, False, 1, 'TIMEOUT'),
                 (sut.GUARD_TERMINAL, True, 2, 'NATIVE')]
        for raw, eof, code, expected in cases:
            p = FakePort()
            p.native_raw, p.native_eof, p.native_si_code = raw, eof, code
            self.assertEqual(self.run_port(p), 1)
            self.assertEqual((p.report['error'], p.report['rpc']), (expected, 'UNKNOWN'))

    def test_real_success_closed_record(self):
        p = FakePort('REAL', encoded((b'invented', b'pkg', b'cert')))
        self.assertEqual(self.run_port(p), 0)
        self.assertTrue(p.report['healthy'])
        self.assertEqual((p.report['sessions'], p.report['rpc']), (1, 'ONE'))
        self.assertEqual(set(p.report), set(sut.result_record('REAL')))
        self.assertEqual(p.report['qualification'], 'REAL_SINGLE_ATTEMPT')
        self.assertEqual(p.output[-1], sut.UPSTREAM_SUCCESS['REAL'])

    def test_real_each_terminal_category_mapping(self):
        codes = {'REJECT_FRAME': 'NATIVE_REJECTED', 'REJECT_CREDENTIALS': 'NATIVE_REJECTED',
                 'CONNECT_FAILURE': 'CONNECT', 'SESSION_FAILURE': 'SESSION',
                 'SESSION_AND_CLEANUP_FAILURE': 'SESSION', 'CLEANUP_FAILURE': 'NATIVE_CLEANUP'}
        for category in sut.CATEGORIES - {'SESSION_HEALTHY'}:
            p = FakePort('REAL', encoded((b'a', b'b', b'c')))
            sessions = int(category in sut.RUNTIME - {'CONNECT_FAILURE'})
            p.native_raw, p.native_status = terminal(category, sessions=sessions), 1
            self.assertEqual(self.run_port(p), 1)
            expected_rpc = 'ZERO' if category in sut.PRE_RUNTIME else 'UNKNOWN'
            self.assertEqual(p.report['rpc'], expected_rpc)
            self.assertEqual(p.report['error'], codes.get(category, 'NATIVE'))

    def test_secret_sentinel_and_exception_never_escape(self):
        sentinel = b'INVENTED-SECRET-SENTINEL-NOT-A-CREDENTIAL'
        for failing in (False, True):
            p = FakePort('REAL', encoded((sentinel, b'pkg', b'cert')))
            if failing:
                p.fail_at, p.custom_exception = 'forward', ValueError(sentinel)
            self.run_port(p)
            exposed = repr((p.report, p.output, p.stderr)).encode()
            self.assertNotIn(sentinel, exposed)
        io = FakeIO(encoded((sentinel, b'pkg', b'cert'))[:-1])
        try:
            sut.bounded_real_input(io, bytearray(12302), 5)
        except sut.ClosedFailure as exc:
            self.assertEqual(exc.args, ('INPUT',))
            self.assertNotIn(sentinel, repr(exc.args).encode())

    def test_timeout_cancel_after_forward_unknown_drain_no_retry(self):
        for code in ('TIMEOUT', 'CANCELLED'):
            p = FakePort('REAL', encoded((b'a', b'b', b'c')))
            p.fail_at, p.fail_code = 'native_terminal', code
            self.assertEqual(self.run_port(p), 1)
            self.assertEqual((p.report['error'], p.report['rpc']), (code, 'UNKNOWN'))
            self.assertEqual((p.forks, p.writes), (1, 1))
            self.assertIn('cleanup', p.events)

    def test_reservation_missing_or_second_run_never_forks(self):
        for initially_missing in (True, False):
            p = FakePort()
            if not initially_missing:
                self.assertEqual(self.run_port(p), 0)
            p.reservation_exists = False
            p.forks = p.writes = 0
            self.assertEqual(self.run_port(p), 1)
            self.assertEqual((p.forks, p.writes), (0, 0))
            self.assertIn(p.report['error'], ('RESERVATION', 'STARTUP'))

    def test_postrename_hash_failure_stays_consumed(self):
        p = FakePort()
        p.bad_reservation = True
        self.assertEqual(self.run_port(p), 1)
        self.assertFalse(p.reservation_exists)
        self.assertTrue(p.report['reservation_consumed'])
        self.assertEqual(p.forks, 0)
        # Exercise the REAL reservation method with only injected filesystem calls.
        real = sut.LinuxPort(sut.arguments(cli()))
        real.prepared, real.owned, real.reservations = True, '/public/fresh-case', '/public/reservations'
        fake_os = SimpleNamespace(listdir=lambda _: [real.args.reservation],
            path=SimpleNamespace(lexists=lambda _: False), rename=mock.Mock())
        with mock.patch.object(sut, 'os', fake_os), \
             mock.patch.object(sut, 'public_directory', lambda _: None), \
             mock.patch.object(real, 'reservation_check', side_effect=sut.ClosedFailure('RESERVATION')):
            with self.assertRaises(sut.ClosedFailure):
                real.consume_reservation()
        fake_os.rename.assert_called_once()
        self.assertTrue(real.reservation_consumed)

    def test_pipe_capacity_and_short_write_fail_without_retry(self):
        p = sut.LinuxPort(sut.arguments(cli('real')))
        p.prepared = p.reservation_consumed = True
        p.pcm = '/public/invented.pcm'
        p.h = SimpleNamespace(empty_children=lambda: True, open_path=lambda _: 20)
        fake_os = SimpleNamespace(pipe2=mock.Mock(side_effect=[(10, 11), (12, 13), (14, 15)]),
            O_CLOEXEC=1, set_blocking=lambda *a: None, fork=mock.Mock())
        with mock.patch.object(sut, 'os', fake_os), \
             mock.patch.object(p, 'final_guard', lambda: None), \
             mock.patch.dict(sys.modules, {'fcntl': SimpleNamespace(fcntl=lambda *a: 8192)}):
            with self.assertRaises(sut.ClosedFailure) as cm:
                p.start()
        self.assertEqual(cm.exception.code, 'FORWARD')
        fake_os.fork.assert_not_called()
        for mode in sut.MODES:
            p = FakePort(mode, sut.PUBLIC_FRAME if mode == 'PUBLIC_GUARD' else encoded((b'a', b'b', b'c')))
            p.short_write = True
            self.assertEqual(self.run_port(p), 1)
            self.assertEqual((p.report['error'], p.writes, p.report['rpc']), ('FORWARD', 1, 'UNKNOWN'))

    def test_buffer_overwrite_every_stage(self):
        for stage in ('prepare', 'reservation', 'start', 'native_ready', 'observe',
                      'ready', 'read_frame', 'forward', 'native_terminal', 'persist', 'success'):
            p = FakePort('REAL', encoded((b'abc', b'def', b'ghi')))
            p.fail_at, p.fail_code = stage, 'INTERNAL'
            self.assertEqual(self.run_port(p), 1)
        buffer = bytearray(b'invented')
        sut.overwrite(buffer)
        self.assertEqual(buffer, bytearray(8))

    def test_cleanup_requires_echild_not_kill_attempt(self):
        for drained, empty in ((False, True), (True, False)):
            p = FakePort()
            p.cleanup_ok, p.empty_ok = drained, empty
            self.assertEqual(self.run_port(p), 1)
            self.assertEqual(p.report['error'], 'CLEANUP')
            self.assertFalse(p.report['cleanup'])
            self.assertNotIn(sut.UPSTREAM_SUCCESS[p.mode], p.output)

    def test_late_pin_failure_keeps_rpc_one(self):
        p = FakePort('REAL', encoded((b'a', b'b', b'c')))
        p.pins_ok = False
        self.assertEqual(self.run_port(p), 1)
        self.assertEqual((p.report['error'], p.report['rpc']), ('PINS', 'ONE'))
        self.assertFalse(p.report['healthy'])

    def test_record_failure_blocks_terminal(self):
        p = FakePort()
        p.fail_at, p.fail_code = 'persist', 'RECORD'
        self.assertEqual(self.run_port(p), 1)
        self.assertIsNone(p.report)
        self.assertEqual(p.output[-1], sut.error_line(p.mode, 'RECORD', 'ZERO'))

    def test_broken_upstream_and_parent_death_block_success(self):
        p = FakePort()
        p.fail_at, p.custom_exception = 'success', BrokenPipeError()
        self.assertEqual(self.run_port(p), 1)
        self.assertTrue(p.report['ok'])  # Persisted OK alone is explicitly insufficient.
        self.assertNotIn(sut.UPSTREAM_SUCCESS[p.mode], p.output)
        self.assertEqual(p.output[-1], sut.error_line(p.mode, 'OUTPUT', 'ZERO'))
        p = FakePort()
        p.parent_ok = False
        self.assertEqual(self.run_port(p), 1)
        self.assertEqual(p.writes, 0)
        self.assertNotIn(sut.UPSTREAM_SUCCESS[p.mode], p.output)

    def test_terminal_flags_counters_and_output_are_closed(self):
        for raw in (sut.REAL_TERMINAL + b'x', sut.REAL_TERMINAL + sut.REAL_TERMINAL,
                    b'x' * 257, b'\xff\n', sut.REAL_TERMINAL[:-1],
                    sut.REAL_TERMINAL.replace(b'pcm_bytes=117440', b'pcm_bytes=117441'),
                    sut.REAL_TERMINAL.replace(b'cleanup_errors=0', b'cleanup_errors=4')):
            p = FakePort('REAL', encoded((b'a', b'b', b'c')))
            p.native_raw = raw
            self.assertEqual(self.run_port(p), 1)
            self.assertEqual((p.report['error'], p.report['rpc']), ('NATIVE_OUTPUT', 'UNKNOWN'))
        for old, new in ((b'pcm_eof=true', b'pcm_eof=false'),
                         (b'source_closed=true', b'source_closed=false'),
                         (b'healthy=true', b'healthy=false'), (b'sessions=1', b'sessions=0')):
            p = FakePort('REAL', encoded((b'a', b'b', b'c')))
            p.native_raw = sut.REAL_TERMINAL.replace(old, new)
            self.assertEqual(self.run_port(p), 1)
            self.assertFalse(p.report['healthy'])

    def test_failed_external_observation_blocks_input(self):
        p = FakePort()
        p.fail_at, p.fail_code = 'observe', 'OBSERVE'
        self.assertEqual(self.run_port(p), 1)
        self.assertNotIn('input', p.events)
        self.assertNotIn(sut.UPSTREAM_READY[p.mode], p.output)
        self.assertEqual((p.writes, p.report['rpc']), (0, 'ZERO'))


if __name__ == '__main__':
    unittest.main()
