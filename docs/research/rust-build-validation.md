# Rust build validation

## Scope and prerequisites

The workspace declares Rust 1.88. The checked-in `Cargo.lock` is the dependency
snapshot for these builds; use `--locked` and review lockfile changes separately.
This gives repeatable dependency selection, not bit-for-bit reproducible binaries:
compiler, runner image, C toolchain, OS SDK, CMake and build environment can
change. The stable toolchain and `*-latest` CI images deliberately move. A
cross-platform pass is required when updating the lockfile or MSRV; do not assume
that a dependency's Rust version on one target proves compatibility on others.

Rust edition 2024 requires an edition-aware toolchain. Native builds need a C
compiler and linker, platform SDK, and CMake. The bundled Opus source in
`opusic-sys` is compiled locally; `ring` also uses native C. On Windows install
Visual Studio C++ build tools and Windows SDK; on Linux use GCC or Clang and
standard development tools; on macOS use Xcode command line tools. CMake must
be on `PATH` or provided in `CMAKE`. Even the CLI feature-off **tests** have
native audio dev-dependencies, so they still need these tools. A short target
path can avoid Windows MSBuild path-length failures.

For a clean local dependency cache, first run `cargo +1.88.0 fetch --locked`
(network required). Then run from the workspace root, without credentials:

```sh
cargo +1.88.0 test --workspace --all-features --locked --offline
cargo +1.88.0 test -p rambler-cli --no-default-features --locked --offline
cargo +stable fmt --all -- --check
cargo +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```

`--offline` applies after fetching; it does not prohibit test code from opening
loopback sockets. Tests use synthetic/local data and loopback, not a live Google
service or `.env`. The independent PyAV audio test is explicitly ignored unless
its separate Python prerequisite is arranged. These commands do not prove live
service interoperability.

## Executed locally

On 2026-11-13, Windows 11 x86_64 MSVC, rustup initially had stable 1.99.1 and
1.98.0, with only `x86_64-pc-windows-msvc` installed. Installed 1.88.0 using
`rustup toolchain install 1.88.0 --profile minimal` without changing the stable
default. CMake 3.31.10 came from an isolated unpacked wheel via `CMAKE`; Visual
Studio 2022 Community MSVC 14.44 and Windows SDK 10.0.26100 were present.
The lockfile was unchanged.

- `cargo +1.88.0 test --workspace --all-features --locked --offline`: passed,
  including audio and loopback; one independent PyAV test ignored. Cold build
  finished in 3m18s before test execution.
- `cargo +1.88.0 test -p rambler-cli --no-default-features --locked --offline`:
  passed, including the feature-off regression.
- `cargo +stable fmt --all -- --check`: passed.
- `cargo +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`:
  passed with rustc 1.99.1.

An initial all-features attempt with `CARGO_TARGET_DIR` under the long session
scratchpad failed in CMake compiler detection. The MSBuild log showed FTK1011
creating long tracking-log paths, not an incompatible C compiler or Rust
dependency. Retrying with the shorter workspace `target` succeeded. No dependency
or cryptographic downgrade and no MSRV increase was necessary for this Windows
locked graph. That result does not validate unselected target dependencies.

## Integrated refinement recheck

The parent then repeated both Rust1.88 commands on the integrated refinement
(safety decoding/policy, encoded-audio observations, event presentation and
adversarial tests): **186 tests passed** for the all-feature workspace, one
independent audio oracle ignored by default; the feature-off CLI passed too.
Stable1.99.1 also passed the workspace, all-target/all-feature Clippy, formatting
and the release CLI build with audio. The PyAV independent oracle was run
separately and passed57 complete containers. None of these local Windows runs
establishes Linux/macOS compatibility or live-service equivalence for new fields.

## Consolidated backend recheck — 2026-11-14

The current snapshot (Linux transfer R5) includes response containers/fallbacks,
progressive Ogg validation and the engine's primary/secondary shutdown-error fixes.

| Execution | Workspace | CLI example | CLI audio feature off | Build / lint |
|---|---:|---:|---:|---|
| Windows11 MSVC, stable1.99.1 |262 passed,1 ignored|4 passed|27 passed|Strict Clippy, formatting, release with/without audio passed|
| Windows11 MSVC,1.88.0 |262 passed,1 ignored|Not separately rerun on1.88|27 passed|Workspace compiled; release validator built with stable|
| Linux x86_64, WSL2 Ubuntu24.04,1.88.0 |262 passed,1 ignored|4 passed|27 passed|Native ELF release audio build passed|
| macOS |Not executed|Not executed|Not executed|No linked/runtime claim|

The independent ignored audio oracle was explicitly run on Windows1.88 and passed
57 complete containers with the installed development PyAV decoder. Python/tools
also passed656 tests (13.82s) with asyncio debug and faulthandler enabled. The CLI's
normal feature-off release build passed with a deliberately nonexistent `CMAKE`;
feature-off tests still have native audio development dependencies.

Linux used the already-running RelayTests WSL distro, GCC13.3 and isolated Rust1.88 /
CMake3.31.10 tools with no profile/global installation changes. Source and fixtures
were transferred without credentials into an isolated Linux directory; the distro's
`/mnt/c` was not a Windows mount. No phone/Android/VM relay or live Google calls
were used in these tests. Earlier8MiB fixture timeout and obsolete driver-error
expectation failed before targeted fixture/contract corrections; R5 is the successful
current-code run, not a relabelling of those failures.

Reproduce the example tests explicitly (the ordinary workspace command does not
run them):

```sh
cargo test -p rambler-cli --features audio --example session_reuse --locked --offline
```

## Shared raw / file preflight / recognizer options recheck

The next integrated snapshot R6 passed **274 workspace tests** (one independent
audio oracle ignored by default), four reuse-example tests and30 feature-off CLI
tests. This ran on Windows stable1.99.1 and Linux1.88; Windows1.88 separately passed
the274 workspace tests and30 feature-off tests. Strict all-target Clippy, formatting,
feature-off release with deliberately invalid CMake, release audio and the native
reuse-validator build passed. Linux release audio passed too. No remote CI or
macOS run is implied.

The Linux FIFO CLI probe changed from externally killed exit124 after2s to immediate
audio-error exit1 with53 bytes of JSON on both commands. Windows named-pipe testing
first exposed a remaining metadata-only check failure; the final descriptor-type
check passed with a fresh no-data pipe instance per command. See
[rust-file-inputs.md](rust-file-inputs.md). No credentials or live RPC were used.

## Written CI matrix, not yet executed

`.github/workflows/rust.yml` defines Windows, Linux and macOS jobs on 1.88.0 and
stable, fetching the locked graph first and then testing offline with and
without the CLI audio feature. A separate stable Linux job checks formatting and
all-target, all-feature Clippy. Hosted runners must provide CMake and a native C
toolchain. The matrix has **not** run remotely; local Linux execution is reported
separately above, and macOS remains unexecuted. Runner images and actions are not
immutable, and fetch/setup needs network. No remote CI, push or PR was triggered
by this validation; code and documentation are committed separately.
