"""SOURCE-ONLY tests. Not executed/qualified by the authoring step.

FakeChannelTests/FixtureTests launch NO processes on any OS. NativeChannelTests
launch ONLY the original invented-public Python fixture, and require Windows AND
RAMBPC_TEST_OWNED_PIPE_PUBLIC_FIXTURE=1. No opt-in means no native launch, including
pytest/unittest discovery. Run native tests separately only after MAIN's review.
"""
import contextlib
import io
import json
import os
from pathlib import Path
import sys
import threading
import unittest
from unittest import mock

from tools import windows_owned_pipe_job as channel
from tools.reference import owned_pipe_fixture as fixture

READY = fixture.READY
TERMINAL = fixture.CHECKED
FRAME = fixture.PUBLIC_FRAME


class _FakeHandle:
    def __init__(self):
        self.closed = False

    def Close(self):
        self.closed = True


class _FakeInput:
    def __init__(self, backend):
        self.backend = backend
        self.closed = False

    def close(self):
        if not self.closed:
            self.closed = True
            self.backend.input_closed()


class _FakeOutput:
    def __init__(self):
        self.closed = False

    def close(self):
        self.closed = True


class _FakeProcess:
    def __init__(self, backend):
        self.backend = backend
        self.stdin, self.stdout = _FakeInput(backend), _FakeOutput()
        self._handle = _FakeHandle()
        self.code = None
        self.waited = self.killed = False

    def poll(self):
        return self.code

    def wait(self, timeout):
        if self.code is None:
            raise TimeoutError()
        self.waited = True
        return self.code

    def kill(self):
        self.killed = True
        self.code = 1
        self.backend.release_write.set()


class _FakeJob:
    def __init__(self, backend):
        self.backend = backend
        self.closed = self.assigned = self.terminated = False

    def assign(self, process):
        if self.backend.assignment_fails:
            raise OSError('invented-public-assignment-diagnostic')
        self.assigned = True

    def active(self):
        return int(self.assigned and self.backend.process.code is None)

    def terminate_and_wait(self, seconds):
        self.terminated = True
        if self.backend.cleanup_fails:
            raise OSError('invented-public-cleanup-diagnostic')
        self.backend.process.code = 1 if self.backend.process.code is None else self.backend.process.code
        self.backend.release_write.set()

    def close(self):
        self.closed = True
        self.backend.release_write.set()


class _FakeBackend:
    def __init__(self, *, prelude=READY, terminal=TERMINAL, early_eof=False,
                 block_write=False, partial=3, echo=False, no_ready=False,
                 assignment_fails=False, verified=True, cleanup_fails=False,
                 broken_write=False, bad_write_count=False, output_hangs=False):
        self.prelude, self.terminal = prelude, terminal
        self.early_eof, self.block_write = early_eof, block_write
        self.partial, self.echo, self.no_ready = partial, echo, no_ready
        self.assignment_fails, self.verified = assignment_fails, verified
        self.cleanup_fails = cleanup_fails
        self.broken_write, self.bad_write_count = broken_write, bad_write_count
        self.output_hangs = output_hangs
        self.lock = threading.Lock()
        self.output = bytearray()
        self.output_eof = False
        self.gate = bytearray()
        self.frame = bytearray()  # Invented public test data ONLY, never persisted.
        self.gated = False
        self.gate_completed = threading.Event()
        self.input_eof = threading.Event()
        self.ready_published = threading.Event()
        self.write_entered = threading.Event()
        self.release_write = threading.Event()
        self.cancel_calls = 0
        self.process = self.job = None
        self.spawn_count = 0
        self.verify_called = False
        self.writer_handle = _FakeHandle()

    def open_writer(self):
        return self.writer_handle

    def close_writer(self, handle):
        if handle is not self.writer_handle:
            raise AssertionError('unowned-writer-handle')
        handle.Close()

    def new_job(self):
        self.job = _FakeJob(self)
        return self.job

    def spawn(self, cwd, env):
        self.spawn_count += 1
        self.process = _FakeProcess(self)
        return self.process

    def verify(self, job, process):
        self.verify_called = True
        return self.verified and job.assigned

    def publish(self, raw, eof=False):
        with self.lock:
            self.output += raw
            self.output_eof = eof

    def read_byte(self, process):
        with self.lock:
            if self.output:
                value = bytes(self.output[:1])
                del self.output[:1]
                return value
            return b'' if self.output_eof else None

    def write(self, process, data):
        if not self.verify_called or not self.job.assigned or not self.verified:
            raise AssertionError('gate-before-verified-job')
        if not self.gated:
            count = min(self.partial, len(data))
            self.gate += data[:count]
            if self.gate.endswith(b'\n'):
                value = json.loads(self.gate)
                if set(value) != {'argv'} or value['argv'] != ['public-fixture']:
                    raise AssertionError('invalid-public-gate')
                self.gated = True
                self.gate_completed.set()
                if not self.no_ready:
                    self.publish(self.prelude, self.early_eof)
                    self.ready_published.set()
            return count
        self.write_entered.set()
        if self.block_write:
            self.release_write.wait()  # Fake owned-Job termination unblocks this.
            raise BrokenPipeError('invented-public-blocked-writer')
        if self.broken_write:
            raise BrokenPipeError('invented-public-broken-pipe')
        if self.bad_write_count:
            return 0
        count = min(self.partial, len(data))
        self.frame += data[:count]
        return count

    def input_closed(self):
        if self.gated and not self.no_ready and not self.early_eof:
            # Deliberately adversarial: even failed writes can elicit a terminal
            # on close. The published writer error must still win, fail closed.
            self.publish(bytes(self.frame) if self.echo else self.terminal,
                         not self.output_hangs)
            if not self.output_hangs:
                self.process.code = 0
        self.input_eof.set()

    def cancel_writer(self, handle):
        if handle is not self.writer_handle or handle.closed:
            raise AssertionError('unowned-or-closed-writer-handle')
        self.cancel_calls += 1
        self.release_write.set()
        return True


class FakeChannelTests(unittest.TestCase):
    def run_channel(self, backend=None, producer=None, timeout=2.0, cancel=None, **changes):
        backend = backend or _FakeBackend()
        values = dict(public_argv=['public-fixture'], public_cwd='public-cwd',
                      public_env={}, ready=READY, terminal=TERMINAL,
                      input_producer=producer or (lambda: FRAME), timeout=timeout,
                      cancel_event=cancel, backend=backend)
        values.update(changes)
        return channel._run(**values)

    def run_to_deadline_after_progress(self, backend, entered, producer=None):
        # Establish the blocked/stalled state explicitly, with a reasonable
        # operation budget, rather than assuming workers run within 50 ms.
        results = []
        runner = threading.Thread(target=lambda: results.append(
            self.run_channel(backend, producer, timeout=2)))
        runner.start()
        try:
            self.assertTrue(entered.wait(1.5), 'worker never reached tested state')
        finally:
            runner.join(timeout=5)
        self.assertFalse(runner.is_alive())
        self.assertEqual(len(results), 1)
        return results[0]

    def assert_safe(self, result):
        # Closed result keys, no argv/env/exception/raw output/frame/hash fields.
        self.assertEqual(set(result), set(channel._result()))
        for key, value in result.items():
            self.assertTrue(type(value) in (bool, int) or key == 'outcome')
        self.assertNotIn('invented-public', repr(result))
        self.assertNotIn('RLA1', repr(result))
        self.assertNotIn('diagnostic', repr(result))

    def test_exact_binary_partial_writes_and_mandatory_eof(self):
        backend = _FakeBackend(partial=1)
        calls = []
        def produce():
            self.assertTrue(backend.ready_published.is_set())
            self.assertTrue(backend.job.assigned and backend.verify_called)
            self.assertEqual(backend.frame, b'')
            self.assertFalse(backend.process.stdin.closed)
            calls.append(True)
            return FRAME
        result = self.run_channel(backend, produce)
        self.assertTrue(result['ok'], result)
        self.assertEqual(calls, [True])
        self.assertEqual(bytes(backend.frame), FRAME)
        self.assertEqual(result['frame_bytes_written'], 85)
        self.assertTrue(result['stdin_eof'] and result['output_eof'])
        self.assertTrue(result['cleanup'] and backend.process.waited)
        self.assertTrue(backend.job.closed and backend.process._handle.closed)
        self.assert_safe(result)

    def test_never_produce_without_ready(self):
        calls = []
        backend = _FakeBackend(no_ready=True)
        result = self.run_channel(backend, lambda: calls.append(True), timeout=0.03)
        self.assertEqual(result['outcome'], 'timeout')
        self.assertEqual(calls, [])
        self.assertEqual(result['producer_calls'], 0)
        self.assertTrue(result['cleanup'])

    def test_delayed_ready_then_producer_once(self):
        backend = _FakeBackend(no_ready=True)
        calls = []
        results = []
        runner = threading.Thread(target=lambda: results.append(
            self.run_channel(backend, lambda: calls.append(True) or FRAME, timeout=5)))
        runner.start()
        try:
            self.assertTrue(backend.gate_completed.wait(2))
            self.assertEqual(calls, [])
            backend.no_ready = False
            backend.ready_published.set()
            backend.publish(READY)
        finally:
            runner.join(timeout=2)
        self.assertFalse(runner.is_alive())
        self.assertTrue(results[0]['ok'], results)
        self.assertEqual(calls, [True])

    def test_pre_ready_junk_surplus_duplicate_and_early_eof(self):
        for prelude, eof in ((b'junk\n', False), (READY + b'X', False),
                             (READY + READY, False), (READY + TERMINAL, False),
                             (b'', True), (READY[:5], True), (READY, True),
                             (b'X' * 4096, False)):
            with self.subTest(size=len(prelude), eof=eof):
                calls = []
                result = self.run_channel(_FakeBackend(prelude=prelude, early_eof=eof),
                                          lambda: calls.append(True) or FRAME)
                self.assertFalse(result['ok'])
                self.assertEqual(calls, [])
                self.assertEqual(result['frame_bytes_written'], 0)
                self.assertTrue(result['cleanup'], result)
                self.assert_safe(result)

    def test_terminal_mismatch_surplus_duplicate_and_echo_never_returned(self):
        for terminal, echo in ((b'bad-terminal\n', False),
                               (TERMINAL + b'X', False),
                               (TERMINAL + TERMINAL, False),
                               (b'X' * 65536, False), (b'', True), (b'', False)):
            with self.subTest(size=len(terminal), echo=echo):
                stdout, stderr = io.StringIO(), io.StringIO()
                with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                    result = self.run_channel(_FakeBackend(terminal=terminal, echo=echo))
                self.assertFalse(result['ok'])
                self.assertEqual(result['producer_calls'], 1)
                self.assertEqual(stdout.getvalue(), '')
                self.assertEqual(stderr.getvalue(), '')
                self.assertLessEqual(result['output_bytes_observed'], len(READY) + len(TERMINAL) + 1)
                self.assert_safe(result)

    def test_failed_callback_discards_exception_no_replay(self):
        calls = []
        def failed():
            calls.append(True)
            raise ValueError('invented-public-frame-key diagnostic')
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            result = self.run_channel(producer=failed)
        self.assertEqual(result['outcome'], 'producer_failed')
        self.assertEqual(calls, [True])
        self.assertEqual(result['frame_bytes_written'], 0)
        self.assertTrue(result['cleanup'])
        self.assertEqual((stdout.getvalue(), stderr.getvalue()), ('', ''))
        self.assert_safe(result)

    def test_bounded_input_type_and_size(self):
        for value in (b'X' * (channel.MAX_INPUT + 1), bytearray(FRAME), 'public'):
            with self.subTest(kind=type(value).__name__):
                backend = _FakeBackend()
                result = self.run_channel(backend, lambda: value)
                self.assertEqual(result['outcome'], 'input_rejected')
                self.assertEqual(result['producer_calls'], 1)
                self.assertEqual(bytes(backend.frame), b'')
                self.assertTrue(result['cleanup'])
                self.assert_safe(result)
        for size in (0, channel.MAX_INPUT):
            backend = _FakeBackend(partial=2048)
            result = self.run_channel(backend, lambda: b'P' * size)
            self.assertTrue(result['ok'], result)
            self.assertEqual(result['frame_bytes_written'], size)
            self.assertTrue(result['stdin_eof'])

    def test_blocked_reader_write_deadline_joins_owned_workers(self):
        backend = _FakeBackend(block_write=True)
        result = self.run_to_deadline_after_progress(backend, backend.write_entered)
        self.assertTrue(backend.write_entered.is_set())
        self.assertEqual(result['outcome'], 'timeout')
        self.assertEqual(result['producer_calls'], 1)
        self.assertTrue(backend.job.terminated)
        self.assertTrue(result['workers_joined'] and result['cleanup'])
        self.assertEqual(result['frame_bytes_written'], 0)

    def test_blocked_producer_cancellation_no_late_input(self):
        backend = _FakeBackend()
        entered, release, cancel = threading.Event(), threading.Event(), threading.Event()
        finished = threading.Event()
        real_produce = channel._produce
        def observed_produce(*args):
            try:
                real_produce(*args)
            finally:
                finished.set()
        calls, results = [], []
        def produce():
            calls.append(True)
            entered.set()
            release.wait()
            return FRAME
        runner = threading.Thread(target=lambda: results.append(
            self.run_channel(backend, produce, timeout=5, cancel=cancel)))
        # Shorten grace only for this fake test; the worker is always released below.
        with mock.patch.object(channel, 'CLEANUP_SECONDS', 0.03), \
             mock.patch.object(channel, '_produce', observed_produce):
            runner.start()
            try:
                self.assertTrue(entered.wait(2))
                cancel.set()
                runner.join(timeout=3)
                self.assertFalse(runner.is_alive())
                self.assertEqual(results[0]['outcome'], 'cancelled')
                self.assertFalse(results[0]['cleanup'])
                self.assertFalse(results[0]['workers_joined'])
                self.assertEqual(results[0]['frame_bytes_written'], 0)
            finally:
                release.set()
                runner.join(timeout=3)
                self.assertTrue(finished.wait(2))
        self.assertEqual(calls, [True])
        self.assertEqual(bytes(backend.frame), b'')
        self.assert_safe(results[0])

    def test_blocked_producer_deadline_no_replay(self):
        entered, release, finished = threading.Event(), threading.Event(), threading.Event()
        real_produce = channel._produce
        def observed_produce(*args):
            try:
                real_produce(*args)
            finally:
                finished.set()
        backend, calls = _FakeBackend(), []
        def produce():
            calls.append(True)
            entered.set()
            release.wait()
            return FRAME
        try:
            with mock.patch.object(channel, 'CLEANUP_SECONDS', 0.02), \
                 mock.patch.object(channel, '_produce', observed_produce):
                result = self.run_to_deadline_after_progress(backend, entered, produce)
            self.assertTrue(entered.is_set())
            self.assertEqual(result['outcome'], 'timeout')
            self.assertFalse(result['cleanup'])
            self.assertEqual(result['frame_bytes_written'], 0)
        finally:
            release.set()
            self.assertTrue(finished.wait(2))
        self.assertEqual(calls, [True])
        self.assertEqual(bytes(backend.frame), b'')

    def test_junk_during_callback_prevents_send(self):
        backend = _FakeBackend()
        def produce():
            backend.publish(b'invented-public-echo\n')
            return FRAME
        result = self.run_channel(backend, produce)
        self.assertEqual(result['outcome'], 'protocol_failed')
        self.assertEqual(result['frame_bytes_written'], 0)
        self.assert_safe(result)

    def test_callback_publishes_junk_during_quiet_read_prevents_send(self):
        backend = _FakeBackend()
        entered, release, published = (threading.Event() for _ in range(3))
        real_read = backend.read_byte
        def produce():
            entered.set()
            if not release.wait(2):
                raise AssertionError('quiet-read barrier never released')
            backend.publish(b'invented-public-echo\n')
            published.set()
            return FRAME
        def read_during_callback(process):
            value = real_read(process)
            if value is None and entered.is_set() and not release.is_set():
                # The quiet observation predates callback output/publication.
                # Do not wait for producer_done here: the controller holds the
                # publication lock, but the callback may queue output itself.
                release.set()
                if not published.wait(2):
                    raise AssertionError('callback did not publish junk')
            return value
        try:
            with mock.patch.object(backend, 'read_byte', read_during_callback):
                result = self.run_channel(backend, produce, timeout=5)
        finally:
            release.set()
        self.assertTrue(published.is_set())
        self.assertEqual(result['outcome'], 'protocol_failed')
        self.assertEqual(result['producer_calls'], 1)
        self.assertEqual(result['frame_bytes_written'], 0)
        self.assertEqual(bytes(backend.frame), b'')
        self.assertTrue(result['cleanup'])
        self.assert_safe(result)

    def test_broken_pipe_and_zero_partial_count(self):
        for options in ({'broken_write': True}, {'bad_write_count': True}):
            with self.subTest(options=options):
                result = self.run_channel(_FakeBackend(**options))
                self.assertEqual(result['outcome'], 'write_failed')
                self.assertEqual(result['producer_calls'], 1)
                self.assertEqual(result['frame_bytes_written'], 0)
                self.assertFalse(result['stdin_eof'])
                self.assertFalse(result['terminal'])
                self.assertFalse(result['ok'])
                self.assertTrue(result['cleanup'])
                self.assert_safe(result)

    def test_assignment_and_membership_fail_closed_before_gate(self):
        for options in ({'assignment_fails': True}, {'verified': False}):
            with self.subTest(options=options):
                backend = _FakeBackend(**options)
                result = self.run_channel(backend)
                self.assertEqual(result['outcome'], 'containment_failed')
                self.assertEqual(backend.gate, b'')
                self.assertEqual(result['producer_calls'], 0)
                self.assertTrue(result['cleanup'])

    def test_cleanup_failure_cannot_be_ok(self):
        result = self.run_channel(_FakeBackend(cleanup_fails=True))
        self.assertFalse(result['cleanup'])
        self.assertFalse(result['ok'])
        self.assertEqual(result['outcome'], 'cleanup_failed')

    def test_interrupted_writer_join_still_closes_job_cleanup_unproved(self):
        backend, cancel = _FakeBackend(block_write=True), threading.Event()
        results, interrupted = [], []
        real_join = threading.Thread.join
        def terminate_without_releasing_writer(job, seconds):
            job.terminated = True
            job.backend.process.code = 1
        def interrupt_owned_join(worker, timeout=None):
            if worker.name == 'owned-pipe-writer':
                interrupted.append(worker)
                raise KeyboardInterrupt()
            return real_join(worker, timeout)
        runner = threading.Thread(target=lambda: results.append(
            self.run_channel(backend, timeout=5, cancel=cancel)))
        with mock.patch.object(_FakeJob, 'terminate_and_wait', terminate_without_releasing_writer), \
             mock.patch.object(backend, 'cancel_writer', return_value=True), \
             mock.patch.object(threading.Thread, 'join', interrupt_owned_join):
            runner.start()
            try:
                self.assertTrue(backend.write_entered.wait(2))
                cancel.set()
                real_join(runner, 3)
                self.assertFalse(runner.is_alive())
                self.assertTrue(backend.job.closed)
                self.assertTrue(interrupted, 'owned writer join never exercised')
                self.assertFalse(results[0]['cleanup'])
                self.assertFalse(results[0]['ok'])
            finally:
                cancel.set()
                backend.release_write.set()
                real_join(runner, 3)
                for worker in interrupted:
                    real_join(worker, 2)
                    self.assertFalse(worker.is_alive())
        self.assertTrue(backend.writer_handle.closed)
        self.assert_safe(results[0])

    def test_interrupted_producer_join_keeps_closed_job_cleanup_unproved(self):
        backend, interrupted = _FakeBackend(), []
        real_join = threading.Thread.join
        def interrupt_owned_join(worker, timeout=None):
            if worker.name == 'owned-pipe-producer':
                interrupted.append(worker)
                raise SystemExit()
            return real_join(worker, timeout)
        with mock.patch.object(threading.Thread, 'join', interrupt_owned_join):
            result = self.run_channel(backend)
        for worker in interrupted:
            real_join(worker, 2)
            self.assertFalse(worker.is_alive())
        self.assertTrue(interrupted, 'producer join never exercised')
        self.assertTrue(backend.job.closed)
        self.assertFalse(result['cleanup'])
        self.assertFalse(result['ok'])
        self.assertEqual(result['outcome'], 'cleanup_failed')
        self.assert_safe(result)

    def test_spawn_gate_and_reader_errors_are_fixed_categories(self):
        for method, expected in (('new_job', 'containment_failed'),
                                 ('spawn', 'spawn_failed'),
                                 ('write', 'gate_failed'),
                                 ('read_byte', 'protocol_failed')):
            with self.subTest(method=method):
                backend = _FakeBackend()
                with mock.patch.object(backend, method,
                                       side_effect=OSError('invented-public diagnostic')):
                    result = self.run_channel(backend)
                self.assertEqual(result['outcome'], expected)
                self.assertEqual(result['producer_calls'], 0)
                self.assertEqual(result['frame_bytes_written'], 0)
                if method in ('new_job', 'spawn'):
                    # Failed acquisition gives no native object proving hidden
                    # partial handles closed; require conservative failure.
                    self.assertFalse(result['cleanup'])
                    self.assertFalse(result['handles_reaped'])
                else:
                    self.assertTrue(result['cleanup'], result)
                self.assert_safe(result)

    def test_nonzero_target_exit_is_not_success(self):
        backend = _FakeBackend()
        original = backend.input_closed
        def close_with_nonzero_status():
            original()
            backend.process.code = 1
        backend.input_closed = close_with_nonzero_status
        result = self.run_channel(backend)
        self.assertEqual(result['outcome'], 'exit_failed')
        self.assertFalse(result['exit_zero'])
        self.assertFalse(result['ok'])
        self.assertTrue(result['cleanup'])

    def test_terminal_or_output_eof_stall_hits_deadline(self):
        for terminal in (b'', TERMINAL[:8], TERMINAL):
            with self.subTest(size=len(terminal)):
                backend = _FakeBackend(terminal=terminal, output_hangs=True)
                result = self.run_to_deadline_after_progress(backend, backend.input_eof)
                self.assertEqual(result['outcome'], 'timeout')
                self.assertEqual(result['producer_calls'], 1)
                self.assertEqual(result['frame_bytes_written'], 85)
                self.assertTrue(result['cleanup'])
                self.assert_safe(result)

    def test_cancellation_while_child_never_reads_joins_writer(self):
        backend, cancel = _FakeBackend(block_write=True), threading.Event()
        results = []
        runner = threading.Thread(target=lambda: results.append(
            self.run_channel(backend, timeout=5, cancel=cancel)))
        runner.start()
        try:
            self.assertTrue(backend.write_entered.wait(2))
            cancel.set()
        finally:
            cancel.set()
            runner.join(timeout=3)
        self.assertFalse(runner.is_alive())
        self.assertEqual(results[0]['outcome'], 'cancelled')
        self.assertEqual(results[0]['producer_calls'], 1)
        self.assertTrue(results[0]['workers_joined'] and results[0]['cleanup'])
        self.assertTrue(backend.writer_handle.closed)

    def test_precancel_never_spawns(self):
        backend, cancel = _FakeBackend(), threading.Event()
        cancel.set()
        result = self.run_channel(backend, cancel=cancel)
        self.assertEqual(result['outcome'], 'cancelled')
        self.assertEqual(result['producer_calls'], 0)
        self.assertEqual(backend.spawn_count, 0)

    def test_invalid_configuration_never_spawns_or_calls_producer(self):
        for changes in ({'timeout': 0}, {'timeout': float('nan')},
                        {'public_env': None}, {'public_argv': ['X' * 70000]},
                        {'ready': b'bad\r\n'}, {'terminal': b'no-newline'}):
            with self.subTest(keys=list(changes)):
                backend = _FakeBackend()
                result = self.run_channel(backend, **changes)
                self.assertEqual(result['outcome'], 'invalid_configuration')
                self.assertEqual(backend.spawn_count, 0)
                self.assertEqual(result['producer_calls'], 0)
                self.assert_safe(result)

    def test_public_entry_rejects_venv_before_backend_or_callback(self):
        base = os.path.abspath('invented-public-base-python.exe')
        redirector = os.path.abspath('invented-public-venv-python.exe')
        # Either mismatch independently suffices, even if the other pair agrees.
        for executable, prefix, base_prefix in ((redirector, 'base', 'base'),
                                                (base, 'venv', 'base'),
                                                (redirector, 'venv', 'base')):
            with self.subTest(executable=executable, prefix=prefix):
                produce = mock.Mock(return_value=FRAME)
                with mock.patch.multiple(channel.sys, executable=executable,
                                         _base_executable=base, prefix=prefix,
                                         base_prefix=base_prefix), \
                     mock.patch.object(channel.os, 'name', 'nt'), \
                     mock.patch.object(channel, '_WindowsBackend') as native, \
                     mock.patch.object(channel, '_run') as run:
                    result = channel.send_owned_windows(
                        public_argv=['public-fixture'], public_cwd='public-cwd',
                        public_env={}, ready=READY, terminal=TERMINAL,
                        input_producer=produce)
                native.assert_not_called()
                run.assert_not_called()
                produce.assert_not_called()
                self.assertEqual(result['outcome'], 'unsupported_interpreter')
                self.assertEqual(result['producer_calls'], 0)
                self.assertFalse(result['ok'])
                self.assert_safe(result)

    def test_public_entry_accepts_normalized_explicit_base_with_fake_only(self):
        base = os.path.abspath('invented-public-base-python.exe')
        equivalent = os.path.join(os.path.dirname(base), 'public-dir', '..',
                                  os.path.basename(base))
        backend = _FakeBackend()
        produce = mock.Mock(return_value=FRAME)
        with mock.patch.multiple(channel.sys, executable=equivalent,
                                 _base_executable=base, prefix='base',
                                 base_prefix='base'), \
             mock.patch.object(channel.os, 'name', 'nt'), \
             mock.patch.object(channel, '_WindowsBackend', return_value=backend), \
             mock.patch.object(channel.subprocess, 'Popen') as popen:
            result = channel.send_owned_windows(
                public_argv=['public-fixture'], public_cwd='public-cwd',
                public_env={}, ready=READY, terminal=TERMINAL,
                input_producer=produce, timeout=2)
        popen.assert_not_called()
        produce.assert_called_once_with()
        self.assertEqual(backend.spawn_count, 1)
        self.assertTrue(result['ok'], result)
        self.assert_safe(result)

    def test_backend_spawn_defensively_rejects_venv_without_popen(self):
        base = os.path.abspath('invented-public-base-python.exe')
        redirector = os.path.abspath('invented-public-venv-python.exe')
        backend = channel._WindowsBackend.__new__(channel._WindowsBackend)
        for executable, prefix in ((redirector, 'base'), (base, 'venv')):
            with self.subTest(prefix=prefix), \
                 mock.patch.multiple(channel.sys, executable=executable,
                                     _base_executable=base, prefix=prefix,
                                     base_prefix='base'), \
                 mock.patch.object(channel.subprocess, 'Popen') as popen:
                with self.assertRaises(channel._Block) as blocked:
                    backend.spawn('public-cwd', {})
                self.assertEqual(blocked.exception.args, ('unsupported_interpreter',))
                popen.assert_not_called()

    def test_backend_spawn_uses_explicit_base_no_path_fallback(self):
        base = os.path.abspath('invented-public-base-python.exe')
        backend = channel._WindowsBackend.__new__(channel._WindowsBackend)
        env = {'PUBLIC_FIXTURE': 'invented'}
        with mock.patch.multiple(channel.sys, executable=base,
                                 _base_executable=base, prefix='base',
                                 base_prefix='base'), \
             mock.patch.object(channel.subprocess, 'Popen') as popen:
            self.assertIs(backend.spawn('public-cwd', env), popen.return_value)
        popen.assert_called_once()
        argv = popen.call_args.args[0]
        self.assertEqual(argv, [base, '-I', '-S', '-B', '-c', channel.PIPE_BOOTSTRAP])
        self.assertEqual(popen.call_args.kwargs['cwd'], 'public-cwd')
        self.assertIs(popen.call_args.kwargs['env'], env)
        self.assertTrue(popen.call_args.kwargs['close_fds'])
        self.assertEqual(popen.call_args.kwargs['bufsize'], 0)

    def test_native_fixture_requires_windows_and_explicit_test_only_optin(self):
        for platform in ('nt', 'posix'):
            for flag in (None, '', '0', 'true', '1'):
                with self.subTest(platform=platform, flag=flag), \
                     mock.patch.object(os, 'name', platform), \
                     mock.patch.object(os, 'getenv', return_value=flag) as getenv:
                    enabled = _native_fixture_enabled()
                    self.assertEqual(enabled, platform == 'nt' and flag == '1')
                    if platform == 'nt':
                        getenv.assert_called_once_with('RAMBPC_TEST_OWNED_PIPE_PUBLIC_FIXTURE')
                    else:
                        getenv.assert_not_called()

    @unittest.skipIf(os.name == 'nt', 'Non-Windows import/platform guard only')
    def test_linux_import_and_public_entry_do_not_call_windll(self):
        with mock.patch.object(channel.ctypes, 'WinDLL', create=True,
                               side_effect=AssertionError('must-not-load')):
            result = channel.send_owned_windows(public_argv=['public-fixture'],
                        public_cwd='public-cwd', public_env={}, ready=READY,
                        terminal=TERMINAL, input_producer=lambda: FRAME)
        self.assertEqual(result['outcome'], 'unsupported_platform')


class FixtureTests(unittest.TestCase):
    def test_invented_tuple_is_exact_85_bytes(self):
        self.assertEqual(len(FRAME), 85)
        self.assertEqual(FRAME[:8], b'RLA1\0\r\n\x1a')
        offset = 8
        for part in fixture.PUBLIC_FIELDS:
            size = int.from_bytes(FRAME[offset:offset + 2], 'big')
            self.assertEqual(FRAME[offset + 2:offset + 2 + size], part)
            offset += 2 + size
        self.assertEqual(offset, 85)

    def fixture_run(self, chunks):
        output, eof = bytearray(), []
        def read(_fd, cap):
            value = chunks.pop(0)
            self.assertLessEqual(len(value), cap)
            if not value:
                eof.append(True)
            return value
        def write(_fd, raw):
            if bytes(output) == READY:
                self.assertTrue(eof, 'terminal emitted before EOF')
            output.extend(raw)
            return len(raw)
        with mock.patch.object(fixture.sys, 'argv', ['public-fixture']), \
             mock.patch.object(fixture.os, 'read', side_effect=read), \
             mock.patch.object(fixture.os, 'write', side_effect=write):
            status = fixture.main()
        return status, bytes(output)

    def test_full_compare_and_eof_before_fixed_terminal(self):
        status, output = self.fixture_run([FRAME[:17], FRAME[17:], b''])
        self.assertEqual((status, output), (0, READY + TERMINAL))
        for chunks in ([FRAME[:-1], b''], [FRAME + b'X', b''],
                       [b'X' * 86, b'Y' * 86, b''], [b'']):
            with self.subTest(count=len(chunks)):
                status, output = self.fixture_run(list(chunks))
                self.assertEqual((status, output), (1, READY + fixture.REJECTED))
                self.assertNotIn(FRAME, output)

    def test_withheld_eof_never_emits_terminal(self):
        class Withheld(Exception):
            pass
        output = bytearray()
        with mock.patch.object(fixture.sys, 'argv', ['public-fixture']), \
             mock.patch.object(fixture.os, 'read', side_effect=[FRAME, Withheld()]), \
             mock.patch.object(fixture.os, 'write', side_effect=lambda fd, raw: output.extend(raw) or len(raw)):
            with self.assertRaises(Withheld):
                fixture.main()
        self.assertEqual(bytes(output), READY)


def _native_fixture_enabled():
    # Test-only switch; never interpreted by the production channel API.
    return (os.name == 'nt' and
            os.getenv('RAMBPC_TEST_OWNED_PIPE_PUBLIC_FIXTURE') == '1')


@unittest.skipUnless(_native_fixture_enabled(),
                     'Requires Windows and RAMBPC_TEST_OWNED_PIPE_PUBLIC_FIXTURE=1')
class NativeChannelTests(unittest.TestCase):
    def test_original_public_python_fixture_exact_frame_and_eof(self):
        root = Path(__file__).resolve().parents[1]
        result = channel.send_owned_windows(
            public_argv=[sys.executable, '-I', '-S', '-B',
                         str(root / 'tools' / 'reference' / 'owned_pipe_fixture.py')],
            public_cwd=str(root),
            public_env={'SystemRoot': 'C:\\Windows', 'PATH': 'C:\\Windows\\System32'},
            ready=READY, terminal=TERMINAL, input_producer=lambda: FRAME, timeout=5)
        self.assertTrue(result['ok'], result)
        self.assertEqual(result['frame_bytes_written'], 85)
        self.assertEqual(result['producer_calls'], 1)
        self.assertTrue(result['job_verified'] and result['job_empty'])
        self.assertTrue(result['stdin_eof'] and result['output_eof'])
        self.assertTrue(result['workers_joined'] and result['handles_reaped'])


if __name__ == '__main__':
    unittest.main()
