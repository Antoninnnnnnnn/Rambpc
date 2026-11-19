# Reducing protobuf serialization allocations

## Multiplatform CI — qualified source `43e918d`

All **13 Rust/Python jobs** on commit `43e918d44c4612cad46c3de4270d59bcf49ce35f` pass: eight native Windows, Ubuntu, macOS ARM64 and Intel × Rust 1.88/stable jobs, lint and four Python jobs. Each workspace reports **411/0/2** on Windows, **412/0/2** elsewhere. The five new fidelity/fixed32 names and three benchmark tests are verified in all eight jobs; the new gate reports **3/0/0** everywhere. The nine guarded SDK tests and existing authentication rejections remain present and pass.

The 64 command groups and **1,545,494 bytes** of logs were recounted with API run/job/step provenance, HEAD, hashes and compiler host lines. One attempt per run, no CI replay. Counts are per command/job, not distinct tests to add together. Release builds/gates, PCM and public roots also pass; the roots smoke test remains a configuration, not a TLS handshake. [Separate CI evidence linked to the exact commit](../evidence/rust-wire-encoding-allocation-ci-20261116.json).

This CI runs the **invariants**, not the allocation/timing campaign. It does not make Windows timings representative of the engine, network or other OSes. The local checkpoint and its “CI pending” evidence below are preserved as history; their build identity and limits are not rewritten. No new dictation, Android parity, Linux live admission or lifting of G1.

## Local decision — November 16, 2026 (historical checkpoint)

**Production optimization selected locally, remote CI still pending.** The encoders in `crates/rambler-protocol/src/wire.rs` now write key, length and value into one vector sized before allocation, without a temporary varint vector. Bytes, signatures, limits and error precedence are preserved; decoder, request assemblers, `audio_request`, codec and dependencies are unchanged. No `unsafe` is added to the library.

The 149 synthetic cases have exactly identical before/after outputs in both runs. The count of `alloc + alloc_zeroed + realloc` calls and sum of requested sizes do not increase in any case; the 108 varint/bytes field cases use strictly fewer calls. The 30 standalone varints remain at one call.

[Typed evidence and all raw measurements](../evidence/rust-wire-encoding-allocation-windows-20261116.json). The measured source is the uncommitted candidate on `d9af4f5`, identified by its six file hashes and selected 182-file manifest. This is not retrospective qualification of every future commit or a TCB attestation.

## Complete request verification

A targeted review identified a real risk to measure: removing spare capacity from small vectors can add reallocations during successive `append` operations. Before any measurement, the benchmark was therefore extended with 11 complete assembly cases in addition to the 138 small encoders. Init/context bodies are copied exactly from the baseline and use its old encoders; types and config validation are shared and unchanged. Fixtures/configs are prepared outside measurement, but validation, assembly and result destruction are included.

Counts **per operation**, observed identically in both runs; bytes are sums of sizes requested from hooks, **not** live or peak memory.

| Synthetic case | Calls before → after | Requested bytes before → after |
|---|---:|---:|
| Default init | 60 → 34 | 1102 → 741 |
| Init with 128-byte hints | 102 → 59 | 4232 → 3674 |
| Init with 1024-byte hints | 102 → 59 | 15616 → 15058 |
| Simple context, 0 bytes | 4 → 2 | 40 → 7 |
| Simple context, 128 bytes | 6 → 2 | 306 → 266 |
| Simple context, 1024 bytes | 6 → 2 | 2098 → 2058 |
| Simple context, 65536 bytes | 6 → 2 | 131125 → 131085 |
| Cursor context, 0 bytes before | 13 → 7 | 160 → 76 |
| Cursor context, 128 bytes before | 14 → 6 | 787 → 698 |
| Cursor context, 1024 bytes before | 14 → 6 | 5267 → 5178 |
| Cursor context, 65536 bytes before | 14 → 6 | 327835 → 327746 |

Init cases with hints add two biases, two preferences, two secondary languages and the three recognizer options. Cursor context also includes a Unicode selection and an after portion containing NUL; no real text is read. A 1024-byte bytes field at number 1 separately goes from 3 to 1 calls.

## Qualification and history

- Local Windows stable 1.99.1 and Rust 1.88: **75/0/0** for protocol each, **3/0/0** for benchmark invariants each. The names of the four new differential tests, fixed32 test and existing golden init are confirmed.
- Stable workspace all features: **411/0/2**; CLI without audio **38/0/0**. Global fmt, strict protocol Clippy all targets and release benchmark build pass. These groups overlap: do not sum them as distinct tests.
- The 14 qualification commands and their logs are verified; sources identical before/after. Five benchmark launches (three no-op, two measurements) exit zero with empty jobs, checked outputs and hashes.
- The first QA batch failed **at compilation**, before tests: my complete context fixture omitted provenance fields `raw/unknown_fields`. Their default values were added, without an engine fix; failure preserved, separate second batch passed. No additional measurement or hidden exclusion.
- Historical release CLI unchanged and not invoked; no credential, service, phone, reservation, global setting or audio profile change. Its old remote result does not qualify this new encoder. G1/Linux live unchanged.
- An explicit CI gate runs the three benchmark tests, not the timing campaign.

## Measurement and limits

One release executable contains baseline and candidate. Per case and version, 128 operations are counted, then a **separate** pass measures six repetitions, with 8 warmups and alternating A/B–B/A order. Two sequential runs, all compilation finished; the 3576 integer durations and all counters are retained.

Instrumented medians for all 11 complete requests are lower in both runs. **This is not a quantified gain for the normal engine**: the benchmark allocator hooks still execute an atomic load when counting is disabled. Call counts differ, so this cost does not cancel between versions. Opaque dispatch, vector destruction, caches and OS noise remain included; frequency, power and affinity are uncontrolled. No outlier removed, significance, process CPU, RSS, WAN latency, ASR quality, Android parity or global optimum claimed. The retained gain is the observed reduction in allocation calls/sizes on these paths, with unchanged fidelity.

## Reproduction

From the root, locked dependencies already available; no key or external input required:

```sh
cargo +1.88.0 test -p rambler-protocol --locked --offline
cargo +1.88.0 test -p rambler-protocol --example profile_wire_encoding --locked --offline
cargo +stable build --release -p rambler-protocol --example profile_wire_encoding --locked --offline
# Wait for all compilation to finish, then run twice sequentially.
./target/release/examples/profile_wire_encoding.exe --benchmark-wire-encoding
./target/release/examples/profile_wire_encoding.exe --benchmark-wire-encoding
```

Omit `.exe` on Unix. The path assumes the default Cargo target. Without the exact flag, with an additional argument or in a standard debug build, the benchmark remains silent and measures nothing. Compare all 149 declared equal outputs, all counters and all durations; do not select only favorable cases.
