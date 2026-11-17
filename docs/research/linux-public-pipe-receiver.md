# Linux public pipe receiver — source-only contract

**No native/SSH qualification.** The source passed independent review and
**31 fake-only tests on Windows** in a bounded owned Job on 2026-11-17. No native
receiver invocation, SSH roundtrip, build, service/device operation or campaign
replay was performed for this bridge. The existing native qualification is a
separate historical observation, not proof of this bridge. Main owns any later
joint Windows/Linux review and explicit public-only roundtrip. No automatic
replay is permitted. [Observed QA and limits](../evidence/linux-public-pipe-receiver-qa-20261117.json).

## Scope and exclusions

`tools/reference/linux_public_pipe_receiver.py` is a narrow developer bridge for
a future owned-stdin Windows sender. It accepts exactly one invented public RLA1
message and EOF, forwards the **compiled-in public constant**, and closes the
native input. It has no arbitrary-frame API, live/single-session mode, endpoint,
network, ASR, credential input, callback, RPC or session reuse. It never enters the
Rust single-session path. A real-credential caller would require a separate design.
`healthy=false`, zero sessions and zero service calls are invariant, including on
success. They are not independent network/syscall telemetry or production claims.

Only these new files belong to this change:

- `tools/reference/linux_public_pipe_receiver.py`
- `tests/test_linux_public_pipe_receiver.py`
- this document

The qualified helper is immutable. Its CLI, build/observe functions, historical
reservations, results and counters are never invoked, read, adopted or modified.
The module's fresh in-memory initialization is not a historical campaign update.
No Windows helper, fixture, documentation, CI or STATUS file belongs to this work.

## Trusted startup and input layout

Future execution requires a trusted Linux x86_64, non-root, equal real/effective
UID and GID, single-threaded CPython started with `-I -S -B`. The trusted launcher
must establish a genuinely public-only environment and arguments **before Python
starts**, not sanitize secrets after interpreter startup. The receiver requires
exactly `PATH=/usr/bin:/bin` and `LC_ALL=C`; no other environment entries. It does
not report these values. Interpreter/stdlib/dynamic-loader trust remains external.
The trusted launcher must independently pin the receiver before invoking it; its
self-check is not a defense against executing a substituted receiver first.

Use a separate, explicit public staging root, never an old campaign directory:

```text
/home/PUBLIC_USER/public-receiver/                 0700, caller UID
  owner.json                                     0600, single link
  inputs/                                        0700
    source-manifest.json                          0600, single link
    source/                                      all directories 0700
      ... exact manifest file set ...            all files 0600, single link
    public.pcm                                   0600, single link
    example-input                                0500, single link, known ELF
  cases/                                         0700
```

The root and `inputs` entry sets are exact; source membership is exact. Existing
`cases` entries are not opened or adopted. Each invocation makes exactly one new
`public-pipe-case-<32 lowercase hex>` directory with `mkdir`, without retries.
All intermediate path components are opened no-follow and must be root/caller
owned and not group/world writable. Final public directories are caller-owned
0700. Every input regular file is caller-owned, single-link, exact-mode, bounded,
SHA-256 checked and stable across its read. No symlink component, path traversal,
relative path, shell expansion, space/control character or non-ASCII component
is accepted. Components use `[A-Za-z0-9_.+-]+`; private/key/capture/config paths
such as `.claude`, `.ssh`, `.env*`, `keys`, `private`, `secrets` are rejected.
A directory under `/tmp` with a world-writable ancestor cannot satisfy this
contract. Staging preparation is external; this source does not provision it.

The root marker has exactly this JSON object (whitespace/order arbitrary, no
duplicate keys, exact JSON types). The basename must match the actual root:

```json
{"schema":"linux-public-pipe-receiver-owner-v1","name":"public-receiver","public_only":true}
```

Its raw-file SHA is a mandatory CLI pin. The receiver script and helper are also
owned single-link 0600 public files at absolute trusted paths. There are exactly
four mandatory options, each as a separate option/value pair, in any order:

```text
--public-root ABSOLUTE_PUBLIC_ROOT
--owner-sha256 LOWERCASE_64_HEX
--receiver-sha256 LOWERCASE_64_HEX
--helper ABSOLUTE_PUBLIC_HELPER_PATH
```

Duplicates, attached `--flag=value` syntax, omissions, unknown options, modes,
`--help`, `--unit`, payloads and binary/source pin overrides are rejected. Errors
do not echo options. Neutral **illustrative only, not an execution instruction**:

```sh
/usr/bin/env -i PATH=/usr/bin:/bin LC_ALL=C /usr/bin/python3 -I -S -B \
  /home/PUBLIC_USER/public-tools/linux_public_pipe_receiver.py \
  --public-root /home/PUBLIC_USER/public-receiver \
  --owner-sha256 OWNER_FILE_SHA256 \
  --receiver-sha256 RECEIVER_FILE_SHA256 \
  --helper /home/PUBLIC_USER/public-tools/linux_admission_public_supervisor.py \
  2>/dev/null
```

The placeholders for digests must be replaced by independently reviewed hashes;
the shown command does not supply valid digests. Stdin and stdout must be owned
anonymous pipe arrangements supplied by the future sender/launcher, not a TTY,
socket or regular file. The receiver verifies FIFO type and initial blocking
state; ownership of the upstream sender remains the launcher's responsibility.
Stderr must refer to `/dev/null`. The upstream pipe is unbuffered at this layer.

### Fixed qualified public pins (no CLI overrides)

| Input | Exact pin |
|---|---|
| Original helper | `bb711f7ce1ada062cee9ed47136ae0496c38d8095d8ced26229bb2f8328ec0d9` |
| Existing qualified Debug example input, 70,095,512 bytes | `1d183b7adece6c2a05ab15a0d8bd5faeaab7bc855cbb89b1091ced81f3883991` |
| Public PCM, 117,440 bytes | `cea1bc1614f738048b1a4f1bcad83f70aee42b9621b8b754f8aacbe1395b08c9` |
| Public source manifest | `50a8a6f2a81b0e9e3abe60d88f9078ee1ab437a98741cc8d5468b126036ed284` |
| Example source entry | `fae7a6b088f3d1a7f6bd7b3f795ae99a3ec119ca6d029cb2ecf9fdf811c2c8c6` |
| Fake test source entry (pin only, never executed) | `269fc87b38482f1185595180039768721dbdcd7c7838f145add245e241cdeb0f` |
| Entire qualified source-tree fingerprint | `1e0fe2437674b6992351bf7e322129ccb64c3a0d7b552cb2188f9a8dbd5bec39` |

The manifest uses the helper's exact `linux-admission-public-source-v1` schema:
`{"schema":...,"files":[{"path":...,"size":INTEGER,"sha256":...},...]}`.
It is accepted only at the fixed raw manifest SHA above. All entries are checked;
no source listing, content or source digest is put into result records. Existing
qualified source bytes include their historical line endings; do not normalize.
The PCM is the previously qualified public raw s16le mono 16 kHz fixture, not
microphone/device input. The example is **existing qualified Debug binary input**;
copying/freezing it does not constitute or claim a fresh build.

The helper is read with bounded, no-follow, owner/mode/link/hash checks. Only the
verified bytes are compiled/executed in a fresh private module namespace; there
is no path-based re-import or alternate-helper fallback. Its twelve reviewed
block functions are retained together (`prctl`, `absolute`, `open_path`,
`directory`, `pinned`, `exclusive`, `empty_children`, `ownership`, `drain`,
`child_exec`, `run`, `read_public`). Exact additional APIs are explicitly checked.
Relevant reviewed functions are reused directly or transitively; build-specific
`run`/`child_exec` are deliberately never called. The helper CLI is never called.

## Protocol and ordering

1. Before any frame read, require trusted startup and call reviewed `initialize`:
   core soft/hard limit zero, dumpable zero, stable-parent `PDEATHSIG=SIGTERM`,
   subreaper, no children, cancellation handlers. Set `no_new_privs=1`; query
   dumpable/no-new-privileges and core limits on guards. A parent death or handled
   cancellation blocks success. No environment content is read from procfs.
2. Validate root marker, all source/manifest/PCM/binary/receiver/helper pins,
   modes, ownership and links, and the helper's exact expected core/Yama policy.
   No sysctl or global policy is changed. Missing support/policy fails closed.
3. Create the fresh case namespace and O_EXCL owner/result records. Freeze a
   fresh, independent single-link 0500 copy of the fixed example. Copy only the
   fixed public PCM into the new case as an O_EXCL 0600 file. No old reservation
   or build report is consulted. A new empty `target-cwd` belongs to this case.
4. Use a fork gate. Acquire a pidfd and prove direct unreaped ownership with
   reviewed `ownership` before writing gate `G`. On acquisition failure the gate
   is never released; gate EOF/alarm and reviewed drain handle cleanup. Invoke
   reviewed `target_exec(..., False)`, with **no argument-mode override**. It uses
   reviewed `restore_target_signals` for every catchable signal, clears the mask,
   sets child core/file/CPU limits and parent-death guard, and constructs:
   - fd 0: blocking anonymous native input pipe;
   - fd 1: native output pipe;
   - fd 2: `/dev/null`;
   - fd 3: this case's owned fixed public PCM;
   - no inherited descriptors above 3; fixed public environment;
   - argv exactly `[FROZEN_EXAMPLE, '--public-frame-check-v1']`.
5. Match native READY exactly. Require no pending extra output. Call unchanged
   `observe_before_write`: living direct child via pidfd, stable start identity,
   UID/GID/parent/thread/tracer/capability/signal/no-new-privileges status, limits,
   policy, and `/proc/<owned-child>/environ` **OPEN returning EACCES**. On an
   unexpected successful open, close the handle without reading a single byte
   and fail closed. Only then emit upstream READY.
6. Read only bounded upstream stdin, with a deadline and mandatory EOF. Do not
   decode, log, hash, persist, return or forward incoming bytes. Partial input
   must always be a prefix of the sole fixed frame. At most 86 bytes are read;
   trailing/oversize/private-like/wrong/truncated input rejects locally. Exactly
   85 bytes without EOF times out and is never forwarded. There is no fallback.
7. Repeat the same full external observation, including a second independent
   EACCES open, **immediately before** a guarded write of the compiled-in 85-byte
   constant. Close the native write end for mandatory EOF. No input-dependent
   frame is passed to this method; it takes no payload argument.
8. Match the exact native success terminal, then native stdout EOF and owned
   child exit 0. Extra output, truncated output, missing EOF or nonzero exit
   rejects. Require the native working directory to remain empty and policy
   unchanged. Close all pipes, perform reviewed pidfd/subreaper drain and prove
   actual `ECHILD`, not just a kill attempt. Recheck source, owner/receiver/helper,
   original/copy binary and original/copy PCM pins, modes, links and directories.
9. Persist/fsync/close the new result record. Only after native proof, full drain,
   pin/source recheck and persistence may upstream success be emitted. Any
   failure/cancellation blocks the terminal and yields a nonzero exit.

### Exact wire bytes

Native and upstream READY are the same 24-byte line:

```text
REAL_ADMISSION_READY_V1\n
```

Here and below `\n` denotes one LF byte, not a literal backslash-n. The input is
85 bytes, without newline: eight-byte magic `52 4c 41 31 00 0d 0a 1a`, then three
two-byte unsigned big-endian lengths followed by their ASCII values, in order:

| Length | Invented public value |
|---|---|
| 25 | `invented-public-frame-key` |
| 20 | `invalid.public.frame` |
| 26 | `invented-public-frame-cert` |

The total is `8 + 6 + 25 + 20 + 26 = 85` bytes. The frame is binary and must
not acquire a newline, text encoding conversion or optional extra fields.

Success is exactly the following **153-byte** line, including its one LF:

```text
REAL_ADMISSION_TERMINAL_V1 PUBLIC_FRAME_CHECKED primary_errors=0 cleanup_errors=0 sessions=0 healthy=false pcm_bytes=0 pcm_eof=false source_closed=false\n
```

Each upstream emission is one nonblocking `os.write`, well below Linux PIPE_BUF:
all bytes or an error, with no Python output buffer left to flush. A blocked,
broken or partial output cannot produce a successful return. No Rust stdout or
stderr is forwarded verbatim; the terminal is independently matched and emitted
as the receiver's constant only after cleanup. Upstream EOF follows process exit.

Errors use only this fixed grammar, with exit 1:

```text
REAL_ADMISSION_ERROR_V1 CODE\n
CODE := STARTUP | NATIVE | INPUT | TIMEOUT | CANCELLED | CLEANUP | PINS | RECORD | OUTPUT | INTERNAL
```

An error may occur before READY or after READY, but never warrants a success
terminal. Receiver stderr is only fixed `PUBLIC_PIPE_RECEIVER_FAILED_V1\n` and
is normally discarded. No exception/Debug/OS text, argv/env, raw native text,
input, input hash or source metadata is logged, persisted or returned. Abrupt
SIGKILL/interpreter/kernel failure may produce EOF without a protocol error;
the sender must treat missing success/exit 0 as failure, with no automatic retry.

### Deadlines and parent-only nonblocking I/O

No deadline CLI overrides exist. The native exchange has a 15-second parent
budget after gate release; upstream frame plus EOF has at most 5 seconds and
never outlives that native budget. The unchanged launcher has a 3-second gated
alarm, 20-second post-gate alarm and 10-second CPU limit; the supervisor sets a
30-second cancellation alarm after initialization. Drain has the helper's fixed
6-second budget. Expiry is failure, not a reason to retry. Parent stdin/output
and the parent-owned pipe ends may be nonblocking; the native input read end
remains blocking. The parent watches pidfd while waiting for upstream input, so
an early child exit cancels the read rather than forwarding later.

## New result schema and ownership

Every new case owns `owner.json`, `result.json`, `example`, `public.pcm` and the
empty `target-cwd`. New files use O_EXCL/no-follow; a collision is terminal. The
case owner is exactly `{"schema":"linux-public-pipe-case-owner-v1","public_only":true}`.
Result JSON is a closed public-only object containing just:

```json
{"schema":"linux-public-pipe-receiver-result-v1","public_only":true,"ok":true,"cleanup":true,"source_unchanged":true,"pins_unchanged":true,"external_checks":2,"public_frames_forwarded":1,"sessions":0,"healthy":false,"service_calls":0,"error":null}
```

Types are exact booleans, closed schema/error strings, counts in `{0,1,2}` for
external checks, `{0,1}` for forwarded public frames, and constant zero session/
service counts. On failure `ok=false`, `error` is a closed code, and flags/counts
reflect completed steps. A partial native write is a failed step, never a new
payload category. No bytes/lengths/hashes/paths/PIDs/source entries appear in
records. The result is a **local proof snapshot before upstream delivery**, not
a delivery acknowledgement: a later cancellation/broken upstream output still
returns failure and never authorizes a caller to rely on the record alone.
Failure to persist also returns failure, even if the file is incomplete.

Only this invocation's acquired pidfds and direct/adopted children are cleaned.
The helper never signals guessed numeric PIDs and never reaps before the pidfd
ownership gate. All descendants must be drained to ECHILD before success. The
case files are deliberately retained as public artifacts; this receiver has no
cleanup/adoption/replay mode for old cases. A maintainer may separately remove
public cases after the owning invocation has terminated.

## Source review and observed pure QA

The supplied tests import safely on Windows and Linux and inject fake kernels,
helper APIs, clocks, queues, pipes, pidfds and result sinks. No test invokes
`initialize`, changes real signal/resource/dumpability state, forks, launches a
native target, reads real PCM or opens child environ. OS-facing method tests
replace the complete receiver OS/helper port, not just a convenient syscall.
Tests cover protocol ordering/two external proofs, all truncations and single-byte
mutations, mandatory EOF/trailing/oversize/private-like rejection, deadlines,
cancellation/early exit, broken writes, native terminal/EOF/exit validation,
fork-gate ordering, pin/mode/link/ownership/stability failure, post-cleanup pin
failure, cleanup failure and record failure. No path can report healthy=true.

Initial delivery checks parsed the sources, verified the fixed helper and API
pins, and counted 31 tests without executing them. Independent review then
returned source-only GO. Main-owned QA subsequently passed all **31 tests, zero
failures/errors/skips**, with exit 0, unchanged receiver/test/helper hashes and
an empty outer Windows Job. One earlier QA stage mistakenly required live GO and
ran no tests; a later controller stopped on a mistyped hash before launching a
Job. Both records remain failures/halts, not test passes.

To reproduce only the fake tests: `python -I -S -B tests/test_linux_public_pipe_receiver.py`.
They import the receiver but never invoke its native entry point. No Linux host
execution of this bridge, upstream sender, SSH channel or credential path is
qualified by these Windows fake tests. Those remain separate future public-only
checks after both ends are reviewed; earlier campaigns must not be replayed.

## Remaining trust and failure limits

This is not a sandbox, credential channel, durable provisioning solution, or
production engine admission. It assumes trusted CPython/stdlib/linker/procfs,
normal Linux pidfd behavior, no concurrent same-UID mutation, and no unrelated
children/reapers/threads. Root, kernel, same-UID attackers, swap and upstream
launcher/SSH buffers remain outside the guarantees. SIGKILL, uninterruptible
children and kernel failure can prevent cleanup; absence of ECHILD prevents
success but cannot repair the host. No new host action is authorized here.
