# Short gRPC prefixes without per-message allocation

**Historical benchmark scope: prefix constructor only.** A [later writer coalescing change](rust-grpc-envelope-coalescing.md) groups prefix and payload in a chained buffer. The measurements below do not qualify that new complete path and are not replayed or reattributed.

## Local decision — November 16, 2026

**At the local checkpoint: candidate selected; remote CI still pending.** The payload was already sent without a framing copy. The only change is its five-byte gRPC prefix: `grpc_prefix` borrows an immutable static table for lengths 0–255, then uses exactly the old constructor for other lengths. The table contains **1,280 bytes of source data**; this is not a measurement of linked size, physical memory or RSS.

Fallible size conversion to u32 remains in the same place. `send_bytes`, capacity/flow control, waits, source, reset, EOF, progress and reception are unchanged. No payload copy removed, HTTP/2 coalescing added, library `unsafe`, dependency or additional audio setting.

[Typed evidence, counters, native sizes and all raw timings](../evidence/rust-grpc-prefix-allocation-windows-20261116.json). Measured source: uncommitted candidate on `7a03f38`, identified by seven hashes and the selected 186-file manifest. This is not a TCB attestation or retrospective qualification of a future commit.

## Later remote CI qualification

Offline CI has since passed on exact source `98d46590ec708c7e1ae6157323a07b8eaa41ddd7`: [Rust run](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43832001772) and [Python run](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43832001623). The [CI evidence](../evidence/rust-grpc-prefix-allocation-ci-20261116.json) records 13 jobs, including 8 native, and 72 Rust command groups: these are not distinct tests to sum. For each toolchain, the workspace reports **418/0/2** on Windows and **419/0/2** on Linux and macOS; macOS ARM and Intel architectures are confirmed. In each native job, prefix benchmark **7/0/0**, guarded SDK **9/0/0** and wire benchmark **3/0/0**, groups not to be added together.

This CI checks invariants, the raw H2 matrix, forced fragmentation and synthetic PCM/Opus sizes. It does not reproduce the local allocation or timing campaign and establishes neither a real voice distribution nor service performance or hits. Local evidence remains unchanged and describes its own checkpoint. G1 remains unmet and Linux live NO_GO; no new qualification of credentials, phone, core or provisioning.

## Allocation actually observed

A release executable contains old and candidate constructors. Two sequential runs, 16 fixed lengths, 4096 counted operations per version/case:

| Measured domain | Before, per constructor | After, per constructor |
|---|---|---|
| 11 short lengths from 0 to 255 | 1 allocation + 1 deallocation, 5 bytes requested | **0 allocations, 0 deallocations, 0 bytes requested** |
| 256, 1024, 65535, 65536, u32::MAX | 1 allocation + 1 deallocation, 5 bytes requested | Identical |

Counters are identical across both runs. All 256 short lengths are also checked functionally and by pointer identity to the table, but only the 11 indicated lengths are instrumented for allocations. The u32::MAX case constructs only five prefix bytes, never a giant payload.

The five fallback controls do not trigger the predefined threshold of an instrumented median more than 10% slower **in both runs**. This does not demonstrate a universal absence of slowdown or acceleration of the complete transport.

## Relevance to the default audio pipeline

To avoid inferring message size from bitrate alone, a test uses the real `Pcm16Opus`, then the real `audio_request` for each nonempty chunk, like `Engine`. Three synthetic non-speech signals of 80017 samples, source in 640-byte chunks, unchanged native mono16k/24kbps/VoIP/VBR profile. Each signal is encoded with 20ms pages and a 1000ms control; each chunk is then wrapped for elapsed 0/128/i64::MAX. This gives 18 observations, not 18 independent encodings. All samples, natural EOFs, single close, destruction and idempotent cleanup are checked.

Counts **per signal/config**, at the maximum tested timestamp sizes:

| Signal | 20ms pages: eligible / messages | Max message | 1000ms pages: eligible / messages | Max message |
|---|---:|---:|---:|---:|
| Silence | 253 / 253 | 79 bytes | 3 / 8 | 502 bytes |
| Harmonic triangles | 253 / 253 | 142 bytes | 3 / 8 | 2859 bytes |
| Deterministic noise | 253 / 253 | 115 bytes | 3 / 8 | 2873 bytes |

The majority required before measurement is therefore established for each of the nine 20ms rows. Other elapsed widths are not measured separately: unchanged `audio_request` arithmetic bounds them between the tested extremes. **Eligibility** only: not a trace of service hits, representative speech, ASR quality or init/context/END sizes. The 1000ms pages remain a control, neither a new defect nor a batching proposal.

## Framing, backpressure and qualification

- Independent oracle on a single numeric loopback RPC/TCP: **260 messages**, all payloads 0–256 then 1024/16384/65536, 115840 payload bytes and 117140 wire bytes. Flag 0, big-endian length, exact payload/order, empty message, stable EOF, OK status only after draining and closing are checked.
- Another small four-byte RPC: remote window 3, source held until the peer's Ping/Pong after SETTINGS. **First DATA of three bytes observed**, prefix correctly fragmented, nine total bytes checked before OK. Only this small fixture uses window 3, not the large corpus.
- Windows stable 1.99.1 and Rust 1.88: transport **115/0/1** each, benchmark **7/0/0** each. Six new test names and four existing authentication rejections are confirmed per command. Stable workspace **418/0/2**, guarded SDK **9/0/0**, feature-off CLI **38/0/0**; format/strict Clippy/build pass. Do not sum replayed groups as distinct tests.
- The 16 QA commands, preliminary debug build and six supervised captures (four no-op, two measurements) pass; jobs empty, sources and binaries unchanged. The historical release CLI binary remains unchanged and is not invoked.
- First QA retained: transport and benchmark passed on both versions, then Clippy rejected `% 2 == 0` in benchmark ordering. Replaced by `is_multiple_of(2)` compatible with 1.88, without a production fix; separate second QA passed. No additional measurement result or removed case.
- The explicit CI gate tests invariants, not the timing campaign. No credential, real service, phone, reservation or global setting; G1/Linux live, durable access and Android parity are not resolved by this change.

## Timing interpretation and reproduction

Eight repetitions of 32768 operations per version/case follow 1024 warmups; A/B–B/A rotates by case/repetition. Allocation and timing passes are separate. All 512 integer durations are retained; zero columns for time in phase 0 and counters in phase 1 are **placeholders**, not observations.

The instrumented allocator's atomic gate still executes when counting is off. Its cost does not cancel out when call counts differ. Opaque dispatch and `Bytes` destruction are included; frequency/affinity/OS noise are uncontrolled. No outlier excluded, significance, process CPU, WAN latency, normal engine gain or global optimum claimed. The retained gain is **one allocation/deallocation avoided in the observed short prefixes**.

```sh
cargo +1.88.0 test -p rambler-transport --locked --offline
cargo +1.88.0 test -p rambler-transport --example profile_grpc_prefix --locked --offline
cargo +stable test -p rambler-cli --features audio --test grpc_prefix_eligibility --locked --offline -- --nocapture
cargo +stable build --release -p rambler-transport --example profile_grpc_prefix --locked --offline
# Wait for all compilation to finish, then run twice sequentially.
./target/release/examples/profile_grpc_prefix.exe --benchmark-grpc-prefix
./target/release/examples/profile_grpc_prefix.exe --benchmark-grpc-prefix
```

Omit `.exe` on Unix. The path assumes the default target. Without the exact flag, with an additional argument or in a standard debug build, the benchmark remains silent. Retain all 288 numeric lines per run, their counters and times.
