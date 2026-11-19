# Rambler composition: evidence and limitations as of 2026-11-13

## Bounded conclusion

**Composition is not demonstrated on the tested native S3 profile.** Five real coordinator sessions completed successfully: a direct question and a message request remain literally dictated, while the rewrite control does produce a polite sentence. None of these results contains `INTENT_COMPOSE=4`. Three additional trials with an invocation prefix also remain literal, but the name «Gboard» is misrecognized («Hey board», «Hagbard»): they **do not cleanly disprove** a possible phonetic trigger. The SDK0.3.0 follow-up adds four paired observations: «Gboard» is now recognized in all four outputs, and a complete «Hey Gboard» or «Okay, Gboard» prefix in three of them, but the results remain literal, intent2 / oneof3. This follow-up therefore **partially** resolves the earlier confound, without establishing composition or intent4. This proves neither the universal absence of composition in Rambler nor its impossibility in another profile or service version.

Three things must remain distinct:

1. The `INTENT_COMPOSE=4` enum and a **mocked** PixelBoard test exist.
2. This APK's backend selector receives an **empty map** for MUSE/MUSE_LITE alternatives: changing their enum is not enough to activate them.
3. A real **Jetson Lite on Android** path exists separately as a fallback, with ASR connected to Speech Services and generation connected to AICore. Its prompts provide for certain generations, but this validates neither their actual execution on a device nor an S3 wire response with intent4. This path is not a public replacement API for the native engine.

The analyst for this pass made no SDK modification, backend change, or network access. The network sessions below are the coordinator's, reviewed in their redacted report.

## Provenance and reference conventions

APK authority: PixelBoard1.3.4, SHA-256 `a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`. PixelBoard source consulted: tag extracted under `Akshayykadam-PixelBoard-9a81f8b`. No APK or PixelBoard source code was executed: textual reading with isolated Python, without importing the examined files.

The following source aliases designate the local research artifacts already identified in [protocol.md](protocol.md):

- `APK`: `scratchpad/pixelboard-jadx/sources`, especially `defpackage`.
- `IHM`: `scratchpad/pixelboard-jadx-ihm/sources/defpackage/ihm.java`.
- `FFH-SIMPLE`: `scratchpad/pixelboard-jadx-ffh-simple/sources/defpackage/ffh.java`.
- `TAG`: `scratchpad/pixelboard-source/Akshayykadam-PixelBoard-9a81f8b`.

The scratchpad for this analysis is `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad`. Numbers below are those of the JADX outputs or cited sources, not lines of original Google code. Missing conditions in incomplete decompilations are not considered proven.

## Enum, S3 output, and PixelBoard filter

- `APK/defpackage/aamp.java:6-12,21-38` establishes, among others, `2=INTENT_EDIT_HISTORY`, `3=INTENT_REWRITE`, `4=INTENT_COMPOSE`.
- `IHM:192-219` accepts only intents 2/3 in the recovered `maybeGetVoiceEditedText` helper. It selects edited text field3 or context field7; other intents do not produce this candidate at the callsite. `IHM:224` then falls back to ASR text, and `IHM:255-285` constructs a `VoiceAction` only if this candidate is present. An enum4 therefore does not demonstrate its exposure by this S3 processor.
- `APK/defpackage/ffh.java:670-695` calls the PixelBoard filter **before** checking the final span and presence of a `VoiceAction`.
- `TAG/pixelboard-patches/extensions/extension/src/main/java/com/akshaykadam/pixelboard/extension/rambler/GboardRamblerDictationRuntime.java:174-183` replaces produced text with the transcript for a local action4 with a nonempty transcript, resets intent to 0, and clears the diff. This is a **local post-result** transformation, not a composition trigger sent to the server. The main body of this method is missing from the general APK decompilation; details of this branch come from the tagged source, and its actual call is visible in APK `ffh`.
- The adjacent tagged test `src/test/java/com/akshaykadam/pixelboard/extension/rambler/GboardRamblerDictationRuntimeTest.java:74-88` explicitly injects a **fake action4**, the question « What is the capital of France? » and an answer about Paris. It tests the client filter: this is **not** a backend capture.

The IME status `ABORT_COMPOSING` encountered in the dictation lifecycle must not be confused with the semantic intent `INTENT_COMPOSE`.

## Backends: wiring actually present

### MUSE/MUSE_LITE selector: alternatives not registered here

`APK/defpackage/mup.java:6-10` defines UNDEFINED0, S3=1, MUSE=2, MUSE_LITE=3. `APK/defpackage/muq.java:31` gives 1 as the declared backend default. `APK/defpackage/ffi.java:21-30` first checks `pref_key_ad_backend_override`, otherwise this flag, then looks up the provider in the injected map. A missing entry falls back to the default S3 provider.

The decisive point is injection, not the enum names:

- `APK/defpackage/erz.java:1895` registers `new ffi(this.cM, wmr.b)`.
- `APK/defpackage/erz.java:1088` constructs `cM` with `ffk`.
- `APK/defpackage/ffk.java:16-17` constructs the `ffh` extension.
- `APK/defpackage/wmr.java:11` defines `b` as a map with zero entries.

The search for MUSE references and construction of `ffi` covered the 19704 Java files available in `APK/defpackage`, excluding `iha.java`; it found only one construction of `ffi`. The two main patch source roots examined (107 Java/Kotlin/XML files) contain no references to backend selection prefs or the backend flag.

**Consequence for this wiring**: a preference 2/3 does not magically activate a MUSE/MUSE_LITE client; the entry is missing and fallback remains `ffh`. No alternative endpoint, authentication scheme, or remote composition client is thereby established. This does not mean all code named Muse is unused: modules and a real separate Lite path exist below.

### Jetson Lite: real Android fallback, no demonstrated portable endpoint

- `APK/defpackage/kkj.java:23-46` selects S3 when available and can select Jetson Lite on offline fallback or unavailability.
- `APK/defpackage/fgu.java:13-23` installs the `fgo` factory when the module is created; `fgo.java:14-23` requires a Jetson session and `muq.K == a.ao(3)`. `a.java:771-772` resolves this value to 1, while declared default `jetson_no_network_fallback_strategy` is 0 (`muq.java:45`). Effective values, module loading, and models available on a device are not established here; none of these gates was modified.
- ASR: `erz.java:1534-1538,1823-1825` supplies the `uxz` client via `sgq.java:91-97` and `ohq.java:69-80`. The channel is constructed to the **Android Binder** component `com.google.android.tts` / `com.google.android.libraries.speech.transcription.recognition.grpc.GoogleAsrService`. The package comes from `GboardSpeechServicesDetector.PACKAGE_NAME`. `uxz.java:15-45` describes `RecognitionSession` and `DownloadModel` on this service. This is not discovery of a remote HTTPS host.
- Channel construction includes an `abfi` policy, a `PackageManager`, and an allowlist containing the Speech Services package (`sgq.java:94-97`). Android binding/identity checks and installation of the real service remain dependencies; no equivalent standalone cloud header is established or proposed.
- Generation: `fgn.java:187-202` creates an AICore client and requests a feature; `fgn.java:82-87` calls the `eog` LLM client. `enm.java:38-40` targets Android services `com.google.android.aicore` / `AiCoreMultiUserService` or `AiCoreService`, via Binder. Feature availability, effective permissions, model, and actual results are not verified here.

`APK/defpackage/igs.java:6` declares exactly the expressions «Okay Gboard» and «Hey Gboard». `APK/defpackage/fgn.java:63` contains an exact comparison of an old prompt and its replacement with another prompt, conditional on equality. The old text provides for an explicit mode with «Hey Gboard» and requested generation; the replacement also provides for executing requests such as « Write me a sick leave email », rather than dictating them literally. These are **embedded local instructions** showing feature intent, not evidence of successful inference, not a predicate of the S3 backend, and not evidence of emitting wire intent4. If the effective prompt differs from the literal compared, this replacement branch does not necessarily apply.

## Preferences: no compose switch demonstrated

`muq.java:58-60` declares app-context (`X`, default false), custom-instruction (`Y`, default false), and rewrite/compose rules (`Z`). The presence of the latter name does not prove it is emitted. No `muq.Z` reference was found in the 19704 examined `defpackage` files.

`FFH-SIMPLE:2010-2077` adds app-context only with gate X and a suitable `EditorInfo`, and a custom instruction only with gate Y and a nonempty preference. The latter's wrapper explicitly limits preferences to dictation style and excludes answers to questions and non-dictation tasks. It must not be reinterpreted as a method to enable composition. Hint transport/timing details are a separate analysis.

## Five real observations of the S3 profile

Coordinator's redacted evidence: [compose-matrix-public.json](../evidence/compose-matrix-public.json), code observed at commit `a5fa05c2a0492e6f6feb186c9175c3253c918624`. Summary: 5 complete sessions, 5 successful unique transports/renderings. Contents are controlled sentences; no opaque payload is reproduced here.

| Case | Context / variable | Observed result | Intent / oneof |
|---|---|---|---|
| `compose-question-en` | Direct question, empty context | The capital question remains a question; no generated answer |2 /3|
| `compose-message-empty-en` | Request for a message to Alex, empty context | Instruction dictated, with «20» normalized |2 /3|
| `compose-message-context-en` | **Same audio file** as previous, preceded by «Message to Alex:» | Scaffold + dictated instruction; no composed message |2 /3|
| `compose-question-literal-control-en` | Question presented as text to include | Quoted question retained as text |2 /3|
| `compose-rewrite-control-en` | Request for politeness, context «Send me the report now.» | «Could you please send me the report at your earliest convenience?» |3 /3|

The two message cases share the same audio SHA-256 in the evidence: the variation is in context, not two separate syntheses. The rewrite control shows that at least one context-dependent instruction works in this setup. It proves neither composition nor history replacement scope for intent3.

Reproducible plans: [compose-validation.json](../../tools/fixtures/compose-validation.json) and [compose-speech.json](../../tools/fixtures/compose-speech.json). They bound observations to five calls, without automatic retries or added flags/auth, without backend change, and without another generative API.

## Follow-up: explicit invocation and ASR confusion

Redacted evidence: [compose-invocation-public.json](../evidence/compose-invocation-public.json). The coordinator's three additional sessions complete successfully, without changing flags, prompt, or backend; results remain intent2 / oneof3:

| Case | Relevant returned text | Bounded consequence |
|---|---|---|
| `compose-invocation-question-en` | «Hey board, what is the capital of France?» | No generated answer, but invocation name is not faithfully recognized |
| `compose-invocation-message-en` | «Hey board, write a short message…» | Request still literal; same name confusion |
| `compose-invocation-literal-en` | Quotation containing «Hagbard, what is the capital of France?» | Control remains quoted text, with another phonetic confusion |

**These negatives are confounded by recognition of the name.** They cannot disprove a possible server trigger dependent on the phonetic signal or correct recognition of «Gboard». Conversely, the Lite client does not prove that such a trigger exists on S3. The hypothesis of different articulation and «Okay Gboard» was subsequently tested in the follow-up below, without activating a discovered compose flag. No local Lite instruction was transplanted into S3 requests.

## SDK0.3.0 follow-up: name recognized with and without initial bias

Redacted evidence: [input-hints-public.json](../evidence/input-hints-public.json), reported SDK version **0.3.0**. The complete batch has 10 successful sessions out of 10; the **four invocation cases** below are a subset, not ten new composition trials. Intents and active members were checked directly in the public report's `final_updates`: **2 /3 in all four cases**, with one candidate per case.

The synthesis texts explicitly use phonetic spellings «Hey, G board» and «Okay, G board». Each baseline/bias pair shares exactly the same WAV SHA-256, en-US language, and empty late context; only initial bias `bias_terms=["Gboard"]` varies. Plans and controlled texts: [input-hints-validation.json](../../tools/fixtures/input-hints-validation.json) and [input-hints-speech.json](../../tools/fixtures/input-hints-speech.json).

| Case | Initial bias | Returned text | Intent / oneof |
|---|---|---|---|
| `invocation-hey-baseline-en` | None | «Hey Gboard, write a short message to Alex saying I will arrive 20 minutes late.» |2 /3|
| `invocation-hey-biased-en` | `Gboard` | «Hey Gboard, write a short message to Alex saying I will arrive 20 minutes late.» |2 /3|
| `invocation-okay-baseline-en` | None | «Gboard, what is the capital of France?» |2 /3|
| `invocation-okay-biased-en` | `Gboard` | «Okay, Gboard, what is the capital of France?» |2 /3|

The name **Gboard is actually recognized**: the «board/Hagbard» confusion in earlier trials therefore no longer alone explains these new results. Both Hey cases and the biased Okay case even retain the full prefix in returned text; despite this, no answer about Paris or composed message is produced. The unbiased Okay case still omits «Okay»: prefix fidelity is not complete in every output.

**Bounded conclusion**: the earlier ASR confound is partially resolved, but composition remains unobserved for these sentences and this S3 profile. Textual presence of the prefix does not prove the server classified it as an invocation or that it would suffice to trigger generation. One observation per arm also cannot reliably attribute retention of «Okay» to bias or generalize a recognition gain. No `INTENT_COMPOSE=4` is observed; no general impossibility of composition is established. No Lite prompt, compose activation flag, authentication change, or alternative backend was used in this follow-up.

## What remains open

- The exact server predicate triggering composition, if it exists for another S3 profile, is not recovered from the client.
- Content generation could be observed under an intent other than 4: content behavior and numeric value must be reported separately.
- Android Lite gates and prompts are real sources, but not validation of availability on a device or a portable native contract.
- These enums, mocks, and small test matrices cannot establish whether composition is available in other Rambler profiles.
- Trials change nothing about authentication, quotas, attestation, consent, or editing policy. No circumvention is proposed.
