# Rust decoder: defensive preallocation, final version

## Decision and traceability

Prototype isolated on **`67ac9a5`**, with no commit or change to `main`.
[Separate final evidence](../evidence/rust-backend-decode-defensive-capacity-profile.json).

The previous **V2 variant with exact capacity for all lists is rejected** after
amplification on a malformed child was measured. Its
[1050 A/B lines and 42 diagnostic lines](../evidence/rust-backend-decode-allocations-profile.json)
remain unchanged and are not presented as measurements of the final fix. The new
JSON separately retains error diagnostics, rejected V2, **intermediate V3** with a
three-match hint, and the **final version with a hint bounded before filtering**.
Sources, patches, binary SHA256 hashes and logs are retained. No model dump or
private corpus is included.

## Measured regression and correction

Fixture: 90,000 repeated three-byte fields, each containing child `[0x00]`,
about **270 KB on the wire**, plus three wrappers. The global budget remains
below 100,000. The first child must fail with `FieldNumber`. A control uses a
valid empty first child followed by invalid children. Both hypothesis/partial
variants are measured without OOM injection.

| Case, error on first child | Baseline: max request / live peak | Rejected V2: max / peak | Final: max / peak |
|---|---:|---:|---:|
| 90,000 hypotheses | 5 242 880 / 9 113 591 | **33 840 000 / 39 353 543** | 5 242 880 / 5 513 543 |
| 90,000 partials | 5 242 880 / 9 113 591 | **8 640 000 / 14 153 543** | 5 242 880 / 5 513 543 |

V2 reserved storage for all models **before validating the first child**, based
only on syntactically parsed wrappers. The risk was real; favorable timings for
valid responses do not justify it. With one valid child before the error, the
final version reaches a peak of **5 515 046 bytes** for hypotheses, **5 513 926**
for partials; the maximum requested remains **5 242 880** in both cases.

The final version:

1. Finds the first child and fully decodes it **before allocating the model
   vector**; zero children returns an empty vector.
2. Then computes a hint over **at most four prefix `RefField` values, before
   filtering by number/wire type**, and reserves between one and four models.
   These references have been parsed; **their children are not thereby
   validated**. This is not a count of all candidates.
3. Successfully decodes each subsequent child **before** its `push` and any
   vector growth. It never reserves in proportion to tens of thousands of
   wrappers whose children have not yet been validated.

Intermediate V3 already reserved at most four models after the first success,
but `take(3)` **after** a filter could rescan a long tail of unknowns to find
three matches. It is replaced by the bound **before filtering**. The 420 V3 A/B
lines remain separate in the new evidence, not deleted.

Usual capacities remain 0/1/2/3/4 for these small lists. Beyond that, growth is
again geometric on success: **257 candidates → capacity 512**, as in baseline,
**not 257** as in V2. A sparse list of two candidates separated by 20,000 unknowns
may retain capacity four rather than rescan that tail solely for a hint. No
universal capacity improvement is promised.

## Other retained private optimizations

`Message::data` is lazy, single parts are on the stack, the first `parse_ref`
vector is reused, and only private parsers change signature. `Bytes`, `String`,
`wire::Field`/`Value`, public models/layouts and retained unknowns are unchanged.
`Hypothesis` is **376 bytes**, `S3Response` **1984** on this target, before and
after.

Overwritten strings are still all checked for UTF-8. fixed32/64 values are
validated by the wire parser before lookup from the end. Each submessage
occurrence is parsed separately before merging; fragments cannot repair one
another. Oneof runs, presence/empties, closed safety and candidate order remain
identical. Hypotheses are never concatenated or automatically applied. The global
**100,000 field** budget, **8 MiB** root, exact copy out of the framer and `Bytes`
ownership are unchanged; the hint reinterprets no bytes and does not charge the
budget a second time. No new `unsafe` or dependency.

## Actually measured final A/B

Windows 11 Pro 10.0.26300, `x86_64-pc-windows-msvc`, Rust **1.99.1**, release,
incremental disabled. Same harness in baseline and final candidate; only
`decode.rs` differs. Five **AB/BA/AB/BA/AB** pairs, binaries built before the
window announced to the coordinator, with no concurrent build/benchmark.

128 warmups then 100,000 requested operations for usual cases, 781 batches of
128 for retention; shapes adapted to candidate count. Two discriminating sparse
cases, one/two candidates and 20,000 unknown fields, 64 measured operations each.
Errors: 12 shapes, 32 operations per shape. The allocation pass is separate from
wall time and all final live-byte checks pass. `black_box` observes the whole
model; fixture/prior assertions, JSON and console remain outside measurement.
The added maximum counter is in the existing non-`unsafe` helper; all `unsafe`
forwarding to `System` is unchanged.

**460 final A/B lines**, 48 final error lines, 72 V2/V3/baseline error lines,
48 initial diagnostic lines and 420 intermediate V3 A/B lines are retained in
the new JSON, in addition to the unchanged old JSON.

Time **ns/response**, median (minimum–maximum), final five pairs only.

| Case | Baseline | Final |
|---|---:|---:|
| decode small | 2105,74 (2033,04–2450,57) | 1526,61 (1484,59–1844,57) |
| decode large | 2985,56 (2884,12–3120,57) | 2400,46 (2296,68–2934,73) |
| retention 128 × small | 3952,00 (3690,94–4092,61) | 2833,42 (2790,39–3586,33) |
| retention 128 × large | 4348,19 (3939,14–4617,41) | 3466,14 (3307,90–3645,62) |
| 257 short candidates | 143670,44 (130413,37–164493,06) | 101265,30 (77194,60–117576,35) |
| 16 oneof runs | 4017,70 (3900,96–4593,02) | 3373,79 (3209,89–4672,48) |
| sparse 1 + 20 000 unknowns | 2173270,31 (2091153,12–2327496,88) | 1917331,25 (1901301,56–2055765,62) |
| sparse 2 + 20 000 unknowns | 2098803,12 (1990204,69–2223492,19) | 1835901,56 (1787662,50–2146471,88) |

All **19 medians** are favorable; this is not a universal CPU guarantee. One
oneof pair is **+17.2%**, one sparse2 pair **+6.1%**, retained without exclusion;
four of five pairs are better for both cases. No locked affinity/frequency, and
real OS/allocator variability. No pipeline/remote-service speedup is inferred
from these microbenchmarks.

### Final allocations and retention, not reused V2 figures

| Measure | Baseline | Final |
|---|---:|---:|
| Allocations per isolated nested response | 39 | 24 |
| Requested, small | 5703 | 3135 |
| Requested, large | 29511 | 26943 |
| Live bytes, small batch | 639488 | 495104 |
| Peak, small batch | 640320 | 495960 |
| Live bytes, large batch | 3686912 | 3542528 |
| Peak, large batch | 3687744 | 3543384 |

Large batch: still **1 591 040 protobuf bytes**, retention saving **144 384
bytes = 3 × 376 × 128**, or **−3.92%** of the `Bytes` baseline. Small batch:
**−22.58%**. Seven batch-vector reallocs unchanged; no realloc on the isolated
response. No field/raw/unknown removed for this gain.

**Explicitly non-monotonic temporary peak tradeoff:**

- 257 candidates: allocations **1302→523**, reallocs **21→14**, requested
  **564256→475352**, but peak **220880→222904 bytes** (+2024, about 0.92%).
- Sparse1: peak **2757285→3266789**; sparse2 **2757674→3267939** (about +18.5%).
  Reusing the `Vec<RefField>` can retain unused geometric capacity while copying
  unknowns, whereas the old second vector was compact. This temporary increase
  is distinct from the large speculative model reservation now removed; it is not
  hidden by the retention gain.

Sizes are logical under the `GlobalAlloc` contract; `max_request_bytes` includes
successful alloc/realloc in **all** categories, not just models. Reallocs charge
their entire new size. **Neither RSS nor physical copy traffic was measured**;
there is no new collector heap guarantee.

## Final validation and limits

**156 protocol + engine `--all-targets` tests pass on stable 1.99.1 and MSRV
1.88.0; 2 doctests pass on each; stable Clippy `-D warnings` passes.** MSRV
Clippy remains unavailable (component absent, no download requested).

Five targeted tests: small capacities/duplicates/order, 257 candidates in
containers 3/5/7 and partials, wrong-wire/unknowns, merge/empty presence, sparse
with 20,000 unknowns, 90,000 invalid wrappers with/without valid prefix, exactly
100,000 fields and rejection of the next. All previous strict oracles for
overwritten UTF-8, malformed fragments without repair, proto2/oneof/safety,
raw/copied-root and budgets remain executed, without relaxation.

No new h2, Linux, audio/Opus/CMake, Android or remote-service profile, credentials
or `.env`. No independent model review is claimed. The parent identified the
defensive case and reviewed the correction's structure; this does not replace an
unavailable independent review.

## Reproduction and disk

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=t cargo test --locked -p rambler-protocol -p rambler-engine --all-targets
# Same command with cargo +1.88.0, then --doc on each toolchain.
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=t cargo clippy --locked -p rambler-protocol -p rambler-engine --all-targets -- -D warnings
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=t cargo build --locked --release \
  -p rambler-engine --example backend_cpu_profile
# Run the preserved binaries in AB/BA order, never Cargo during timing.
./t/release/examples/backend_cpu_profile.exe --decode-only --iterations 100000 --repeats 1
./t/release/examples/backend_cpu_profile.exe --decode-shapes --iterations 100000 --repeats 1
./t/release/examples/backend_cpu_profile.exe --decode-sparse --iterations 100000 --repeats 1
./t/release/examples/backend_cpu_profile.exe --decode-errors --iterations 32 --repeats 1
```

Short target `t`, dev/test debug=0, incremental=0. Maximum observed for the
corrective rerun: **998 MiB**, then deletion of the stable debug cache before
MSRV; **851 MiB** after MSRV. Sources/binaries and lightweight evidence remain in
the dedicated scratchpad for review, separate from removable caches. After local/
parent review of the fix and evidence verification, `t` was deleted; preserved
executables, snapshots and metrics occupy about **5 MiB** in the dedicated scratchpad.
