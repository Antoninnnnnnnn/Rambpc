# Developer-only owned Windows stdin pipe (public fixture QA)

## Status and scope

Source review is **GO** and subsequent public-only QA passed the fake/fixture
selection and one explicitly opted-in native fixture test described below. This is
not real Windows-stdin/caller qualification or live GO. Source authoring itself
performed only static AST and hash checks. The initial source review was **NO_GO**:
interpreter redirector, error-priority and automatic native discovery blockers were
fixed before the subsequent tests. The initial QA handoff also omitted GO and
named the wrong test class, selecting zero tests; that is failed handoff history,
not a new user-authorization issue or a passing run. The corrected QA is separate.
No earlier explicit-base public Job evidence is retroactively classified as
venv-qualified or invalidated by these changes.

The existing native public Linux result (11 cases, eight pre-input EACCES checks)
is separate evidence; it does not qualify this Windows caller. READY is only an
exact protocol marker, **not external Linux protection proof**. Before any future
real channel, MAIN must independently pin and qualify the remote controller and
its pre-input protection checks, interpreter/tools, SSH configuration, host key,
and the complete invocation. None of that is implemented here.

Owned additions are `tools/windows_owned_pipe_job.py`,
`tests/test_windows_owned_pipe_job.py`, `tools/reference/owned_pipe_fixture.py`,
and this document. No CI, STATUS, Linux documentation, historical evidence,
Cargo.lock, oldReleaseCLI or original helper changes are required.

## Exact developer API

```python
send_owned_windows(*, public_argv, public_cwd, public_env, ready, terminal,
                   input_producer, timeout=10.0, cancel_event=None) -> dict
```

Import from `tools.windows_owned_pipe_job`. There is no command-line interface,
credential loader, SSH launcher, retry/resume mechanism or generic transport API.
The private `_run(..., backend=...)` seam exists only for trusted fake QA. It
intentionally bypasses production platform/interpreter/backend construction:
selecting it proves fake state-machine behavior, not interpreter/native safety.
Never pass a native backend through this seam to bypass the public API guard.

Production Windows callers MUST start the API with an explicitly reviewed base
Python interpreter. Before backend construction, Job acquisition, process launch
or producer invocation, `sys.executable` and `sys._base_executable` must be nonempty
absolute strings with identical normalized absolute paths (`abspath`/`normcase`),
and `sys.prefix == sys.base_prefix`. Otherwise the fixed outcome is
`unsupported_interpreter`. This rejects venv redirectors that could create an
unassigned bootstrap descendant before gate release. The backend's `spawn` repeats
the guard defensively, and uses that same validated absolute executable; it never
silently switches to `_base_executable`, PATH Python or another tool. Equality is
not executable provenance verification or a defense against trusted in-process
code changing interpreter properties. Non-Windows entry remains
`unsupported_platform` without WinDLL construction.

- `public_argv`: explicit list/tuple of 1..128 public strings, first nonempty.
- `public_cwd`: explicit nonempty public working-directory string.
- `public_env`: explicit dictionary of public string keys/values; **never None**.
  Nothing is copied from `os.environ`. Use absolute, reviewed executable paths
  rather than PATH-dependent resolution in any later qualified caller.
- `ready`: exact trusted ASCII line, printable bytes plus final LF, 2..256 bytes.
- `terminal`: different exact trusted ASCII line, same shape, 2..512 bytes.
- `input_producer() -> bytes`: trusted callback, at most one invocation on a daemon
  worker, only after exact READY and no observable extra output/EOF. Exact built-in
  `bytes` only, length 0..12302. No coercion, framing, newline addition or replay.
  Zero-length input still requires EOF. No credential source is implemented.
- `timeout`: finite numeric seconds, `0 < timeout <= 300`, from API entry through
  bootstrap, callback, binary write, terminal, output EOF and zero child exit.
- `cancel_event`: optional trusted `threading.Event`-compatible cancellation flag.

Every startup string MUST be audited secret-free, including `sys.executable`,
argv, cwd, the complete environment, public JSON gate and protocol constants.
Structural checks cannot detect a secret hidden in an arbitrary string. Passing
these arguments is a developer obligation, not proof of secret detection. Public
argv contains no input frame. Producer-owned state must not already leak through
process startup, logging, an exception hook or another background worker.

The result's closed keys are `outcome`, `ok`, `ready`, `terminal`,
`producer_calls`, `frame_bytes_written`, `output_bytes_observed`, `stdin_eof`,
`output_eof`, `gate_released`, `job_verified`, `job_empty`, `exit_zero`,
`workers_joined`, `handles_reaped`, `cleanup`. Except `outcome`, values are only
booleans or counts. `outcome` is one of `ok`, `invalid_configuration`,
`unsupported_platform`, `unsupported_interpreter`, `containment_failed`,
`spawn_failed`, `gate_failed`,
`protocol_failed`, `early_eof`, `input_rejected`, `producer_failed`, `write_failed`,
`exit_failed`, `timeout`, `cancelled`, `cleanup_failed`, `internal_failed`.
The last is the initial defensive category. No arbitrary exit values, payload,
payload hash, argv, cwd, env, raw output or exception strings are returned.

## Gate and closed protocol

The immutable `tools/windows_owned_job.py` is imported unchanged for its
`ProcessJob` and canonical JSON primitives. Its `capture_windows` cannot send a
frame: its EOF bootstrap launches the target with DEVNULL stdin. It is not called
or modified by the new channel.

The new bootstrap is necessarily a separate small fixed Python program, launched
with `-I -S -B` and explicit cwd/env, raw unbuffered binary pipes and merged
stdout/stderr. It remains blocked on a public JSON newline until its *owned
process handle* has been assigned to the new unnamed kill-on-close Job, then
membership in that exact Job verified with `IsProcessInJob`. Only then can the
writer send a gate. No target is created before that release.

The gate is canonical `{"argv": [...]}` plus LF, at most 65536 bytes. Bootstrap
uses `os.read(0, 1)` for every byte, checks its bound/schema, then starts the target
with stdin descriptor 0 and stdout/stderr descriptors 1/2. It never prefetches a
frame byte and never substitutes DEVNULL. Descendants inherit Job containment;
no breakaway flags or shared service/daemon delegation are requested.

The controller reads one byte only after `PeekNamedPipe` reports data. Each byte
is immediately compared against the configured READY/terminal constant; no raw
output buffer exists. stderr participates in the same strict protocol. Junk,
invalid byte, oversized line, duplicate marker, early terminal and surplus byte
fail closed. Queued output and EOF after READY are drained/rechecked before the
callback starts. The same drain continues while the callback runs; observable
junk prevents input release. Callback completion is snapshotted **before** the
quiet drain that authorizes sending; completion during a drain requires another
quiet pass. Published writer/producer errors are checked before and after the
drain. Each nonblocking byte observation is serialized with error/EOF publication,
so close-induced output after a failed write cannot overwrite `write_failed` with
a premature-terminal error. Already-observed genuine protocol errors still fail
as `protocol_failed`; there is no blanket exception-to-writer-error override.
Only callback success plus quiet output enables the writer. Writes handle positive
partial counts until the exact bytes are consumed, then close stdin for mandatory
EOF. Terminal is accepted only after published
EOF, followed by output EOF and child exit zero. Terminal alone is not success.

The observation/release boundary is not atomic with child or remote activity.
A byte arriving just after a quiet check may race callback dispatch or a write;
READY does not make malicious code trustworthy. Repeated checks are not a remote
sandbox attestation. There are no retries, producer replay or resends after error.

## Deadlines, cancellation, cleanup and data lifetime

The only I/O worker is the sole raw stdin writer; there is no blocking reader
thread or buffered-stream lock. A writer opens a HANDLE to **itself while alive**,
retains it for cancellation, and closes it in its finalization. The controller
never looks up a potentially recycled thread ID. Job termination closes owned
readers, and the implementation repeatedly calls `CancelSynchronousIo` against
the retained writer HANDLE for an in-flight blocked pipe write/syscall-entry race.
Native effectiveness for that race remains unproven; tests of it use fakes only.
Cancellation and
expiry are checked before writes and before callback invocation/publication.
Data returned by a callback after cancellation/expiry is discarded, never sent.
An already in-flight kernel write cannot be retroactively undone.

Cleanup first signals cancellation, terminates/waits for the owned Job, reaps the
owned bootstrap, cancels/joins the writer, closes owned handles and joins the
producer. Cleanup grace is two seconds total after operation completion/failure;
OS/driver calls themselves still depend on the OS returning. Only the bootstrap
can be killed by its Popen-owned process HANDLE when Job assignment failed and
no gate was released. No numeric-PID, taskkill, process inventory or group kill
is used. Job cleanup uses the original helper unchanged.

`cleanup=True` requires proven empty Job (or no launched target), worker joins,
and handle/process reaping. It is never a mere termination attempt. Constructor
or Popen exceptions lacking a usable returned object conservatively report
cleanup/handles unproved, because hidden partial acquisitions cannot be inspected.
Cleanup failure forces `ok=False`, even with an otherwise complete protocol.
Writer cancellation/join failures (including interruption) cannot skip the Job
close, which is in a `finally`; producer join interruption is guarded as well.
Caught cleanup interruption always leaves `cleanup=False`, even if subsequent
observations show no live worker. Such a return is not permission to continue.

Python cannot safely kill a blocked arbitrary callback thread. The function
returns after its deadline plus bounded cleanup grace with `cleanup=False` and
`workers_joined=False` if that callback remains alive. It cannot subsequently
publish input to the stopped writer. Such a result is a hard stop: MAIN must not
start a real operation in that contaminated interpreter; require a cooperative,
bounded, no-logging callback and a separate lifecycle decision. No universal
in-process callback isolation is claimed. A deliberately noncooperative callback
can consume CPU/memory, keep the GIL or perform its own side effects; it is trusted
code, not a sandboxed plugin. Callback exceptions are discarded without text.

The implementation opens no capture/report/input file and never persists input or
raw output, even privately. Payload lives only in callback/worker memory; unknown
output is compared and discarded. References are released on completion and
cancellation, **not securely erased**. This does not defend against same-UID
inspection, administrator/root, the OS, debuggers, crash dumps, pagefile/swap,
Python allocator copies, inherited interpreter policy, remote SSH buffering or
controller-side memory/storage. The local Job is lifetime containment, not a
confidentiality boundary or universal isolation guarantee.

## Original public fixture

`tools/reference/owned_pipe_fixture.py` is an original Python CLI with no modes.
Use only `python -I -S -B <fixture-path>` after MAIN review. It reads no environment
content and uses no network, microphone, phone, service or credentials. It sends
fixed safe ASCII READY/terminal lines and never echoes input.

Its sole invented public RLA1 tuple is 85 bytes: magic `RLA1\0\r\n\x1a`, then
three big-endian u16-length-prefixed fields `invented-public-frame-key`,
`invalid.public.frame`, `invented-public-frame-cert`. It compares the entire tuple
and requires EOF before either fixed terminal. At most 86 input bytes are retained;
extra input is discarded until EOF. Success exits zero; rejection exits one.
Nothing is persisted. This is not the real Linux controller.

## Public QA and test selection

The sanitized public QA record is
[`docs/evidence/windows-owned-pipe-public-qa-20261117.json`](../evidence/windows-owned-pipe-public-qa-20261117.json).
After source review GO, a separate fake/fixture selection ran 32 tests: 31 passed,
one non-Windows guard skipped on Windows, and zero failures or errors. A separate
explicitly opted-in native run selected and passed exactly
`NativeChannelTests.test_original_public_python_fixture_exact_frame_and_eof`.
Its assertions cover the 85-byte public frame, one producer call, verified and
empty Job, both EOFs, worker joins and handle reaping; `ok` requires zero child
exit and the complete protocol. Both outer capture Jobs were empty and their
captures exited zero. The runner used an explicitly selected, verified base Python
image. Source, helper, lockfile and old release CLI pins were unchanged before and
after QA. There were no SSH roundtrips, service calls or credentials loaded; this
is not live GO. No native blocked-write or `CancelSynchronousIo` qualification
was performed: those paths have fake coverage only.

33 unittest methods are authored: 29 fake channel methods, three pure fixture
methods and one Windows-native public fixture method. Native integration requires
**both Windows and the explicit test-only environment opt-in
`RAMBPC_TEST_OWNED_PIPE_PUBLIC_FIXTURE=1` at test-module import**; absent that flag,
unittest/pytest discovery skips it even on Windows.
No flag is needed for isolated fake/fixture QA. The flag authorizes no production
API behavior and is never read by that API. Linux imports instantiate no WinDLL;
that guard is itself covered in fake QA (and skips on Windows). All tests use
invented public data only. A failure must be fixed or reported, never relaxed for
green output.

For a separately reviewed repeat, from the repository root, replace `BASE_PYTHON`
with an explicitly reviewed absolute base-interpreter executable. These illustrative
commands select the intended classes and add public repository/test paths even in
isolated mode; they are not a transcript of the capture invocation. Ensure the opt-in
is absent from the parent environment for the fake selection and normal discovery.

```text
BASE_PYTHON -I -S -B -c "import os,sys,unittest; sys.path[:0]=[os.getcwd(),os.path.join(os.getcwd(),'tests')]; unittest.main(module=None,argv=['qa','test_windows_owned_pipe_job.FakeChannelTests','test_windows_owned_pipe_job.FixtureTests'])"
BASE_PYTHON -I -S -B -c "import os,sys,unittest; sys.path[:0]=[os.getcwd(),os.path.join(os.getcwd(),'tests')]; os.environ['RAMBPC_TEST_OWNED_PIPE_PUBLIC_FIXTURE']='1'; unittest.main(module=None,argv=['qa','test_windows_owned_pipe_job.NativeChannelTests.test_original_public_python_fixture_exact_frame_and_eof'])"
```

The second command sets the test-only flag only inside that test process, before
import; do not persist it. The fake selection starts threads/fakes, no child process.
Without opt-in, full discovery selects 33 methods, with two skips on Windows or
one skip off Windows; these discovery counts follow the decorators and are not a
claim that full discovery was executed.

New fake coverage includes independent executable/prefix venv rejection before
backend construction or callback, normalized-base acceptance via a fake backend,
defensive backend spawn rejection, a mocked-Popen accepted-base argv assertion,
Windows-plus-opt-in truth-table checks, callback junk published during a quiet
read, and writer/producer join interruptions with Job closure and cleanup false.
No guard test launches an interpreter or invokes native backend construction.
The failed-write fake deliberately emits a terminal on stdin close even for an
incomplete frame; `write_failed` must still win, with no successful EOF/terminal.
Deadline tests establish blocked-writer/callback/output-stall progress with event
barriers and a two-second operation budget, not a 50 ms scheduling assumption.

Existing coverage retains exact binary bytes + partial writes + EOF,
late/never-before-READY producer invocation, delayed READY, junk,
surplus/duplicate/early terminals, EOF in partial states, bounded oversized output,
echo/exception non-disclosure, callback failure/type/size caps including empty and
12302-byte input, broken pipes, zero write counts, assignment/membership failure,
constructor/spawn/read/gate errors, nonzero exit, cleanup failure, terminal/EOF
stalls, pre-cancellation, blocked non-reading child, writer cancellation, blocked
producer deadline/cancellation and verified no late input or producer replay.

Fixture fakes separately verify exact field encoding, partial input assembly,
full equality, rejection, EOF before terminal and withheld-EOF silence.

Only after source/fake review may MAIN separately select `NativeChannelTests` on
Windows, starting the QA runner with an **explicit reviewed base interpreter** and
setting `RAMBPC_TEST_OWNED_PIPE_PUBLIC_FIXTURE=1` only for that reviewed test run
(before import). Do not persist this test-only flag in project/user configuration.
Selection alone without the opt-in still skips. That sole test launches only the
original public Python fixture inside
an owned Job; it asserts 85 exact bytes, EOF, exact protocol, Job membership/empty,
zero exit, worker joins and handle reaping. It uses an explicit public environment.
It is not a blocked-writer native qualification suite or SSH qualification. Review
and separately qualify actual Windows blocking/cancellation semantics before any
future sensitive caller; no such qualification is claimed by these sources.
