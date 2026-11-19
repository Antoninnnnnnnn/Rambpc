# Windows validation of capture and integration fixes

18 November 2026 · Tested source: `72d79fc793411fa65d2703d61e0e65cef31a7420` · Windows/MSVC. Tests use invented samples, texts and test responses, with controlled HTTP/2 loopback peers. No device or microphone, provider call or access metadata.

| Case | Verified observation |
| --- | --- |
| [Capture buffer](../../crates/rambler-cli/tests/capture_rollover.rs) | The peer withholds the final while blocks representing 60, 600 or 900 ms are offered. Packets and granules match the expected PCM. The continuous 48 → 16 kHz path is also covered. Overflow beyond 54 slots and abandonment are distinct failures, not normal EOFs: previous segments remain available. |
| [Combined capture and delivery path](../../crates/rambler-cli/tests/capture_rollover_delivery.rs) | 90 blocks of 20 ms, two transitions with the final withheld, three RPCs on one TCP connection. Three independent Ogg streams with distinct serial numbers, Opus packets and granules identical to separate encoding of the three expected slices. The sink receives ranges `[0,9600)`, `[9600,19200)`, `[19200,28800)` in order, only after validation. |
| [Interruption and normal completion](../../crates/rambler-cli/src/tests.rs) | The Ctrl-C branch is exercised through the same supervision mechanism as the binary. During the second RPC, an injected signal returns the first range in JSON, no global text or unvalidated or blocked text, and code 130. The [SDK tests](../../crates/rambler-cli/tests/long_pcm_dictation.rs) also cover normal completion with five leftover samples and a late gRPC failure, without replay. |
| [CLI and SDK joining](../../crates/rambler-cli/tests/interactive_join_fidelity.rs) | Both public entry points retain individual finals and produce the same rendering for `L’` + `équipe arrive.`, `明天` + `开会。`, a paragraph break or an already supplied space. Unit tests also cover quotation marks, accents and Unicode graphemes. Across 4,096 regional-indicator joins, the instrumented check examines 8,193 newly added scalars and never queries the cursor over the entire prefix. |
| [Styled editing](../../crates/rambler-engine/tests/splice.rs) | In `oui oui`, structured replacement of the first `oui ` produces UTF-16 range `0..4` and retains the link on the second `oui`. An unproven anchor, stale revision or invalid grapheme boundary produces no targeted proposal. |
| [Three real local TCP cuts](../../crates/rambler-engine/tests/app_tcp_recovery.rs) | While waiting for PCM, waiting for context or waiting for gRPC status after half-close: no incomplete text validated, no new PCM read by the disconnected engine, no implicit reconnection. After explicit opening, the next dictation processes only its new source. |

## Reproducible checks

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-features --locked --offline
cargo test -p rambler-cli --no-default-features --locked --offline
```

| Toolchain | Workspace with audio (including doctests) | CLI without audio |
| --- | --- | --- |
| Stable | 559 passed, 2 ignored | 24 passed |
| 1.88.0 | 559 passed, 2 ignored | 24 passed |

Formatting, strict Clippy and the audio dependency graph check passed. Tests with both toolchains ran under owned Windows Jobs that were empty after each command. A preliminary Clippy pass flagged a constant-size `chunks_exact` in the new check; replacing it with `as_chunks` also verifies that the slice has no remainder.

The simulated production intervals do not measure hardware, actual callback delay or battery consumption. The suite demonstrates neither transcription accuracy on either side of a cut, WAN latency nor Gboard parity. This Windows evidence does not replace cross-platform CI. See the [second audit](../research/audit-parite-integration-20261118.md), [SDK guide](../research/sdk-integration-guide.md) and [buffer limits](../research/rust-pcm-capture.md).

## Android ARM64 cross-compilation of the published batch

Exact source: `9120decab7df7b2ecb277445ef24ff8fb66aa1aa`. Windows/MSVC host, Rust 1.88.0, NDK `27.1.12297006` and target `aarch64-linux-android`, API 24. The following command builds the CLI and the test linking `capture_channel` and `stream_long_pcm_dictation` together, **without running them**:

```sh
cargo +1.88.0-x86_64-pc-windows-msvc test -p rambler-cli --features audio \
  --test capture_rollover_delivery --target aarch64-linux-android --no-run --locked --offline
```

Use the [NDK compiler and linker settings](../../crates/rambler-transport/ANDROID_TLS.md#validation), including `-Wl,-z,max-page-size=16384`. For static Opus, this build also used the NDK CMake toolchain with `arm64-v8a`, `ANDROID_PLATFORM=android-24` and `ANDROID_STL=none`, CMake 3.31.10 and Ninja 1.12.1. The command completed in 228.7 s under a Windows Job emptied after the build: this is compilation time, not an engine performance measurement.

`llvm-readelf -h -l -W` confirmed two ELF64 AArch64 binaries, each with four `PT_LOAD` segments aligned to `0x4000` and file offsets/virtual addresses congruent modulo 16 KiB. This check verifies neither APK packaging, loading on an ARM64 kernel with 16 KiB pages, nor capture or TLS behavior in that configuration. Temporary test binaries were removed.
