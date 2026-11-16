# Native Linux public core-control experiment

## Result and scope — 16 November 2026

A supervised **public-only** experiment passed on a native Linux x86_64 host,
distinct from the host used for the historical WSL pipe-policy failure. The
previous WSL G1 result is unchanged. This is not a universal G1 qualification.
The [sanitized evidence](../evidence/linux-public-core-control-20261116.json)
records the artifact pins and results.

Rust 1.99.0, host `x86_64-unknown-linux-gnu`, built the standalone target with
edition 2021, `panic=unwind`, and optimization level 1. The separate pure driver
`--unit` invocation passed 25 checks without launching targets. The observation
invocation launched exactly two public targets and reaped both after SIGABRT:

- Positive control: a nonempty bounded core file and the raw `WCOREDUMP` bit.
- Protected case: no core file and no `WCOREDUMP` bit; opening its proc environ
  returned `EACCES`, without reading environ bytes.

The protected case **combines** dumpable=0 and core limit=0, so the core result
does not isolate dumpability's effect. The observer checked core metadata only,
not core bytes. Cleanup and the separate Windows Job-empty check passed. A later
read-only postverification reported the source, binary and helpers unchanged,
the cases directory empty, and the exact policy unchanged; it confirmed two total
public target launches. No experiment or tests were rerun for this publication.

The observation driver has separate per-invocation result fields: its `unit_ok`
is false and `unit_checks` is zero in observation mode. That is not a unit failure;
the successful 25-check unit run occurred separately during the build stage.

## Reproduction boundaries

The byte-exact public sources are
[`linux_g1_public_target.rs`](../../tools/reference/linux_g1_public_target.rs) and
[`linux_g1_public_driver.py`](../../tools/reference/linux_g1_public_driver.py).
Their original bodies and measured hashes are retained; only the filenames changed.
The actual tested binary and the machine-specific orchestration are not published.
A hash pins an artifact but does not itself prove source-to-binary provenance; the
reported linkage comes separately from the supervised build procedure and report.

This is **not** a command to run the driver unsupervised. A future reproduction
requires an independently reviewed, trusted external caller and Linux process
supervision, including bounded deadlines, ownership-checked child termination and
reaping, fail-closed cleanup, and a separate bridge/job cleanup check where used.
The original reviewed supervisors are not distributed here. Caller and supervisor
must establish a private, exclusively owned public workspace, prevent concurrent
same-user mutation, pin source, driver and newly built binary, and keep all inputs,
arguments, environment, output and working directories free of secrets. Do not
transport credentials, contact a service, or use a live mode. The trusted compiler,
linker, interpreter and kernel environment are assumptions, not a sandbox.

For a new public-only attempt, the supervised build uses an existing trusted native
Rust compiler, the options above, and runs the driver's pure `--unit` separately.
The caller independently verifies the output and all pins before observation. A
fresh binary need not hash identically to the reported binary; retain its own
verified build provenance and pin. Do not treat the old binary hash as a guarantee
of reproducibility across toolchains or hosts.

Before each launch and around each crash, the driver requires exact Linux x86_64
policy values `core_pattern=core` (literal, not a pipe), `core_uses_pid=1`,
`suid_dumpable=0`, and `yama/ptrace_scope=0`; it also checks process identity,
capabilities, tracer and signal state, and the initially soft-zero/hard-unlimited
core limit. Incompatible environments must fail closed, not be administratively
changed to fit. The positive target alone enables public dumping with a 16 MiB
core and file-size cap; the protected target uses core limit zero. A cleared fixed
public environment, pinned regular files, pidfd ownership, readiness, bounded
waits, exact two-case maximum, metadata-only core checks and cleanup are required.
Abort rather than retry or launch extra targets on a failed gate. Core artifacts
can contain process memory even in this public experiment; never insert secrets.

## Non-claims

The literal policy gate excludes a pipe-pattern host for this experiment; it does
not prove the universal absence of collectors. There was no independent kernel
trace or eBPF confirmation. No credentials, service calls, or global policy
changes occurred. Real credential channel, real SDK, TLS and service behavior are
not qualified, nor is the whole backend complete. **Linux live GO remains false.**
