"""Gated, bounded capture of owned Windows child processes for repository CI.

The generic capture definitions are reused unchanged from the reviewed native
qualification helper. No Cargo configuration, credentials, private paths or
source-snapshot machinery is embedded here. Callers supply explicit owned paths.
"""
from __future__ import annotations

import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=True,
                      separators=(',', ':')).encode('ascii')


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


BOOTSTRAP = r'''
import json, subprocess, sys
raw = sys.stdin.buffer.read(1048577)
if len(raw) > 1048576:
    raise SystemExit(124)
payload = json.loads(raw)
if (not isinstance(payload, dict) or set(payload) != {'argv'}
    or not isinstance(payload['argv'], list) or not payload['argv']
    or any(not isinstance(x, str) or '\x00' in x for x in payload['argv'])
    or not payload['argv'][0]):
    raise SystemExit(124)
proc = subprocess.Popen(payload['argv'], stdin=subprocess.DEVNULL)
status = proc.wait()
# CRT exit takes signed int; preserve all DWORD status bits, including crashes.
if status > 0x7fffffff:
    status -= 0x100000000
raise SystemExit(status)
'''


class ProcessJob:
    """A new unnamed, non-inheritable job; never assigns the caller or other PIDs."""
    def __init__(self):
        class Basic(ctypes.Structure):
            _fields_ = [('PerProcessUserTimeLimit', ctypes.c_longlong),
                        ('PerJobUserTimeLimit', ctypes.c_longlong), ('LimitFlags', wintypes.DWORD),
                        ('MinimumWorkingSetSize', ctypes.c_size_t), ('MaximumWorkingSetSize', ctypes.c_size_t),
                        ('ActiveProcessLimit', wintypes.DWORD), ('Affinity', ctypes.c_size_t),
                        ('PriorityClass', wintypes.DWORD), ('SchedulingClass', wintypes.DWORD)]
        class IO(ctypes.Structure):
            _fields_ = [(n, ctypes.c_ulonglong) for n in ('ReadOperationCount', 'WriteOperationCount',
                'OtherOperationCount', 'ReadTransferCount', 'WriteTransferCount', 'OtherTransferCount')]
        class Extended(ctypes.Structure):
            _fields_ = [('BasicLimitInformation', Basic), ('IoInfo', IO),
                        ('ProcessMemoryLimit', ctypes.c_size_t), ('JobMemoryLimit', ctypes.c_size_t),
                        ('PeakProcessMemoryUsed', ctypes.c_size_t), ('PeakJobMemoryUsed', ctypes.c_size_t)]
        class Accounting(ctypes.Structure):
            _fields_ = [(n, ctypes.c_longlong) for n in ('TotalUserTime', 'TotalKernelTime',
                        'ThisPeriodTotalUserTime', 'ThisPeriodTotalKernelTime')] + [
                        (n, wintypes.DWORD) for n in ('TotalPageFaultCount', 'TotalProcesses',
                        'ActiveProcesses', 'TotalTerminatedProcesses')]
        self.Accounting = Accounting
        self.kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        signatures = {
            'CreateJobObjectW': ([ctypes.c_void_p, wintypes.LPCWSTR], wintypes.HANDLE),
            'SetInformationJobObject': ([wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD], wintypes.BOOL),
            'QueryInformationJobObject': ([wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p], wintypes.BOOL),
            'AssignProcessToJobObject': ([wintypes.HANDLE, wintypes.HANDLE], wintypes.BOOL),
            'TerminateJobObject': ([wintypes.HANDLE, wintypes.UINT], wintypes.BOOL),
            'CloseHandle': ([wintypes.HANDLE], wintypes.BOOL)}
        for name, (args, result) in signatures.items():
            fn = getattr(self.kernel, name)
            fn.argtypes, fn.restype = args, result
        self.handle = self.kernel.CreateJobObjectW(None, None)
        if not self.handle:
            raise OSError('job_create_failed')
        info = Extended()
        info.BasicLimitInformation.LimitFlags = 0x2000
        if not self.kernel.SetInformationJobObject(self.handle, 9, ctypes.byref(info), ctypes.sizeof(info)):
            self.close()
            raise OSError('job_configuration_failed')

    def assign(self, process):
        if not self.kernel.AssignProcessToJobObject(self.handle, int(process._handle)):
            raise OSError('job_assignment_failed')

    def active(self):
        info = self.Accounting()
        if not self.kernel.QueryInformationJobObject(self.handle, 1, ctypes.byref(info), ctypes.sizeof(info), None):
            raise OSError('job_query_failed')
        return info.ActiveProcesses

    def terminate_and_wait(self, seconds=5):
        if not self.kernel.TerminateJobObject(self.handle, 1):
            raise OSError('job_termination_failed')
        deadline = time.monotonic() + seconds
        while self.active():
            if time.monotonic() >= deadline:
                raise OSError('job_cleanup_timeout')
            time.sleep(0.01)

    def close(self):
        if self.handle:
            handle, self.handle = self.handle, None
            if not self.kernel.CloseHandle(handle):
                raise OSError('job_close_failed')


def capture_windows(argv, log, *, timeout, limit, cwd, env, job_factory=ProcessJob):
    """Direct private log; sampled output/time bounds, capped retained bytes.

    Output may overshoot between 20ms samples; truncate only AFTER job cleanup.
    No pipe reader thread, process inventory, PID-based kill, or breakaway flag.
    """
    start = time.monotonic()
    argv = [str(x) for x in argv]
    payload = canonical({'argv': argv})
    if not argv or not argv[0] or any('\0' in x for x in argv) or len(payload) > 1048576:
        raise ValueError('invalid_gate_payload')
    result = dict(actual_exit=None, timeout=False, output_limit=False, spawn_error=False,
                  capture_error=False, containment_error=False, cleanup_error=False,
                  gate_released=False, job_empty=False, command=argv)
    process = job = None
    assigned = False
    stage = 'spawn'
    with Path(log).open('xb', buffering=0) as output:
        try:
            if os.name != 'nt':
                raise OSError('windows_required')
            stage = 'containment'
            job = job_factory()
            stage = 'spawn'
            process = subprocess.Popen([sys.executable, '-I', '-S', '-B', '-c', BOOTSTRAP],
                cwd=cwd, env=env, stdin=subprocess.PIPE, stdout=output,
                stderr=subprocess.STDOUT, close_fds=True)
            stage = 'containment'
            job.assign(process)
            assigned = True
            stage = 'capture'
            # A single small bounded payload; EOF is the only execution gate.
            process.stdin.write(payload)
            process.stdin.close()
            result['gate_released'] = True
            deadline = start + timeout
            while True:
                if os.fstat(output.fileno()).st_size > limit:
                    result['output_limit'] = True
                    break
                if process.poll() is not None:
                    break
                if time.monotonic() >= deadline:
                    result['timeout'] = True
                    break
                time.sleep(0.02)
        except (OSError, ValueError, subprocess.SubprocessError):
            result[{'spawn': 'spawn_error', 'containment': 'containment_error',
                    'capture': 'capture_error'}[stage]] = True
        finally:
            if job is not None:
                try:
                    if assigned:
                        job.terminate_and_wait()
                        result['job_empty'] = True
                except OSError:
                    result['cleanup_error'] = True
                finally:
                    try:
                        job.close()
                    except OSError:
                        result['cleanup_error'] = True
            if process is not None:
                try:
                    # Unassigned bootstrap has received NO payload, so has no children.
                    if not assigned and process.poll() is None:
                        process.kill()  # Owned process HANDLE, never a reusable PID.
                    result['actual_exit'] = process.wait(timeout=5)
                except (OSError, subprocess.TimeoutExpired):
                    result['cleanup_error'] = True
                finally:
                    if process.stdin is not None:
                        try:
                            process.stdin.close()
                        except OSError:
                            result['capture_error'] = True
                    process._handle.Close()
            result['observed_bytes'] = os.fstat(output.fileno()).st_size
            result['output_limit'] |= result['observed_bytes'] > limit
            if result['observed_bytes'] > limit:
                output.truncate(limit)
    raw = Path(log).read_bytes()
    result.update(log_bytes=len(raw), log_sha256=digest(raw),
                  elapsed_seconds=round(time.monotonic() - start, 3))
    return result, raw


def capture_ok(result):
    return result['actual_exit'] == 0 and result['gate_released'] and result['job_empty'] and not any(
        result[k] for k in ('timeout', 'output_limit', 'spawn_error', 'capture_error',
                           'containment_error', 'cleanup_error'))
