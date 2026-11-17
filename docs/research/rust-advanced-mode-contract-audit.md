# Advanced-mode client fidelity gap analysis

Source reviewed read-only at `ee336cf309bb8cd8e654a435e0a85ce24632f8fb` (2026-11-17).
Scope: sanitized versioned Rust and research documents only. No caller was invoked; no test, build, network, device, provider, credential, or private-capture access occurred.

## Verdict

**No already-confirmed, offline-correctable client fidelity defect survives in the scoped COMPOSE / RAMBLE / RAMBLE_HISTORY, structured-command, or editing-context paths.** The wire fields and client branches established by the supplied evidence are represented. Lack of operational semantics or activation for intents 4/5/6 is not an implementable protobuf or presentation fix. This is not a claim of full Android/PixelBoard parity or of current server access.

## Confirmed request/context contracts already implemented

1. **Application context and custom instructions have a wire path.** The evidence terminates at InputBoxInfo extension 1203 field 5, repeated string (`docs/research/rust-application-context-boundary.md:22-41`). `ProtocolConfig::preferences` is public (`crates/rambler-protocol/src/request.rs:27-30`), and `initial_request` emits every entry as field 5 inside extension 1203 (`request.rs:172-180`). Missing Android `EditorInfo` collection/gates/formatting is platform integration, not a missing S3 field (`rust-application-context-boundary.md:11-14`). Adding an `application_context` wire member or guessed compose switch would duplicate or invent evidence.

2. **Initial hints do not displace late editing context.** Bias terms and preferences are init-only (`request.rs:129-131,172-180`); `context_request` emits only extension 1203 after audio (`request.rs:237-253`). This matches the documented separation (`docs/research/protocol.md:186-201`). No field merge between initial hints and late before/selection/after is required by the evidence.

3. **Both confirmed context projections are encodable.** Cursor-aware input writes nested field 4 with before/selected/after fields 1/2/3 (`request.rs:242-246`). Legacy input writes top-level field 1 (`request.rs:247-252`); a selection-only legacy projection is already `before = selection`, with selected/after empty, as documented (`docs/research/rust-feature-parity.md:96-100`; `rust-recognizer-options.md:148-150`). Rejecting selected/after when `cursor_aware=false` (`request.rs:248-250`) preserves the oneof choice rather than losing text.

4. **Recognizer options are not compose activators.** Fields 7, 40, and 64 are explicitly present-capable and absent by default (`request.rs:18-26,41-43,142-151`), matching `docs/research/rust-recognizer-options.md:10-21`. Account-state signals remain deliberately omitted, not false; no safe offline value can be inferred (`docs/research/protocol.md:143-170`). Neither observation creates an advanced-intent gap.

## Confirmed response/merge/candidate contracts already implemented

5. **VoiceEdit fields and oneof 2/3/7 are decoded without candidate coalescing.** `VoiceEditResult` retains transcript, alternative_text, edited_text, edited_context, raw intent, active member, raw, and unknowns (`crates/rambler-protocol/src/models.rs:41-52`). The decoder keeps consecutive occurrences of the same member together, validates every string/context, and clears the prior candidate when the active member changes (`crates/rambler-protocol/src/decode.rs:144-160,181-201`). Thus field 7 message occurrences merge, scalar fields use their last value, and members 2/3/7 are not simultaneously presented as active. No confirmed field 4/5/6 response member exists to add: 4/5/6 are **intent enum values**, not VoiceEdit oneof numbers (`docs/research/protocol.md:271-295`).

6. **The outer recognition oneof and independent language strings are preserved.** Fields 3/4 decode separately as language/spoken_language (`decode.rs:287-292`; `models.rs:148-158`), while all documented oneof alternatives are enumerated (`decode.rs:286`). A newly active alternative clears recognition/speech; opaque alternatives retain raw bytes (`decode.rs:294-315`). This matches `docs/research/protocol.md:238-245` and `rust-response-language.md:11-25`; treating language as a result/candidate ID would be wrong, but current code does not.

7. **Containers 3/5/7 and fallback candidates remain structurally available.** They are independent optional containers, not a oneof (`models.rs:107-130`); the decoder recursively merges singular wrappers and retains repeated order (`decode.rs:97-143,236-276`). `all_hypotheses` traverses primary then fallback in canonical 3/5/7 order without claiming rank (`models.rs:90-99,118-131,169-178`). Rambler's static helper does not establish promotion of containers 5/7/fallback, so simplified rendering's primary-candidate restriction is deliberate, not a demonstrated omission (`docs/research/rust-response-containers.md:51-76`).

8. **Safety is checked across retained candidates before simplified projection.** `check_response_safety` traverses all active 3/5/7/fallback hypotheses and rejects blocked or unrecognized field-30 data (`crates/rambler-engine/src/results.rs:80-113`). `single_hypothesis` requires exactly one final update and one primary hypothesis (`results.rs:114-120`). This is intentionally stricter than Android's accepted-candidate branch; relaxing it for advanced intents would weaken safety, not improve confirmed fidelity (`docs/research/rust-feature-parity.md:69-85`).

## Intent presentation and editor scope

9. **Intent 2/3 behavior matches the recovered S3 helper.** `S3ClientCompatible` accepts intents 2/3 with member 3/7, joins field 7 as before+selected+after, and otherwise falls back to `Hypothesis.text` with an explicit reason (`results.rs:14-17,152-189`). Strict rendering remains narrower (`results.rs:191-209`). The field-7 join is full history text, not a delta, consistent with `docs/research/protocol.md:283,315-319`.

10. **COMPOSE/RAMBLE/RAMBLE_HISTORY refusal/fallback is justified by evidence, not a missing decoder arm.** All three numeric intents survive in `VoiceEditResult.intent`; there is no closed Rust intent enum rejecting them (`models.rs:47`; `decode.rs:183-186`). The recovered S3 helper accepts only intents 2/3 for edited candidates (`docs/research/composition.md:57-66`). Current compatible rendering therefore falls back to ASR for 4/5/6, while Strict/proposals refuse unsupported semantics. Assigning generation, note cleanup, summary, or history scope from enum names would fabricate behavior (`docs/research/rust-feature-parity.md:44-47`).

11. **PixelBoard's local COMPOSE rewrite is not a missing S3 helper contract.** Its source rewrites an already-created local action 4 with nonempty transcript to dictation and clears the diff (`docs/research/composition.md:67-80`); the recovered S3 helper does not establish creation of that action from intents 4/5/6 (`composition.md:61-66`; `rust-feature-parity.md:50-52`). `S3ClientCompatible` explicitly models the S3 textual helper, not PixelBoard's subsequent filter (`results.rs:14-17`). Adding transcript projection for every wire intent 4 would conflate those stages and could replace text without a confirmed VoiceAction.

12. **Replacement scope is deliberately constrained.** Proposals require the captured late snapshot and intent 2; member 7 additionally retains structured replacement context (`results.rs:481-520`). Opt-in compatible policy adds only intent 3/member 7, not an unscoped intent-3 string (`results.rs:523-567`). `validate_current` compares all three snapshot strings and optional revision (`results.rs:453-472`), and selection ranges are derived locally in UTF-16 (`results.rs:474-479`). This fulfills the confirmed context-fidelity requirement; extending proposals to 4/5/6 has no established range or effect contract.

## Structured commands

13. **Classification consumes accepted edited text, not raw ASR/transcript.** `classify_command` requires a unique final candidate and compatible VoiceEdit provenance (`results.rs:607-624`). DEFAULT SEND/SENT, CLEAR, and CLEAR ALL patterns are implemented in order with Java-compatible ASCII case/whitespace handling (`results.rs:652-682`). SEND requires IME action 4; CLEAR variants are explicitly rejected by the recovered client branch; every candidate has `applied:false` (`results.rs:583-605,625-648`). This matches `docs/research/rust-feature-parity.md:116`. Arbitrary remote regex execution or automatic CLEAR/SEND would be a safety/effect expansion, not a fidelity fix.

## Missing server/reference evidence (not locally fixable findings)

- No supplied observation establishes the server predicate, request difference, or response semantics for operational intents 4/5/6. The invocation corpus still reports intent 2/member 3 even with recognized Gboard prefixes (`docs/research/composition.md:216-255`). `muq.Z` has no found reference, so its name is not a wire switch (`composition.md:152-164`).
- Runtime flag provenance/provisioning and current REAL-caller configuration remain unestablished. Recognizer/account fields must not be guessed to force activation; server treatment of omissions and advanced requests is unknown (`docs/research/protocol.md:163-170,327-334`).
- MUSE/MUSE_LITE and Jetson Lite evidence does not supply an alternate portable S3 contract or authorize provider substitution (`docs/research/composition.md:85-150`).

## Precise discriminators needed before another client change

1. **For any claimed intent 4/5/6 gap:** a sanitized, version-matched Android reference trace containing the actual emitted request and raw final response, with VoiceEdit intent, active oneof member, transcript, Hypothesis.text, and full late context. It must also show the VoiceAction and text **before and after** PixelBoard filtering. This single chain distinguishes a missing request field from S3-helper projection and from PixelBoard-local rewriting; an enum value or fake action cannot.
2. **For a claimed application/custom-instruction gap:** a reference init decode showing extension 1203 field 5's exact ordered strings alongside the runtime-gate values and corresponding result. If it is field 5, current `preferences` already encodes it; a different destination would be new falsifying evidence (`rust-application-context-boundary.md:54-59`). No further blind preference prompt trial identifies a client defect.
3. **For a claimed command gap:** a versioned non-DEFAULT pattern payload or a reference callsite showing a different accepted candidate/IME precondition. Current code scopes itself to `S3DefaultPatternsV1`; only such evidence can distinguish a pattern-version gap from disabled runtime feature gating.
4. **For a claimed edit-fidelity gap:** paired input snapshot/revision and raw member-3/7 result, plus the reference replacement range and final editor snapshot. This would test full-window versus selection scope and absent-versus-empty behavior without risking an actual SEND/CLEAR or editor mutation.

## Bottom line

The confirmed client contract is already decoded/encoded and conservatively exposed. The next useful artifact is a stage-preserving reference trace for a real advanced result, not another activation guess, mock, or blind COMPOSE/RAMBLE request list.
