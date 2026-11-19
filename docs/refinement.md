# Engine refinement and result API

This document distinguishes locally verified fixes from real service capabilities. The 42 historical native sessions and their limits remain documented in [STATUS](STATUS.md) and the [evidence](evidence/README.md); they do not validate the new options or every situation below.

## Robustness fixes

- **Proto2 protobuf merging.** Occurrences of the same singular *message* field must be merged, not replaced wholesale by the last occurrence. For a `oneof`, repeating the same message member merges it; changing member clears the old one. Singular scalars remain “last valid value”, repeated fields remain ordered, and original bytes remain accessible. Targeted tests are in [`test_protocol.py`](../tests/test_protocol.py). An independent check against the official protobuf implementation also compared 100 deterministic cases. This defect was not identified in the 42 historical calls; this fix does not prove that they were affected.
- **Asynchronous transport cleanup.** The transport closes the request iterator it owns, including on cancellation before the writer starts, and awaits asynchronous cleanup; producer, cleanup and transport errors are handled by the tested paths rather than silently lost. Loopback tests are in [`test_transport.py`](../tests/test_transport.py). This defect was not observed in the 42 historical calls.

## Collecting a result without applying an edit

`collect_dictation` consumes the stream completely and returns a `DictationResult` only after S3 completion, successful gRPC completion and the engine EOF event. Late S3/gRPC errors, cancellation and production errors remain errors; a candidate received before EOF does not become a confirmed result. The engine remains owned by the caller. On error or limit exceedance, the stream is closed and cooperative sources are cleaned up.

- `final_updates` retains the **complete S3 responses** whose recognition state is 1, in order, with hypotheses, scores when present, `VoiceEditResult`, raw bytes and unknown fields. Updates are not merged, deduplicated or concatenated into segments according to an invented rule.
- `metrics` contains metrics from the last EOF event; its default is `None` for manual construction. These metrics are not a new benchmark.
- `single_hypothesis` requires exactly one final update and one hypothesis; otherwise `AmbiguousResultError`. `text` also requires this unique case: without voice edit, it returns the hypothesis text; for intention 2, it renders edited field 3 or concatenates the three strings in field 7. For intention 3, only field 3 is rendered, without inferring replacement scope. Other intentions, intention 3/field 7 and field 2 with unknown semantics remain ambiguous. The caller can inspect complete responses if a different policy is needed.
- `max_final_updates` (128 by default) and `max_result_bytes` (8 MiB by default) bound retained final responses; the latter counts raw protobuf bytes, **not** Python object memory overhead. Exceeding a limit raises `ResultLimitError`, with no partial result presented as successful.

Standalone example: supply a continuous mono 16 kHz Ogg/Opus file as an argument and configure authorized application metadata as described in the [authentication notes](research/authentication.md). The example reads no configuration file itself and changes no text.

```python
import asyncio
import sys

from rambler import (AmbiguousResultError, EditingContext, EnvironmentCredentials,
                     ProtocolConfig, RamblerEngine, collect_dictation)

async def chunks(path):
    with open(path, "rb") as source:
        while data := source.read(8192):
            yield data

async def main(path):
    async with RamblerEngine(credentials=EnvironmentCredentials()) as engine:
        result = await collect_dictation(
            engine, chunks(path), config=ProtocolConfig(language="en-US"),
            context=EditingContext(before="The meeting is tomorrow at ten."),
            max_final_updates=128, max_result_bytes=8 * 1024 * 1024,
        )
        print("final updates:", len(result.final_updates))
        print("EOF metrics:", result.metrics)
        try:
            print("unique rendered result:", result.text)
        except AmbiguousResultError:
            print("No unambiguous supported rendering; inspect final_updates.")

asyncio.run(main(sys.argv[1]))
```

The displayed text may represent **the entire history window**, not an addition or verbatim transcript. Do not append it to existing context.

## Proposing a replacement with a precondition

`replacement_proposal(result, expected_revision=...)` accepts only the verified intention 2 case with edited field 3 or 7, a captured context and a single final hypothesis. Other cases raise `UnsupportedEditError` or `AmbiguousResultError`. The proposal describes replacement of the **entire captured window**, without modifying an interface or file. `replacement_text` is the text of that window; `replacement_context` retains the three strings when present. `selection_utf16_range` is a range in UTF-16 units **within the replacement text** for field 7; it is `None` for field 3, which supplies no cursor. Do not treat these numbers as Python indices. Any text markers are not arbitrarily removed.

`proposal.validate_current(current_context, revision=current_revision)` raises `ContextConflictError` if the three strings or expected revision no longer match. `expected_revision` is the revision **associated with the captured snapshot**, not the one reread after the request. It must be an immutable `str` or `int`. To detect an ABA sequence, the editor must advance the revision on every change; text comparison alone cannot do so. Even after validation, the actual application must check and replace atomically on the editor side: this module does not do that.

Here is a standalone delayed-capture pattern; in a real integration, `atomic_snapshot()` must read context and revision in the same operation protected by the editor. The example keeps the revision in a closure and replaces nothing:

```python
import asyncio
import sys

from rambler import (AmbiguousResultError, ContextConflictError, EditingContext,
                     EnvironmentCredentials, ProtocolConfig, RamblerEngine,
                     UnsupportedEditError, collect_dictation, replacement_proposal)

class ExampleEditor:
    def atomic_snapshot(self):
        # Example without concurrency; a real editor provides its own atomicity.
        return EditingContext("The meeting is tomorrow at ten."), 1

async def chunks(path):
    with open(path, "rb") as source:
        while data := source.read(8192):
            yield data

async def main(path):
    editor = ExampleEditor()
    captured_revision = None

    async def context_at_audio_end():
        nonlocal captured_revision
        context, captured_revision = editor.atomic_snapshot()
        return context

    async with RamblerEngine(credentials=EnvironmentCredentials()) as engine:
        result = await collect_dictation(
            engine, chunks(path), config=ProtocolConfig(language="en-US"),
            context=context_at_audio_end,
        )
        try:
            proposal = replacement_proposal(result, expected_revision=captured_revision)
            current, revision = editor.atomic_snapshot()
            proposal.validate_current(current, revision=revision)
            print("Proposed whole window:", proposal.replacement_text)
            print("UTF-16 selection:", proposal.selection_utf16_range)
        except (AmbiguousResultError, UnsupportedEditError, ContextConflictError):
            print("No safe supported proposal for this session and snapshot.")

asyncio.run(main(sys.argv[1]))
```

## Real validation on 2026-11-13

The first [redacted functional corpus](evidence/refinement-capabilities-public.json) deliberately retains its initial assessment of **4 unique renderings out of 5**, despite five successful transports: the rewrite was then rejected by the conservative `result.text` policy. This historical report was not reinterpreted after the code changed.

| Case | Real observation and limit |
|---|---|
| Natural English LibriSpeech, 3.505 s | “Concord returned to its place amidst the tents.”; normalized WER 0/8 words. One clip, not a general evaluation. |
| Meeting notes, 40.56 s PCM | Complete source; Friday, 6 files and 200 dollars corrected; final reference “Silver Meadow” retained. |
| Rewrite in context | Intention 3/field 3, polite text different from the context + command hypothesis. Rendering now supported; no automatic replacement authorized. |
| Composition request | “Write a short message to Alex saying I will arrive 20 minutes late.”: dictated instruction, not composed message. Intention 4 not validated. |
| Selection `ten` with context `🙂` | Three-part return with `eleven`; emoji and context retained, context/revision conflicts detected. UTF-16 coordinates remain locally calculated. |

An [additional simulated-rate session](evidence/refinement-long-realtime-public.json) consumed the **1,298,034 PCM bytes** of the long case, in 20 ms pages. Complete collection took **41.454 s** for 40.564 s of audio, with the same final text as accelerated upload. This total includes encoding, streaming and EOF; its difference from audio duration does not isolate a delay after stopping the microphone. One 40 s session is not an extended endurance test.

After extending rendering, a [new rewrite test](evidence/refinement-rewrite-public.json) succeeded: `Could you please send me the report at your earliest convenience?`. The hypothesis and transcript contain the context and command; they must not be confused with this edited result. An independent binary inspection and the APK callsite `ihm.maybeGetVoiceEditedText` agree on the text choice for this intention. This still does not prove a general replacement scope.

### Provenance and privacy

Initial measurements use checkpoint `4b96696`; the rewrite follow-up then uses the new `results.py`, whose exact hash appears in its report. Per-module hashes take precedence over HEAD alone when the tree is modified. The strict local suite reaches **438 tests passed** (asyncio debug, warnings treated as errors). The [final 0.2.0 wheel](evidence/refinement-wheel-smoke.json) was built with standard isolation then installed in a clean venv without PyAV; public imports, module equality with source and derived fixture rendering pass. This smoke test is offline, not a new Google session. The prior build attempt without isolation failed because `setuptools` was not installed in the venv; the documented standard procedure does not depend on it being in that venv.

The rewrite response also contains unknown extension `74956225` with internal text-processing metadata and an opaque service identifier. The SDK preserves these bytes; public copies do not expose them. The [regression fixture](../tests/fixtures/live_rewrite_response.json) is **derived and redacted**, not an original payload: it states the source response hash, safe retained components and removal of the extension. The three original historical fixtures remain separate.

`tools/sanitize_corpus_report.py` creates a **new** closed-allowlist projection: raw, unknown values and server/session identifiers are replaced with summaries or omitted, without changing the private original. Texts are hashed by default; `--include-text` requires a controlled, inspected corpus. This tool does not automatically make personal dictation publishable; even hashes can correlate or reveal low-entropy text. Remote metadata remains data, not instructions executed by the engine.

```sh
# Real network explicitly enabled; authorized audio and access variables required.
# Complete results first remain in artifacts/, ignored by git.
.venv/Scripts/python -I tools/validate_corpus.py --live --manifest tools/fixtures/refinement-validation.json --output artifacts/my-validation/raw.json --timeout 90 --include-results
.venv/Scripts/python -I tools/sanitize_corpus_report.py --input artifacts/my-validation/raw.json --output local/my-validation-public.json --include-text
# Inspect the copy before any publication. Every output must be new.
```

Additional synthetic preparation is described in [refinement-speech.json](../tools/fixtures/refinement-speech.json) and the [refinement-validation.json](../tools/fixtures/refinement-validation.json) manifest. `tools/make_fixture.ps1 -CasesFile tools/fixtures/refinement-speech.json -NormalizePcm -OutputDirectory artifacts/my-refinement-synthetic` prepares normalized WAVs; then adapt manifest paths if necessary. The natural sample has provenance, hash and **CC BY 4.0** attribution in [natural-corpus-inputs.json](evidence/natural-corpus-inputs.json); audio remains outside git. The French sources tried were unavailable without authentication or beyond the scan limit; no new credential was requested.

## Performance and remaining open capabilities

The [20-session grid](evidence/refinement-page-grid.json), five per 20/40/60/100 ms duration, retains decoded PCM and **all** ordered final updates. 20 ms pages remain the default; 60 ms saves about 26% of Ogg bytes for about 17 ms additional final median in this small benchmark. This is not a new speed gain or proof of latency equivalence. Distributions, outlier and CPU limits are in [performance.md](performance.md).

Intentions 0 through 6 are decodable, but only **2 and 3** were observed in this functional evidence. Hints are now exposed as described below; their encoding does not guarantee their server effect. Remaining work includes representative natural voice evaluation, longer endurance, truly degraded networks, cold start and the scope of other intentions. The additional observations below do not replace historical reports.

## Continuation: natural corpus, composition and endurance

### Ten new natural clips and three progressive tests

The [inputs and licenses](evidence/natural-expansion-inputs.json) cover **7 English LibriSpeech clips and 3 French MLS clips**, each with a distinct speaker and chapter identifier within its language. The 72.825 s of reading is normalized to mono 16 kHz PCM16 without trimming. References come from the datasets, not independently corrected transcripts.

The [ten accelerated sessions](evidence/natural-expansion-public.json) finish with complete source, S3/gRPC completion and unique rendering. The [quality summary](evidence/natural-expansion-quality.json) counts 3/109 words in English and 3/79 in French under punctuation/case/whitespace-normalized WER, **without** equivalence between numbers and words or between compound words. `cannon balls`/`cannonballs`, `narwhale`/`narwhal` and `cinq`/`5` therefore contribute to the score; `eut achevé`/`achevait` is a real tense difference from the reference, without independent audio reannotation. Do not present these ratios as general accuracy, or every difference as audio loss.

[Three simulated-rate sessions](evidence/natural-expansion-realtime-public.json), one English and two French, received the same texts as their accelerated versions with the same source PCM consumed. These are still short read clips, not noise, spontaneous dialogue or a real microphone.

### Composition: research and observations, not simply a forgotten feature

The [dedicated analysis](research/composition.md) distinguishes the `COMPOSE` enum/mock, empty wiring of MUSE alternatives, the Android Lite path and the native S3 service. The five tests without invocation remain dictation for creation requests, with a positive rewrite for the known control. Later invocation tests recognize “Hey Gboard” phonetically but retain it literally with and without bias; complete “Okay” with bias also remains literal. They do not validate COMPOSE 4. No universal impossibility or flag-based activation is inferred.

### Startup hints: SDK 0.3

`ProtocolConfig` accepts optional **tuples** `bias_terms`, `preferences` and `secondary_languages`, empty by default. They are serialized in initialization: `InputBoxInfo` extension 1203, repeated fields 3 and 5, then `UserInfo` extension 293000, repeated field 4 for languages. Editing context remains a separate request **after audio**, before completion. Without options, goldens remain identical. Example:

```python
from rambler import ProtocolConfig
config = ProtocolConfig(language='en-US', bias_terms=('Zunarel', 'Valmera'),
                        preferences=('Use British spelling.',),
                        secondary_languages=('fr-FR',))
```

The SDK imposes at most 1,024 strings per bias/preferences category, a total UTF-8 budget of 1 MiB for those two categories, and at most 16 secondary languages with a total budget of 1,024 UTF-8 bytes; secondary tags are nonempty. Other message limits still apply. These are SDK bounds, not demonstrated server quotas or limits. Contacts and instructions can be sensitive: send them only with authorization.

The [bounded 10/10 experiment](evidence/input-hints-public.json) retains the manifest hash and summaries: with bias, `Zunarell` becomes `Zunarel`, but `and` becomes a comma; one case does not demonstrate an overall gain. British spelling preference has no demonstrated effect; both FR/EN configurations yield the same correct mixed text, without isolated benefit from secondary languages. Observed acceptance of these requests does not demonstrate universal use of hints.

Initial placement is supported by `ihv` lines 153–211, `knv-simple` lines 259–309 and `zxh` lines 243–255 in the static outputs identified by [protocol.md](research/protocol.md). Late context remains independent. Do not activate flags, quotas or authentication paths by assumption.

### Progressive endurance: first real stage

The [118.682 s stage](evidence/endurance-120-public.json) consumed its **3,797,836 PCM bytes**, received a single final and retained “Amber Orchard”, “Cobalt Lantern” and “Violet Harbor” in that final, without concatenation or deduplication. Collection/pipeline: 119.880 s, process CPU 5.891 s; this CPU includes encoding, hashing, instrumentation and engine, not just the remote service.

Sampled current RSS: 58,511,360 bytes at the start, 60,723,200 at the end, sampled maximum 60,907,520. The maximum gap between samples is 1.015 s, below the 3 s coverage ceiling. This is neither the exact peak nor the historical high-water mark, nor proof of absence of leaks. The benchmark distinguishes transport success, source integrity, memory coverage and semantic divergences.

`tools/validate_endurance.py` limits its campaign to three sequential sessions, 300 s audio and 360 s timeout per session; **these are benchmark guardrails, not established server or SDK limits**. The 300 stage requires a complete compatible 120 report, not just a `healthy` boolean. The local report is nevertheless not a tamper-proof attestation. A sampling gap makes the profile inconclusive without turning a successful transport into a failure.

The [300 stage](evidence/endurance-300-public.json) comprises three healthy sequential sessions of **288.3515625 s** each, same channel and simulated 20 ms cadence; each **9,227,250-byte** PCM source is complete, markers are present and ordered final sets are identical. The three pipeline durations are approximately 291.238 / 290.908 / 290.747 s, with process CPU 17.094 / 14.469 / 14.672 s. Sampled RSS maxima are 60,796,928 / 61,120,512 / 61,661,184 bytes; these are neither exact peaks nor proof of absence of leaks. These measurements use the SDK 0.2 baseline **before integration of hints**, with module hashes in the reports; they do not measure the new 0.3 code.

The benchmark's structured reports contain no raw bytes, unknown values or service identifiers. `--include-text` still requires review of a controlled corpus; personal dictation is not automatically publishable. Private continuation captures remain in `artifacts/continuation-private/`, outside git.

Optional reproduction with authorized sources already prepared, authorized environmental access variables and the existing `local/` directory; this contacts the real service and consumes audio time, without automatic retry:

```sh
.venv/Scripts/python -I tools/validate_corpus.py --live --manifest tools/fixtures/input-hints-validation.json --output artifacts/my-hints/raw.json --timeout 90 --include-results
.venv/Scripts/python -I tools/validate_endurance.py --live --manifest tools/fixtures/endurance-validation.json --id endurance-two-minute-en --stage 120 --timeout 180 --output local/endurance-120.json
.venv/Scripts/python -I tools/validate_endurance.py --live --manifest tools/fixtures/endurance-validation.json --id endurance-five-minute-en --stage 300 --previous-report local/endurance-120.json --sessions 3 --timeout 360 --output local/endurance-300.json
```

Output files must be new; audio preparation is local and must follow the manifests. `validate_corpus --include-results` reports contain raw data and require a redacted projection and review before publication; do not confuse them with structured endurance reports.
