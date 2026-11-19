# Native macOS Intel qualification

## Corrected validation — November 15, 2026

On `171ef084cc1b1e7f3b82daefdcd6bb0aadafe442`, both native Intel jobs (Rust 1.88.0 and stable 1.100.0) pass, with 403/0/2 in the workspace, then 10, 1, 3, and 38 passes in the four replayed groups. The group of three checks rejection of zero budgets on the release binary; the close regression also passes. The first bounded close can return `Ok` or the independent `Connect` error; the second must succeed. The previous run `7327f51` remains historically failed on three macOS jobs because of the old close assertion. This success does not change that result. [Evidence and provenance](../evidence/rust-close-ledger-zero-budgets-ci-20261115.json). No evidence of a minimum macOS version or real service.

## Native matrix executed — November 15, 2026 (historical)

On commit `4264126852b2c03e25fc231c60f9582569a84ae9`, both GitHub `macos-26-intel` jobs passed with Rust 1.88.0 and stable (1.100.0), observed host `x86_64-apple-darwin`, image `macos-26`, CMake 4.4.2. Each reports 400 workspace tests passed, 0 failed, 2 ignored; reuse 10, exact release finalization 1, and feature disabled 35. The public roots smoke test passes through the direct launcher; synthetic release PCM reports 1017 input samples, 3051 decoded at 48 kHz, one EOS, and zero service calls. [Details and provenance](../evidence/rust-pcm-admission-expanded-ci-20261115.json). This qualifies these runs, not a minimum macOS version, a real handshake, or service/Android parity.

## Matrix added — historical pending checkpoint

The Rust matrix adds the explicit standard runner `macos-26-intel`, for Rust 1.88.0 and stable, without removing `macos-latest` (ARM64 in previous qualifications). No paid `-large` label is selected. Existing steps remain identical: synthetic/loopback tests, public OS roots configuration without a connection, session reuse, release CLI and release PCM, audio feature disabled. No Rambler credentials or calls are needed.

At this checkpoint, the CI change did not yet qualify Intel: the conclusions of the two jobs and their logs on the exact commit still had to be awaited. Earlier ARM64 successes do not replace them. It also does not establish the minimum macOS version supported by a distributed binary, Android parity, or a real handshake/service on macOS. The local results and limitations of the previous PCM/admission batch remain in [its evidence](../evidence/rust-pcm-progress-admission.json).

## Label sources consulted November 15, 2026

- [GitHub standard runners for public repositories](https://docs.github.com/en/actions/reference/runners/github-hosted-runners): macOS Intel row, labels `macos-15-intel` and `macos-26-intel`.
- [Available images and inventories](https://github.com/actions/runner-images#available-images): macOS 26 x64 row and associated inventory. No GA/Beta status is inferred from the absence of a badge.
