"""Developer-only, unqualified Windows owned-Job stdin channel; no CLI.

No credential loader, SSH policy, persistent capture, or remote protection proof.
The caller MUST supply audited PUBLIC argv/cwd/env (including interpreter path),
trusted protocol constants and a trusted, preferably cooperative producer. Nothing
is inherited from os.environ. See docs/research/windows-owned-pipe-channel.md.
"""
from __future__ import annotations

import ctypes
from ctypes import wintypes
import math
import os
import subprocess
import sys
import threading
import time

from tools.windows_owned_job import ProcessJob, canonical

MAX_INPUT = 12302
MAX_GATE = 65536
CLEANUP_SECONDS = 2.0

# Unlike the immutable helper's EOF gate/DEVNULL target, this gate consumes ONLY
# a public JSON newline, one kernel byte at a time. No frame-byte prefetch.
PIPE_BOOTSTRAP = r'''
import json, os, subprocess
try:
    raw = bytearray()
    while True:
        b = os.read(0, 1)
        if not b or len(raw) >= 65536:
            os._exit(124)
        raw += b
        if b == b'\n':
            break
    value = json.loads(raw)
    if (type(value) is not dict or set(value) != {'argv'}
        or type(value['argv']) is not list or not 1 <= len(value['argv']) <= 128
        or any(type(x) is not str or '\x00' in x for x in value['argv'])
        or not value['argv'][0]):
        os._exit(124)
    child = subprocess.Popen(value['argv'], stdin=0, stdout=1, stderr=2,
                             close_fds=True)
    status = child.wait()
    if status > 0x7fffffff:
        status -= 0x100000000
    os._exit(status)
except BaseException:
    os._exit(124)
'''


class _Block(Exception):
    """Only a fixed category, never untrusted diagnostic text."""


def _line(value, cap):
    return (type(value) is bytes and 2 <= len(value) <= cap
            and value.endswith(b'\n')
            and all(32 <= x <= 126 for x in value[:-1]))


def _configuration(argv, cwd, env, ready, terminal, producer, timeout):
    # Structural checks cannot identify a secret hidden in an arbitrary string.
    # Public-only startup is an explicit caller obligation, not a detector claim.
    if not (type(argv) in (list, tuple) and 1 <= len(argv) <= 128
            and all(type(x) is str and '\0' not in x for x in argv) and argv[0]
            and type(cwd) is str and cwd and '\0' not in cwd
            and type(env) is dict
            and all(type(k) is str and k and '=' not in k and '\0' not in k
                    and type(v) is str and '\0' not in v for k, v in env.items())
            and _line(ready, 256) and _line(terminal, 512) and ready != terminal
            and callable(producer) and type(timeout) in (float, int)
            and math.isfinite(timeout) and 0 < timeout <= 300):
        raise _Block('invalid_configuration')
    argv, env = list(argv), dict(env)
    gate = canonical({'argv': argv}) + b'\n'
    if len(gate) > MAX_GATE:
        raise _Block('invalid_configuration')
    return argv, env, gate


def _base_interpreter():
    """Reject venv redirectors; never substitute another interpreter silently."""
    executable = sys.executable
    base = getattr(sys, '_base_executable', None)
    if (type(executable) is not str or not executable
            or type(base) is not str or not base
            or not os.path.isabs(executable) or not os.path.isabs(base)
            or os.path.normcase(os.path.abspath(executable)) !=
               os.path.normcase(os.path.abspath(base))
            or sys.prefix != sys.base_prefix):
        raise _Block('unsupported_interpreter')
    return os.path.abspath(executable)


class _WindowsBackend:
    """Lazy Win32 operations; importing this module on Linux calls no WinDLL."""

    def __init__(self):
        self.kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        signatures = {
            'IsProcessInJob': ([wintypes.HANDLE, wintypes.HANDLE,
                                ctypes.POINTER(wintypes.BOOL)], wintypes.BOOL),
            'PeekNamedPipe': ([wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                               ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD),
                               ctypes.c_void_p], wintypes.BOOL),
            'OpenThread': ([wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.HANDLE),
            'CancelSynchronousIo': ([wintypes.HANDLE], wintypes.BOOL),
            'CloseHandle': ([wintypes.HANDLE], wintypes.BOOL),
        }
        for name, (args, result) in signatures.items():
            fn = getattr(self.kernel, name)
            fn.argtypes, fn.restype = args, result

    def new_job(self):
        return ProcessJob()  # Reuse unchanged, including kill-on-close policy.

    def spawn(self, cwd, env):
        # Defense in depth for direct/private backend use and changed sys fields.
        executable = _base_interpreter()
        return subprocess.Popen(
            [executable, '-I', '-S', '-B', '-c', PIPE_BOOTSTRAP],
            cwd=cwd, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, close_fds=True, bufsize=0)

    def verify(self, job, process):
        member = wintypes.BOOL()
        return bool(self.kernel.IsProcessInJob(int(process._handle), job.handle,
                                               ctypes.byref(member)) and member.value)

    def read_byte(self, process):
        # One reader only; peek then a <= available raw read, never a buffered
        # stream lock or a blocking reader worker. Merged stderr is equally strict.
        import msvcrt
        available = wintypes.DWORD()
        handle = msvcrt.get_osfhandle(process.stdout.fileno())
        if not self.kernel.PeekNamedPipe(handle, None, 0, None,
                                         ctypes.byref(available), None):
            if ctypes.get_last_error() in (109, 232, 233):
                return b''
            raise OSError()
        return os.read(process.stdout.fileno(), 1) if available.value else None

    def write(self, process, data):
        return os.write(process.stdin.fileno(), data)

    def open_writer(self):
        # Called INSIDE the live writer itself: no recycled thread-ID lookup from
        # the controller. Retain this owned HANDLE until the worker's finally.
        handle = self.kernel.OpenThread(0x0001, False, threading.get_native_id())
        if not handle:
            raise OSError()
        return handle

    def close_writer(self, handle):
        if not self.kernel.CloseHandle(handle):
            raise OSError()

    def cancel_writer(self, handle):
        # Job termination closes owned readers first. Only the retained owned
        # writer HANDLE is cancelled; repeat during join for syscall-entry races.
        ok = self.kernel.CancelSynchronousIo(handle)
        return bool(ok or ctypes.get_last_error() == 1168)


class _State:
    def __init__(self):
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.send = threading.Event()
        self.frame = None
        self.producer_done = False
        self.producer_calls = 0
        self.producer_error = None
        self.writer_error = None
        self.gate_done = False
        self.eof = False
        self.written = 0
        self.writer_handle = None
        self.writer_handle_error = False
        self.deadline = 0.0
        self.cancel_event = None

    def cancelled(self):
        return (self.stop.is_set() or time.monotonic() >= self.deadline
                or (self.cancel_event is not None and self.cancel_event.is_set()))


def _produce(state, producer):
    data = None
    try:
        # Only the controller starts this worker, after exact READY + quiet peek.
        with state.lock:
            if state.cancelled():
                return
            state.producer_calls += 1
        data = producer()
        with state.lock:
            if not state.cancelled():
                if type(data) is not bytes or len(data) > MAX_INPUT:
                    state.producer_error = 'input_rejected'
                else:
                    state.frame = data
    except BaseException:
        with state.lock:
            if not state.stop.is_set():
                state.producer_error = 'producer_failed'
    finally:
        data = producer = None  # Best effort reference release, NOT secure erasure.
        with state.lock:
            state.producer_done = True


def _write_all(state, process, backend, data, frame=False):
    view = memoryview(data)
    offset = 0
    try:
        while offset < len(view):
            if state.cancelled():
                raise _Block('cancelled')
            count = backend.write(process, view[offset:])
            if type(count) is not int or not 0 < count <= len(view) - offset:
                raise _Block('write_failed')
            offset += count
            if frame:
                with state.lock:
                    state.written += count
    finally:
        view.release()


def _writer(state, process, backend, gate):
    data = None
    category = 'gate_failed'
    try:
        handle = backend.open_writer()
        with state.lock:
            state.writer_handle = handle
        _write_all(state, process, backend, gate)
        gate = None
        with state.lock:
            state.gate_done = True
        while not state.send.wait(0.01):
            if state.cancelled():
                return
        if state.cancelled():
            return
        with state.lock:
            data, state.frame = state.frame, None
        category = 'write_failed'
        _write_all(state, process, backend, data, frame=True)
        # Raw FileIO only. EOF and publication are serialized with the output
        # parser; there is no buffered close/flush waiting for a writer lock.
        with state.lock:
            process.stdin.close()
            state.eof = True
    except BaseException:
        with state.lock:
            if not state.stop.is_set():
                state.writer_error = category
    finally:
        data = gate = None
        try:
            process.stdin.close()  # Sole stdin owner after worker starts.
        except BaseException:
            with state.lock:
                state.writer_error = category
        with state.lock:
            if state.writer_handle is not None:
                try:
                    backend.close_writer(state.writer_handle)
                    state.writer_handle = None
                except BaseException:
                    state.writer_handle_error = True


def _result():
    return dict(outcome='internal_failed', ok=False, ready=False, terminal=False,
                producer_calls=0, frame_bytes_written=0, output_bytes_observed=0,
                stdin_eof=False, output_eof=False, gate_released=False,
                job_verified=False, job_empty=False, exit_zero=False,
                workers_joined=False, handles_reaped=False, cleanup=False)


def _run(*, public_argv, public_cwd, public_env, ready, terminal,
         input_producer, timeout, cancel_event, backend):
    """Private injection seam for fake-only QA; backend must be trusted."""
    result, state = _result(), _State()
    process = job = writer = producer = None
    assigned = False
    resources_uncertain = False
    process_reaped = False
    ready_pos = terminal_pos = 0
    stage = 'invalid_configuration'
    start = time.monotonic()
    try:
        argv, env, gate = _configuration(public_argv, public_cwd, public_env,
                                          ready, terminal, input_producer, timeout)
        end = start + timeout
        state.deadline, state.cancel_event = end, cancel_event
        if cancel_event is not None and cancel_event.is_set():
            raise _Block('cancelled')
        stage = 'containment_failed'
        job = backend.new_job()
        stage = 'spawn_failed'
        process = backend.spawn(public_cwd, env)
        stage = 'containment_failed'
        job.assign(process)
        assigned = True
        if not backend.verify(job, process):
            raise _Block('containment_failed')
        result['job_verified'] = True
        writer = threading.Thread(target=_writer, args=(state, process, backend, gate),
                                  name='owned-pipe-writer', daemon=True)
        writer.start()  # FIRST possible gate byte; never before Job verification.
        stage = 'protocol_failed'
        while True:
            if cancel_event is not None and cancel_event.is_set():
                raise _Block('cancelled')
            if time.monotonic() >= end:
                raise _Block('timeout')
            # Snapshot completion BEFORE the quiet drain that can authorize send.
            # Callback completion during this drain requires a fresh quiet pass.
            with state.lock:
                error = state.writer_error or state.producer_error
                produced = state.producer_done
            if error:
                raise _Block(error)
            # Drain all currently observable output before starting the producer.
            # Compare in place against pinned constants; retain no output buffer.
            while not result['output_eof']:
                # Serialize a nonblocking observation with error/EOF publication.
                # A failed write publishes its error BEFORE close can elicit an
                # early terminal. Do not reclassify that causal output as protocol.
                with state.lock:
                    error = state.writer_error or state.producer_error
                    if error:
                        raise _Block(error)
                    byte = backend.read_byte(process)
                    eof = state.eof
                if byte is None:
                    break
                if byte == b'':
                    result['output_eof'] = True
                    if not result['terminal']:
                        raise _Block('early_eof')
                    break
                result['output_bytes_observed'] += len(byte)
                if type(byte) is not bytes or len(byte) != 1:
                    raise _Block('protocol_failed')
                if not result['ready']:
                    if byte != ready[ready_pos:ready_pos + 1]:
                        raise _Block('protocol_failed')
                    ready_pos += 1
                    result['ready'] = ready_pos == len(ready)
                else:
                    if not eof or result['terminal'] or byte != terminal[terminal_pos:terminal_pos + 1]:
                        raise _Block('protocol_failed')
                    terminal_pos += 1
                    result['terminal'] = terminal_pos == len(terminal)
                byte = None
            with state.lock:
                error = state.writer_error or state.producer_error
                gate_done = state.gate_done
            if error:
                raise _Block(error)
            if result['ready'] and producer is None:
                # A second peek immediately before callback dispatch catches queued
                # junk/EOF. Output arriving AFTER this check is a residual race,
                # monitored while the callback runs, not an atomic remote proof.
                surplus = backend.read_byte(process)
                if surplus is not None:
                    result['output_eof'] = surplus == b''
                    result['output_bytes_observed'] += len(surplus)
                    surplus = None
                    raise _Block('protocol_failed')
                if not gate_done:
                    time.sleep(0.002)
                    continue
                if time.monotonic() >= end:
                    raise _Block('timeout')
                if cancel_event is not None and cancel_event.is_set():
                    raise _Block('cancelled')
                producer = threading.Thread(target=_produce, args=(state, input_producer),
                                            name='owned-pipe-producer', daemon=True)
                producer.start()
                input_producer = None
            if produced and not state.send.is_set():
                # The output drain above must still be quiet before enabling input.
                state.send.set()
            code = process.poll()
            if result['output_eof'] and code is not None:
                with state.lock:
                    complete = state.eof and state.writer_error is None
                result['exit_zero'] = code == 0
                if not complete or code != 0:
                    raise _Block('exit_failed')
                result['outcome'] = 'ok'
                break
            time.sleep(0.002)
    except _Block as block:
        result['outcome'] = block.args[0]
    except BaseException:
        result['outcome'] = stage
        # A constructor/Popen exception supplies no object with which to prove
        # that an internal partially acquired native handle has been reaped.
        resources_uncertain = process is None and stage in ('containment_failed', 'spawn_failed')
    finally:
        # Never hold a state/stream lock while terminating, waiting or joining.
        state.stop.set()
        state.send.set()
        input_producer = None
        cleanup_end = time.monotonic() + CLEANUP_SECONDS
        clean = not resources_uncertain
        if job is not None and assigned:
            try:
                job.terminate_and_wait(seconds=max(0, cleanup_end - time.monotonic()))
                result['job_empty'] = job.active() == 0
            except BaseException:
                clean = False
        elif process is None:
            result['job_empty'] = not resources_uncertain
        try:
            if process is not None:
                try:
                    if not assigned and process.poll() is None:
                        # Only this owned bootstrap exists before gate release.
                        process.kill()
                    process.wait(timeout=max(0, cleanup_end - time.monotonic()))
                    process_reaped = True
                except BaseException:
                    clean = False
                # No close-from-another-thread buffer lock. An interrupted join
                # marks cleanup unproved, but MUST NOT skip the Job close below.
                while writer is not None and writer.is_alive() and time.monotonic() < cleanup_end:
                    try:
                        with state.lock:
                            if state.writer_handle is not None:
                                clean = backend.cancel_writer(state.writer_handle) and clean
                        writer.join(timeout=min(0.01, max(0, cleanup_end - time.monotonic())))
                    except BaseException:
                        clean = False
                        break
                if writer is None or not writer.is_alive():
                    try:
                        process.stdin.close()
                    except BaseException:
                        clean = False
                else:
                    clean = False
                try:
                    process.stdout.close()
                    process._handle.Close()
                    result['handles_reaped'] = process_reaped and (writer is None or not writer.is_alive())
                except BaseException:
                    clean = False
            else:
                result['handles_reaped'] = not resources_uncertain
        except BaseException:
            clean = False
            result['handles_reaped'] = False
        finally:
            if job is not None:
                try:
                    job.close()
                except BaseException:
                    clean = False
                    result['handles_reaped'] = False
        if producer is not None and producer.ident is not None:
            try:
                producer.join(timeout=max(0, cleanup_end - time.monotonic()))
            except BaseException:
                clean = False
        result['workers_joined'] = all(w is None or not w.is_alive() for w in (writer, producer))
        with state.lock:
            if state.writer_handle_error or state.writer_handle is not None:
                clean = False
                result['handles_reaped'] = False
            state.frame = None
            result['frame_bytes_written'] = state.written
            result['producer_calls'] = state.producer_calls
            result['stdin_eof'] = state.eof
            result['gate_released'] = state.gate_done
        # An unassigned, reaped bootstrap never created a target; no Job member.
        if not assigned and process is not None and result['handles_reaped']:
            result['job_empty'] = True
        result['cleanup'] = bool(clean and result['job_empty'] and
                                 result['workers_joined'] and result['handles_reaped'])
        if result['outcome'] == 'ok' and not result['cleanup']:
            result['outcome'] = 'cleanup_failed'
        result['ok'] = result['outcome'] == 'ok' and result['cleanup']
    return result


def send_owned_windows(*, public_argv, public_cwd, public_env, ready, terminal,
                       input_producer, timeout=10.0, cancel_event=None):
    """One invocation, one Job, at most ONE producer call; never retries/replays.

    input_producer() -> exact bytes, length 0..12302. Called on a daemon worker
    ONLY after exact configured READY and no observable surplus/EOF. Deadline
    includes bootstrap, callback, write, terminal and output EOF. A non-returning
    callback cannot be killed safely: bounded return, cleanup=False, no late send.
    Result contains fixed categories/counts/booleans ONLY. No output is returned.
    Windows production entry rejects venv/redirector interpreter properties before
    any backend/Job acquisition; launch this API with an explicit base interpreter.
    This developer API does not authorize a real channel or qualify remote Linux.
    """
    if os.name != 'nt':
        result = _result()
        result['outcome'] = 'unsupported_platform'
        return result
    try:
        _base_interpreter()  # Before WinDLL, Job acquisition, process or producer.
    except _Block:
        result = _result()
        result['outcome'] = 'unsupported_interpreter'
        return result
    try:
        backend = _WindowsBackend()
    except BaseException:
        result = _result()
        result['outcome'] = 'containment_failed'
        return result
    return _run(public_argv=public_argv, public_cwd=public_cwd,
                public_env=public_env, ready=ready, terminal=terminal,
                input_producer=input_producer, timeout=timeout,
                cancel_event=cancel_event, backend=backend)
