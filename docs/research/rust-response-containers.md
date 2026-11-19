# Rust response containers: structure without a rendering policy

Status: 2026-11-14. Static schema, offline tests; **no new service capability validated**, no new network call or Android test. This batch changes neither the request, nor the wire parser, nor the engine's text selection.

## Evidence and scope

The following Java references concern the APK build described in [rust-feature-parity.md](rust-feature-parity.md). Raw metadata verification was confirmed by the batch parent after the Kimi analysis `a99ede6646ef4ac75`; the positions below are landmarks for this build, not durable API identifiers.

| Container | Verified metadata | Rust API |
|---|---|---|
|`RecognitionResult` / `aaln.java:33`|field5: singular `aalp`, presence bit4 (`0x10`); field7: singular `aalp`, presence bit5 (`0x20`); independent of 3/4 and each other, **not a oneof**|`hypothesis_result_field5`, `hypothesis_result_field7`: `Option<HypothesisResult>`|
|`HypothesisResult` / `aalp.java:29`|field3: existing repeated `aalg`; field5: singular `aalo`, presence `0x10`|`hypotheses` unchanged; `fallback_field5: Option<HypothesisFallback>`|
|`HypothesisFallback` / `aalo.java:27`|field1: singular `aalg`, presence bit0; **full hypothesis, not a string**|`hypothesis: Option<Hypothesis>`, `raw`, `unknown_fields`|

Number-based names do not assign an unproven business role. `Fallback` here denotes the verified wrapper, not a selection instruction. `RecognitionResult` field10 remains opaque: knowing that `aalm.field5` contains an `int32 -> int64` map is not enough to name its role.

## Merging and preservation

- Each singular message recursively merges all its proto2 occurrences. An empty occurrence preserves presence and does not erase previous data. An empty field5 wrapper is distinct from a wrapper with an empty field1, which is itself distinct from an absent wrapper.
- Repeated field3 hypotheses retain order and duplicates within their container. The three containers and partial result field4 coexist.
- The wrapper calls the **same `Hypothesis` decoder** as the primary list: text, confidence, closed safety field30 0/1, VoiceEdit/oneof/context, raw and unknowns are not projected into a simple string.
- Wrong wire types remain unknown and create neither presence nor a value change. Unknown safety values do not erase an already recognized 0/1.
- Root `raw` remains exact; `raw` for a merged singular is the logical concatenation of the original payloads, not a single capture. Each occurrence is parsed separately: two malformed fragments cannot repair each other.
- The root limit remains **8 MiB** and the global budget **100000 fields**. Each interpreted field is charged only once, including safety values and branches subsequently cleared by the envelope oneof. Unknown payloads are not traversed recursively.

Length-delimited occurrences formerly opaque are now validated as messages: malformed content on these paths is now an error, even if a later occurrence would have supplied a usable result.

## Structural reading and absence of business priority

`hypotheses()` and `partials()` remain limited to their existing primary paths. No text is concatenated, promoted, selected, or applied.

`all_hypotheses()` is available on `HypothesisResult`, `RecognitionResult`, and `S3Response` for inspection of all candidates in the active result. Its order is **canonical by container**, not a ranking or global wire order:

1. RecognitionResult field3 container: repeated list, then any fallback;
2. field5 container: repeated list, then any fallback;
3. field7 container: repeated list, then any fallback.

Public model fields allow this provenance to be recovered. The iterator does not resurrect results already cleared by the outer oneof and does not include partial hypotheses, which have a different schema. The batch parent separately adapts the engine's safety policy.

A priority from another client must not be transplanted:

- SmartDictation (`suz.java:128–189`) uses primary 3, then fallback only if its list is empty; it can use 7 for non-final.
- Rambler (`ihm.java:142–157,224–253`) ignores 5/7/fallback on these paths.

These distinct static behaviors prove no server priority or new remote feature.

## Offline validation

`tests/response_containers.rs` provides eight tests, including:

- **11111 exhaustive sequences** of length 0 to 4, ten independent operations: list, fallback and empty fallback in each of containers 3/5/7, and partials 4; operation-based oracle independent of production encoders/decoders, envelope merging and model/raw equality;
- recursive fallback → full hypothesis → VoiceEdit → context merge, preserving safety 1 despite unknown values/empty occurrences;
- invariance to wrong wire types with exact presence and unknowns;
- every interior cut of a field at the new singular boundaries, including VoiceEdit/context and subsequently cleared branches; positive controls for cuts between fields;
- invalid wire/UTF-8 cannot be masked, field1 verified as a message;
- exact budget 100000, rejection at 100001, with safety and merged/cleared branches; exact 8 MiB limit on a new path, rejection above it; field10 still opaque.

Verified with `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`, short target local to the worktree:

- new tests: **8/8**;
- existing suites: adversarial 4, decode 20, requests 10, safety 5, wire 4, fixture mutations/truncations 1; doctest 1: **all pass**;
- `cargo clippy -p rambler-protocol --all-targets --locked -- -D warnings`: OK.

The initial full suite revealed a historical assertion for the parent to adapt in `tests/fixtures.rs:146`: it expected RecognitionResult field5 among the unknowns. It logically becomes structural; fixture files, payloads and SHA-256 are not changed by this batch. The affected test is not counted among the successes above. The last full run explicitly excluded only this historical assertion pending integration.

These bounded tests are neither an exhaustive fuzzing campaign nor service evidence. These structures alone authorize no new rendering.
