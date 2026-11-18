# Offline edit acceptance contract (v1)

This check answers one question: did a matched candidate produce the **predeclared intent and strict replacement proposal outcome** for every declared editing case? It checks readiness to make that narrow assertion, not transcription quality, editor behavior, or a service qualification. No existing corpus was evaluated in introducing this contract. All examples and tests here are invented.

## Why a new input pair?

[`validate_corpus.py`](../../tools/validate_corpus.py) reports WER, rendering, and validity/conflict checks on an intent-2 proposal. Its success status excludes edit quality. [`sanitize_corpus_report.py`](../../tools/sanitize_corpus_report.py) retains some of these observations but does not add independent expectations or qualify cases for exact comparison. A valid proposal, successful render, low WER, equal report hashes, or a missing proposal key is **not** an edit acceptance result. Neither existing report schema is an input to this checker, including the sanitizer's optional controlled-text output.

## Local input schemas

Two separate, local, untracked UTF-8 JSON regular files are required. Both schemas use closed objects (all listed fields required, no extra or duplicate keys). Each file is at most 1 MiB; the declaration has 1–100 rows and the observation has 0–100. File/row identities are case-sensitive opaque ASCII tokens matching `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`; do not encode names, accounts, paths, or text in them. Each `case_id` is unique within its file; row order is immaterial.

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

- **Identity/provenance.** Match `corpus_id` (frozen corpus revision), `run_id` (one planned collection), and `candidate_id` (the exact build/configuration under test), with fixed `policy: "strict"`. Match both `case_id` and `input_id` per row. The latter identifies the immutable stimulus, language/options/hints and exact captured input window, not a label reused after inputs change. Use distinct `artifact_id`s. Locally bind these identifiers to the actual inputs, executable/configuration and separate artifacts before interpreting the counts; do not guess or backfill a candidate's identities from the declaration. Matching labels and distinct artifact IDs are only structural checks, not authentication or proof of independence.
- **Declaration.** Freeze and review the identities and expected semantics before collection. `intent` is raw `2` (edit history), `3` (rewrite), or `null` (**no** `VoiceEdit`, not an absent/undecodable intent field or numeric zero). Exactly `{"kind":"no_edit"}` means strict proposal rejection or verified no proposal. Only intent `2` may expect `edit`: v1 uses the existing strict proposal policy ([Rust](../../crates/rambler-engine/src/results.rs), [Python](../../src/rambler/results.py)), not the Rust opt-in client-compatible proposal policy. A renderable rewrite string does not qualify as a strict edit. No semantics are defined for 4/5/6 or other excluded codes.
- **Independent observation.** A later, separately produced candidate artifact records what was observed, not what the declaration wanted. Use `status: "observed"` only with a completed, unambiguous final, a verified linkage to the exact run/input and captured context, a known raw intent (or confirmed absence of `VoiceEdit`), and a recorded strict proposal result, including explicit rejection or verified absence if `no_edit`. Do not substitute transcript, rendered/ASR text, compatible fallback, or reconstruct a proposal or its offsets from the declaration. Record the candidate's actual result even when it violates the strict policy. Failed/partial/ambiguous collection, missing evidence or intent, or lack of an explicit proposal check is `unqualified` (omit `intent` and `outcome`), or leave the row missing. Any other observed integer intent (signed 32-bit) is well-formed but unqualified, and its outcome is not compared. `no_edit` is evidence about the strict proposal decision only, not evidence that an editor did or did not mutate.
- **Edit outcome.** `window` is the complete replacement for the captured window, not a diff, selection-only string, transcript, or full document. Exact Unicode scalar equality is required, without normalization or trimming. `selection_utf16` is either `null` (no structured selection returned) or a half-open `[start,end]` in UTF-16 code units local to that replacement. Both offsets must be on scalar boundaries, from zero through the window length inclusive. A collapsed selection has equal endpoints, not `null`. An empty window is an edit, not a rejection. Windows have at most 65,536 UTF-16 code units and may not contain unpaired surrogates.

## Run and interpret

```sh
python -I -S -B tools/check_edit_acceptance.py EXPECTATIONS_JSON OBSERVATIONS_JSON
```

The checker reads only the named input documents, never writes them, and has no producer or integration with the app, service, capture pipeline, or corpus validator. It compares only after **both** documents pass syntax and schema checks. The result is one compact JSON record (`rambpc-edit-acceptance-result-v1`, scope `offline-strict-proposal-only`), with these verdicts:

| Exit | Verdict | Meaning |
| --- | --- | --- |
| 0 | `conforming` | All expected cases qualified and matched intent and outcome, with no undeclared rows. |
| 1 | `mismatch` | At least one qualified case differed. This does not hide missing or unqualified counts for the remaining cases. |
| 2 | `inconclusive` | No qualified mismatch, but evidence is missing, unqualified, or contains undeclared rows. A global provenance mismatch or shared artifact ID makes all expected cases unqualified without comparing rows. |
| 3 | `invalid` | Bad argument count, unreadable input, or syntax/schema failure. Emits a fixed role-level reason instead of counts; no partial comparisons. |

`expected` is the sum of `conforming_no_edit`, `conforming_edit`, `mismatch`, `missing`, and `unqualified`. With matched global provenance, `missing` counts absent declared rows, `unqualified` counts rows with wrong input IDs, explicit unqualified status or unsupported intent, and `unexpected` counts undeclared rows separately. `unexpected` is not assessed when the global pairing is unqualified.

## Privacy and what still needs review

Only the fixed result schema, scope, verdict, and aggregate counts or fixed error reason appear in output. No labels, paths, text, text lengths/digests, per-case results, account/environment details, audio, or exceptions are emitted. Do not track raw inputs or redirect the report into an input file; use a separate destination if saving stdout. Prefer a reviewed, synthetic corpus; for any later authorized use, keep raw inputs in approved local storage, outside the repository, with appropriate access and retention. A text hash is not anonymization. Review even aggregate publication for small-group disclosure; do not offer this as an oracle for repeated, adaptive or single-case probes of private observations.

Before any real corpus claim, review consent and scope, precommit the oracle independently, review the producer and provenance bindings (including exact context/input, candidate build and policy), and verify the observation records the actual proposal and offsets. The evaluator cannot attest any of those facts from matching tokens. No historical outcome, service behavior, conformance on a natural corpus, Android parity, advanced intent 4/5/6 behavior, editor mutation, transport performance, or general editing quality is qualified by this contract or its synthetic tests.
