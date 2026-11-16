# Guarded Linux SDK loopback

## Local Windows qualification — 16 November 2026

The example passes **9/0/0** on Windows stable 1.99.1 and Rust 1.88.0: seven
memory tests and two real codec/SDK loopback tests with fake admission. Workspace
stable all features passes **406/0/2**, feature-off **38/0/0**; global formatting,
strict CLI Clippy all targets with audio, and the example build pass. The 178
selected source files are unchanged during qualification; the original release
CLI is unchanged. These groups are per command, not distinct tests to add.

The actual Windows executable passes silent no-op (exit 0, no output) and
unsupported-target rejection (exit 1, exactly `PRECRED_FAIL_HARDEN
`, 20 bytes),
both under owned-job supervision. Neither executes native Linux hardening.
[Typed evidence and hashes](../evidence/rust-linux-guarded-loopback-windows-20261116.json).
Native Linux opt-in and remote CI are not executed at this checkpoint. An explicit
example-test CI step is added; the default workspace command alone does not run
these example tests.

Main applied the focused review corrections before qualification: a formatting-
robust tripwire, exact typed late gRPC rejection with one independently drained
peer, exact environment-name allowlist, scoped non-Linux annotations, descriptor
startup caveat, 30-second test envelope and an exactly-once upstream close count.
No production defect or optimization is claimed.

## Initial source-only checkpoint (historical)

16 November 2026. **Not compiled, formatted, tested, or natively executed by this
source-only task.** Main owns execution, qualification and publication. Linux live
remains **NO_GO**; historical G1 and all budgets are unchanged. The Windows one-case
opportunity remains consumed.

## Distinct scope

`crates/rambler-cli/examples/linux_guarded_loopback.rs` is a development example,
not production integration, a credential receiver, or another standalone canary
measurement. It connects the actual native `Pcm16Opus::new_with_progress`, Engine,
and gRPC transport to an in-process plaintext numeric-loopback tonic peer. It does
not use another ASR engine. The only CLI opt-in is `--fixed-canary-v1`; no endpoint,
credential, file, audio, microphone, context, or result argument is supported.

The separate frozen reference
`tools/reference/linux_precredential_canary_20261116.rs` is untouched. Its reported
Linux 1.88 measurement (12 fakes, four public native cases, three external environ
open denials/root owners, verified cleanup) does **not** qualify this new SDK
example. Its frozen source SHA-256 is
`a53b95f0e43231d93f7ffaed8b771dc93ef5813202b197af4eedc204fc40a11b`.
See `tools/reference/LINUX_PRECREDENTIAL_CANARY.md` for that experiment's limits.

## Admission and output contract

No arguments means silent success without proc reads, environment scan, hardening,
stdin, runtime, source/provider construction, or sockets. Only Linux x86_64 has a
native admission implementation. An explicit invocation on other targets fails
with `PRECRED_FAIL_HARDEN\n` before readiness, stdin, runtime, or sockets. Unexpected
arguments fail closed; on Linux their exact validation follows hardening and the
other pre-read checks, as in the reference.

Before creating a Tokio runtime or constructing audio/metadata providers, the
requested path performs these ordered checks:

1. Set `PR_SET_DUMPABLE=0` through the existing Unix libc dependency; verify
   `PR_GET_DUMPABLE==0`.
2. Read bounded `/proc/self/status`, requiring exactly one zero `TracerPid`.
3. Check open standard descriptors at `main` and exact proc enumeration names
   0/1/2/3, with fd 3 belonging to the directory after the status handle drops.
   Extra or malformed names fail. Rust startup may reopen closed standard fds on
   `/dev/null`: this does not prove the caller originally supplied them. The
   supervisor must provide the intended pipes and require exact readiness/output.
4. Admit only environment names `PATH` and `LC_ALL`; require the sole exact
   argument `--fixed-canary-v1`. Fixed values and an entirely secret-free launch
   remain supervisor preconditions.
5. Flush `PRECRED_READY_V1\n`, then recheck dumpability before raw stdin reads.
6. Request one byte at a time through libc `read(0, ...)`, with at most **27 total
   requested bytes**, including the EOF probe. Admit only the exact public
   26-byte `RAMBLER-PRECRED-CANARY-V1\n` followed by EOF. No generic metadata or
   credential payload is accepted.

A clear, entirely secret-free environment is a **supervisor precondition**, not a
claim that the program detects every secret. Rust `vars_os()` internally snapshots
values, even though this code examines only names. Arguments must also contain no
secrets. Memory is not securely zeroized. No process hardening is applied by the
automatic tests: they use a minimal in-memory Admission fake instead.

Only after admission does the example construct three fixed invented Metadata
values, bind **numeric `127.0.0.1:0`**, derive `TransportConfig::loopback` from that
listener, connect one transport, and start one Engine session. It generates 1017
mono 16 kHz signed-16-bit samples in memory (2034 bytes). It never opens audio or
private fixtures. There is no retry, replay, credential refresh, real receiver,
cloud mode, Google route call, or arbitrary endpoint mode.

Success additionally requires:

- independently drained peer HTTP upload, exactly one RPC and exact three invented
  metadata values;
- exact initial request, only nonempty audio wrappers, one fixed context, exact
  S3 END, and no requests after END (elapsed wire values are read back, not timed);
- complete Ogg framing without gaps/trailing bytes, serial/sequence/BOS/EOS checks,
  independent Ogg-reader CRC checks, and actual Opus decode with pre-skip/final
  granule coverage of 1017 samples;
- encoder progress `input_bytes=2034`, `encoded_input_samples=1017`, natural input
  and output EOF; independent upstream EOF and exactly one close receipt;
- strict rendering of the single fixed synthetic result, in memory only, never
  printing its text; the collector's successful terminal EOF, cleanup metrics,
  S3 END and healthy terminal gRPC status;
- awaited successful Engine close, transport close and local-server shutdown.

The terminal success line is emitted **only after** those conditions. Native
stdout is at most two fixed lines / 64 bytes; errors use fixed categories only:
`HARDEN`, `VERIFY`, `TRACED`, `FD`, `ENV`, `ARGV`, `IO`, `SDK`, `INTERNAL`, or the
fixed `PRECRED_REJECT_PAYLOAD` line. Exit status is zero only for no-op or complete
success, otherwise one. No paths, IDs, raw errors, text, metadata, or PCM are
logged. A failed/partial writer is never followed by another output attempt.
Ordinary panic diagnostics are silenced only by the executable's `main`, not by
tests. Fatal native/runtime failures and external termination are not covered by
that ordinary-unwind contract.

The server owns its JoinHandle, cancels/aborts on drop and joins on normal close.
Connect, collection, Engine close, transport close and server close each have a
five-second watchdog; test envelopes are 30 seconds. All closures are attempted
after a started session, even on collection failure. There are no sleeps, timing
benchmarks, subprocesses, or intentionally detached tasks. Watchdogs require
cooperative runtime progress; raw stdin/EOF withholding and native stalls require
an **external** supervisor deadline and verified cleanup.

## Commands for main — not executed here

From the repository root, using existing locked/offline dependencies and native
Opus build prerequisites:

```sh
cargo +1.88.0 test -p rambler-cli --features audio --example linux_guarded_loopback --locked --offline
cargo +1.88.0 build -p rambler-cli --features audio --example linux_guarded_loopback --locked --offline
```

The first command is safe to qualify on Windows and Linux: it runs seven pure
memory tests and two synthetic loopback tests, **not native process hardening**.
Cargo/test harness output is separate from the executable's fixed-stdout contract.

Exact test names, under `tests::`:

1. `default_noop_never_enters_admission_or_sdk`
2. `admission_failures_precede_all_source_provider_and_socket_actions`
3. `exact_canary_and_eof_are_required_before_action`
4. `io_failure_and_failed_action_cannot_emit_success`
5. `fixed_admission_parsers_fail_closed_in_memory`
6. `request_order_and_corrupt_ogg_reject_in_memory`
7. `fixed_source_has_no_real_receiver_or_arbitrary_endpoint`
8. `real_sdk_fixed_pcm_opus_loopback_is_healthy_only_after_cleanup_and_close`
9. `real_sdk_late_grpc_failure_never_becomes_canary_success`

The ninth test requires the exact typed terminal gRPC `PERMISSION_DENIED`,
one peer call and independently drained upload, not merely a generic SDK error.
The peer supplies a synthetic final and S3 END before that status; connection,
codec or timeout failures cannot satisfy this oracle. The whitespace-normalized
source tripwire checks dumpability stays zero even after formatting. Tripwires
and fake action counters remain regression aids, not security proofs.

For a **supervised Linux x86_64 child only**, the executable command is:

```text
/absolute/repository/target/debug/examples/linux_guarded_loopback --fixed-canary-v1
```

The supervisor must clear the environment (only fixed public `PATH=/usr/bin:/bin`
and `LC_ALL=C` if needed), provide exactly fds 0/1/2, own a hard overall deadline,
retain child/bridge ownership and reap/verify cleanup. Do not launch an unsupervised
shell pipeline or run native mode automatically in the test suite. Readiness comes
before Tokio or socket creation, so external observation may be performed while
stdin is held. Only after exact readiness and the supervisor's required external
checks may it send the public literal plus EOF. Never send credentials/private
input. Expected successful child stdout, exactly 38 bytes:

```text
PRECRED_READY_V1
PRECRED_CANARY_OK_V1
```

A fixed invalid public input plus EOF instead ends in `PRECRED_REJECT_PAYLOAD`;
withheld EOF never reaches the SDK and must be killed/reaped by the supervisor.
Default no-op must be tested separately with no arguments and empty stdout.
Unsupported-target rejection must occur without any stdin read or listener.
Do not infer that the reference's previously measured native cases automatically
pass for this binary.

## Outstanding compilation/qualification checks

No builds/tests/processes/system queries were run in this task. Main must run
rustfmt and the commands above, verify Rust 1.88 plus the project's normal native
platform/toolchain matrix, existing opus build availability, dev-dependency
availability to the example, tonic raw-service generic compatibility and Ogg
reader/decoder behavior. The source follows existing local-service and audio
example APIs but this is not a compilation claim. No dependency, feature, lockfile,
production source, or frozen-reference change is intended; Cargo adds only one
`[[example]]` requiring `audio`.

## Residuals and non-claims

G1 remains unmet. No core-producing crash, kernel pipe-helper dispatch test,
`core_pattern`/Yama/sysctl inspection or modification, durable provisioning,
authentication, authorization, attestation, quota, ASR quality, service parity, or
performance qualification is established. No safety/auth/TLS/quota gate is bypassed
or relaxed. Plaintext loopback establishes **no TLS trust-root loading, TLS
handshake, hostname verification, or real Google service behavior**.

Proc/dumpability checks are point-in-time, not a race-proof isolation boundary.
Root, `CAP_SYS_PTRACE`, WSL root reachable by the same Windows user, VM memory,
swap, core/pipe relay policy, WER, supervisor/relay memory, inherited kernel pipe
buffers and fatal runtime failures remain outside this protection. Local peers
can interfere with an unauthenticated loopback port; this is not an authenticated
receiver. A Windows bridge job does not by itself contain Linux descendants;
ownership-checked Linux supervision and cleanup remain separate requirements.
Nothing reopens the consumed Windows case, weakens historical G1, grants Linux
live permission, consumes/reserves real service budget, or validates private data.
