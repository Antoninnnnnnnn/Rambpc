# Offline edit acceptance contract (v1)

This check answers one question: did a matched candidate produce the **predeclared intent and replacement proposal outcome** for every declared edit case, under one selected policy? The default remains `Strict`. The separately scoped `S3ClientCompatible` check requires explicit opt-in, and its results must not be pooled with strict results. Neither mode measures transcription quality, actual editor behavior, or provider conformance. The examples and tests below are synthetic.

## Why a new input pair?

[`validate_corpus.py`](../../tools/validate_corpus.py) reports WER, rendering, and validity/conflict checks on an intent-2 proposal. Its success status excludes edit quality. [`sanitize_corpus_report.py`](../../tools/sanitize_corpus_report.py) retains some of these observations but does not add independent expectations or qualify cases for exact comparison. A valid proposal, successful render, low WER, equal report hashes, or a missing proposal key is **not** an edit acceptance result. Neither existing report schema is an input to this checker, including the sanitizer's optional controlled-text output.

## Local input schemas

Two separate, local UTF-8 JSON regular files are required. Keep observations and any non-synthetic declaration untracked. Both schemas use closed objects (all listed fields required, no extra or duplicate keys). Each file is at most 1 MiB; the declaration has 1–100 rows and the observation has 0–100. File/row identities are case-sensitive opaque ASCII tokens matching `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`; do not encode names, accounts, paths, or text in them. Each `case_id` is unique within its file; row order is immaterial.

Invented expectation document:

```json
{
  "schema": "rambpc-edit-expectations-v1",
  "artifact_id": "declared-plan",
  "provenance": {
    "corpus_id": "toy-corpus", "run_id": "planned-run",
    "candidate_id": "candidate-build", "policy": "strict"
  },
  "cases": [
    {"case_id": "toy-selection", "input_id": "input-01", "intent": 2,
     "outcome": {"kind": "edit", "window": "🦔: bleu!", "selection_utf16": [4, 8]}},
    {"case_id": "toy-rewrite", "input_id": "input-02", "intent": 3,
     "outcome": {"kind": "no_edit"}}
  ]
}
```

Invented observation document (illustrates one complete and one unqualified observation, so the pair is **inconclusive**):

```json
{
  "schema": "rambpc-edit-observations-v1",
  "artifact_id": "separate-observation",
  "provenance": {
    "corpus_id": "toy-corpus", "run_id": "planned-run",
    "candidate_id": "candidate-build", "policy": "strict"
  },
  "cases": [
    {"case_id": "toy-selection", "input_id": "input-01", "status": "observed", "intent": 2,
     "outcome": {"kind": "edit", "window": "🦔: bleu!", "selection_utf16": [4, 8]}},
    {"case_id": "toy-rewrite", "input_id": "input-02", "status": "unqualified"}
  ]
}
```

- **Identity/provenance.** Match `corpus_id` (frozen corpus revision), `run_id` (one planned collection), `candidate_id` (the exact build/configuration under test), and `policy` (must match the invocation in both files). Match both `case_id` and `input_id` per row. The latter identifies the immutable stimulus, language/options/hints and exact captured input window, not a label reused after inputs change. Use distinct `artifact_id`s. Locally bind these identifiers to the actual inputs, executable/configuration and separate artifacts before interpreting the counts; do not guess or backfill a candidate's identities from the declaration. Matching labels and distinct artifact IDs are only structural checks, not authentication or proof of independence.
- **Declaration.** Freeze and review the identities and expected semantics before collection. `intent` is raw `2` (edit history), `3` (rewrite), or `null` (**no** `VoiceEdit`, not an absent/undecodable intent field or numeric zero). Exactly `{"kind":"no_edit"}` means a verified proposal rejection or absence under the selected policy. In default `strict`, only intent `2` may expect `edit`; this uses the historical policy ([Rust](../../crates/rambler-engine/src/results.rs), [Python](../../src/rambler/results.py)). A renderable rewrite string does not qualify as a strict edit. No semantics are defined for 4/5/6 or other excluded codes.
- **Independent observation.** A later, separately produced candidate artifact records what was observed, not what the declaration wanted. Use `status: "observed"` only with a completed session, one unambiguous safe final hypothesis, a verified linkage to the exact run/input and captured context, a known raw intent (or confirmed absence of `VoiceEdit`), and a recorded proposal decision under the selected policy, including explicit rejection or verified absence if `no_edit`. Do not substitute transcript or rendered/ASR text, or reconstruct a proposal or its offsets from the declaration. Record the candidate's actual result even when it violates the policy. Failed/partial/ambiguous collection (including identical duplicate finals), rejected or uninterpreted safety status, missing evidence or intent, or lack of an explicit proposal check is `unqualified` (omit `intent` and `outcome`), or leave the row missing. Any other observed integer intent (signed 32-bit) is well-formed but unqualified, and its outcome is not compared. `no_edit` is evidence about a proposal decision only, not evidence that an editor did or did not mutate.
- **Edit outcome.** `window` is the complete replacement for the captured window, not a diff, selection-only string, transcript, or full document. Exact Unicode scalar equality is required, without normalization or trimming. `selection_utf16` is either `null` (no structured selection returned) or a half-open `[start,end]` in UTF-16 code units local to that replacement. Both offsets must be on scalar boundaries, from zero through the window length inclusive. A collapsed selection has equal endpoints, not `null`. An empty window is an edit, not a rejection. Windows have at most 65,536 UTF-16 code units and may not contain unpaired surrogates.

### Explicit compatible proposals

Pass `--policy s3-client-compatible` and set `provenance.policy` to `"s3-client-compatible"` in **both** inputs. The role/schema names and row matching are unchanged; the policy is never selected from input data. An input pair may not mix policies. To compare the same input across modes, collect a separately bound proposal observation under each policy and run twice. `AsrOnly` is a render/comparison control, not a proposal acceptance mode.

In this mode each `edit` outcome also requires `provenance`. For example, a compatible intent-3 rewrite from field 7 could declare:

```json
{"kind":"edit","window":"L | é🇫🇷👩‍💻 | R","selection_utf16":[4,15],"provenance":"voice_edit_context"}
```

- `voice_edit_context` (member 7) is eligible for intent `2` or `3`, with a non-null returned selection (which may be collapsed).
- `voice_edit_text` (member 3) is eligible for intent `2` only, with `selection_utf16: null`. Renderable intent `3` member 3 does **not** establish a replacement range or proposal.
- `hypothesis_text` is never an expected proposal. An otherwise qualified edit observation that reports this source is a **mismatch**, even if its text and offsets exactly match. Likewise, a structurally valid observation with the wrong source or selection mismatches; an absent or unknown source label is invalid. Derive source from the actual candidate projection backing the proposal, not from the words, raw intent alone or the oracle. Literal ASR can contain the same command words and remains literal. Verified `no_edit` has no source field, as in strict mode; neither mode scores literal transcription.

The checker validates UTF-16 scalar boundaries and exact declared offsets. It does not infer grapheme segmentation, text scope, document identity or authorization. The declaration must independently include both preserved sides, and the host must check revision, document and anchor inside the application transaction. Check splice grapheme boundaries in whole-document context where applicable; do not infer document-relative offsets from a window-relative range alone. A command classification (e.g. send or erase) is not authority for a host effect.

## Run and interpret

```sh
python -I -S -B tools/check_edit_acceptance.py EXPECTATIONS_JSON OBSERVATIONS_JSON
python -I -S -B tools/check_edit_acceptance.py --policy s3-client-compatible EXPECTATIONS_JSON OBSERVATIONS_JSON
```

The checker reads only the named input documents, never writes them, and has no integration with the app, service, capture pipeline, or corpus validator. It compares only after **both** documents pass syntax and schema checks. The result is one compact JSON record (`rambpc-edit-acceptance-result-v1`, scope `offline-strict-proposal-only` or `offline-s3-client-compatible-proposal-only`), with these verdicts:

| Exit | Verdict | Meaning |
| --- | --- | --- |
| 0 | `conforming` | All expected cases qualified and matched intent and outcome, with no undeclared rows. |
| 1 | `mismatch` | At least one qualified case differed. This does not hide missing or unqualified counts for the remaining cases. |
| 2 | `inconclusive` | No qualified mismatch, but evidence is missing, unqualified, or contains undeclared rows. A global provenance mismatch or shared artifact ID makes all expected cases unqualified without comparing rows. |
| 3 | `invalid` | Bad argument count, unreadable input, or syntax/schema failure. Emits a fixed role-level reason instead of counts; no partial comparisons. |

`expected` is the sum of `conforming_no_edit`, `conforming_edit` (intent 2), `mismatch`, `missing`, and `unqualified`, plus `conforming_rewrite` (intent 3) **only** in compatible results. These buckets do not overlap. With matched global provenance, `missing` counts absent declared rows, `unqualified` counts rows with wrong input IDs, explicit unqualified status or unsupported intent, and `unexpected` counts undeclared rows separately. `unexpected` is not assessed when the global pairing is unqualified. The existing two-argument strict invocation, strict input shape, and strict output record are unchanged.

## Reproduce with the synthetic native SDK fixtures

From the repository root, with the Rust/audio dependencies already cached, a trusted Python 3 as `python` on `PATH`, and IPv4 loopback available:

```sh
cargo test --offline --locked -j 2 -p rambler-cli --features audio --example native_qualification -- \
  --include-ignored edit_acceptance:: --show-output
```

The opt-in tests write the declarations before starting an owned localhost peer. The peer verifies upload of fixed, non-speech PCM and sends independently hand-built S3 replies. The Rust collector yields the finals and captured context; each observation uses the raw intent and actual proposal outcome, never an ASR substitution. Both seams compare the same observation against a conforming declaration and a valid negative declaration:

- **Strict**: intent 2/member 7 yields exact window/selection `[13,22]` (negative endpoint `23`); renderable intent 3/member 3 yields no proposal. Expected: one edit, one no-edit, or one mismatch and one no-edit.
- **Compatible**: intent 3/member 7 yields an exact window and `[7,18]` over a decomposed accent, flag and joined emoji. Negative `[8,18]` begins inside the accent cluster. Intent 3/member 3 and a literal command with the same ASR words as the rewrite input both yield no proposal. The `AsrOnly` control has hypothesis provenance and rejects proposals. Expected: one rewrite and two no-edits, or one mismatch and two no-edits. The test also revalidates a scalar splice `[8,15]` versus a grapheme splice `[7,18]` and tests a scoped in-memory host application: text outside the first window (including an identical second window) is not changed. A stale revision, changed split, document or anchor conflicts before mutation. The document/anchor checks belong to this test host, not the SDK or Python tool.

A decoded-fixture test separately verifies duplicate finals, multiple hypotheses and unknown safety fail to qualify instead of becoming `no_edit`. The normal native qualification example has separate preview coverage (replacement, not concatenation). None of these fixtures are provider observations.

The test takes no corpus, endpoint, or output path. It uses a fresh local temporary directory for the synthetic JSON and deletes it on normal completion; `--show-output` reveals only verified aggregates. The normal native qualification report v1 is unchanged. This tests the Rust-to-Python contract, not real service behavior or editing quality.

## Privacy and what still needs review

Only the fixed result schema, scope, verdict, and aggregate counts or fixed error reason appear in output. No labels, paths, text, text lengths/digests, per-case results, account/environment details, audio, or exceptions are emitted. Do not track raw inputs or redirect the report into an input file; use a separate destination if saving stdout. Prefer a reviewed, synthetic corpus; for any later authorized use, keep raw inputs in approved local storage, outside the repository, with appropriate access and retention. A text hash is not anonymization. Review even aggregate publication for small-group disclosure; do not offer this as an oracle for repeated, adaptive or single-case probes of private observations.

Before any real corpus claim, review consent and scope, precommit the oracle independently, review the producer and provenance bindings (including exact context/input, candidate build and policy), and verify the observation records the actual proposal and offsets. The evaluator cannot attest any of those facts from matching tokens. Real-voice quality, real editor behavior, matched PixelBoard/Gboard comparison, and advanced intents 4/5/6 remain pending. See the [FR/EN scenario and annotation plan](quality-annotation-plan.md) for separate intent, text and scope scoring. No transport performance or live-provider behavior is qualified here.
