# Linux session pipe receiver — SOURCE ONLY, not live GO

## Status and ownership

New, developer-only source: `tools/reference/linux_session_pipe_receiver.py`.
The design's **controller** name is retained in CLI selectors and JSON schema;
the delivered filename is **receiver**, as explicitly assigned. There is no
credential loader, `.env` support, SSH launcher, endpoint override, anonymous
fallback, ASR substitute, build step, reconnect, replay or retry.

The source authoring delivery did **not** execute either Linux mode, a Native
binary or an SDK session. A subsequent bounded fake-only suite passed **49 tests
on Windows and 49 on Linux**, with source pins unchanged and owned Windows Jobs
clean. Linux source transfer/test launch used SSH, but never invoked the
controller's native entry point. [QA record and limits](../evidence/linux-session-pipe-fake-qa-20261117.json).
Neither a native PUBLIC_GUARD nor REAL attempt has run. The existing public SSH
success uses a different public-only receiver and remains separate evidence,
not a secret channel or single-session qualification. Source/fake approval is
not live GO; MAIN separately owns any later native attempt.

The old receiver/helper, Windows Job/channel/fixtures, Rust source/snapshot,
existing Debug binary and public PCM are immutable inputs; no historical campaign,
reservation or result is adopted, replayed or overwritten. This implementation
uses a **new**, independently marked root, never the old receiver root or cases.

## API and exact selectors

The pure protocol seam is `run_once(port, mode) -> 0 | 1`, where mode is
`PUBLIC_GUARD` or `REAL`. `LinuxPort(args)` construction is inert. Importing the
module on Windows or Linux performs no hardening, process launch or file I/O.
`main(argv=None)` is a future Linux entry point, not an execution instruction.

Exactly eight mandatory, unique, separated option/value pairs are accepted:

| Option | Public-only meaning |
| --- | --- |
| `--root` | Absolute trusted Linux path to the NEW root |
| `--owner-sha256` | Out-of-band pin of the root's public marker |
| `--controller-sha256` | Pin of this receiver's exact bytes |
| `--helper` | Absolute path to the immutable public supervisor source |
| `--mode` | Exactly `public-guard` or `real` |
| `--reservation` | `<mode>-<32 lowercase hex>.json`, basename only |
| `--reservation-sha256` | Out-of-band pin of that public reservation |
| `--helper-sha256` | Exactly the compiled-in helper pin below |

Attached `=`, duplicates, missing/unknown options, private path components and
mode/reservation-name mismatches fail before launch. A malformed CLI has no
trusted mode: stdout is silent, stderr is the fixed failure line, exit is 1.
After a valid CLI, protocol errors use the mode-specific fixed error below.
No selector accepts credentials, endpoint, arbitrary Native flags or a PCM path.

## New public-input root and reservation

All ancestors are no-follow traversed and must be root/current-UID-owned,
non-group/world-writable directories. Root is current-UID-owned `0700`.
Regular public metadata is owned, single-link, stable while read and `0600`.
Directories under the controlled tree are `0700`; the executable input is `0500`.

```text
ROOT/
  owner.json
  inputs/
    source-manifest.json
    source/                 exact pinned Rust snapshot
    public.pcm              fixed public PCM only
    example-input           existing pinned Debug binary, not rebuilt
  reservations/
    <mode>-<32hex>.json      exactly one pre-created reservation
  cases/
```

Root marker is exactly the closed object (with the actual public root basename):

```json
{"schema":"linux-session-pipe-controller-owner-v1","name":"ROOT","public_inputs_only":true}
```

`public_inputs_only` describes files on disk, not the REAL pipe contents.
MAIN must separately create a fresh mode-bound reservation before each attempt:

```json
{"schema":"linux-session-pipe-reservation-v1","mode":"PUBLIC_GUARD","id":"11111111111111111111111111111111"}
```

`mode` must equal the selected uppercase mode; `id` must match the basename.
The controller creates one random `session-pipe-case-<32hex>` directory with one
`mkdir`, no collision retry. Its marker is
`{"schema":"linux-session-pipe-case-owner-v1","public_inputs_only":true}`.
It exclusively reserves `result.json`, freezes the pinned binary and copies only
the pinned public PCM, then creates an empty `target-cwd`. No old case is opened.

Before fork and before upstream READY, it renames the single reservation into
its fresh case as `reservation.json` and re-verifies the byte pin/schema through
`trusted_open`. The consumed flag is set immediately after rename, even if the
post-rename hash check fails. It is never restored. A subsequent invocation with
that absent reservation fails STARTUP before fork. A failure before consumption
has not consumed it; this is not permission for an automatic retry.

Postchecks verify the root/input sets, empty reservations directory, consumed
reservation, exact owned-case set, empty target cwd, all input/case pins, full
source tree, policy and actual ECHILD. The case set is limited to `owner.json`,
`result.json`, `example`, `public.pcm`, `target-cwd`, `reservation.json`. Directory
names, IDs, pins, paths, lengths, PIDs and raw text do not enter the result object.

## Immutable pins and reuse

| Input | SHA-256 |
| --- | --- |
| Public helper | `bb711f7ce1ada062cee9ed47136ae0496c38d8095d8ced26229bb2f8328ec0d9` |
| Old public receiver | `fd70a045c606ef3bf9f9a0b8352707499ef1943e2056a307feb5bab2f98c89c0` |
| Windows Job | `3ac53d5e3f202512b18cb3e4bafdc27c8b87add5c4d9afb70a3dd07462fd76aa` |
| Windows stdin channel | `a9e6c5890e39461549d1abcf898f7d405c10ca2171715e3a26e52c469ae86ea5` |
| Native example source | `fae7a6b088f3d1a7f6bd7b3f795ae99a3ec119ca6d029cb2ecf9fdf811c2c8c6` |
| Native fake test source | `269fc87b38482f1185595180039768721dbdcd7c7838f145add245e241cdeb0f` |
| Existing Debug binary | `1d183b7adece6c2a05ab15a0d8bd5faeaab7bc855cbb89b1091ced81f3883991` |
| Public PCM | `cea1bc1614f738048b1a4f1bcad83f70aee42b9621b8b754f8aacbe1395b08c9` |
| Source manifest | `50a8a6f2a81b0e9e3abe60d88f9078ee1ab437a98741cc8d5468b126036ed284` |
| Source tree | `1e0fe2437674b6992351bf7e322129ccb64c3a0d7b552cb2188f9a8dbd5bec39` |

Source hashes of the six named qualified Python/Rust source files were measured
by read-only hashing during authoring and match. Binary, PCM, manifest and tree
values are retained from the qualified receiver; they were **not remeasured**
against actual staged inputs during this source-only delivery.

Runtime byte-pin-loads **only the helper**, executing the already verified
in-memory bytes rather than reopening a path. It does not import the old receiver.
`public_path`, `trusted_open`, `public_bytes`, `public_directory`, `load_helper`
and `bounded_public_input` are copied verbatim. Unchanged `LinuxPort` pieces
include source checking, ownership guards, waits, native READY, external
observation, cleanup, record checking/persistence and fixed-byte emit.

The new `session_target_exec` has the same AST as helper `target_exec` except
its name/signature (no `no_args`) and one fixed argv assignment:
`[binary, '--single-session-v1']`. It is bound with `types.FunctionType` to the
byte-pinned helper's globals before fork, so the verbatim signal/resource/prctl
calls use those exact helper primitives, not guessed module-global substitutes.
Both modes use this one exec path. It retains the cleared signal mask, reset of
all catchable signals, 3s gated alarm, PDEATHSIG SIGKILL, both parent checks,
20s launch alarm, core 0/0, FSIZE 512MiB, CPU 10/10, cwd, descriptor preservation
above 10, descriptors 0..3 only, exact PATH/LC_ALL environment and `_exit(126)`.

`start` and `recheck` cannot literally equal the old versions: their mandatory
new reservation/root checks, pipe-size check, target call and deadlines differ.
Fake QA includes precise AST normalization of **only those declared deltas**;
other copied functions/methods have direct AST equality assertions. Preparation
and mode-specific emit/input/terminal functions are explicitly new/adapted.
No claim of verbatim equality for those changed methods is made.

## State machine and confidentiality boundary

One attempt, with no loop around launch or forward:

1. Prepare under `-I -S -B`, non-root stable identities, env exactly
   `PATH=/usr/bin:/bin`, `LC_ALL=C`, umask 077. Byte-pin helper, initialize
   core0/dumpable0/PDEATHSIG-to-cancellation/subreaper/single thread/no children,
   install the mode alarm and NO_NEW_PRIVS, verify FIFO blocking fd0/fd1,
   fd2 `/dev/null`, pins/manifest/source tree/policy, frozen binary and PCM copy.
2. Consume reservation.
3. Fork once, acquire pidfd, prove direct-child ownership, release `G` gate.
4. Exact 24-byte Native READY plus quiet peek.
5. First unchanged helper observation: living direct child, stable stat identity,
   status/limits/policy and **OPEN must fail EACCES** for child environ. Unexpected
   open success is closed unread. Environment content is never inspected.
6. Emit exact mode-specific upstream READY.
7. Read the bounded frame and require EOF.
8. Second identical observation immediately before the one forward operation.
9. Forward once and close the native stdin owner for EOF; never resend.
10. Read a closed Native terminal, require stdout EOF, observe exit and cwd/policy.
11. Close owned pipe handles, drain only owned children, prove actual ECHILD.
12. Recheck all pins/source/case sets/policy, then final parent/hardening guard.
13. Persist via the held O_EXCL result descriptor, fsync, close.
14. Final guard, fixed success terminal, exit 0; otherwise fixed error, exit 1.

The fork/exec happens **before the first upstream input read**. Once input has
been read there is no further fork, exec, subprocess, reconnect or retry.
READY is emitted only after the first external check; it is not a general remote
attestation or a credential-provisioning decision. Upstream caller must select
an exact trusted mode READY before invoking its producer.

## PUBLIC_GUARD

The only permitted frame is the original compiled-in 85-byte invented public
tuple, followed by EOF. The verbatim comparator reads at most 86 bytes in 5s.
The no-argument `forward_guard()` writes **only that constant**, never caller data.
Both modes launch Native with `--single-session-v1`, not public-frame-check mode.

Guard success requires exactly this 151-byte Native line, EOF, normal exit 1,
empty cwd, unchanged policy/pins/source and successful parent cleanup:

```text
REAL_ADMISSION_TERMINAL_V1 REJECT_CREDENTIALS primary_errors=1 cleanup_errors=0 sessions=0 healthy=false pcm_bytes=0 pcm_eof=false source_closed=false
```

The pinned Rust source rejects that exact tuple in `admit()` before `snapshot`,
PCM release, runtime or connection. `session()` cannot produce that category.
The guard's final upstream line (82 bytes), with controller exit 0, is:

```text
RAMBLER_SESSION_GUARD_TERMINAL_V1 PUBLIC_TUPLE_REJECTED_BEFORE_RUNTIME sessions=0
```

Normal exit 0, PUBLIC_FRAME_CHECKED, CONNECT/SESSION/CLEANUP categories, or
sessions=1 is GUARD_BREACH with rpc UNKNOWN. Other source-proven pre-runtime
categories are NATIVE_UNEXPECTED with rpc ZERO. Malformed/truncated/surplus
output is NATIVE_OUTPUT; a missing EOF times out. Signal exit is NATIVE and
rpc UNKNOWN. IO_FAILURE and INTERNAL_FAILURE are conservatively UNKNOWN,
not a zero-RPC assertion. The total exchange budget is 15s after gate release;
parent alarm is 30s and owned-child drain has the unchanged 6s bound.

A future PASS would qualify only the new single-session exec path, fork gate,
ownership, two EACCES observations, channel and public-tuple rejection. It would
not qualify REAL secrets, network/TLS/DNS, CPU headroom, provisioning or credential
confidentiality. No such native PASS is claimed here.

## REAL — separate attempt, pending qualification

`bounded_real_input` allocates one 12302-byte bytearray and reads into memoryview
slices using the `readv` seam. Each read asks for only the current segment's
missing bytes: exact 8-byte magic with progressive prefix checks, then three
2-byte big-endian lengths and respective bodies. Each length must be 1..4096.
Invalid first length stops after 10 bytes; a later invalid length stops at its
header after the unavoidable preceding fields, without reading that invalid body.
The final one-byte read must return EOF: at most 12303 input bytes are consumed,
with a 5s input budget and pidfd/parent guards throughout. There is no prefetch.

The parent does not decode, hash, log, echo or persist the frame, and never places
it in argv, environment or a file. Invalid UTF-8 and metadata validity are left
to Native, which returns REJECT_FRAME/REJECT_CREDENTIALS. This avoids Python
UnicodeDecodeError retaining input. Exact equality with PUBLIC_FRAME is rejected
locally as PUBLIC_TUPLE, rpc ZERO, no native write. Canonical field framing makes
byte equality equivalent to equality of this particular tuple.

Before fork REAL requires `F_GETPIPE_SZ` (1032) >=16384 on the native stdin pipe.
The empty nonblocking pipe receives one write of the filled slice, then close
for EOF. A short write or error is FORWARD, with no continuation or resend.
The filled bytearray is overwritten immediately after the attempt and again via
the finally hook on every path, including read/validation failures. This is
**best effort, not secure erasure**: CPython, OS, SSH/launcher buffers and swap
can retain copies; retained references cannot be revoked.

Native output is at most 256 bytes through LF, followed by mandatory EOF and
exit. A closed bytes regex accepts only the categories listed below, integer
source ranges (primary 0..1, cleanup 0..3, sessions 0..1, PCM 0..117440) and exact
boolean spelling. Non-ASCII, oversized, multiline, extra or truncated output
fails closed. Raw output and counters other than the permitted result fields
are neither forwarded nor persisted.

Success additionally requires exactly the 150-byte Native line and normal exit 0:

```text
REAL_ADMISSION_TERMINAL_V1 SESSION_HEALTHY primary_errors=0 cleanup_errors=0 sessions=1 healthy=true pcm_bytes=117440 pcm_eof=true source_closed=true
```

Both Native engine/transport owner closures and runtime drop precede this line.
Parent cleanup, ECHILD, post-run pins and persisted result must also succeed.
Only then is the following upstream line emitted (60 bytes), with exit 0:

```text
RAMBLER_SESSION_REAL_TERMINAL_V1 SESSION_HEALTHY sessions=1
```

SESSION_HEALTHY means the fixed full PCM upload, terminal gRPC OK and clean
cleanup. **It does not establish ASR correctness or the presence of a final
recognition.** No transcript comparison, endpoint/ASR substitution or response
text/ID collection is implemented.

REAL budgets: Native READY 10s after gate; frame 5s; forward through terminal,
EOF and exit 47s (10 connect + 20 session + 5 engine close + 5 transport close +
7 margin); drain 6s. Parent alarm is **110s immediately after helper initialize**,
not the old 30s. Native's own 90s alarm replaces its inherited launch alarm and
is only a backstop for unbounded runtime drop. Parent alarm exceeds all stage
budgets plus drain. A later MAIN Windows channel timeout should be 150..300s.
Timeout/cancel drains owned children, never reconnects or resends, and normally
records rpc UNKNOWN after forwarding.

## Closed result and failure protocol

`case/result.json` is O_EXCL-reserved before launch, verified still empty and
same-inode before writing, fsynced and closed. Its exact keys are:

```text
schema mode qualification ok error cleanup source_unchanged pins_unchanged
reservation_consumed external_checks frames_forwarded native_category
native_exit sessions rpc healthy
```

- schema: `linux-session-pipe-controller-result-v1`.
- mode: `PUBLIC_GUARD` / `REAL`.
- qualification: `PUBLIC_GUARD_ONLY` / `REAL_SINGLE_ATTEMPT`; this is the attempt's
  scope label, **not** a claim that qualification passed.
- flags: actual booleans; checks 0..2; frames_forwarded 0..1 (complete forwards).
- native_category: null or one of BLOCKED_HARDEN, BLOCKED_SAFETY, BLOCKED_ARGS,
  BLOCKED_PCM, REJECT_FRAME, REJECT_CREDENTIALS, IO_FAILURE, RUNTIME_FAILURE,
  INTERNAL_FAILURE, CONNECT_FAILURE, SESSION_FAILURE,
  SESSION_AND_CLEANUP_FAILURE, CLEANUP_FAILURE, SESSION_HEALTHY. Guard also permits
  PUBLIC_FRAME_CHECKED only as a detected GUARD_BREACH, never a pass.
- native_exit: null, 0, 1, OTHER (never a raw exit number or signal).
- sessions: 0, 1, UNKNOWN. Guard PASS is 0; healthy is always false in guard.
- healthy can be true only for a fully successful REAL SESSION_HEALTHY attempt.
- rpc: ZERO / ONE / UNKNOWN, **source-derived**, not service telemetry.

RPC ZERO applies before forwarding, or after a normal exit-1 terminal with
sessions=0 in BLOCKED_*, REJECT_FRAME, REJECT_CREDENTIALS or RUNTIME_FAILURE.
ONE means a closed SESSION_HEALTHY terminal was observed; it remains ONE if a
later parent cleanup/pin/record/output step fails. Everything else after the
forward may have started is UNKNOWN, including partial writes, timeout,
cancellation, missing/malformed terminals, signal exit, IO/INTERNAL failures,
CONNECT_FAILURE (network contacted) and any failing sessions=1 outcome. A guard
breach always overrides to UNKNOWN. Happy-eyeballs TCP attempts are connection
attempts, not counted RPCs. No network counter is invented.

Error codes (closed): STARTUP, RESERVATION, NATIVE, OBSERVE, INPUT, PUBLIC_TUPLE,
FORWARD, NATIVE_OUTPUT, NATIVE_REJECTED, CONNECT, SESSION, NATIVE_CLEANUP,
GUARD_BREACH, NATIVE_UNEXPECTED, TIMEOUT, CANCELLED, CLEANUP, PINS, RECORD,
OUTPUT, INTERNAL. `error` is null only for a persisted successful attempt.

Upstream errors have exactly this shape, with exit 1:

```text
RAMBLER_SESSION_ERROR_V1 <PUBLIC_GUARD|REAL> <CODE> rpc=<ZERO|ONE|UNKNOWN>
```

Stderr is only `SESSION_PIPE_RECEIVER_FAILED_V1\n`. Mode-specific upstream READY
lines are 38 and 30 bytes respectively:

```text
RAMBLER_SESSION_READY_V1 PUBLIC_GUARD
RAMBLER_SESSION_READY_V1 REAL
```

There are no frame/key/cert/token fields, hashes, raw errors, paths, IDs, lengths,
transcripts or diagnostic logs in the output record. If persistence fails no
success marker is sent. If the final guard or upstream write fails after fsync,
the already-persisted record cannot be rewritten: **even `ok:true` on disk is
not independently trustworthy without the exact upstream success plus exit 0**.
A late error must not be reported as a completed qualification.

## Finite fake QA and authoring verification

`tests/test_linux_session_pipe_receiver.py` contains **49 test methods**, split
among `StaticTests`, `ParserAndFrameTests`, `FlowTests`. All 49 passed separately
on Windows and Linux (zero failures/errors/skips). The import-safe
fake suite uses `FakeIO`/`FakePort`, injected helper operations and whole fake OS
objects. No test calls helper initialize, real fork/exec, hardening, live signal
reset, Native, actual PCM/procfs, SDK/session/network or a native test option.
Ordinary pytest discovery has no route into a native integration test.

Coverage authored: import traps; exact parser/root/reservation checks; pinned
source reads; direct and normalized AST equivalence; fixed single Native flag;
one-launch/one-forward ordering; guard truncations/flips/EOF; independent REAL
field boundaries, maximum size, invalid lengths, segment-exact read requests,
truncation/trailing/EOF, invalid UTF-8 delegation and local public-tuple rejection;
non-disclosure sentinel; all closed categories; malformed/extra/non-ASCII output;
timeout/cancel; consumed reservations and post-rename failure; low pipe capacity
and short writes; overwrite hooks; actual fake ECHILD requirement; changed pins;
record/upstream failures; parent death; Native success flags; EACCES ordering and
fake-clock deadline arithmetic; exact protocol lengths.

Authoring performed only static AST/compile/hash work. Initial independent review
found no production-source blocker but rejected two test defects: a missing fake
cwd and a Windows-incompatible `signal.alarm` patch. Both test-only defects were
fixed before the first suite launches; the production source stayed byte-identical.
A narrow follow-up review verified those changes and the two 49/0/0/0 reports.
The source-only NO_GO is preserved, not relabeled as a passing run. Native guard,
secret-channel and real single-session qualification remain absent.

Byte-pinned qualification sources have explicit LF checkout rules in
`.gitattributes`; runtime byte/hash checks are not relaxed or normalized.

## Limits that still block REAL execution

This is not a sandbox or completed provisioning system. Trusted CPython/kernel,
procfs, public-only startup, trusted source/helper pin selection and absence of
same-UID/root tampering are assumptions. Same-UID/root inspection, swap,
interpreter/SSH/launcher copies and unexpected kernel behavior remain outside
an erasure/confidentiality guarantee. SIGKILL or uninterruptible kernel work can
prevent completion of the cleanup/record protocol.

The immutable Debug binary may exceed CPU 10s under TLS/Opus load: SIGXCPU means
UNKNOWN, not a safe retry. sshd may close a session or deliver SIGHUP; runtime
Drop can block on DNS until Native alarm(90). None has been natively qualified in
single-session mode. A first Native PUBLIC_GUARD run would itself be separate
MAIN-owned qualification, not a replay of the old public-frame campaign.

Durable credential provisioning, producer review/confidentiality, rotation,
revocation, and COMPOSE/RAMBLE activation are unresolved. The Windows REAL
producer is outside this delivery and may be called only after exact REAL READY
and separately reviewed prerequisites. No source-only design approval or fake
PASS authorizes credentials, SSH or a real SDK session.
