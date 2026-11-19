# Contributing to Sambler

## Before you start

Read the [status log](docs/STATUS.md), [decisions](docs/DECISIONS.md), and
[protocol](docs/research/protocol.md) and [authentication](docs/research/authentication.md)
notes. Label static evidence, inference, and tested behavior explicitly. S3
refers to the **Google Speech S3** protocol.

## Local development

Engine development takes place in the **Rust** workspace. Python 0.3 serves as
the conformance reference. See the [Rust guide](docs/rust.md) and the
[porting contract](docs/architecture/portage-rust.md).

```sh
cargo fmt --all -- --check
cargo test --workspace --all-features --locked
cargo test -p rambler-cli --features audio --example session_reuse --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --release --locked -p rambler-cli --features audio
```

Keep Rust tests offline or on loopback, including backpressure, cancellation,
late errors, and cleanup. Test full sessions in addition to protocol tests and
result fixtures. Keep dependency snapshots from worktrees out of the reference
implementation.

- Use Python 3.11 or later and a local virtual environment for the reference
  engine and tools.
- Keep local tests offline. Use a gRPC loopback server when needed.
- Run live service tests explicitly, with limits on duration and attempt count.
  Obtain consent before sending personal audio.
- After an error, avoid automatic session replay. An ambiguous server state
  can produce duplicate results.
- Keep cookies, tokens, personal audio, raw logs, and metadata captures out of
  Git. Label synthetic fixtures.
- Treat downloaded APKs and source files as untrusted. Store them in separate
  directories and use them for analysis only. Run Python analysis scripts in
  isolated mode (`python -I`) from a safe directory. Leave extracted scripts
  unexecuted.

## Propose a change

1. Start a branch. Describe the problem and the expected level of evidence.
2. Include a test that reproduces the defect or exercises the new capability.
3. Document network fields with their source, artifact version or hash, and
   confidence level. Keep decompiled source out of distributed materials.
4. For optimizations, compare the baseline and variant under matching
   conditions. Use the same audio, settings, network, session count, and
   success criteria. Report failed sessions alongside timings. Describe
   client-observed latency as such.
5. Update the status log, reproduction commands, and limitations as needed.

The README contains installation instructions and tracks the current engine.
Research notes also cover capabilities that are still under investigation.
