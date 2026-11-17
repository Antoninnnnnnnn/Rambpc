# Windows owned-pipe blocked-write qualification

## Scope and status

**Two native public cases passed; real-channel/live GO remains false.** After
independent source review, exactly two selected cases ran in a verified outer
owned Job: deadline and explicit cancellation. Both passed with zero failures,
errors or skips, pinned sources unchanged and outer Job empty. Buffer-query and
no-return observations succeeded; `CancelSynchronousIo` was **not invoked** in
either case. [Bounded evidence](../evidence/windows-owned-pipe-native-cancellation-20261117.json).

The cases use `tools.windows_owned_pipe_job.send_owned_windows` at public-channel
baseline `e1ec3cd`. They do not change that API, either immutable Job helper,
existing tests or the positive fixture. The already-passing positive fixture was
**not rerun**. Earlier fake and positive results remain separate evidence.

Owned additions:

- `tools/reference/owned_pipe_blocked_public_fixture.py`
- `tests/test_windows_owned_pipe_native_public.py`
- this document

The separate source handoff contains SHA-256 pins and the exact future selection.
Nothing here qualifies Linux/Rambler, a remote controller, a real caller, SSH,
services, devices, credentials or a confidential transport. The only payload is
the fixed invented public constant `b'P' * 12302`; `MAX_INPUT` stays 12302.

## Fixed target and startup

The new original fixture accepts **no arguments or modes**, emits only
`OWNED_PIPE_BLOCKED_PUBLIC_READY_V1\n`, never reads stdin (not even EOF), waits a
fixed 25 seconds, and exits nonzero without a terminal. Exceptions produce no
traceback. The configured wait is <=30 seconds; it is not a guarantee about OS
scheduling or a hung OS call. Owned Job cleanup should end the target first.
There is no environment/file loader, arbitrary path/argv dispatch, logging,
network, persistence or connection to the positive fixture.

Each case starts the public API with the reviewed base interpreter as both runner
and target interpreter, exact `-I -S -B <new-fixture-path>` argv, explicit public
repository cwd, and a fresh literal environment containing only
`SystemRoot=C:\Windows` and `PATH=C:\Windows\System32`. Interpreter/base path and
prefix equality are asserted without substitution or guard patching. Those
literal Windows installation paths must match the future reviewed host; do not
silently inherit an environment or fall back to PATH Python. The production API
also retains its own guards. The producer returns the public constant once and
has no blocking callback, secret source, input file or environment loader.

## Establishing a genuine blocking opportunity

Before the real writer releases even its public gate, the observation wrapper
queries **that real `process.stdin` write HANDLE**, obtained by
`msvcrt.get_osfhandle`, using `GetNamedPipeInfo`. It records flags and inbound and
outbound sizes. The precondition is a successful query with
`0 < outbound_buffer_bytes < 12302`. Zero, an API error (including access failure),
or a size at least as large as the payload aborts gate release and **fails** the
case, never skips/passes it. No assumption about a universal 4096-byte default,
replacement pipe, fake backend, extra filler or larger input cap is allowed.

Microsoft API references consulted statically during authoring:

- [GetNamedPipeInfo](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-getnamedpipeinfo)
  explicitly accepts anonymous handles returned by `CreatePipe`. A write-only
  handle requires `GENERIC_WRITE` and `FILE_READ_ATTRIBUTES`. `lpOutBufferSize`
  is the outgoing buffer size in bytes; **zero means allocated as needed**.
  A zero function return is failure. The test supplies the documented HANDLE,
  DWORD-pointer and BOOL ctypes signature; no guessed capacity is substituted.
- [CreatePipe](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-createpipe)
  says `nSize` is only a suggestion, zero selects the system default, and anonymous
  pipes use a uniquely named pipe internally. Its remarks explain that a full
  buffer can prevent `WriteFile` returning until a reader frees space.

Reported buffer size is a **blocking opportunity**, not an absolute capacity
ceiling or universal proof of a blocked kernel instruction. The untouched target
source supplies the non-reading behavior. A narrow wrapper around the real
backend `write` records frame-call entry immediately before calling the original
method (`os.write`), and records return count or exception/finalization afterward.
It never sleeps or waits to manufacture blocking. The public API constructs the
real backend normally; no injected backend or private `_run` entry is used.

A separate owned observer establishes a 0.25-second event interval with **no real
frame-call return or exception** after entry. This is a bounded observation, not a
claim about all Windows versions, kernel progress, thread scheduling, universal
OS behavior or exact syscall-entry timing. Capacity evidence alone, a timeout
without writer entry, or a fabricated callback stall cannot pass.

## Exactly two native cases

`NativeBlockedPublicChannelTests` is decorated to skip on **all platforms** unless
both Windows and the exact test-only flag
`RAMBPC_TEST_OWNED_PIPE_BLOCKED_PUBLIC=1` are present at module import. The unrelated
positive-fixture flag does not enable it. Default discovery and CI are unchanged
apart from discovering these skipped cases. One additional pure constants/opt-in
guard test is authored; it creates no processes/threads or native API calls.

1. `test_blocked_public_write_deadline_drains_owned_job`: after the actual frame
   write entry/no-return interval, the API's eight-second deadline must return
   fixed category `timeout`. No external cancellation is supplied.
2. `test_blocked_public_write_explicit_cancel_drains_owned_job`: the independent
   observer requests an owned cancellation Event **only after** actual frame
   entry and that no-return interval. The public API must return `cancelled`
   rather than success, timeout or a writer/protocol error.

Both require exact READY, released gate, one producer call (also counted outside
the API), exactly one 12302-byte frame-write attempt/no replay, no successful stdin
EOF/terminal/zero-exit protocol, `ok=False`, verified owned Job, empty Job,
`workers_joined=True`, `handles_reaped=True` and `cleanup=True`. They require
original write finalization and API completed-byte accounting to agree. Neither
requires protocol `output_eof`: cleanup can close output without parsing that EOF.

The API call must return within eight seconds plus its unchanged two-second
cleanup grace plus two seconds of scheduling allowance. This is a broad bounded
return assertion, **not a latency benchmark**. A native hang can prevent the
in-process assertion from being reached; the outer owned-Job watchdog remains
mandatory. OS/driver calls are not made universally time-bounded by Python tests.

The observer has an owned stop Event, finite waits and a `finally` join. Its only
cancellation action is setting the owned Event; it never closes another thread's
handle or invokes a PID kill. API cleanup owns the pipe writer, producer, inner
Job and handles. No timer thread, taskkill, process inventory or generic kill is
introduced. Run serially: the real-method observation wrappers are temporarily
class-scoped, not suitable for concurrent channel calls in the same interpreter.

## Evidence and cancellation interpretation

Each future native case emits one fixed-schema public JSON line prefixed
`OWNED_PIPE_BLOCKED_PUBLIC_QA `. It includes the API's closed result, capacity,
entry/finalization and no-return flags, attempted frame sizes/total,
known-completed returned sizes/total, exception count, explicit-cancel request,
observer join and `real_cancel_writer_calls`. It emits no payload, argv/env,
handles, PID or exception diagnostic. A failure record is not a passing result.

These counts have different meanings:

- **Attempted frame bytes**: bytes offered to the original backend write.
- **Known-completed frame bytes**: positive byte counts actually returned by
  `os.write` and accounted by the API as `frame_bytes_written`.
- **Unknown in-flight kernel bytes**: a blocked/aborted `os.write` may already
  have partially transferred bytes even if `frame_bytes_written == 0` and no
  successful count was returned. The test does not measure this quantity.

Do not interpret a zero completed count as zero bytes delivered, rollback,
confidentiality, secure erasure or any statement about secret network traffic.
All data in these cases is deliberately public and local.

The narrow real `cancel_writer` wrapper counts entries and unconditionally calls
its pinned original method, which directly calls `CancelSynchronousIo`. If Job
termination closes the readers and the writer exits first, the count can be
**zero**; the JSON then reports `cancel_synchronous_io_invoked=false`. This is an
acceptable result if the other assertions pass, and MUST be reported as **not
invoked**, not omitted or relabeled as proof of that API's effectiveness. A
positive count establishes invocation in this pinned implementation, not that it
was necessary, sufficient, unique, or the cause of unblocking. The qualification
target is the actual non-reading pipe, Job drain, worker/handle cleanup and bounded
return. The suite does not force a `CancelSynchronousIo`-only race.

## Reproduction contract and observed QA

The observed selection ran each of the two native cases once, without retrying
either test or the earlier positive fixture. A report parser initially missed
JSON diagnostics embedded in unittest output; the retained capture was parsed
again without re-executing the tests. Both recorded outcomes were the required
`timeout` / `cancelled`. Each attempted one 12302-byte public write and reported
zero known-completed bytes; unknown in-flight kernel bytes remain unmeasured.
Both real cancel-writer call counts were zero, so the syscall's effectiveness is
not qualified. These are bounded observations, not a latency benchmark.

For separately authorized reproduction, select **only**
`test_windows_owned_pipe_native_public.NativeBlockedPublicChannelTests` (two
methods). Never select whole discovery, the existing native positive class or the
pure guard test for this native QA step. No opt-in means two skips, not qualification.
The future runner must explicitly check two tests executed with zero skips, errors
and failures. Use fail-fast to stop the selection after a failed case; fewer than
two executed tests cannot be recorded as a full pass.

Use the unchanged outer owned-Job capture helper and a separately reviewed
absolute base-Python runner. Its bounded public capture can retain the two JSON
lines and unittest status. A 40-second outer operation budget and 16384-byte output
limit are proposed for the finite selection; outer Job cleanup/reaping must also
be proved. Use clean explicit public env/cwd and reviewed public output paths.
Do not create or run that driver during this source-only step. Do not persist the
opt-in in configuration, CI or a parent environment. The runner should set it
only in its own process before module import, and install only the reviewed public
repository/test directories into isolated `sys.path`.

The source authoring step performed no Python/module/test or native execution;
independent review and the later Main-owned bounded run are distinct. Source
hashes and the observed QA result are recorded in the linked public evidence.

Remaining risks include actual handle access/query behavior, host buffering and
scheduling, non-atomic entry observations, cancellation/job termination ordering,
OS/driver stalls, and nested-Job support under the outer helper. A failed capacity
query or failure to observe a no-return interval is unresolved qualification, not
permission to relax a precondition. Any unproved cleanup is a hard stop, not a
reason to continue to a real operation. There is intentionally no native arbitrary
blocked-callback case: Python cannot safely kill such a callback; existing fake
coverage's cleanup-false warning remains in force and unchanged.
