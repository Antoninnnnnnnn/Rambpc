# Linux pre-credential fixed-canary reference

**Development-only, offline.** This is not part of the SDK or production CLI,
not an authentication mechanism, and not a credential receiver. Never send it
credentials, private text or audio. A passing run does **not** meet the historical
G1 gate or authorize Linux live testing.

The [single Rust source](linux_precredential_canary_20261116.rs) has no dependencies
and is outside the Cargo workspace. With no arguments it exits silently without
hardening or reading stdin. On unsupported targets, explicitly requested mode
fails closed before readiness or stdin. Linux x86_64 is the only native path.

## Reproduce the memory-only tests

From the repository root, using Git Bash/Linux shell and an installed toolchain:

```sh
mkdir -p target
rustc +1.88.0 --edition=2021 -C panic=unwind --test \
  tools/reference/linux_precredential_canary_20261116.rs \
  -o target/precredential-canary-fakes.exe
./target/precredential-canary-fakes.exe --test-threads=1
```

The `.exe` suffix is harmless for an ELF test executable on Linux and avoids a
platform-specific output name. These 12 fake-system tests launch no children and
do not execute Linux hardening syscalls. They cover call order, failed gates,
read/output limits, EOF, exact payload matching, fixed errors and a source tripwire.
The tripwire is not a general proof of security. Preserve `panic=unwind`; do not
use `panic=abort`.

## Native protocol — supervised public input only

Do not launch an unsupervised shell pipeline. A separate bounded, non-root
supervisor with no `CAP_SYS_PTRACE` must own the child, its deadline and cleanup.
The measured development harness also used a guest subreaper, ownership-checked
pidfds and a separate Windows bridge job; that job does not by itself contain
Linux descendants. The private harness is not a portable public runner.

Explicit mode is `--fixed-canary-v1`. Before emitting `PRECRED_READY_V1\n`, the
process sets dumpable to zero, verifies it, rejects a tracer, checks fds 0–2 plus
the proc-directory fd, rejects forbidden environment names, and checks arguments.
It rechecks dumpability after readiness and before reading stdin.

Only after exact readiness and external checks may the supervisor send the fixed
public literal `RAMBLER-PRECRED-CANARY-V1\n` (26 bytes, final LF), then close stdin.
Acceptance requires EOF; total requested stdin bytes are at most 27. The only
success line is `PRECRED_CANARY_OK_V1\n`. Other input is rejected; stdout is limited
to fixed lines, at most 64 bytes in the tested native cases. There is no service
or general payload action. A sender withholding EOF requires supervisor cleanup.

Native launch environment must contain **no secret**, including under unrelated
names. The measured native child received only fixed public `PATH`/`LC_ALL`.
Although only keys are inspected, Rust `vars_os()` internally materializes values.
There is no secure-zeroization guarantee.

## Observed result, 16 November 2026

[Typed evidence and hashes](../../docs/evidence/linux-precredential-canary-offline-20261116.json):

- Same 12 fakes pass on Windows stable 1.99.1, Windows 1.88.0 and Linux 1.88.0;
  these are repeated combinations, not 36 distinct tests.
- Linux no-op, exact-literal acceptance and fixed-invalid rejection pass.
- Three readiness-blocked children have root-owned proc `environ` entries; the
  independent unprivileged observer is denied opening them and never reads bytes.
- The held-EOF case is terminated through its ownership-checked pidfd with SIGKILL
  and reaped. This is not a core-producing crash test.
- Guest cleanup and the Windows bridge job are verified, with zero credential
  reads, service calls, reservations, phone actions or global setting changes.

The committed source bytes match the measured source. Binary hashes refer to the
private build location, not a reproducible-binary or full toolchain attestation.
Core CI for `1d66138` is separate: it does not compile this standalone reference.

## Limits and unchanged G1

Readiness implies the verified source's self-checks; the external probe observes
proc access/ownership effects, **not** actual core suppression or whether a pipe
helper is dispatched. No core crash is induced and no `core_pattern`, Yama or
sysctl setting is inspected or changed by this experiment. G1 remains unmet and
Linux live remains NO_GO. The previously consumed Windows opportunity remains
consumed; nothing resets any budget.

Root, `CAP_SYS_PTRACE`, the same Windows user through WSL root, VM memory, swap,
WER, relay processes and kernel pipe buffers remain outside this protection.
Fatal runtime failures and external termination can defeat cleanup guarantees.
No production integration, durable provisioning, service authorization, ASR,
performance, Android parity or perfection is established.

Kernel API background: [PR_SET_DUMPABLE](https://man7.org/linux/man-pages/man2/PR_SET_DUMPABLE.2const.html),
[core](https://man7.org/linux/man-pages/man5/core.5.html),
[ptrace](https://man7.org/linux/man-pages/man2/ptrace.2.html).
