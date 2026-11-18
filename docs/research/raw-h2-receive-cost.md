# Raw h2 receive deframing cost

**Decision:** Keep a borrowed fast path for one complete, uncompressed gRPC envelope in one DATA frame. Retain the reusable `BytesMut` path when it can avoid an allocation. In a stream of aligned messages with two prior outputs still live, the candidate reduced decoder allocation requests from 128 to 96 per 64 nonempty messages (25%). It copied 160 fewer input bytes in each such run: the prefix is skipped on 32 compact copies. The copy saving is modest for larger messages; the allocator-request saving is the main reason for this change. A complete empty message requires neither a copy nor an allocation.

A cold nonempty message establishes the reusable buffer (same costs as before). If there is insufficient spare capacity and the consumed prefix area cannot be reclaimed, copy just the validated payload and drop the empty decoder buffer. A subsequent frame can establish reuse again. Fragmented and coalesced frames keep incremental buffering. Returning a slice of the h2 `Bytes` was rejected: `h2` 0.4.20 freezes slices from its length-delimited read buffer, and a DATA payload's length does not bound the backing allocation. Compact outputs own independent storage; the read loop drops the DATA `Bytes` before waiting on the application queue.

Full DATA credit is still released before any decode error or response-queue wait. The decoder emits one message at a time, so a valid prefix is sent before an invalid suffix is inspected. Compression, size and truncated-EOF errors keep their meanings. Post-drain pending input is at most the message limit plus four bytes; a fragment append can temporarily add one DATA frame, as before. No response queue sizes, limits, or trailer handling change.

## Matched native session timing — Windows, 2026-11-18

The [native qualification kit](native-qualification-kit.md) compared the exact
`3421da0` workspace with one receive-only baseline: `wire.rs` at `4cf9c4d`
(the new private deframer is unused in that variant). Both Release builds used
the same toolchain, lock, fixtures and harness. Before timing, the strict
comparator matched both runs on PCM, ordered responses, rendered output,
counts and Ogg length. The harness itself also checked exact Opus packets
and Ogg framing. Random Ogg serials were not compared as full stream bytes.

Each version ran six repeats per process, alternating candidate and baseline
processes for four blocks. The same strict fidelity comparison passed for each
block. Deltas are candidate minus baseline, in microseconds; negative is faster.

| Case | First response, median [min, max] | Session, median [min, max] |
|---|---:|---:|
| Literal | −10.5 [−793, +114] | −11 [−812, +116] |
| Rewrite | −25.5 [−430, +276] | −29 [−449, +278] |
| Selection | −25 [−1060, +111] | −26.5 [−1085, +112] |
| Preview | +1.5 [−698, +262] | **+0.5 [−1028, +384]** |

The predeclared primary metric, preview through SDK EOF (before rendering),
was faster in 12 paired observations and slower in 12. Block medians were
−40, +73, +76 and −880 µs; excluding the first repeat from each block did not
change the overall median. Negative medians on secondary cases did not repeat
consistently across blocks. There are 24 observations per case and variant,
not 24 independent process launches. **No repeatable end-to-end gain resolved,
and no proof of equivalence.** The earlier allocation counts remain a separate
result. This fixture does not measure real DATA alignment, retention rates,
service latency, or production speed.

Two earlier builds stopped before timing because CMake did not identify the C
compiler; even a short spelling of the deep directory did not help. A separate
minimal C project reproduced a FileTracker error at a deep path, not a proven
root cause for those earlier Cargo failures. Both actual variants then built
with physically shallow targets ([build guidance](../../crates/rambler-audio/BUILDING.md#local-windows-compiler-identification-failures)).
Their disposable caches were cleaned. The [evidence](../evidence/rust-deframer-native-loopback-windows-20261118.json)
retains all 192 timed rows, the gate, full dispersion, and both failed statuses.

## Reproduce

Run from the repository root using the cached stable toolchain. On Windows, use a dedicated Cargo target directory and the pinned `tools/windows_owned_job.py` job helper with the approved build-only environment if a bounded capture is needed.

```sh
cargo +stable test --locked --offline -p rambler-transport --lib wire::deframer::tests -- --nocapture
cargo +stable check --locked --offline -p rambler-transport --tests
cargo +stable test --locked --offline -p rambler-transport --lib --test http2 --test loopback --test connection_window --test lifecycle
```

The baseline test path repeats the append, advance, and split used by `4cf9c4dafaabc1c04e668ec10fe50e2e33b25a81`. The positional oracle independently checks the exact first availability and bytes of each expected envelope for every three-way partition of a small stream, including early EOF, empty DATA chunks and a zero-length message. A second oracle checks that valid output precedes a suffix error. The existing localhost suites also exercise exact-limit fragmentation, coalescing, trailers, cancellation, backpressure and flow control.

The counters use newly generated S3 NORMAL responses carrying a synthetic optional unknown field, plus an empty-envelope control. They exclude allocation of the input, the h2 stack, and consumer work. `hold=2` keeps the last two outputs live as decoding of the next one starts; it models overlap rather than a real client's retention rate. `copied` counts explicit input bytes passed to append or payload copy, not copies internal to an allocator or to `BytesMut`. `alloc` includes zeroed allocations, `realloc` is separate, and `requested` sums allocation sizes and the new sizes of reallocations. `peak_live` is peak logical requested heap bytes during deframing (not RSS); `pending_peak` is sampled after each DATA frame has been drained. No clocks, traffic capture, provider service, or previously recorded data were used. Results do not establish alignment frequency, end-to-end allocation savings when consumers clone messages, or any wall-time improvement.

## Observed counters

Windows x64, stable, test profile, locked `bytes` 1.12.1. The bounded capture exited successfully and reported an empty owned Windows job.

```text
case=cold-single variant=base frames=1 messages=1 hold=0 copied=101 alloc=2 realloc=0 requested=141 peak_live=141 pending_peak=0
case=cold-single variant=candidate frames=1 messages=1 hold=0 copied=101 alloc=2 realloc=0 requested=141 peak_live=141 pending_peak=0
case=exact-drop variant=base frames=64 messages=64 hold=0 copied=6464 alloc=2 realloc=0 requested=141 peak_live=141 pending_peak=0
case=exact-drop variant=candidate frames=64 messages=64 hold=0 copied=6464 alloc=2 realloc=0 requested=141 peak_live=141 pending_peak=0
case=exact-held-tiny variant=base frames=64 messages=64 hold=2 copied=640 alloc=128 realloc=0 requested=3200 peak_live=150 pending_peak=0
case=exact-held-tiny variant=candidate frames=64 messages=64 hold=2 copied=480 alloc=96 realloc=0 requested=1760 peak_live=105 pending_peak=0
case=exact-held-96 variant=base frames=64 messages=64 hold=2 copied=6464 alloc=128 realloc=0 requested=9024 peak_live=423 pending_peak=0
case=exact-held-96 variant=candidate frames=64 messages=64 hold=2 copied=6304 alloc=96 realloc=0 requested=7584 peak_live=378 pending_peak=0
case=exact-held-512 variant=base frames=64 messages=64 hold=2 copied=33088 alloc=128 realloc=0 requested=35648 peak_live=1671 pending_peak=0
case=exact-held-512 variant=candidate frames=64 messages=64 hold=2 copied=32928 alloc=96 realloc=0 requested=34208 peak_live=1626 pending_peak=0
case=exact-held-4096 variant=base frames=64 messages=64 hold=2 copied=262464 alloc=128 realloc=0 requested=265024 peak_live=12423 pending_peak=0
case=exact-held-4096 variant=candidate frames=64 messages=64 hold=2 copied=262304 alloc=96 realloc=0 requested=263584 peak_live=12378 pending_peak=0
case=exact-growing-drop variant=base frames=5 messages=5 hold=0 copied=5481 alloc=2 realloc=4 requested=7363 peak_live=5521 pending_peak=0
case=exact-growing-drop variant=candidate frames=5 messages=5 hold=0 copied=5481 alloc=2 realloc=4 requested=5521 peak_live=4141 pending_peak=0
case=exact-zero variant=base frames=64 messages=64 hold=2 copied=320 alloc=128 realloc=0 requested=2883 peak_live=138 pending_peak=0
case=exact-zero variant=candidate frames=64 messages=64 hold=2 copied=0 alloc=0 realloc=0 requested=0 peak_live=0 pending_peak=0
case=fragmented variant=base frames=128 messages=64 hold=2 copied=6464 alloc=128 realloc=64 requested=9221 peak_live=423 pending_peak=3
case=fragmented variant=candidate frames=128 messages=64 hold=2 copied=6464 alloc=128 realloc=64 requested=9221 peak_live=423 pending_peak=3
case=coalesced variant=base frames=32 messages=64 hold=2 copied=6464 alloc=64 realloc=0 requested=7744 peak_live=484 pending_peak=0
case=coalesced variant=candidate frames=32 messages=64 hold=2 copied=6464 alloc=64 realloc=0 requested=7744 peak_live=484 pending_peak=0
case=limit-fragmented variant=base frames=20 messages=4 hold=2 copied=262164 alloc=8 realloc=6 requested=835744 peak_live=393336 pending_peak=65536
case=limit-fragmented variant=candidate frames=20 messages=4 hold=2 copied=262164 alloc=8 realloc=6 requested=835744 peak_live=393336 pending_peak=65536
```
