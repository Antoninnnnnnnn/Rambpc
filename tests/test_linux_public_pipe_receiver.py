"""PUBLIC-ONLY fake-kernel tests. No hardening, signals, fork, real PCM or native run.

These tests are source-only at delivery. Importing the receiver performs no IO;
all exercised OS-facing methods below have their whole OS/helper port replaced.
"""
import importlib.util
from pathlib import Path
import types
import unittest
from unittest.mock import MagicMock, patch

_SOURCE = Path(__file__).resolve().parents[1] / 'tools/reference/linux_public_pipe_receiver.py'
_SPEC = importlib.util.spec_from_file_location('public_pipe_receiver_under_test', _SOURCE)
r = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(r)


class FakePort:
    def __init__(self, fail=None, failure=None):
        self.events = []
        self.output = []
        self.fail = fail
        self.failure = failure or RuntimeError('must-never-be-logged: PRIVATE-LIKE')
        self.persisted = None
        self.cleaned = True
        self.unchanged = True

    def event(self, name):
        self.events.append(name)
        if self.fail == name:
            raise self.failure

    def prepare(self):
        self.event('prepare')

    def start(self):
        self.event('start')

    def native_ready(self):
        self.event('native-ready')

    def observe(self):
        self.event('observe')
        if self.fail == 'second-observe' and self.events.count('observe') == 2:
            raise self.failure

    def emit_ready(self):
        self.event('upstream-ready')
        self.output.append(r.READY)

    def read_public_frame(self):
        self.event('input-eof')

    def forward_public(self):
        self.event('forward-fixed-public-and-eof')

    def native_terminal(self):
        self.event('native-terminal-eof-exit0')

    def cleanup(self):
        self.event('drain-echild')
        return self.cleaned

    def recheck(self):
        self.event('pins-source-recheck')
        return self.unchanged

    def final_guard(self):
        self.event('final-guard')

    def persist(self, report):
        self.event('record')
        self.persisted = dict(report)

    def emit_success(self):
        self.event('upstream-success')
        self.output.append(r.SUCCESS)

    def emit_error(self, code):
        self.event('upstream-error')
        self.output.append(b'REAL_ADMISSION_ERROR_V1 ' + code.encode('ascii') + b'\n')


class InputIO:
    """Only a memory queue and synthetic clock. No developer stdin inspection."""
    def __init__(self, chunks, fail_guard=None):
        self.chunks = list(chunks)
        self.clock = 0
        self.limits = []
        self.fail_guard = fail_guard
        self.ready_calls = 0

    def guard(self):
        if self.fail_guard:
            raise r.ClosedFailure(self.fail_guard)

    def now(self):
        return self.clock

    def input_ready(self, end):
        self.ready_calls += 1
        self.clock += 0.1
        return bool(self.chunks)

    def input_read(self, cap):
        self.limits.append(cap)
        chunk = self.chunks.pop(0)
        if isinstance(chunk, BaseException):
            raise chunk
        if len(chunk) > cap:
            self.chunks.insert(0, chunk[cap:])
            chunk = chunk[:cap]
        return chunk


class ProtocolTests(unittest.TestCase):
    def assert_closed(self, port, expected=None):
        self.assertEqual(r.receive_once(port), 1)
        self.assertNotIn(r.SUCCESS, port.output)
        self.assertTrue(port.output[-1].startswith(b'REAL_ADMISSION_ERROR_V1 '))
        self.assertNotIn(b'PRIVATE', b''.join(port.output))
        self.assertIn('drain-echild', port.events)
        if expected:
            self.assertEqual(port.output[-1], b'REAL_ADMISSION_ERROR_V1 ' + expected + b'\n')

    def test_exact_public_contract(self):
        self.assertEqual(len(r.PUBLIC_FRAME), 85)
        self.assertEqual(len(r.SUCCESS), 153)
        self.assertEqual(r.READY, b'REAL_ADMISSION_READY_V1\n')
        self.assertEqual(r.PUBLIC_FRAME[:8], b'RLA1\0\r\n\x1a')
        self.assertEqual(r.SUCCESS, b'REAL_ADMISSION_TERMINAL_V1 PUBLIC_FRAME_CHECKED primary_errors=0 cleanup_errors=0 sessions=0 healthy=false pcm_bytes=0 pcm_eof=false source_closed=false\n')

    def test_ready_two_external_proofs_and_terminal_order(self):
        port = FakePort()
        self.assertEqual(r.receive_once(port), 0)
        self.assertEqual(port.events, [
            'prepare', 'start', 'native-ready', 'observe', 'upstream-ready',
            'input-eof', 'observe', 'forward-fixed-public-and-eof',
            'native-terminal-eof-exit0', 'drain-echild', 'pins-source-recheck',
            'final-guard', 'record', 'final-guard', 'upstream-success'])
        self.assertEqual(port.output, [r.READY, r.SUCCESS])
        self.assertTrue(port.persisted['ok'])
        self.assertEqual(port.persisted['external_checks'], 2)
        self.assertEqual(port.persisted['public_frames_forwarded'], 1)
        self.assertFalse(port.persisted['healthy'])
        self.assertEqual(port.persisted['sessions'], 0)

    def test_no_ready_or_read_before_first_external_proof(self):
        for stage in ('prepare', 'start', 'native-ready', 'observe'):
            with self.subTest(stage=stage):
                port = FakePort(stage)
                self.assert_closed(port)
                self.assertNotIn(r.READY, port.output)
                self.assertNotIn('input-eof', port.events)
                self.assertNotIn('forward-fixed-public-and-eof', port.events)

    def test_second_proof_failure_never_forwards(self):
        port = FakePort('second-observe')
        self.assert_closed(port, b'NATIVE')
        self.assertEqual(port.output[0], r.READY)
        self.assertNotIn('forward-fixed-public-and-eof', port.events)

    def test_timeout_cancel_early_exit_and_broken_write(self):
        for stage, error in (('input-eof', 'TIMEOUT'), ('input-eof', 'CANCELLED'),
                             ('input-eof', 'NATIVE'), ('native-ready', 'NATIVE'),
                             ('forward-fixed-public-and-eof', 'NATIVE'),
                             ('native-terminal-eof-exit0', 'NATIVE')):
            with self.subTest(stage=stage, error=error):
                port = FakePort(stage, r.ClosedFailure(error))
                self.assert_closed(port, error.encode('ascii'))
                if stage == 'input-eof':
                    self.assertNotIn('forward-fixed-public-and-eof', port.events)

    def test_no_success_on_native_extra_eof_or_nonzero_failure(self):
        for failure in (BrokenPipeError('PRIVATE'), EOFError('PRIVATE'),
                        RuntimeError('exit1 PRIVATE')):
            self.assert_closed(FakePort('native-terminal-eof-exit0', failure), b'NATIVE')

    def test_cleanup_failure_always_blocks_success(self):
        port = FakePort()
        port.cleaned = False
        self.assert_closed(port, b'CLEANUP')
        self.assertFalse(port.persisted['ok'])
        self.assert_closed(FakePort('drain-echild'), b'CLEANUP')

    def test_postcleanup_source_or_pin_failure_blocks_success(self):
        port = FakePort()
        port.unchanged = False
        self.assert_closed(port, b'PINS')
        self.assert_closed(FakePort('pins-source-recheck'), b'PINS')

    def test_record_and_final_guard_failures_block_success(self):
        self.assert_closed(FakePort('record'), b'RECORD')
        self.assert_closed(FakePort('final-guard', r.ClosedFailure('CANCELLED')), b'CANCELLED')

    def test_closed_report_has_no_frame_source_or_process_metadata(self):
        port = FakePort()
        self.assertEqual(r.receive_once(port), 0)
        self.assertEqual(set(port.persisted), {
            'schema', 'public_only', 'ok', 'cleanup', 'source_unchanged',
            'pins_unchanged', 'external_checks', 'public_frames_forwarded',
            'sessions', 'healthy', 'service_calls', 'error'})
        self.assertNotIn('PRIVATE', str(port.persisted))


class BoundedInputTests(unittest.TestCase):
    def test_exact_frame_requires_eof_and_accepts_arbitrary_chunking(self):
        for chunks in ([r.PUBLIC_FRAME, b''],
                       [r.PUBLIC_FRAME[:1], r.PUBLIC_FRAME[1:84], r.PUBLIC_FRAME[84:], b''],
                       [bytes([item]) for item in r.PUBLIC_FRAME] + [b'']):
            io = InputIO(chunks)
            self.assertIsNone(r.bounded_public_input(io, 20))
            self.assertFalse(io.chunks)
            self.assertLessEqual(sum(io.limits), 86 * 86)
            self.assertEqual(io.limits[-1], 1)

    def test_full_frame_without_eof_times_out(self):
        io = InputIO([r.PUBLIC_FRAME])
        with self.assertRaises(r.ClosedFailure) as failure:
            r.bounded_public_input(io, 0.5)
        self.assertEqual(failure.exception.code, 'TIMEOUT')

    def test_empty_short_trailing_oversize_and_private_like_never_forward(self):
        cases = [b'', r.PUBLIC_FRAME[:-1], r.PUBLIC_FRAME + b'x',
                 r.PUBLIC_FRAME + b'x' * 100000, b'PRIVATE-LIKE-password',
                 b'RLA1\0\r\n\x1a\0\x03key', b'X' + r.PUBLIC_FRAME[1:], b'\xff']
        for raw in cases:
            with self.subTest(length=len(raw)):
                io = InputIO([raw, b''])
                port = FakePort()
                port.read_public_frame = lambda: r.bounded_public_input(io, 2)
                self.assertEqual(r.receive_once(port), 1)
                self.assertNotIn('forward-fixed-public-and-eof', port.events)
                self.assertNotIn(r.SUCCESS, port.output)
                self.assertNotIn(b'PRIVATE', b''.join(port.output))
                self.assertNotIn('PRIVATE', str(port.persisted))
                self.assertTrue(all(1 <= n <= 86 for n in io.limits))

    def test_every_truncation_is_rejected(self):
        for length in range(85):
            with self.subTest(length=length):
                io = InputIO([r.PUBLIC_FRAME[:length], b''])
                with self.assertRaises(r.ClosedFailure) as failure:
                    r.bounded_public_input(io, 20)
                self.assertEqual(failure.exception.code, 'INPUT')

    def test_each_single_byte_mutation_is_rejected(self):
        for index in range(85):
            altered = bytearray(r.PUBLIC_FRAME)
            altered[index] ^= 1
            with self.subTest(index=index):
                with self.assertRaises(r.ClosedFailure):
                    r.bounded_public_input(InputIO([bytes(altered), b'']), 20)

    def test_eagain_no_data_timeout_cancel_and_early_child_exit(self):
        io = InputIO([BlockingIOError(), r.PUBLIC_FRAME, b''])
        self.assertIsNone(r.bounded_public_input(io, 2))
        for code in ('CANCELLED', 'NATIVE'):
            with self.assertRaises(r.ClosedFailure) as failure:
                r.bounded_public_input(InputIO([r.PUBLIC_FRAME, b''], code), 2)
            self.assertEqual(failure.exception.code, code)
        with self.assertRaises(r.ClosedFailure) as failure:
            r.bounded_public_input(InputIO([]), 0.2)
        self.assertEqual(failure.exception.code, 'TIMEOUT')


class StartupTests(unittest.TestCase):
    def base(self):
        return ['--public-root', '/home/public/public-receiver', '--owner-sha256', 'a' * 64,
                '--receiver-sha256', 'b' * 64, '--helper', '/home/public/public-helper.py']

    def test_cli_closed_unique_no_mode_or_argument_overrides(self):
        self.assertEqual(r.arguments(self.base()).public_root, '/home/public/public-receiver')
        for extra in ('--single-session-v1', '--live', '--no-args', '--frame', '--endpoint',
                      '--binary-sha256', '--help', '--unit', '--observe-only'):
            with self.subTest(extra=extra), self.assertRaises(r.ClosedFailure):
                r.arguments(self.base() + [extra, 'public'])
        for values in (self.base()[:-2], self.base()[2:] + self.base()[:2] * 2,
                       self.base()[:6] + ['--receiver-sha256', 'b' * 64]):
            with self.assertRaises(r.ClosedFailure):
                r.arguments(values)

    def test_paths_and_hashes_are_not_guessed_or_expanded(self):
        for path in ('relative', '~/public', '//public', '/public/../x', '/public//x',
                     '/public/.env', '/public/.claude/x', '/public/keys/x',
                     '/public/private/x', 'C:/public', '/public/x\0', '/public/x\\y'):
            with self.subTest(path=path), self.assertRaises(r.ClosedFailure):
                r.public_path(path)
        bad = self.base()
        bad[3] = 'A' * 64
        with self.assertRaises(r.ClosedFailure):
            r.arguments(bad)

    def test_helper_bad_bytes_never_execute_or_fallback_import(self):
        # public_bytes performs the hash gate. Fake it as a rejection, so not a
        # single byte of the actual helper is imported or executed by this test.
        with patch.object(r, 'public_bytes', side_effect=r.ClosedFailure('STARTUP')) as read:
            with self.assertRaises(r.ClosedFailure):
                r.load_helper('/public/helper.py')
            read.assert_called_once_with('/public/helper.py', r.HELPER_SHA, 128 * 1024)

    def test_invalid_main_has_only_fixed_output_and_no_process_port(self):
        fakeos = types.SimpleNamespace(set_blocking=MagicMock(), write=MagicMock())
        with patch.object(r, 'os', fakeos), patch.object(r, 'LinuxPort') as port:
            self.assertEqual(r.main(['--live']), 1)
            port.assert_not_called()
        self.assertEqual(fakeos.write.call_args_list[0].args,
                         (1, b'REAL_ADMISSION_ERROR_V1 STARTUP\n'))
        self.assertEqual(fakeos.write.call_args_list[1].args,
                         (2, b'PUBLIC_PIPE_RECEIVER_FAILED_V1\n'))


class FakeKernelTests(unittest.TestCase):
    def make_start(self, child=False):
        events = []
        fakeos = types.SimpleNamespace(
            O_CLOEXEC=1, pipe2=MagicMock(side_effect=[(10, 11), (12, 13), (14, 15)]),
            set_blocking=MagicMock(), close=MagicMock(side_effect=lambda fd: events.append(('close', fd))),
            getpid=lambda: 7, fork=MagicMock(return_value=0 if child else 42),
            pidfd_open=MagicMock(side_effect=lambda pid, flag: events.append(('pidfd', pid)) or 17),
            write=MagicMock(side_effect=lambda fd, raw: events.append(('write', fd, raw)) or len(raw)),
            _exit=MagicMock(side_effect=SystemExit(126)))
        helper = MagicMock()
        helper.empty_children.return_value = True
        helper.open_path.return_value = 16
        helper.ownership.side_effect = lambda fd: events.append(('ownership', fd)) or None
        port = r.LinuxPort(types.SimpleNamespace())
        port.h, port.prepared = helper, True
        port.pcm, port.binary, port.cwd = '/public/pcm', '/public/example', '/public/cwd'
        port.final_guard, port.guard = MagicMock(), MagicMock()
        return port, helper, fakeos, events

    def test_fork_gate_pidfd_precedes_release_and_parent_only_nonblocking(self):
        port, helper, fakeos, events = self.make_start()
        with patch.object(r, 'os', fakeos), patch.object(r.time, 'monotonic', return_value=1):
            port.start()
        self.assertLess(events.index(('pidfd', 42)), events.index(('write', 15, b'G')))
        self.assertLess(events.index(('ownership', 17)), events.index(('write', 15, b'G')))
        self.assertEqual(fakeos.set_blocking.call_args_list[0].args, (16, True))
        self.assertEqual([call.args for call in fakeos.set_blocking.call_args_list[1:]],
                         [(12, False), (11, False)])
        self.assertEqual(port.handles, {11, 12})
        helper.target_exec.assert_not_called()

    def test_child_uses_reviewed_target_exec_fixed_false_never_mode_override(self):
        port, helper, fakeos, events = self.make_start(child=True)
        with patch.object(r, 'os', fakeos), self.assertRaises(SystemExit):
            port.start()
        helper.target_exec.assert_called_once_with('/public/example', '/public/cwd',
                                                   10, 13, 16, 14, 7, False)
        fakeos.pidfd_open.assert_not_called()

    def test_child_preexec_failure_exits_without_parent_protocol_fallback(self):
        port, helper, fakeos, events = self.make_start(child=True)
        fakeos.close.side_effect = OSError('PRIVATE preexec failure')
        with patch.object(r, 'os', fakeos), self.assertRaises(SystemExit):
            port.start()
        helper.target_exec.assert_not_called()
        fakeos._exit.assert_called_once_with(126)
        fakeos.pidfd_open.assert_not_called()

    def test_pidfd_acquisition_failure_does_not_release_gate(self):
        port, helper, fakeos, events = self.make_start()
        fakeos.pidfd_open.side_effect = OSError('PRIVATE kernel text')
        with patch.object(r, 'os', fakeos), self.assertRaises(OSError):
            port.start()
        self.assertTrue(port.launched)
        self.assertFalse(any(e[0] == 'write' for e in events))
        self.assertIn(15, port.handles)  # Cleanup closes gate EOF, then reviewed drain.

    def test_cleanup_drains_even_on_descriptor_close_error_and_closes_pidfd_last(self):
        port = r.LinuxPort(types.SimpleNamespace())
        helper = MagicMock()
        helper.drain.return_value = helper.empty_children.return_value = True
        port.h, port.initialized, port.pidfd = helper, True, 17
        port.handles = {10, 11}
        events = []
        def close(fd):
            events.append(('close', fd))
            if fd == 10:
                raise OSError('PRIVATE')
        helper.drain.side_effect = lambda: events.append(('drain',)) or True
        fakeos = types.SimpleNamespace(close=close)
        with patch.object(r, 'os', fakeos):
            self.assertFalse(port.cleanup())
        self.assertIn(('close', 11), events)
        self.assertLess(events.index(('drain',)), events.index(('close', 17)))
        helper.empty_children.assert_called_once()

    def test_external_checks_use_only_exact_reviewed_helper(self):
        port = r.LinuxPort(types.SimpleNamespace())
        port.h = MagicMock()
        port.pid, port.pidfd, port.read_out = 42, 17, 12
        port.guard = MagicMock()
        with patch.object(r.select, 'select', return_value=([], [], [])):
            port.observe()
        port.h.observe_before_write.assert_called_once_with(42, 17)
        self.assertEqual(port.guard.call_count, 2)

    def test_forward_has_no_payload_argument_only_constant_and_eof(self):
        port = r.LinuxPort(types.SimpleNamespace())
        port.guard = MagicMock()
        port.write_in, port.handles = 11, {11}
        events = []
        fakeos = types.SimpleNamespace(
            write=lambda fd, raw: events.append(('write', fd, raw)) or len(raw),
            close=lambda fd: events.append(('eof', fd)))
        with patch.object(r, 'os', fakeos):
            port.forward_public()
        self.assertEqual(events, [('write', 11, r.PUBLIC_FRAME), ('eof', 11)])
        self.assertFalse(port.handles)

    def test_exact_native_terminal_requires_eof_and_exit_zero(self):
        for payload, exit_status, accepted in (
                (r.SUCCESS, 0, True), (r.SUCCESS + b'PRIVATE', 0, False),
                (r.SUCCESS[:-1], 0, False), (r.SUCCESS, 1, False),
                (r.READY + r.SUCCESS, 0, False)):
            with self.subTest(exit_status=exit_status, accepted=accepted):
                remaining = [payload]
                def read(fd, cap):
                    chunk, remaining[0] = remaining[0][:cap], remaining[0][cap:]
                    return chunk
                port = r.LinuxPort(types.SimpleNamespace())
                port.h = MagicMock()
                port.h.ownership.return_value = types.SimpleNamespace(si_code=1, si_status=exit_status)
                port.read_out, port.pidfd, port.end, port.cwd = 12, 17, 10, '/public/cwd'
                port.wait, port.final_guard, port.now = MagicMock(return_value=True), MagicMock(), lambda: 0
                fakeos = types.SimpleNamespace(read=read, CLD_EXITED=1, listdir=lambda path: [])
                with patch.object(r, 'os', fakeos):
                    if accepted:
                        port.native_terminal()
                        port.h.policy.assert_called_once()
                    else:
                        with self.assertRaises(r.ClosedFailure):
                            port.native_terminal()

    def test_public_file_digest_mode_owner_links_and_stability_with_fake_io(self):
        raw = b'fixed-public-file'
        digest = r.hashlib.sha256(raw).hexdigest()
        def info(**overrides):
            values = dict(st_mode=r.stat.S_IFREG | 0o600, st_uid=7, st_nlink=1,
                          st_size=len(raw), st_dev=1, st_ino=20, st_mtime_ns=1, st_ctime_ns=1)
            values.update(overrides)
            return types.SimpleNamespace(**values)
        for before, after, pin, accepted in (
                (info(), info(), digest, True),
                (info(st_mode=r.stat.S_IFREG | 0o644), info(), digest, False),
                (info(st_uid=8), info(), digest, False),
                (info(st_nlink=2), info(), digest, False),
                (info(), info(st_ctime_ns=2), digest, False),
                (info(), info(), '0' * 64, False)):
            with self.subTest(accepted=accepted, pin_matches=(pin == digest)):
                fakeos = types.SimpleNamespace(getuid=lambda: 7,
                    fstat=MagicMock(side_effect=[before, after]),
                    read=MagicMock(side_effect=[raw, b'']), close=MagicMock())
                with patch.object(r, 'os', fakeos), patch.object(r, 'trusted_open', return_value=20):
                    if accepted:
                        self.assertEqual(r.public_bytes('/public/file', pin, 100), raw)
                    else:
                        with self.assertRaises(r.ClosedFailure):
                            r.public_bytes('/public/file', pin, 100)
                fakeos.close.assert_called_once_with(20)

    def test_result_record_is_empty_owned_single_link_and_same_held_inode(self):
        def info(**overrides):
            values = dict(st_mode=r.stat.S_IFREG | 0o600, st_uid=7, st_nlink=1,
                          st_size=0, st_dev=1, st_ino=20)
            values.update(overrides)
            return types.SimpleNamespace(**values)
        for path_info, held_info, accepted in (
                (info(), info(), True), (info(), info(st_ino=21), False),
                (info(st_mode=r.stat.S_IFREG | 0o644), info(), False),
                (info(st_uid=8), info(), False), (info(st_nlink=2), info(), False),
                (info(st_size=1), info(), False)):
            with self.subTest(accepted=accepted):
                port = r.LinuxPort(types.SimpleNamespace())
                port.owned, port.record_fd = '/public/case', 20
                fakeos = types.SimpleNamespace(getuid=lambda: 7,
                    fstat=MagicMock(side_effect=[path_info, held_info]), close=MagicMock())
                with patch.object(r, 'os', fakeos), patch.object(r, 'trusted_open', return_value=22):
                    if accepted:
                        port.check_record()
                    else:
                        with self.assertRaises(r.ClosedFailure) as failure:
                            port.check_record()
                        self.assertEqual(failure.exception.code, 'RECORD')
                fakeos.close.assert_called_once_with(22)

    def test_no_new_privs_dumpable_core_and_parent_guard_are_fail_closed(self):
        port = r.LinuxPort(types.SimpleNamespace())
        helper = MagicMock()
        helper.CANCELLED = False
        helper.resource.getrlimit.return_value = (0, 0)
        helper.LIBC.prctl.side_effect = lambda op, *args: {3: 0, 39: 1}[op]
        port.h, port.initialized, port.parent = helper, True, 7
        fakeos = types.SimpleNamespace(getppid=lambda: 7)
        with patch.object(r, 'os', fakeos):
            port.final_guard()
            helper.LIBC.prctl.side_effect = lambda op, *args: 0
            with self.assertRaises(r.ClosedFailure):
                port.final_guard()
            helper.CANCELLED = True
            with self.assertRaises(r.ClosedFailure) as failure:
                port.final_guard()
            self.assertEqual(failure.exception.code, 'CANCELLED')


if __name__ == '__main__':
    unittest.main()
