# Windows native 48 kHz session CPU profiling

Base: `5922fc161a2b1580318421168b994266ae03540c` (symmetric FIR). This is a local synthetic CPU experiment, not a provider, real-time, recognition-quality or device result.

## Protocol fixed before measurement

- Fixed 180 seconds plus seven samples of integer-only mono PCM16LE at 48 kHz, split into identical 1,921-byte slices (including odd boundaries). Two input modes: immediately-ready slices and `pcm_channel()` with a concurrent reserve/send/finish producer and its normal one-slot backpressure. Both feed `Pcm48kTo16k → Pcm16Opus` with unchanged default Opus/Ogg settings; the Engine uploads to an owned ephemeral IPv4 h2 peer. The peer drains the upload, then sends the synthetic native-qualification literal final and S3 END. The real collector waits for clean session completion and audio cleanup.
- A fresh release executable for each leg runs one excluded, independently checked warm-up in the same input mode, then one session. Timing covers `Engine::start` through the collector, producer and peer upload receipt; it excludes generation, listener/connect and transport/server shutdown, and post-session proof. One Tokio current-thread executor drives both sides. Attribution is by exclusive synchronous running brackets nested around audio source method invocations and polls, using Windows thread cycle deltas, not clock ticks while pending. Session is the balance after ingress, decimator, and Opus/Ogg; it includes unbracketed scheduling, Engine/client, h2, peer capture and collector work, not an isolated network or collector measure. Whole-process CPU time (Windows user + kernel) and wall are independent totals. Unmetered controls omit the extra wrappers and per-bracket measurements; they provide a perturbation check. Cycle shares describe the runtime thread, not all kernel work, and do not attribute CPU milliseconds by subtraction.
- Seven attribution repeats per mode with metered/control order and mode order alternating. A stage is clearly dominant only if, in every metered sample in both input modes, it accounts for more than half of cycles and exceeds the next stage by at least 1.5×. No production optimization without that and a credible exact-output candidate in the dominant stage.
- If both conditions are met: run one unmetered A/B batch, ten adjacent baseline/candidate pairs per mode, reversed A/B order per repeat and reversed mode order per repeat. The primary metric is paired whole-process CPU change `(candidate − baseline) / baseline`; report paired wall and thread cycles as secondary measures. Compare raw Ogg bytes including CRC, serial, sequence, laces and granule (same second serial in separate warmed processes), and length-prefixed Opus packet digest. Do not retain an optimization without byte identity, median CPU improvement in both modes, and at least 15 of 20 CPU pair wins; also reject if either median wall change regresses by over 1%. No retries of completed timing batches.
- Metadata gate before each executable leg: snapshot builder executable basenames only; wait in five-second increments for at most five minutes and log checks. No other process is opened, inspected or terminated. A builder observed in the post-leg snapshot invalidates the batch. Snapshot checks do not prove exclusive host access between checks.

The attribution qualified the Opus/Ogg stage (14/14). The sole A/B candidate is a compile-time 256-entry Ogg CRC table replacing the eight bitwise iterations per byte. It uses the same polynomial, bit order, zero initialization and checksum placement. No codec knob or invocation changes. Separate proof invocations compare the raw reconstructed Ogg byte arrays directly in both modes before timing; all timed legs also require the matching raw Ogg and Opus digests. The A/B executables share the same proof-enabled harness; attribution used its predecessor without raw stream export.

## Attribution results

Windows 11 build 26300, Intel Core i5-12450H (12 logical CPUs), MSVC x64 release, libopus 1.6.1. Each entry is **median [min, Q1, Q3, max]** of seven session shares (% of the executor thread's cycles, inclusive quartiles). The rows are mutually exclusive; medians are taken independently.

| Stage | Ready source | One-slot `pcm_channel` |
|---|---:|---:|
| 48→16 kHz decimator | 16.88 [16.52, 16.58, 17.11, 17.34] | 16.74 [16.36, 16.49, 17.16, 17.58] |
| Opus encode + Ogg mux | 55.65 [54.24, 54.53, 55.69, 57.47] | 55.55 [53.55, 54.76, 56.82, 57.65] |
| Engine/h2/peer/collector and unbracketed work | 27.10 [24.84, 26.86, 28.56, 28.85] | 26.35 [24.42, 25.45, 28.14, 29.08] |
| Ingress, including the channel producer where applicable | 0.35 [0.35, 0.35, 0.36, 0.37] | 0.73 [0.70, 0.72, 0.74, 0.76] |

For absolute scale, median measured cycles (millions) were 406.78/408.02 decimator, 1,338.05/1,365.94 Opus/Ogg, 656.34/702.41 session and 8.56/18.30 ingress (ready/channel). Whole-process CPU in the unmetered controls was 921.875 [781.250, 835.938, 929.688, 1171.875] ms ready and 968.750 [796.875, 820.312, 976.562, 1390.625] ms channel. The metered-vs-control paired median executor-cycle overhead was +8.07% ready (−10.24% to +17.68%) and +3.17% channel (−13.58% to +12.54%); share estimates include instrumentation perturbation. `GetProcessTimes` values were quantized in roughly 15.625 ms steps on this host. These are unpaced sessions with a two-response local peer, not representative audio or h2 network traffic; the codec and mux are not separated inside the Opus/Ogg bracket, and no local-thread cycle share includes work charged to outside threads or processes.

## Candidate result: not retained

The peer received 9,006 request messages, including 9,003 Ogg pages with 9,001 Opus packets. In both source modes the full 707,807 Ogg bytes were equal **without** normalization, with independent framing/CRC validation, serial 2, pre-skip 312, final granule 8,640,321 and raw SHA-256 `f82bcbd18579a0cfd6d2a9165239287ca5a7e88a9733078b681cb3c5eb35d254`. All timed legs matched the same Opus/Ogg digests.

One paired batch, no retry. Candidate minus baseline change in percent, **median [min, Q1, Q3, max]** across ten pairs per mode:

| Metric | Ready source | `pcm_channel` |
|---|---:|---:|
| Process CPU | −1.98 [−10.53, −5.11, 0.00, +4.08] | −3.92 [−8.47, −5.24, −0.50, +7.84] |
| Wall | −2.62 [−9.20, −4.57, −0.14, +4.45] | −2.39 [−7.33, −4.34, −0.81, +9.68] |
| Executor thread cycles | −2.80 [−9.63, −4.23, −0.13, +4.45] | −2.17 [−7.31, −4.43, −0.95, +9.47] |

CPU wins were 6/10 ready (3 ties) and 7/10 channel (1 tie): **13/20**, below the predeclared 15/20. The byte-identical CRC table is not retained and no production speedup is claimed. All 72 executable legs (28 attribution, 4 separate byte proofs, 40 paired legs) had clear metadata gates before and after, with no waits. The [machine evidence](rust-native-pcm48-session-cpu-windows-20261118.json) contains every timing row, full dispersion, fingerprints and gate outcomes.

The runner uses the pinned Windows job helper, two Cargo jobs and offline locked release builds in a shallow owned target. A preliminary untimed build exposed a stale `TEMP`/`TMP` in the supplied environment; a shallow temp subdirectory of the marked target fixed the linker failure before timing. Largest checked target size: 659,243,264 bytes. Parser fixtures passed before timing; candidate validation passed 76 audio tests (one ignored) and 7 example tests. After removing the CRC change: 75 audio tests passed (one ignored), 7 example tests passed, Clippy `--all-targets -D warnings` passed for audio and CLI with audio enabled, and workspace format check passed. No historical campaign was run.
