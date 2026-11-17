"""SOURCE ONLY: two authored native cases, NOT executed by the authoring step.

NativeBlockedPublicChannelTests is skipped on EVERY platform unless Windows AND
RAMBPC_TEST_OWNED_PIPE_BLOCKED_PUBLIC=1 at import. Future QA must select ONLY that
class inside the unchanged outer owned-Job helper with an explicit base Python.
No positive fixture rerun, fake backend, private channel entry or blocked callback.
"""
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import sys
import threading
import time
import unittest
from unittest import mock

from tools import windows_owned_pipe_job as channel
from tools.reference import owned_pipe_blocked_public_fixture as fixture

PUBLIC_PAYLOAD = b'P' * 12302
TERMINAL = b'OWNED_PIPE_BLOCKED_PUBLIC_UNUSED_TERMINAL_V1\n'
OPERATION_SECONDS = 8.0
NO_RETURN_SECONDS = 0.25
RETURN_ALLOWANCE_SECONDS = 2.0  # Scheduling allowance, NOT a latency target.
OBSERVER_JOIN_SECONDS = 2.0
PUBLIC_ENV = {'SystemRoot': 'C:\\Windows', 'PATH': 'C:\\Windows\\System32'}


def _native_blocked_public_enabled():
    return (os.name == 'nt' and
            os.getenv('RAMBPC_TEST_OWNED_PIPE_BLOCKED_PUBLIC') == '1')


class _WriteObservation:
    """Counts/events only; never retain a process, handle, payload or exception."""

    def __init__(self):
        self.lock = threading.Lock()
        self.produced = threading.Event()
        self.entered = threading.Event()
        self.finished = threading.Event()
        self.no_return_observed = threading.Event()
        self.stop = threading.Event()
        self.observer_failed = threading.Event()
        self.capacity_checked = False
        self.outbound_buffer_bytes = None
        self.inbound_buffer_bytes = None
        self.pipe_flags = None
        self.attempted_sizes = []
        self.completed_sizes = []
        self.write_exceptions = 0
        self.real_cancel_writer_calls = 0
        self.explicit_cancel_requested = False

    def check_capacity(self, backend, process):
        # Microsoft documents GetNamedPipeInfo for CreatePipe anonymous handles:
        # https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-getnamedpipeinfo
        # Query THIS real stdin write handle, before releasing even the gate.
        # Zero means allocated as needed, NOT a proven zero-capacity pipe.
        import msvcrt
        info = backend.kernel.GetNamedPipeInfo
        pointer = ctypes.POINTER(wintypes.DWORD)
        info.argtypes = [wintypes.HANDLE, pointer, pointer, pointer, pointer]
        info.restype = wintypes.BOOL
        flags, outbound, inbound = (wintypes.DWORD() for _ in range(3))
        handle = msvcrt.get_osfhandle(process.stdin.fileno())
        if not info(handle, ctypes.byref(flags), ctypes.byref(outbound),
                    ctypes.byref(inbound), None):
            raise OSError('public_pipe_capacity_unestablished')
        with self.lock:
            self.outbound_buffer_bytes = outbound.value
            self.inbound_buffer_bytes = inbound.value
            self.pipe_flags = flags.value
        if not 0 < outbound.value < len(PUBLIC_PAYLOAD):
            raise OSError('public_pipe_capacity_not_smaller_than_payload')
        self.capacity_checked = True

    def watch(self, cancel, explicit_cancel):
        try:
            # Stop is an owned wakeup: an early API failure cannot leave a thread
            # waiting the full operation budget for an entry that never occurred.
            end = time.monotonic() + OPERATION_SECONDS
            while not self.entered.is_set():
                if self.stop.wait(0.01) or time.monotonic() >= end:
                    return
            if self.stop.is_set():
                return
            # Independent bounded interval, not a delay inserted into the writer.
            # "finished" means original write RETURNED OR RAISED, not kernel
            # byte progress. Entry immediately precedes the real backend call.
            if not self.finished.wait(NO_RETURN_SECONDS) and not self.stop.is_set():
                self.no_return_observed.set()
                if explicit_cancel:
                    with self.lock:
                        self.explicit_cancel_requested = True
                    cancel.set()
        except BaseException:
            self.observer_failed.set()

    def record(self, case, result, observer_joined):
        with self.lock:
            return dict(
                case=case, result=result,
                capacity_established=self.capacity_checked,
                outbound_buffer_bytes=self.outbound_buffer_bytes,
                inbound_buffer_bytes=self.inbound_buffer_bytes,
                pipe_flags=self.pipe_flags,
                attempted_frame_write_sizes=list(self.attempted_sizes),
                attempted_frame_bytes=sum(self.attempted_sizes),
                known_completed_frame_write_sizes=list(self.completed_sizes),
                known_completed_frame_bytes=sum(self.completed_sizes),
                frame_write_exceptions=self.write_exceptions,
                frame_write_entered=self.entered.is_set(),
                frame_write_finished=self.finished.is_set(),
                no_return_interval_seconds=NO_RETURN_SECONDS,
                no_return_interval_observed=self.no_return_observed.is_set(),
                explicit_cancel_requested=self.explicit_cancel_requested,
                real_cancel_writer_calls=self.real_cancel_writer_calls,
                cancel_synchronous_io_invoked=self.real_cancel_writer_calls > 0,
                observer_joined=observer_joined,
                observer_failed=self.observer_failed.is_set())


class BlockedPublicSourceGuardTests(unittest.TestCase):
    def test_fixed_constants_and_windows_plus_exact_opt_in_guard(self):
        # Constants/guard only: no fixture main, native calls, threads or process.
        self.assertEqual(PUBLIC_PAYLOAD, b'P' * 12302)
        self.assertEqual(channel.MAX_INPUT, 12302)
        self.assertEqual(fixture.READY, b'OWNED_PIPE_BLOCKED_PUBLIC_READY_V1\n')
        self.assertTrue(0 < fixture.WAIT_SECONDS <= 30)
        for platform in ('nt', 'posix'):
            for flag in (None, '', '0', 'true', '1'):
                with self.subTest(platform=platform, flag=flag), \
                     mock.patch.object(os, 'name', platform), \
                     mock.patch.object(os, 'getenv', return_value=flag) as getenv:
                    self.assertEqual(_native_blocked_public_enabled(),
                                     platform == 'nt' and flag == '1')
                    if platform == 'nt':
                        getenv.assert_called_once_with(
                            'RAMBPC_TEST_OWNED_PIPE_BLOCKED_PUBLIC')
                    else:
                        getenv.assert_not_called()


@unittest.skipUnless(
    _native_blocked_public_enabled(),
    'Requires Windows and RAMBPC_TEST_OWNED_PIPE_BLOCKED_PUBLIC=1')
class NativeBlockedPublicChannelTests(unittest.TestCase):
    def _run_public_case(self, *, explicit_cancel):
        # No redirector fallback or monkeypatch of interpreter/platform guards.
        base = getattr(sys, '_base_executable', None)
        self.assertIs(type(base), str)
        self.assertTrue(os.path.isabs(base))
        self.assertTrue(os.path.isabs(sys.executable))
        self.assertEqual(os.path.normcase(os.path.abspath(base)),
                         os.path.normcase(os.path.abspath(sys.executable)))
        self.assertEqual(sys.prefix, sys.base_prefix)
        self.assertEqual(channel.MAX_INPUT, 12302)
        self.assertEqual(len(PUBLIC_PAYLOAD), channel.MAX_INPUT)
        root = Path(__file__).resolve().parents[1]
        argv = [os.path.abspath(base), '-I', '-S', '-B',
                str(root / 'tools' / 'reference' /
                    'owned_pipe_blocked_public_fixture.py')]
        observation, cancel = _WriteObservation(), threading.Event()
        producer_calls = []
        original_write = channel._WindowsBackend.write
        original_cancel = channel._WindowsBackend.cancel_writer

        def produce():
            producer_calls.append(True)
            observation.produced.set()
            return PUBLIC_PAYLOAD  # Fixed invented public bytes; no loader.

        def observed_write(backend, process, data):
            if not observation.capacity_checked:
                # Failure aborts gate release, fails the assertions below, and
                # NEVER substitutes a fake block, another pipe or a larger cap.
                observation.check_capacity(backend, process)
            if not observation.produced.is_set():
                return original_write(backend, process, data)  # Real gate write.
            with observation.lock:
                observation.attempted_sizes.append(len(data))
            observation.entered.set()
            try:
                count = original_write(backend, process, data)  # Real os.write.
                with observation.lock:
                    observation.completed_sizes.append(count)
                return count
            except BaseException:
                with observation.lock:
                    observation.write_exceptions += 1
                raise
            finally:
                observation.finished.set()

        def observed_cancel(backend, handle):
            with observation.lock:
                observation.real_cancel_writer_calls += 1
            # Pinned real method directly calls CancelSynchronousIo. Do not force
            # it to run: Job termination may already have unblocked the writer.
            return original_cancel(backend, handle)

        observer = threading.Thread(
            target=observation.watch, args=(cancel, explicit_cancel),
            name='owned-pipe-public-block-observer', daemon=True)
        case = 'explicit_cancel' if explicit_cancel else 'deadline'
        result = None
        started = False
        elapsed = None
        observer_joined = False
        try:
            # Narrow observation wrappers only. Construct the REAL backend in the
            # public API; do not replace it or enter private _run with a backend.
            with mock.patch.object(channel._WindowsBackend, 'write', observed_write), \
                 mock.patch.object(channel._WindowsBackend, 'cancel_writer', observed_cancel):
                observer.start()
                started = True
                start = time.monotonic()
                try:
                    result = channel.send_owned_windows(
                        public_argv=argv, public_cwd=str(root),
                        public_env=dict(PUBLIC_ENV), ready=fixture.READY,
                        terminal=TERMINAL, input_producer=produce,
                        timeout=OPERATION_SECONDS,
                        cancel_event=cancel if explicit_cancel else None)
                finally:
                    elapsed = time.monotonic() - start
        finally:
            observation.stop.set()
            cancel.set()  # Owned flag only; no PID kills or cross-thread closes.
            if started:
                observer.join(timeout=OBSERVER_JOIN_SECONDS)
            observer_joined = not observer.is_alive()
            record = observation.record(case, result, observer_joined)
            # Fixed/public structured evidence for the future outer QA capture.
            # No frame contents, argv/env, handles, PIDs or exception messages.
            print('OWNED_PIPE_BLOCKED_PUBLIC_QA ' +
                  json.dumps(record, sort_keys=True, separators=(',', ':')),
                  flush=True)

        self.assertTrue(observer_joined, record)
        self.assertFalse(record['observer_failed'], record)
        self.assertTrue(record['capacity_established'], record)
        self.assertTrue(0 < record['outbound_buffer_bytes'] < len(PUBLIC_PAYLOAD), record)
        self.assertTrue(record['frame_write_entered'], record)
        self.assertTrue(record['no_return_interval_observed'], record)
        self.assertTrue(record['frame_write_finished'], record)
        self.assertEqual(record['attempted_frame_write_sizes'], [12302], record)
        self.assertEqual(record['explicit_cancel_requested'], explicit_cancel, record)
        self.assertEqual(producer_calls, [True], record)
        self.assertIsNotNone(result, record)
        self.assertEqual(result['outcome'], 'cancelled' if explicit_cancel else 'timeout', record)
        self.assertFalse(result['ok'], record)
        self.assertTrue(result['ready'] and result['gate_released'], record)
        self.assertEqual(result['producer_calls'], 1, record)
        self.assertTrue(result['job_verified'] and result['job_empty'], record)
        self.assertTrue(result['workers_joined'] and result['handles_reaped'], record)
        self.assertTrue(result['cleanup'], record)
        self.assertFalse(result['stdin_eof'], record)
        self.assertFalse(result['terminal'], record)
        self.assertFalse(result['exit_zero'], record)
        self.assertEqual(result['output_bytes_observed'], len(fixture.READY), record)
        self.assertTrue(0 <= record['known_completed_frame_bytes'] < 12302, record)
        self.assertEqual(result['frame_bytes_written'],
                         record['known_completed_frame_bytes'], record)
        self.assertEqual(len(record['known_completed_frame_write_sizes']) +
                         record['frame_write_exceptions'], 1, record)
        self.assertLessEqual(elapsed, OPERATION_SECONDS + channel.CLEANUP_SECONDS +
                             RETURN_ALLOWANCE_SECONDS, record)
        # Neither output_eof nor CancelSynchronousIo invocation is required:
        # cleanup may close output unparsed, and Job kill may unblock first.
        # A zero completed count DOES NOT prove zero kernel bytes transferred.
        return record

    def test_blocked_public_write_deadline_drains_owned_job(self):
        self._run_public_case(explicit_cancel=False)

    def test_blocked_public_write_explicit_cancel_drains_owned_job(self):
        self._run_public_case(explicit_cancel=True)


if __name__ == '__main__':
    unittest.main()
