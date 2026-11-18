# Native PCM ingress hot path — Windows, 2026-11-17

**Decision: no production change.** On this bounded local workload the standalone Opus frame calls were on the same elapsed-time scale as the complete pipelines, while the standalone Ogg mux was much smaller. Increasing ingress from 201 to 1,003 chunks did not show a repeatable material shift in complete-pipeline time. Neither replacing mux code nor weakening ingress/cleanup behavior is supported by this result.

## Matched workload and checks

The [new `profile_pipeline --native-ingress` mode](../../crates/rambler-audio/examples/profile_pipeline/ingress.rs) uses one new deterministic stepped-pulse clip: 64,271 mono PCM16LE samples (4.017 s at 16 kHz), with a nonzero partial-frame tail. Both a directly ready source and the real one-slot `pcm_channel` feed the exact same bytes to the default `Pcm16Opus` profile. Ordinary delivery uses 640-byte (20 ms) chunks; fragmented delivery cycles `3, 47, 119, 160, 313` bytes, crossing both sample and frame seams. Both paths are unpaced, single-threaded and in memory; the channel producer reserves, sends and finishes concurrently with the consumer via `tokio::join!`.

All collected streams matched the standalone continuous encoder's **202 exact packets** (11,595 bytes) and the reference's canonical Ogg digest. The independent Ogg reader checked real CRCs and framing; the oracle checked one packet per page, sequence, BOS/EOS, and every page granule. There were 204 pages (17,355 bytes), pre-skip 312, and final granule 193,125: exactly 192,813 samples after trim at 48 kHz. The decoded digest uses the same libopus, not an independent decoder. All timed pipelines reached natural and repeated EOF with one upstream close/drop, unchanged by explicit close. Observer-enabled warmups additionally checked actual sample/frame counters and that upstream EOF and cleanup precede EOS visibility, while output EOF follows it. The timed runs left that observer off.

## Measurements

One release run on Windows x64/MSVC, Rust stable 1.99.1, libopus 1.6.1, at production base `4cf9c4dafaabc1c04e668ec10fe50e2e33b25a81` plus the example-only change. All 72 batches were kept; six scopes occupied every position twice in opposite rotations. Each batch sums individual clip windows (five clips, or 40 for mux). Values below are median and full range of per-clip batch means, in elapsed milliseconds.

| Scope | Median | Min–max |
| --- | ---: | ---: |
| Standalone native frame calls | 12.4987 | 11.7148–13.0767 |
| Standalone Ogg mux | 0.0682 | 0.0612–0.0799 |
| Ready, 20 ms | 12.6162 | 12.1459–13.4240 |
| Channel, 20 ms | 12.7105 | 12.0138–14.0673 |
| Ready, fragmented | 12.9503 | 12.4758–14.0613 |
| Channel, fragmented | 13.0063 | 12.3632–16.1995 |

Within the same block, fragmentation versus 20 ms was **+1.84%** median for the ready source (range −6.55% to +15.77%, 8/12 positive), and **+3.89%** for the channel (−8.07% to +34.84%, 9/12 positive). The latter includes the slowest batch; it was not discarded. Channel versus ready had paired medians +0.65% at 20 ms and −0.24% fragmented, also with ranges crossing zero. These results do not meet the discovery criterion of a repeatable roughly 10% or larger complete-pipeline shift; they do not prove zero ingress overhead. The native scope measures calls over preconverted padded frames plus safe-wrapper loop/stores; the mux scope separately includes headers, pages, CRC and collecting bytes. The full pipeline also includes source and codec setup, cooperative yields, all page collection and cleanup. The scopes are not additive and no residual subtraction attributes production time to a function.

The timing job (4.356 s) ran after the release build had joined, in a project timing slot with no scheduled overlapping builds or profiles. Unrelated host activity, frequency and affinity were not controlled. Timer windows exclude generation, validation, hashing, decoding, printing, runtime creation and dropping the collected output. Validation can affect caches. These are local `Instant` wall-clock observations on a single synthetic clip, not process CPU, latency under paced ingress, battery, WAN or quality claims. Neither historical Opus/build nor CRC ABBA campaigns were replayed or changed.

## Reproduce and audit

Run from an isolated worktree with an owned `CARGO_TARGET_DIR` and cached toolchain. On Windows the measured run used the pinned `tools/windows_owned_job.py` and the approved OS/tool-only environment. Wait for the release build and verified child cleanup before running the example directly, with other builds stopped. This mode takes no arguments and needs no device, recording, service or credentials.

```text
cargo +stable fmt -p rambler-audio -- --check
cargo +stable test --offline --locked -j 2 -p rambler-audio --all-targets
cargo +stable clippy --offline --locked -j 2 -p rambler-audio --all-targets -- -D warnings
cargo +stable build --offline --locked -j 2 -p rambler-audio --release --example profile_pipeline
<CARGO_TARGET_DIR>/release/examples/profile_pipeline.exe --native-ingress
```

Validation: release build and format check passed; all-target audio tests passed (71 passed, 1 opt-in PyAV test ignored), including the existing odd terminal byte, sticky error, and cleanup cases. The first strict Clippy invocation was interrupted without a join receipt. Its partial log was retained; only Clippy was run again and joined successfully. No timing was repeated. The owner-marked cache was removed after validation.

Raw batch sums (`ns`, blocks 0–11; divide by 40 for mux, by 5 otherwise):

```text
native_frames       60000300,63044300,65383500,63361000,62697700,62289500,64815700,61879400,60668100,63398700,58574200,59503000
mux                  2659300, 3196300, 2853200, 2449400, 3060900, 2966300, 3005200, 2580900, 2754700, 2704000, 2687500, 2458700
ready_20ms          65358000,67120000,61061300,61777400,63296600,60729300,66368000,62722200,62363400,65507800,62865200,63837700
channel_20ms        68953000,65181200,64719000,62490100,61807800,60817800,60069000,70336600,64521900,62583100,65483100,60391400
ready_fragmented    65247600,62724400,62379100,62627400,64255700,70306300,69061200,68863700,69986700,62930200,65630800,63270100
channel_fragmented  63389600,65759100,67267300,66387800,64352700,64616500,80997600,65081000,66112200,64981700,61816100,62868200
```

SHA-256 fingerprints from the measured run:

```text
pcm                 e4f4a89a86558018ef69620388a6a0b6790cd9d3903bb1c1d3417d09e10a2626
length+audio packets dfa455a47a78a0ba973a0d75d2275057adcbebd57d30d9bdc769b09cc92d79d5
canonical Ogg       c44802ed837e81b83e48fdf6965f637c07377dc795dcf724cf7b0db95d6bd6d5
trimmed PCM at 48k  41ad27d334e2ab87ffb792084d5717d11526c397450ca13d513336f4c6e7efb5
release example     2c4320e46820ec9cfe5ab334e95bf77df3eb95eb0939bd4f95e4d15884a3b0d7
raw timing log      1199e50f2585b382bf66201ec37db198ed3a427aed32ebe7ba9b4272066820ce
```
