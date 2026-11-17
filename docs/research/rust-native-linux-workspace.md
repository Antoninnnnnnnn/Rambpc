# Native Linux workspace qualification — 2026-11-16

**Result: pass, within the boundaries below.** This records an actual native, non-root `x86_64-unknown-linux-gnu` qualification of frozen commit `82e63941b0ed9895dfdd92a24d0a9cc0cbd23f42`. The [sanitized evidence](../evidence/rust-native-linux-workspace-20261116.json) is derived from the qualification report and the final execution notification; this documentation is an audit of those records, not a rerun. Existing project reservations remain unchanged.

## Results and provenance

Rust and Cargo were 1.99.0; portable CMake 3.31.10 was SHA-256 verified. There was one public locked-dependency fetch attempt; subsequent Cargo work was offline. The workspace all-features run recorded **430 passed, 0 failed, 2 ignored**, across 70 libtest groups. The release audio CLI built successfully.

Additional checks were subgroups or replays, not a separate aggregate to add to 430:

- A debug test harness against a frozen copy of the real release CLI passed four checks: one finalization and three invalid-budget checks.
- Guarded SDK: nine passed. Session reuse: ten passed.
- Direct public-roots libtest: one passed, 44 filtered. This is an OS public-roots smoke check, not a TLS handshake or service-authentication test.

The source snapshot contained 120 files totaling 1,443,953 bytes; its recorded pre- and post-run source hashes matched. The new uncommitted Linux admission example was not part of frozen HEAD and this run makes no claim about that new source, its protections, or credentials.

| Artifact | SHA-256 |
| --- | --- |
| Source archive | `ace671cc5cb827810f148d7358538db401a5795789757a5a58b431e53c4d96e2` |
| Source manifest | `8bfb0c482338d9aba7ccc4ec595ecdd33e915675b80bdf0a3ee35d968941dcf8` |
| Source pre/post | `99d2915a754c01ed2d0d4a23df49f831c2affc59da4519fde1991cc845bb5afa` |
| Portable CMake | `576c050dab1e1418b6703b5cfb523330567683dad0c60a5ff9cc23128143812e` |
| Release CLI | `f7a48b0980122f1e2d0e80d7d1780bc2067f28b7d97f244211b9f2342e544f20` |
| Native-roots test binary | `702212c2820d25ef07875187568dfdc4a30bab5dda0c644ea24a9adc3b367452` |

The qualification elapsed time was 293.647 seconds. It describes this run, not a performance gain or backend benchmark.

## Execution boundaries

No network namespace or OS-level egress block was used. The no-RPC conclusion rests on verified public code, a cleared environment, and loopback test configuration; it is not a general network-isolation guarantee. The report records no credential access or credentials sent, no live service, and `live_go: false`; no actual ASR was performed. This evidence is not a general claim about all features, platforms, or production readiness.

Caller-controlled supervision used reviewed pidfd/subreaper cleanup and 16 owned helpers. The report records cleanup, successful bridge capture, an empty Windows job and no cancellation; the final outer Bash notification reported exit status zero. The original supervision and helper sources were caller-controlled and are not distributed here; hashes in the evidence identify the recorded versions, not publicly available reproduction scripts.

## Preparation history and reproduction frame

Before the first run, review blocked the initial supervisor because Cargo could create two hard links; the permitted link-count range was corrected to 1–2. A planned Cargo launch of the roots test was replaced by qualified public helper selection and direct execution of the frozen test binary to avoid Cargo environment injection. Eight pure main-patch checks and all 16 owned helpers were byte-identical before execution. An initial patch unit fixture failed on Windows-versus-POSIX path handling before any patch write or native execution; `PurePosixPath` corrected the fixture. There was no actual SDK retry or duplicate dependency fetch.

A comparable independent reproduction requires the exact frozen source and lockfile, a native non-root GNU/Linux x86-64 environment, Rust/Cargo 1.99.0, verified portable CMake 3.31.10 and appropriate native build prerequisites. It also requires a controlled public locked-dependency fetch followed by offline Cargo operation, an explicitly cleared and reviewed environment with loopback test configuration, a frozen release executable for the debug harness, and independently reviewed process supervision and cleanup. Select the qualified roots test artifact and execute its frozen binary directly rather than using Cargo as its launcher. The caller-controlled supervisor used for this run is not published, so these are prerequisites and an execution frame, not a complete turnkey reproduction recipe. Do not supply service credentials or treat this frame as authorization for a live service call.
