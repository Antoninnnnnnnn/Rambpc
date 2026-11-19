# Rust decoder private allocations: compatible prototype

> **Historical — unbounded V2 variant rejected after defensive review.**
> The decisions, “final” figures, exact capacities of large lists and validations below concern only this older measured variant. They do not describe the code ultimately retained. The A/B data remains fully preserved, but an error on the first child can request 33.84 MB of speculative model storage. See the [final fix with a bounded hint before filtering](rust-backend-decode-defensive-capacity.md) and its separate new evidence. No historical V2 gain measures the final fix.

## Scope and decision

Baseline **`67ac9a5`**, Windows x86_64, 2026-11-14. Prototype only in the working worktree, **without a commit or change to `main`**. The previous `Bytes` optimization is already in this baseline; its historical gain of 70.82% is not reattributed to this work.

[Structured evidence and measured sources](../evidence/rust-backend-decode-allocations-profile.json): **420 final A/B rows**, **630 initial three-way rows fully preserved**, 42 diagnostic rows, SHA256 of the three binaries/sources, variant and common harness patches, summaries, paired reports and validation logs. No slow sample removed.

**Retained:** exact capacities for repeated vectors, lazy private lookups, single-occurrence parts on the stack, reuse of the first vector produced by `parse_ref`. No new public type, change to `Bytes`, `String`, `wire::Field`/`Value`, dependency, `unsafe` or production transport/engine code. The profiler's existing instrumented allocator is reused unchanged.

## Diagnosis and changes

Actual inventory for this target gives `size_of::<Hypothesis>() = 376` and `size_of::<S3Response>() = 1984` bytes, **before and after**. Initial estimates of 384/1952 are therefore not the values for these executables.

- A single candidate in `HypothesisResult` retained capacity for four, or **1504 bytes**. Preallocating from the number of already parsed fields with the correct number **and wire type** reduces this allocation to **376 bytes**. Two candidates: capacity 4→2; 257: 512→257. Zero remains allocation-free. Same treatment for `PartialResult`, independently for each container.
- Counting traverses **already validated references** a second time, not protobuf bytes; it never charges the budget a second time. Linear work bounded by 100,000 fields, no arithmetic from an unvalidated wire length. Preallocation may precede rejection of a malformed child: this is not a new heap-bound guarantee.
- `Message::data` returns a clonable iterator rather than a temporary `Vec`. All string occurrences remain validated in order before copying the last. Fixed32/64 values have all already been validated by `parse_ref`; the final lookup can therefore traverse references from the end.
- Private parsers borrow a slice of parts. Root and repeated child no longer allocate a `vec![part]`. For a singular submessage, zero occurrences remains absent, one uses a stack slice, several retain a temporary vector and then the existing merge.
- `Message::new` directly reuses the first `Vec<RefField>`; other occurrences are still parsed separately then extended. No double vector/field copy for the common single-occurrence case.

The **1984 inline root bytes** and public layouts remain unchanged. Oneof run vectors, parts of actual merges and unknown-field capacities remain present; this does not remove provenance.

## A/B method

Same harness/fixtures in the three release executables, only `decode.rs` differs. First comparison: five passes with 20,000 requested operations, rotating order baseline / capacity only / full candidate. It isolates the fact that capacity alone already gives the retention reduction, but leaves **39 allocations** per response versus **24** for the full candidate. It is not retained as the final variant, without being a fidelity failure.

One initial oneof pair was **+27.0%**, one large-retention pair **+1.3%**. This triggered **one longer bounded repetition**, announced to the coordinator, without concurrent compilation/benchmark: five strictly alternating pairs **AB / BA / AB / BA / AB**, 100,000 requested operations, 128 warmups per case. Old values are neither excluded nor mixed with the final ones. No further repetition after this window.

Retention: 781 batches of 128 responses, actual movement into an unchanged growing `Vec`, then destruction. Repeated shapes: iteration count adapted to candidate count; 6250 for partials/merges/oneof, 100 for 100,000 fields. Exact counts are in each row. Allocation counters disabled during wall timing, then separate pass; `black_box` observes the entire model. Fixture construction, preliminary assertions, JSON and output are excluded. Live bytes return exactly to their initial value after each counted pass.

Windows 11 Pro 10.0.26300, `rustc 1.99.1`, target `x86_64-pc-windows-msvc`, Cargo release profile, incremental disabled. No CPU affinity or frequency locking. The allocator delegates to `System`; its branch remains in wall passes. Reallocations charge their entire new size. These are logical bytes under the `GlobalAlloc` contract, **not RSS or copy traffic**.

## Final results

Time **ns per response**, median (minimum–maximum), five pairs.

| Case | Baseline | Candidate |
|---|---:|---:|
| small decode (526 wire bytes) | 2119.05 (2025.25–2834.80) | 1579.65 (1480.99–2097.11) |
| large decode (12 430 wire bytes) | 2996.11 (2819.39–4238.29) | 2376.04 (2318.86–3392.03) |
| retention 128 × small | 3786.18 (3583.75–5074.69) | 2884.33 (2813.37–3710.83) |
| retention 128 × large | 4173.90 (3835.01–4902.14) | 3175.88 (3090.30–5722.55) |
| 1 short candidate | 1271.24 (1245.53–1673.22) | 846.91 (807.51–1723.64) |
| 16 short candidates | 6368.62 (6191.09–8900.00) | 3480.38 (3303.50–4548.43) |
| 257 short candidates | 142757.07 (125493.83–178908.23) | 84091.52 (81762.72–113678.92) |
| 16 partials | 5662.62 (5431.49–8945.06) | 2855.25 (2764.35–4059.18) |
| merge of 16 containers | 7647.62 (7453.30–12172.88) | 4482.21 (4321.57–6646.67) |
| 16 oneof runs | 4345.65 (3816.50–7770.80) | 3367.22 (3188.50–4527.97) |
| root 100,000 fields | 3526607 (3395157–4285408) | 2652096 (2624928–3517055) |

All 17 medians, including empty/status/speech and lists of 0/2/4 elements, are favorable. **This is not proof of absence of CPU regression**: one final oneof pair is still +14.7%, one large-retention pair +42.4%; four of five pairs are better for both cases, with overlapping intervals. Paired reports for all cases are in the evidence.

Oneof-specific analysis: its grouping and vectors are unchanged, and no repeated-list `count()` pass is exercised in this fixture. Absence lookups still scan; the two singular `next()` calls share an iterator, not two full traversals. Copying the first field vector and lookup materializations are removed, without new asymptotic complexity. Thus a **favorable wall-time trend in this microbenchmark** is retained, not a guaranteed universal CPU gain, and no remote h2/service acceleration is claimed.

### Deterministic allocations

| Measure | Baseline | Candidate |
|---|---:|---:|
| Allocations, isolated nested small/large response | 39 | 24 |
| Requested bytes, small | 5703 | 3135 |
| Requested bytes, large | 29511 | 26943 |
| Live bytes, batch 128 × small | 639488 | 495104 |
| Logical peak, small batch | 640320 | 496336 |
| Live bytes, batch 128 × large | 3686912 | 3542528 |
| Logical peak, large batch | 3687744 | 3543760 |

The large batch still carries **1 591 040 bytes** of protobuf and saves **144 384 live bytes, or 3.92%** of the current `Bytes` baseline. The small batch saves the same 144 384 bytes, or **22.58%**. The explanation is exact: **3 × 376 × 128** of excess capacity removed. Strings (456/12360 bytes per response), unknowns (502), exact raw and sharing metadata are unchanged. The batch vector retains one initial allocation and seven reallocations; isolated responses make none.

At 257 candidates: allocations **1302→523**, reallocations **21→7**, requested bytes **564256→188464**. At 100,000 root fields: logical peak **9442896→5442904**. These temporary volumes are not equated with retained memory or RSS.

## Invariants and validation actually performed

- **154 protocol + engine `--all-targets` tests** pass on Windows stable 1.99.1 and **MSRV 1.88.0**; **2 doctests** pass on each.
- **Stable Clippy `-D warnings` passes**. Clippy 1.88 was attempted but its component is absent; no download/install or invented success.
- Three targeted tests added: capacities and order/duplicates for 0/1/2/3/4/5/16/257 elements in the three containers and partials, wrong-wire/unknowns, merges/empty with presence, **exactly 100,000 fields** and rejection of the next.
- Existing tests retain their oracles: exact 8 MiB root/copied outside framer, shared raw and surviving children, fragments that do not heal, overwritten UTF-8 still rejected, proto2 merges, oneofs, fail-closed safety, noncanonical, ordered opaque unknowns, and each interpreted field counted only once.

No candidate text concatenation, edit application, collector/limit change, sharing across sessions or field loss. No new h2, Linux, codec/Opus/CMake, Android, external network or private corpus profile. No independent review is claimed.

## Options not retained

- **Capacity only**: measured separately, retains the memory gain but keeps temporary allocations; dominated here by the full variant for common goals. Its sources/binaries/630 rows remain in the evidence.
- `shrink_to_fit` after collection: not prototyped, no invented measurement; potentially imposing an avoidable reallocation/copy is unnecessary when the valid count is already available before allocation.
- Boxing/layout change to reduce the 1984 root bytes: outside public API constraint; no gain claimed.
- Removal/compaction of unknowns, inter-session sharing and oneof run rewriting: not applied. Quantified gains do not depend on these approaches.

## Reproduction

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=t cargo test --locked -p rambler-protocol -p rambler-engine --all-targets
# Repeat with cargo +1.88.0; --doc separately for both toolchains.
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=t cargo clippy --locked -p rambler-protocol -p rambler-engine --all-targets -- -D warnings
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=t \
  cargo build --locked --release -p rambler-engine --example backend_cpu_profile
./t/release/examples/backend_cpu_profile.exe --decode-only --iterations 100000 --repeats 1
./t/release/examples/backend_cpu_profile.exe --decode-shapes --iterations 100000 --repeats 1
```

Build baseline/candidate **before** the timing window, preserve each executable then alternate five pairs; never compile during measurements. The baseline/harness patch and capacity-only variant patch are included in the evidence to reconstruct all three sources without touching `main`. Short target `t`, dev/test debug=0, incremental=0; maximum observed **1.1 GiB**. The stable debug cache was removed before the MSRV build to stay within the disk bound; structured evidence is independent of these caches. After self-review and hash verification, the remaining `t` cache (851 MiB) was removed. Only the three small measured binaries/sources and evidence scripts are preserved in the dedicated scratchpad (approximately 1.7 MiB) for review.
