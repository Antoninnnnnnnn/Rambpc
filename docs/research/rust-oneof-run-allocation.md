# Oneof runs: measured local temporary-allocation reduction

Date: 2026-11-17. Baseline production source: `c89f140` (the separate safety-marker
optimization is already present in both variants).

## Private change and preserved semantics

The decoder previously collected every selected oneof run into an outer vector,
with a second vector for each run's payload references. A private `Parts` now
keeps a singleton on the stack and promotes it to a vector only for a true
same-member merge. An iterator groups already parsed fields; the two existing
consumers still parse every interpreted run, including overwritten members.

Runs are consecutive in the selected-member sequence, not byte-adjacent fields.
Independent strings, unrelated fields and wrong wires do not switch a member.
Parent validation/budgets precede child parsing; separate truncated occurrences
cannot heal each other. Opaque selected members stay opaque and retain their
ordered unknown fields. Full/raw/context bytes, empty presence, merge/reset,
UTF-8 errors, field limits and rendering safety remain unchanged.

No public API, model layout, dependency, audio path, network policy or production
`unsafe` is added or changed. This targets temporary grouping, not retained models.

## Validation before measurement

- Independent source review authorizes only bounded offline validation.
- One private unit test covers nine literal run tables. Seven new integration
  tests independently encode wire fixtures and expected complete models, including
  context raw (excluded by its derived equality), exact overwritten errors,
  singleton/merged raw ownership and seven exact-budget fixtures.
- All seven integration tests pass on both unchanged baseline and candidate.
  Existing independent trace, malformed, interior-cut, raw and boundary suites
  remain unchanged.
- Candidate protocol/engine/transport all-targets command: **336 passed, 1 ignored,
  47 target suites**. These are command/target counts, not a deduplicated global
  inventory; do not add overlapping focused-test counts.
- Strict targeted Clippy (`-D warnings`) and workspace formatting check pass.
- Each variant starts with a separate empty build cache. Exact baseline bytes are
  checked against `c89f140`; both release binaries use the identical formatted
  harness. Different binary hashes alone would not establish a different decoder
  because build paths can differ; observed allocation counts provide that evidence.
- Two separate `--decode-oneof --check-only` processes check all manual models and
  exact errors, then return before any warmup or benchmark pass.

## Fixed new campaign

**Five pairs: B/C, C/B, B/C, C/B, B/C; ten measured processes, no retries.**
Each process checks all oracles again, then emits 25 wall and 25 allocation rows:
23 decode workloads and two actual move-retain128 workloads. All **500 raw rows**
are retained. The two oracle-only processes are not measured processes.

Ordinary workloads use 20,000 iterations; large/sparse/error workloads use 256.
Retain workloads use 156 batches of 128 models (**19,968 models**, not 20,000).
There are 128 warmups per workload; counting is separate from wall timing. Full
models/results are observed with `black_box`; fixture/oracle/label/JSON creation
is outside instrumented passes. All Jobs have verified membership and empty
completion; source and owned binary pins remain unchanged.

Selected deterministic per-response observations, stable in all five processes:

| Workload | Allocations, baseline → candidate | Requested bytes |
|---|---:|---:|
| One recognition run | 7 → 5 | 657 → 513 |
| Nested alternative text, 128 bytes | 14 → 10 | 1929 → 1641 |
| One recognition run with 16 merged parts | 24 → 23 | 5541 → 5429 |
| Sixteen alternating recognition/speech runs | 37 → 20 | 5085 → 3933 |
| Two selected parts around 20,000 unknown fields | 20011 → 20010 | 6411827 → 6411651 |

No-oneof controls keep their allocation counts. Genuine unknown fields are still
owned; the sparse workload is not optimized by dropping them. The 128 retained
nested-alternative models keep **341120 logical bytes in both variants**; removing
temporary grouping is not a retained-model memory reduction. Peak temporary
logical sizes can fall, but neither peak nor retained counters measure RSS.
Requested bytes charge the full new realloc size, not physical copy traffic.

Local medians include recognition **533 → 489 ns/model**, nested alternative
**1041 → 898 ns/model**, and sixteen typed switches **3313 → 2976 ns/model**;
the five-sample ranges do not overlap for these workloads. The 16-part merge is
roughly unchanged (**2023 → 2017 ns**), and empty-root medians slightly worsen
(**50.33 → 50.785 ns**) with overlapping ranges. Short error samples and other
noisy results remain in the artifact.

The MAIN-recorded analysis plan predates measurement. Its conservative screen
flags only nonoverlapping slower ranges with a median increase exceeding both
10% and 2 ns/model. No workload flags; this is not proof of zero regression on
these or other inputs. No affinity/frequency lock, real-traffic frequency,
end-to-end/service latency, RSS, universal speed or fastest-engine claim.

## Provenance and current qualification

[Structured evidence and every raw sample](../evidence/rust-oneof-run-allocation-windows-20261117.json)
include per-step outcomes, exact source/binary hashes, the fixed plan and limits.
The manifests cover 118 selected Rust/Cargo files, not the complete build/toolchain
closure. Initial source-review issues (cache separation, exact baseline binding
and an unnecessary closure drop) were addressed before any build or campaign.
Independent numerical review found no discrepancy in raw rows, aggregates,
ordering, normalization or receipt linkage. MAIN accepts this bounded local
allocation reduction. The [separate source CI](../evidence/rust-oneof-run-allocation-ci-20261117.json)
passes all 14 jobs for `b49bc5c` after one runner-capacity retry; it does not
remeasure the performance campaign. The public reports do not independently
timestamp the analysis plan or prove the entire build environment.

No credentials, provider calls, device or microphone were used. Historical CLI,
Cargo.lock, frozen Opus study and completed safety-marker campaign stay unchanged.
Native provisioning and advanced-mode activation are not resolved by this work.
