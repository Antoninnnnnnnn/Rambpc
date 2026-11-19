# Opus Release on Windows MSVC: local qualification

On November 16, 2026, an offline synthetic comparison on Windows x64 locally qualified a configuration change to the native Opus Release build. Across three signals and two measured pairs, `native_frames` is 3.0 to 3.6 times faster in elapsed time; complete audio pipeline elapsed time with default pages decreases by 67.4 to 71.6%. This result is local, not a production or multiplatform guarantee. At commit `2334b76`, all 14 CI jobs are green, including the functional VS17 check; two macOS ARM jobs required one rerun after runner unavailability, with no steps executed on the first attempt.

[Public evidence and raw measurements](../evidence/rust-opus-release-build-windows-20261116.json) · [Build and reproduction instructions](../../crates/rambler-audio/BUILDING.md) · [Benchmark source](../../crates/rambler-audio/examples/profile_pipeline.rs) · [Earlier audio profiling](rust-audio-profiling.md)

## Comparison and provenance

A is a fresh baseline and B the candidate: same Rust source (`8cb381e`), same lockfile, profile and audio example. Local candidate configuration identified by SHA-256 `6e4e7dc9…`; documentation was at `0c5daf4`. Configuration and its CI check were integrated by `95ba795`, then Cargo output discovery was fixed by `2334b76`. Builds use Windows x64, Visual Studio 2022/17, CMake 3.31.10 and MSVC 14.44. This is not a comparison with old Windows figures: the effective historical C build environment and archive–executable association are not sufficiently attested there.

The 152 actual Opus C compilation records carry no explicit optimization option for A; B records carry `/O2`, `/Ob2` and `NDEBUG`. `/MD`, `/fp:precise`, `/GS`, hardening options, RTCD and SIMD remain comparable; `/Brepro` duplication decreases from two to one, as expected. No change to quality, bitrate, complexity, frame duration or default page value is sought. These elements are not an attestation of object code or CPU dispatch actually executed; selected sources do not represent all tools and transitive inputs.

## Protocol and result

Three synthetic signals, silence, harmonic and noise, each contain 80,017 mono PCM16LE samples at 16 kHz. The codec uses VoIP, VBR 24 kbit/s and 20 ms frames; default pages are measured, and 1,000 ms pages serve as a legacy control, without changing the default.

Before reserving measurements, the six legacy proofs and their metadata are equal between builds: notably packet and 48 kHz decoded PCM SHA, canonical Ogg, pages, packets, granule and pre-skip. Opus is lossy: equality concerns decoded results between builds, not equality with original PCM.

Actual order of the reserved four-run budget is A1, B1, B2, A2; each contains seven repetitions of five iterations. All 588 attribution measurements, including outliers, are retained in the evidence; no fifth run. The preregistered criterion required at least 10% native gain for each signal in both pairs and rejected any total regression of at least 10% for a signal in either pair. It is satisfied locally.

The ranges stated at the top cover the three signals and both A1/B1 and A2/B2 comparisons, on their per-run medians; `total_off` denotes the complete pipeline with progress observer disabled. Detailed values by signal and pair appear in the public evidence.

`native_frames` includes the wrapper loop and result storage, not only isolated library calls. Measurement windows are not additive. Generation, hashing, decoding, runtime creation and display are outside the timer; exact scopes appear in benchmark headers. These are elapsed times, not cycles or process CPU. No extrapolation to WAN, ASR, natural speech, recognition quality, allocation, RSS or global optimality.

## Tests, incident and Debug

The first candidate phase failed on a hard-coded expectation of five example tests: eight actually passed, five direct and three included Ogg tests. Failure report `bce3` and original controller `8ceb` remain preserved unchanged. The targeted and reviewed supplement `7357` continued only steps not yet executed, without another build or test attempt; all 12 encoding tests and eight Release example tests passed. This correction does not turn the initial failure into a historical success.

A separate Debug qualification, locally audited across 11 captures and 96 retained artifacts, reports eight example tests passed under stable and, for the workspace, 429 passed, zero failed and two ignored under both stable and MSRV Rust 1.88. Old and new metadata match exactly for properties, sources and tokens after normalization of their own root only. All 152 Debug commands carry `/Od`, `/Ob0`, `/RTC1`, `/Zi`, `/MD`, `/GS` and `/fp:precise`, with one `/Brepro` and three AVX2 sources. Release executables, native archives and sealed measurement directories remained unchanged. Historical metadata equality does not attest the identity of the compiler executed at the time; no Debug gain is claimed.

## Fix scope and reproduction

The repository Cargo configuration supplies the unforced Visual Studio 17 2022 generator for the Windows MSVC x86_64 target. It permits building with VS 2022 without Ninja; the build script's choice of Ninja remains higher priority. `force = false` protects only an environment variable with exactly the same key: a generic generator key can be masked and the hyphenated variant takes precedence. For another VS version, explicitly set the appropriate key; see details and commands in [BUILDING](../../crates/rambler-audio/BUILDING.md).

Cargo discovers this configuration from its current directory in the workspace, not automatically for a downstream consumer or an external invocation with `--manifest-path`. Those must provide their own Ninja or exact generator variable. An existing CMake cache can retain the old generator: first preserve useful artifacts, then use a fresh target or targeted profile cleanup according to BUILDING, never blind cleanup of `target`.

The result concerns only Windows x64 and the listed local versions. The Windows key is not used for Linux/macOS; other architectures and VS versions are unqualified. The explicit functional job on `windows-2022` does not measure performance and the eight ordinary native jobs remain distinct. No production acceptance is anticipated here.

## Preserved CI incident and functional verification

At commit `95ba795`, 13 of 14 jobs pass, including the eight ordinary native jobs; the new VS17 job builds and passes its eight tests, then fails on `expected_one_opus_cache`. This result remains a failure, not VS17 qualification. The log shows a Cargo layout different from the expected glob; no blind rerun or additional measurement was made.

Fix `2334b76` uses the documented [`build-script-executed` JSON message](https://doc.rust-lang.org/cargo/reference/external-tools.html#build-script-output) and its `out_dir`, without assuming the location of Cargo's private files. Thirty synthetic tests pass. The [hosted VS17 job](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43917647762/job/133476039222) succeeds: eight Release tests, VS17/x64/host=x64, 152 C commands, three AVX2 sources, one `/Brepro` per command, required definitions and built/installed archive identity verified. The archive SHA concerns this checkout, not binary identity across machines.

The corrected commit's first attempt ends with 12 successful jobs and two macOS ARM jobs cancelled before any step: GitHub explicitly reports insufficient capacity and no runner assigned. One targeted rerun of these two unexecuted jobs succeeds. GitHub exposes the eight reused Rust successes under new view identifiers; their execution timestamps and steps are identical to the originals, without reexecution, including the VS17 job. The four Python successes remain from their first attempt.

The [ten Rust jobs](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43917647762) — eight native, VS17 and lint — and [four Python jobs](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43917647705) are thus verified at exact commit `2334b768fd42c7451692daa827ee31f14d37b31d`. Final metadata and 14 logs are retained with their SHA-256; initial failures remain historical failures. The initial local Windows CLI remains intact; CI builds use their own artifacts. No new timing or real service call.
