# Rambler / Gboard parity: verifiable matrix

Initial analysis status: 2026-11-14; application-context and account-state signal boundaries clarified on 2026-11-16. Initial security milestone: `e427fb1`, on `4fdf29e`; the explicit rendering and container APIs described here belong to the following batch. This is a **subset of the Rambler S3 client**, not a clone of the entire Gboard application. A decoded enum is not a validated remote feature, and a remote result is not necessarily the text applied by Android.

## Sources and levels of evidence

- Statically analyzed APK: SHA-256 `a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`, rehashed during this review. The obfuscated names below concern **this build**.
- Local JADX extracts: `pixelboard-jadx/sources/defpackage/`, and targeted extract `pixelboard-jadx-ihm/sources/defpackage/ihm.java`. They are not executed. Metadata retrieval is described in [protocol.md](protocol.md) and uses `tools/research/jadx_metadata.py` from a trusted location.
- Analyzed PixelBoard sources: revision `9a81f8b`, file `GboardRamblerDictationRuntime.java`, lines 174–224 and 232–264. The local filter operates on an already-created VoiceAction; its mock does not prove an S3 response.
- **Real Rust**: [first Ogg slice](../evidence/rust-first-slice-public.json), [PCM, rewriting, selection](../evidence/rust-pcm-functional-public.json), [three sessions on one engine](../evidence/rust-native-session-reuse-public.json), [November 18 Rust and PixelBoard observations](../evidence/pixelboard-vs-native-real-20261118.json).
- **Real Python only**: corpus/hints and channel reuse documented in [the observations](../evidence/README.md). Not retrospectively attributed to Rust.
- **Static/offline**: schema, client branch, synthetic test. Not server feature activation or a matched comparison with a device.

## Current acceptance and remaining work

- **Established, without claiming overall parity**: historical native Rust sessions on the Rambler service, without an Android relay; established client contracts and conservative safeguards documented below. The [advanced audit](rust-advanced-mode-contract-audit.md) finds no advanced fidelity defect already confirmed and correctable offline.
- **Access required**: obtain from the provider or an authorized channel a non-secret contract or supporting evidence covering the exact service, the native client and its right of use, and legitimate issuance, renewal and revocation. The [bounded public research](native-access-public-research.md) did not find this route; historical successes do not prove rights or durable access ([authentication](authentication.md)). Do not substitute another service.
- **Advanced modes**: an authorized, version-matched Android reference must connect the emitted request, raw response, late context, VoiceAction before/after filtering and editor result; details and discriminators in the [audit](rust-advanced-mode-contract-audit.md#precise-discriminators-needed-before-another-client-change). Implement only observed contracts, without weakening safety refusals or inventing activation or semantics for intentions 4/5/6.
- **Quality and latency**: measure representative natural speech inputs and client/session latency, with comparable protocol and reference; offline synthetic profiles prove neither natural quality nor WAN performance ([status](../STATUS.md)). The observations below are not a benchmark.

## November 18, 2026 observations on the real service

The [November 18 record](../evidence/pixelboard-vs-native-real-20261118.json) records three native CLI sessions with synthetic speech:

- **Dictation**: `The kite is blue. Marker stays.` exactly, in one utterance (intention 2, field 3).
- **Targeted replacement**: `The kite is green. Marker stays.` exactly, with the control sentence preserved (intention 2, field 7).
- **Polite rewrite**: with `Send me the report now.` selected and the request `Make this more polite.`, the service returned `Could you please send me the report at your earliest convenience?` in `edited_context` (intention 3, field 7). `Strict` rendering rejected it as ambiguous in this test. To render text from this type of response, the CLI offers the explicit `--result-policy s3-client-compatible` option ([guide](sdk-integration-guide.md#opt-in-to-selection-rewrites)); the record does not report a second real call with this option.

On a Xiaomi 11 Lite 5G NE (Android 14, non-Pixel), **PixelBoard 1.3.5** produced the exact dictation when spoken in two parts with a pause; in one utterance, attempts added only `The kite is blue.`. The targeted replacement was exact. In rewrite tests, the spoken instruction literally replaced the selection instead of rephrasing it. This was not official Gboard; voices, network and measurement points were not matched.

## Intentions and result policies

Enum: `aamp.java:5`; VoiceEditingResult: `aamq.java:20`. The S3 helper `ihm.java:192–212` accepts intentions 2/3 with oneof 3/7; the others fall back to its ASR path. `Strict` rendering deliberately remains more cautious. `AsrOnly` and `S3ClientCompatible` expose an explicit text choice with provenance, without inventing intention functionality or Android candidate selection.

| Value / feature | Rust API and policy | Evidence and limit |
|---|---|---|
|0 DICTATION|Intention preserved; `Strict` rejects VoiceEdit 0, opt-in modes project ASR|Static enum. No wire value 0 demonstrated in published evidence. Conservative facade difference from Android fallback.|
|1 EDIT|Decodable; `Strict` rejects, opt-in modes project ASR without interpreting EDIT|Static name only; does not automatically mean deletion, sending or delta.|
|2 EDIT_HISTORY|`DictationResult::text`, `replacement_proposal`, snapshot/revision; fields 3 and 7|**Real Rust**: EN/FR dictations, correction, `ten→eleven`, selection with emoji in context. Complete window, not blind append.|
|3 REWRITE|`Strict` unchanged: field 3 rendered, field 7 ambiguous. Opt-in `S3ClientCompatible`: fields 3/7 rendered; for a selection rewrite, use `replacement_proposal_with_policy` (window proposal for field 7), validating revision and anchor.|**Real Rust**: polite rewrite in field 3, then field 7 with selection on [November 18](#november-18-2026-observations-on-the-real-service). The Android helper also statically accepts this case; the observation does not qualify application on Android.|
|4 COMPOSE|Decodable; no COMPOSE implementation. Strict/proposal reject; opt-in ASR fallback is not composition|**Not demonstrated**. Eight historical tests remain distinct from local mocks and prompts; see [composition](composition.md). No S3 switch verified.|
|5 RAMBLE|Decodable; no RAMBLE semantics, opt-in ASR fallback only|Enum certain, operational semantics not established by its name; not observed.|
|6 RAMBLE_HISTORY|Decodable; no RAMBLE_HISTORY semantics, opt-in ASR fallback only|Same limit; no evidence of summarizing or cleaning up notes.|
|VoiceEdit field 2|`alternative_text`, `active_field`; Strict ambiguous, compatible falls back to ASR|Same oneof as 3/7. Neither an ASR synonym nor a proven edited fallback.|
|Multiple finals/hypotheses|Preserved in order; no arbitrary single text|Structured API, bounded collection; no invented concatenation/deduplication.|

The PixelBoard filter can remove diffs and convert a local action 4 with transcript to 0; it preserves 5 in a branch where others become 0. This **does not prove** that the S3 helper creates an action 4/5/6.

## Safety marker: discrepancy fixed in this batch

Precise static chain:

1. `aalg.java:37`, RawMessageInfo: **hypothesis.field30 enum**, presence 21, Java member `int e` (`aalg.java:11`). Not a wire boolean.
2. `ywl.java:12,36–41`, validator `r`, case 17 to `a.aY`; `a.java:490–495,677–679`: only values 0/1 are recognized.
3. `ihm.java:192–212`: for the accepted VoiceEdit candidate, value 1 becomes `iid.e=true`, then `ihm.java:287–291` copies it into `aakw.k`. `aakw.java:33` locates this **local boolean** at VoiceText field 10; it is not a second S3 safety field to send.
4. `fga.java:11–15` and `ffh.java:703–708`: `SAFETY_BLOCKAGE` and return **before** command classification (`:767`), IME send (`:780`) and editing (`:804`).

Before this batch, Rust preserved field 30 as unknown, but `text()` and `replacement_proposal()` ignored it. A final intention 2/field 3 with value 1 could therefore be rendered/proposed contrary to this Android branch.

The port now decodes `Hypothesis.safety_status` with presence 0/1. Other values and wrong wire types remain in `unknown_fields`; an unknown occurrence does not erase the last recognized value (proto2). Rendering/proposal helpers reject every blocked candidate and every uninterpreted field 30 in retained responses, including containers 3/5/7 and fallbacks. This path does not authorize rendering secondary candidates. The CLI returns an error without text/context/finals. Structured responses remain explicitly available to integrators.

**Deliberate hardening, not a claim of Android parity**: rejection outside VoiceEdit and on unknown values; u64 numeric values other than 0/1 are kept opaque, without int32 truncation that could make them appear as 0. A long varint actually representing 0/1 remains recognized. No safety blockage case was observed on the service; synthetic/offline validation only.

## S3 parameters added after initial consolidation

The `knv` mutator is indeed injected on the Rambler NewS3 path, not just a generic ASR. `ProtocolConfig` now exposes RecognizerInfo fields 7 (INT32 0/2, offensive word preference), 40 (BOOL auto-punctuation), 64 (STRING nonempty secondary routing). They are opt-in and absent by default; their server effects are not validated. The CLI exposes them too. No safety change or COMPOSE activation is inferred. [Call chain and schema](rust-recognizer-options.md).

Legacy projection of a selection alone is already encodable via `EditingContext { before: selection, ..Default::default() }`; it is not a missing wire capability. Do not treat this projected context as the complete original snapshot when replacing the window: scope and revision must remain under integrator control.

## Other capabilities and boundaries

| Capability | Static evidence / Rust API | Qualification |
|---|---|---|
|Session, audio, normal stop|`ihv.a`, `iho.java:40`; `Engine`, `AudioSource`, init/audio/context/END then half-close|**Real Rust** Ogg and native PCM. Completion requires S3 END + complete upload + cleanup + gRPC OK; not an early final.|
|Cancellation / timeouts|`EngineSession::{request_cancel,close}`, per-stream h2 transport|Loopback/backpressure/concurrency and real Windows Ctrl-C; no automatic replay.|
|Late context|`iho.a/b/g`, `iho.java:96`; `ContextInput`, `ContextProvider`|Real Rust editing; bounded callback/cleanup order tested offline. No on-device comparison.|
|UTF-16 selection|`ffq.java:64–74,84–105`; `selection_utf16_range`, `validate_current`|Selection of `ten` after emoji already observed in real Rust; [one additional native case](../evidence/rust-native-non-bmp-selection-public.json) selects U+1F642 and returns U+1F60E, both non-BMP (2 UTF-16 units). Structured interoperability observed, semantics unconfirmed; ranges derived offline, no validation of proposal APIs or matched Android parity.|
|Partials|`ihm.java:310–355`, score<0.9 test; `PartialTranscript`, borrowed parts and unfinished flag|Offline decoding/projection. [One dedicated native test](rust-native-partials.md): 20 responses, 0 field4, healthy final; real partial rendering and pre-state1 progress remain inconclusive. `first_text`/first response are insufficient.|
|Segments vs overall completion|Independent `aaln.field3/4/5/7`; `RecognitionCompletion`, events, `collect_segments`|Strict state1 collector; segments collector retains 3/5/7 and state1 markers without universal finality. No implicit assembly.|
|Other oneof results|`aapd.java:34`: 1/2, 5–13, 15, 16, 18–21; independent fields 3/4|`active_field`, `active_raw`, unknowns preserved; **no semantic support** for opaque variants.|
|Future extensions|Unknowns, aalo fallback, containers 5/7 and extension 74956225|Raw/unknowns and limits preserved. No guarantee of semantics of a future unknown oneof.|
|Biases, preferences, secondary languages|InputBox1203 fields 3/5, UserInfo4; `ProtocolConfig`|Rust encoding tested; ten **Python** transports accepted, limited effects. No general gain or COMPOSE activation established.|
|Android application context|`ffh.u` → `user_preferences` → `kmy.o`/`iic.g` → InputBox1203 field 5; `ProtocolConfig::preferences` already encodes it|[Static trace to the serializer](rust-application-context-boundary.md), no missing wire field established. OS collection/gates/formatting not ported; runtime activation and server effect not observed.|
|SEND/CLEAR commands|`fgc.java:13–30` regex; `ffh.java:767–781` accepts only SEND with IME_ACTION_SEND|`classify_command(S3DefaultPatternsV1, ime_action)` without side effects. SEND requires IME 4; CLEAR/CLEAR ALL recognized but rejected by the branch. Arbitrary remote regexes not interpreted.|
|Diffs, spaces, cursor, application|`ffh.E`, AgenticDictationTextEditor|Native proposal without side effects; no copy of Android heuristics/fallbacks. Snapshot/revision check more cautious than forced concurrent replacement.|
|MUSE/MUSE_LITE|`ffi.java:21–30`, injection `erz:1895`, `wmr.b` empty map|No other missing remote backend established; changing enum may return to the S3 provider.|
|Jetson Lite / local generation|`kkj.a`, `ohq.java:69` Binder Speech Services, `enm.java:38` Binder AICore|Distinct Android path, required services/models/gates. Not ported; embedded prompts ≠ S3 activation. No other ASR/Gemini API substituted.|
|Authentication / access lifecycle|Injected application metadata, conditional Zwieback branch|Real profile without account/Zwieback. No automatic provisioning, renewal/revocation or durable access guarantee.|
|Account-state signals: ClientInfo22 / RecognizerInfo91.2 / UserInfo20|**Deliberately not ported**: no legitimate account-state provider, no true/false assertion API|[Schema and static chain](protocol.md): supervision linked to MinorsChecker; Android emits true if UnsupervisedAccount is not ready. Native omission is neither false nor reproduction of this Android default; server semantics unknown. No new remote test or safety policy change.|
|Microphone, GUI, OS insertion|Not implemented|Outside initial engine scope; not presented as missing cloud features.|

Non-BMP observation from 2026-11-15: one synthetic native call, without replay, CLI 0, 4581 bytes until EOF and successful cleanup; intention 2/field 7, boundaries `Answer: ` and `.` and `cursor_aware` flag preserved. Input and replacement ranges `[8,10)` are **derived offline**, not returned on the wire or by a proposal API. The CLI exercised neither proposal, revision validation nor editor application. The same audio had yielded `eleven` in the old `ten` context; here the replacement is U+1F60E, neither `eleven` nor `11`. This unexpected difference remains published: `structured_unicode_interop_observed_semantics_unconfirmed`, `asr_matches_prior_known_word_variant=false`, `prior_result_word_reproduced=false`. This is a contextual observation, not a controlled ASR quality comparison or proof of a client bug. Qualified source reference: `1561f878a0b8714af669771fc7bd091784b1f3d9`, unchanged binary (SHA-256 in evidence); no qualification of a future documentation commit follows.

## Useful next steps and actual blockers

- Safety marker: fix and offline tests in this batch, no sensitive audio sent to artificially trigger a blockage.
- Presentation/partials/candidate collection integrated; synthetic tests, never authorization to apply or proof of completion. [Containers and client branches](rust-response-containers.md).
- The exact **service → S3 helper → PixelBoard filter → IME** comparison would require a matched Android reference with effective backend and inert test editor. At most three short discriminating cases (repetition near similar text, known rewrite, SEND) would be preferable to new blind COMPOSE tests. [Android reference preparation](android-reference-validation.md) is now executed on an authorized device (inert editor/SDK diagnostics). Three [subsequent supervised UI observations](../evidence/android-supervised-public-voice-20261116.json) are neither three semantic successes nor a comparison with matched same PCM or wire; route and backend remain unproven. Initial preflight remains inconclusive; a second preflight and minimal construction/read control reproduce an ART SIGSEGV before arming, with successful restoration. The read hook alone holds for 20s, but no compatible controlled construction is qualified. `connectOnly` completes without audio. Separate native calls do not compare the Android/IME chain.

Missing traces from the historical long incident cannot be recreated by a series of successes. Its cause remains undetermined; new audio counters improve future diagnosis without claiming to resolve that old incident.

Update 2026-11-18: the [static request trace](advanced-mode-request-trace.md) shows no client-side mode selector; intention is decided by the server. The only missing field, ClientInfo 16 (`--trigger-application-id`), is now supported but [did not trigger composition](../evidence/rust-trigger-application-id-real-20261118.json).
