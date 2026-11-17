# Hypothesis safety fields: measured local allocation reduction

Date: 2026-11-17. Baseline production source: `ee336cf`.

## Change and fidelity

`Message::new_matching` classifies borrowed fields before constructing owned
unknowns. Ordinary message parsing keeps its existing number/wire predicate.
Only hypotheses additionally recognize field 30 with the exact parsed u64 value
0 or 1. Previously these occurrences were copied into owned unknown fields and
then removed with `retain`, leaving empty-vector backing storage allocated.

The last-recognized safety lookup is unchanged. Unknown enum values, wide
varints, wrong wire types, noncanonical encodings, order, raw bytes, presence,
UTF-8 errors, overwritten malformed occurrences and proto2 merges remain intact.
Every field is parsed and charged before classification; 8 MiB / 100,000-field
bounds, rendering safety and public model layout are unchanged. No new dependency
or production `unsafe` is added.

## Independent validation

- The two new storage tests failed on the unchanged baseline as expected; all
  semantic assertions preceding their capacity assertions passed. This was not a
  newly observed safety failure.
- Candidate: protocol, engine and transport all-targets command passed **328 tests
  across 46 target suites, 1 ignored**, zero failures. These are command/target
  counts, not a deduplicated global inventory. Existing exhaustive safety traces
  and malformed/budget checks remain unchanged.
- Strict Clippy (`-D warnings`) and `cargo fmt --all -- --check` pass.
- Independent source review confirmed equal field classification and unchanged
  safety semantics; independent data review recomputed the observations below.

## Fixed new measurement campaign

Same formatted `backend_cpu_profile --decode-safety` source for both release
binaries. Seven manually encoded fixtures each assert an independent **full
S3Response** before timing: absent marker, 0, 1, sixteen alternating duplicates,
noncanonical 0/1, and interleaved recognized/unknown/wrong-wire fields.

Five fixed process pairs: **B/C, C/B, B/C, C/B, B/C**. Each process uses 128
warmups and 20,000 ordinary decodes; retain128 uses 156 iterations of actual
128-model moves. Allocation counting is a separate pass using the existing
unchanged System-backed allocator. All ten processes passed: **350 raw JSON
rows**, no retries, full models observed with `black_box`, all owned Jobs verified
and empty. Fixtures, assertions and JSON formatting are outside timed windows.
No affinity/frequency lock; Windows scheduling noise is retained.

Deterministic per-decode allocation observations, identical across five processes:

- Absent marker: **11 -> 11** allocations, 1359 -> 1359 requested bytes.
- One recognized 0 or 1: **13 -> 11**, 1589 -> 1362 requested bytes;
  unknown-vector capacity **4 -> 0**.
- Sixteen alternating markers: **28 -> 11**, 5263 -> 3647 requested bytes;
  unknown capacity **16 -> 0**. Reallocations: 5 -> 3.
- Mixed genuine unknowns: **19 -> 16**, 2398 -> 1941 requested bytes;
  unknown capacity **8 -> 4**, with the same ordered unknown fields.
- Noncanonical recognized 0/1: **13 -> 11**, 1593 -> 1364 requested bytes.

For 128 retained single-marker models, logical retained bytes fall
**336128 -> 307456** (28672 fewer bytes). Sixteen-marker models fall
**427904 -> 313216**. These are allocator-contract sizes, **not RSS**.
Requested bytes charge the full new size of reallocations, not physical copies.

Local synthetic decode medians: sixteen markers 2290 -> 1128 ns/model; mixed
1556 -> 1034 ns/model, with non-overlapping ranges in this sample. Ordinary cases
and several retain cases overlap or are noisy; all ranges and samples are kept.
**No end-to-end, service-latency, real-traffic-frequency or fastest-engine claim.**

## Provenance and limits

[Structured evidence and raw samples](../evidence/rust-safety-field-allocation-windows-20261117.json)
record selected source hashes, binary hashes, all 350 raw rows, build/test
outcomes, the baseline negative-test receipt, Clippy and per-process Job receipts.
The first published artifact in `c89f140` mistakenly omitted the raw rows while
retaining their count and aggregates; this correction restores the original data
without a new measurement. Source manifests are not a complete build-input or
toolchain closure. The baseline negative tests preceded formatting; their exact
source hashes are separate from the two measured binaries, which share the final
formatted harness.

MAIN reported the independent source/data reviews, strict Clippy flags, formatting
check and two CMD setup failures followed by a successful Windows-normalized
executable path. The evidence labels these observations separately: it does not
embed an independent review receipt, separate local formatting-check receipt or
setup-failure receipts. Historical release CLI, Cargo.lock, old native/public
campaigns and the exact Opus study remain unchanged.

No credentials, real provider calls, microphone or Android runtime were involved.
Native provisioning/authorization, advanced-mode server activation and full
backend parity remain unresolved. CLI/platform integrations remain outside the
initial engine scope.
