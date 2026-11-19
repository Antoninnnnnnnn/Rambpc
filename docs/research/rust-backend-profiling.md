# Profiling the Rust backend outside the audio codec

## Scope and method

Baseline: `e427fb1`. Exclusively synthetic data, local execution without
credentials, `.env`, private corpus or external service connection. The codec and
`rambler-audio` sources are unchanged. The previously rejected CRC is not reassessed.

Two independent executables in `crates/rambler-engine/examples`:

- `backend_cpu_profile` measures initial/context/audio requests, nested response
  decoding and decoding with ordered retention in batches of 128. Fixtures contain
  synthetic text, a VoiceEdit, recognized safety status and unknown fields at the
  root and hypothesis levels.
- `engine_loopback_profile` uses the real `Engine`, tonic/h2 transport and real
  `collect_session`, with a local IPv4 TCP server on an ephemeral port. It reuses
  the raw server from engine tests, without a development dependency on
  `rambler-audio` (no cycle). Its allocator is not instrumented.

### CPU and allocations

Each CPU case performs 128 warmups, then five wall-time passes of 20,000 operations
(156 batches of 128 for retention), then a separate counting pass. For audio,
reference and production alternate order in each pair, with separate warmup and
counting on each pass. The reference is the frozen `e427fb1` assembly, including
nested temporaries; results are compared byte for byte before measurement.
`black_box` observes produced payloads and prevents their elimination. Fixture
allocations, JSON formatting and console output are outside measurements.

The CPU global allocator delegates to `System`. During the counting pass,
`alloc`/`alloc_zeroed`, `realloc`, requested bytes and peak additional live bytes
are counted exactly after successful allocation. A `realloc` charges **its entire
new size**, not just its growth. This volume is therefore neither actual copy
traffic nor RSS. The peak counts logical sizes under the `GlobalAlloc` contract,
not internal OS reserves or possible internal overlap in `System::realloc`. All
allocations of each operation are born and die during the pass; preexisting
fixtures are neither modified nor freed. The final live value must return exactly
to its starting value. Execution is synchronous and single-threaded, without a
concurrent network runtime. Wall passes disable these counters but retain the
allocator branch: this is not a CPU profile without any instrumentation overhead.

CPU retention is a move operation into `Vec<S3Response>` with an 8 MiB limit, not
a second authoritative collector implementation. The network executable exercises
actual collection and its termination invariants.

### Pipeline and backpressure

By default, each session sends 16,384 opaque 640-byte chunks, totaling 10,485,760
bytes, followed by a late Unicode context and the END marker. Each chunk contains
its sequence number; it is **not** valid Ogg/Opus and the local server does not
claim to recognize speech. The engine must treat the encoded stream as opaque.
The server returns one response per chunk, including 128 evenly spaced finals,
then a single S3 END.

Four modes, in rotating order across five repetitions:

1. `collect_active`: real collector active without intentional delay;
2. `observe_active`: active event consumer without retention;
3. `collect_reader_stall`: real collector active; server reading suspended for at
   least 1 ms before one chunk in 64;
4. `observe_consumer_stall`: server reading active; event consumer suspended for
   at least 1 ms after one response in 64.

The server response queue has capacity **1**; production queues are unchanged.
A producer is never left running without a consumer. Delays use the Tokio timer,
do not guarantee exactly 1 ms and do not imitate the real-time pace of audio capture.

Wall time starts just before `Engine::start` and ends after session, engine,
transport and server shutdown and joining. Fixture construction, prior validation
and initial connection are excluded. It includes server validations: exact equality
of init, reassembled audio chunks with their actually observed elapsed, context
and END; actual upload order and EOF; raw response equality and client-side final
order. Server audio verification uses the public parser and frozen `e427fb1`
request assembly, even in the candidate binary: a client serializer gain is thus
not counted a second time on the server. This verification allocates: that cost
is intentionally included and prevents interpreting these measurements as the
cost of transport or engine alone. Observation mode checks every response, the
collector only retained finals; their difference therefore does not mathematically
isolate collection cost.

Every run checks protobuf volumes in both directions, chunk and final counts, a
single engine EOF, upload EOF, the three cleanup flags and exactly one source
close call. The collector retains its default limits (128 finals / 8 MiB); the
protocol retains its 8 MiB / 100,000 field limits, safety and proto2/unknown semantics.

## Measurement environment

Windows 11 Pro `10.0.26300`, target `x86_64-pc-windows-msvc`,
`rustc 1.99.1 (58cf8f9c3 2026-10-13)`, Cargo `1.99.1`, LLVM `23.1.1`.
Processor reported by the environment: `Intel64 Family 6 Model 154 Stepping 3,
GenuineIntel`, 12 logical processors. Default Cargo release profile;
`RUSTFLAGS`, `CARGO_BUILD_RUSTFLAGS` and `CARGO_PROFILE_RELEASE_LTO` unset.
Single-threaded Tokio pipeline runtime. No CPU affinity or frequency lock; the
observed spread is retained without promising an absence of Windows noise. Each
window was announced to the coordinator and reserved without other compilation
or benchmarks by agents on the assignment.

## Reproduction

From the repository root, in a window without compilation or other benchmarks:

```sh
cargo build --release -p rambler-engine --examples
# Run binaries directly to exclude Cargo/compilation from wall time.
./target/release/examples/backend_cpu_profile --iterations 20000 --repeats 5
./target/release/examples/engine_loopback_profile --chunks 16384 --repeats 5
```

On Windows, executables have the `.exe` suffix. Each line is a standalone JSON
object. `wall_ns` is monotonic elapsed time; `ns_per_unit` normalizes per request
or response, including batches of 128. CPU sizes 128/4096 describe padding text,
not total response size. Network volumes are protobuf payloads without gRPC/h2/TCP
envelopes. Request `elapsed_ms` depends on actual execution and may slightly
change their size between repetitions; audio content and order do not change.

This offline profile provides neither remote-service latency, recognition quality,
codec memory consumption nor Android performance.

## Retained results (consolidated 2026-11-14)

[Structured evidence, 96 CPU lines and 40 pipeline sessions](../evidence/rust-backend-offline-profile.json).
The last pass uses the reference with nested temporaries and `Vec::extend(field)`
from `e427fb1`. Intermediate passes are privately marked `superseded` and
**excluded** from the figures below. Agent finalization encountered its quota;
the parent checked all 40 sessions and directly consolidated already produced
data, without inventing a new run.

### Audio requests

| Opaque audio | Reference median ns (min–max) | Candidate median ns (min–max) | Alloc/realloc before→after | Requested bytes before→after | Logical peak before→after |
|---:|---:|---:|---|---:|---:|
| 640 | 1399.605 (1332.790–1548.935) | 48.445 (47.130–50.765) | 8+3→1+0 | 2030→658 | 1950→658 |
| 4096 | 1416.995 (1378.970–1577.565) | 74.660 (70.290–78.555) | 8+3→1+0 | 12398→4114 | 12318→4114 |
| 65536 | 6925.355 (6137.865–15918.405) | 1653.345 (1607.680–1753.890) | 8+3→1+0 | 196723→65556 | 196643→65556 |

**Retained local optimization**: sizes precomputed and bounded before allocation,
direct writing of the same varints/extensions, one audio copy. No quality loss
or reduction in validation. The independent oracle covers 220 boundaries and
2000 deterministic vectors, the exact 8 MiB root message and rejection of the
next byte. The wall-time gain belongs to this Windows microbenchmark with the
instrumented allocator disabled but present, not a promise of faster remote
dictation or a language comparison.

### Full pipeline, five alternating pairs

| Mode | Baseline median ms (min–max) | Candidate median ms (min–max) |
|---|---:|---:|
| collect_active | 924.1896 (852.0531–1019.7183) | 823.8450 (794.5236–1060.5637) |
| observe_active | 916.9192 (899.9580–1025.5379) | 890.5094 (790.2901–993.2578) |
| collect_reader_stall | 3916.1529 (3644.0743–3961.6783) | 3910.3259 (3400.9980–3958.1498) |
| observe_consumer_stall | 3976.8663 (3972.0477–4003.1131) | 3981.2580 (3974.9819–4006.4920) |

Distributions overlap; **no robust overall gain is retained**. All 40 sessions
check 16,384 chunks/10 MiB, 128 finals, order, exact payloads, upload/engine EOF
and single cleanup. The server also checks every request with the frozen builder;
this work contributes to timings. Stall modes include Windows timer granularity,
not exactly 256 ms added.

The batch of 128 large responses retains 1,591,040 protobuf bytes but reaches
12,463,808 logical additional heap bytes on this model. There is no leak in the
pass (final live bytes equal the start), but the collector's 8 MiB limit applies
to payloads, **not** hierarchical allocations. Raw/unknown fields are not removed
to artificially improve this figure.

The report contains SHA hashes of actually measured sources/binary and delivered
benchmarks. Only Clippy attributes/comments were subsequently added to both `main`
functions to retain runtime rejection of debug mode. They do not change the
workload; this is not presented as a new benchmark. The measured source batch
predates the new rendering/segment policies and later transport fixes, and thus
does not retroactively represent all final code.

## Shared `raw` retention prototype (2026-11-14, not integrated)

Current baseline **`fdc9fd9`**, distinct from the historical profile above.
[Structured evidence: 100 CPU lines and 40 h2 sessions](../evidence/rust-backend-shared-raw-profile.json).
The prototype remains in the worktree, without a commit or integration into `main`.

### Diagnosis before modification

For the same batch of **128 responses of 12 430 bytes**, or **1 591 040 protobuf
bytes**, the current baseline retains **12 635 008 logical bytes** and reaches a
peak of **12 635 840 bytes**. The difference from the old peak of 12 463 808
comes from models enriched since the historical profile; these measurements are
not compared as if the models were identical.

The capacity and `size_of` inventory explains retention per response exactly,
without treating it as RSS:

| Allocation retained per response | `Vec` baseline | `Bytes` prototype |
|---|---:|---:|
| Backing of `raw` and `active_raw` | 82 625 | 12 430 |
| `Bytes` sharing metadata | 0 | 24 |
| Text string capacities | 12 360 | 12 360 |
| Unknown vectors/fields/values | 502 | 502 |
| Hypothesis vector allocation | 1 408 | 1 504 |
| Root `S3Response` vector element | 1 816 | 1 984 |
| **Total** | **98 711** | **28 804** |

The seven baseline copies are root `raw` (12 430), response (12 410), recognition
(12 390), list (12 385), hypothesis (12 382), VoiceEdit (8 238), and `active_raw`
(12 390). Their **82 625 bytes represent 83.7%** of retention. Temporary parsing
allocations do not dominate retention: the peak exceeds live bytes for the full
batch by only **832 bytes**, both before and after. Unknown duplicates (`Field::raw`
and `Value::Bytes`) remain present and are not removed.

### Minimal choice and invariants

Keeping multiple independent, mutable public `Vec<u8>` values prevents safely
sharing their backing. Reducing their capacities does not remove copies of the
same bytes at each level. The prototype limited to the 0.1 API therefore changes
**only** model `raw` storage to `bytes::Bytes`, and
`RecognitionResponse::active_raw` to `Option<Bytes>`.

The decoder checks the root limit **before allocation**, copies the root payload
exactly once, then uses `Bytes::slice_ref` for single-occurrence children. A real
proto2 merge allocates a contiguous logical concatenation of exact size; each
occurrence is still parsed separately before concatenation. For known oneof
members, `active_raw` shares the decoded model's `raw`, including after merging.
The parser continues to visit overwritten members and charge each interpreted
field only once against the global budget. Unknowns remain opaque, ordered and intact.

No new `unsafe` code, new workspace dependency (`bytes` was already present),
transport change, removal of unknowns, permissive decoding, GC or change to the
**8 MiB / 100 000 field** limits. The root copy is intentional: this is not
zero-copy transport.

**Ownership tradeoff:** retaining a small child `raw` can keep its entire exact
root payload alive, up to **8 MiB**. It never retains a larger framer/transport
buffer unrelated to the payload. To explicitly detach a small child, use
`bytes::Bytes::copy_from_slice(&child.raw)` or `child.raw.to_vec()`; the former
retains an immutable `Bytes` type, the latter produces an independent mutable
`Vec`. Multiple merges can still retain their additional concatenations: the
measured gain is not a new collector heap bound guarantee.

### Matched A/B measurements

Five process pairs, baseline/candidate order alternating each pair, identical
unchanged fixtures. Each CPU case retains 128 warmups then a wall-time pass of
20 000 responses (156 batches of 128 for retention), and a **separate** allocation
pass. Counters are disabled during wall time; their allocator branch remains.
New `retained_increment_bytes` reading immediately after filling the batch and
before destruction, outside formatting/output. All passes return to their initial
live bytes after destruction.

| Case | Baseline median ns/response (min–max) | Prototype median ns/response (min–max) |
|---|---:|---:|
| decode 128 | 4 965,54 (2 939,31–5 568,70) | 2 806,84 (2 657,80–4 069,25) |
| decode 4096 | 6 185,47 (5 782,81–10 048,85) | 4 280,60 (2 971,53–5 774,55) |
| retention 128 × small | 9 482,36 (7 168,98–10 638,64) | 5 896,12 (4 915,88–8 147,60) |
| retention 128 × large | 23 052,54 (18 714,34–27 205,86) | 16 781,54 (10 830,02–20 183,81) |

For each isolated large response: **44 → 39 allocations**, no realloc,
**99 586 → 29 511 requested bytes**. The batch retains its single initial vector
allocation and seven growth reallocations; response movement is not replaced by
an artificial collector. For the large batch:

- live retention **12 635 008 → 3 686 912 bytes**, or **−70.82%**;
- additional peak **12 635 840 → 3 687 744 bytes**;
- for the small batch, live bytes **953 216 → 639 488 bytes**.

Logical sizes of all `raw` fields and wire volumes are unchanged. The additional
264 metadata bytes per response are included, not hidden. **No RSS gain is inferred
from these `GlobalAlloc` sizes**, and allocated bytes are not presented as a
physical measure of copy traffic.

The real h2 engine/collector ran 40 additional sessions of 16 384 chunks, 10 MiB
opaque audio and 128 finals, with the same payload, order, EOF and cleanup checks.
The four modes run in the same order in each process (`--repeats 1`); variant order
alternates between processes. Baseline → prototype medians: `collect_active`
**1 087,32 → 1 116,04 ms**, `observe_active` **1 219,85 → 1 011,45 ms**, stalled
reader **2 979,01 → 2 819,98 ms**, stalled consumer **3 996,50 → 3 997,72 ms**.
Spread is high and intervals overlap: **no overall pipeline gain is claimed**.
Only the logical retention gain is retained as a robust result of this prototype.

### Public API and migration

Types affected by `raw: Vec<u8> → Bytes`: `EditingContext`, `VoiceEditResult`,
`Hypothesis`, `PartialHypothesis`, `HypothesisFallback`, `HypothesisResult`,
`PartialResult`, `RecognitionResult`, `SpeechEventResult`,
`RecognitionResponse`, `S3Response`. `RecognitionResponse::active_raw` becomes
`Option<Bytes>`. This is an explicitly accepted source break for this 0.1
prototype, not a promise of silent compatibility.

- Construction from a `Vec`: `raw: vector.into()`; option: `Some(vector.into())`.
- Slice access: `raw.as_ref()` or `&raw[..]`, instead of `as_slice()`.
- Mutation: explicitly create `let mut owned = raw.to_vec()`, modify that `Vec`,
  then replace the field with `raw = owned.into()` if needed.
- `raw.clone()` now shares immutable bytes; use the detachment described above
  if ownership must become independent.
- `decode_response(&[u8])`, all wire helpers, `wire::Field`, `wire::Value`,
  request returns, strings and other public fields remain unchanged. The engine
  required **no production change**, only adaptations to tests/examples that
  assumed `Vec`.

### Validation and reproduction

**144 protocol + engine all-targets tests** pass on Windows stable 1.99.1 and MSRV
**1.88.0**, with **2 doctests** and Clippy `-D warnings`. The two additional tests
check the exact copy out of a large framer buffer, shared child views, their survival
after destruction of parent and input, explicit detachment, and the `active_raw`
alias after merging. Existing tests retain their independent oracles: Unicode,
NaN, presence/empties, oneofs, invalid fragments that do not heal and exact budgets.

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=t cargo test --locked -p rambler-protocol -p rambler-engine --all-targets
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=t cargo +1.88.0 test --locked -p rambler-protocol -p rambler-engine --all-targets
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=t cargo build --release -p rambler-engine --examples
./t/release/examples/backend_cpu_profile --decode-only --iterations 20000 --repeats 1
./t/release/examples/engine_loopback_profile --chunks 16384 --repeats 1
```

SHA256 hashes of the four retained binaries, baseline git blobs, measured
candidate sources, raw lines and calculated summaries are in the structured
evidence. Baseline binaries include only the common inventory added to the
profiler; their codec is that of `fdc9fd9`. The A/B window was coordinated
without other builds/benchmarks. No benchmark was rerun to eliminate its spread.
The above measurements do not claim to validate the entire workspace or Linux
after this prototype.
