# Advanced-mode request trace — PixelBoard 1.3.4

## Answer

**No request-side COMPOSE, RAMBLE, or RAMBLE_HISTORY selector is present in the traced Rambler/NewS3 producer.** The visible Rambler UI creates an ordinary `session_type="jetson"` session. The NewS3 writers send audio, recognizer/client/locale configuration, optional routing and hints, and current editor text; they do not write `aamp`, an intent enum, a previous-utterance transcript, or a conversation ID. `VoiceEditingResult.field4` is a **response** field (`S/aamq.java:16-30`), not a request field to reproduce.

Jetson Lite is a different Android path: Speech Services ASR plus AICore generation, with local prompts and local history handling. Its terminal result producer hardcodes local intent 2, including for generated text. Thus neither Lite generation nor the UI name “Ramble” establishes wire intents 4/5/6.

The only concrete editor-derived request omission found in MAIN is **ClientInfo field 16, string `triggerApplicationId`**. It has no established advanced-mode effect; a bounded falsification test is specified below. No other missing field has enough evidence for an advanced-mode service experiment.

### Source aliases and scope

All cited Java was treated as untrusted text; nothing from the APK/JADX was executed, imported, or built. Citations use the original generated line numbers. Negative claims are bounded to this closed Rambler/NewS3 chain and the same-APK writer inventory, not every unrelated Google service.

- `S` = `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx/sources/defpackage`
- `F` = `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx-ffh-simple/sources/defpackage/ffh.java`
- `K` = `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx-knv-simple/sources/defpackage/knv.java`
- `H` = `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx-ihm/sources/defpackage/ihm.java`
- `T` = `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-source/PixelBoard/pixelboard-patches/extensions/extension/src/main/java/com/akshaykadam/pixelboard/extension/rambler/GboardRamblerDictationRuntime.java`
- `R` = `C:/Users/FSOS/Documents/project/Rambpc/crates/rambler-protocol/src/request.rs`

## 1. Complete NewS3 request-writer inventory

| Writer | What it emits | Mode relevance |
|---|---|---|
| `ihv.a` / `S3HeaderProducer.buildS3Header` (`S/ihv.java:22-169`) | Builds initial `aama`: field 1 service string, InputBox hints and optional initial editor context, AudioInfo, then applies the injected mutator at 125/166. | The only service selection is at line 24; no intent/mode enum is written. |
| `knv.a` / `S3RequestMutator` (`K:25-375`) | Mutates that initial builder: S3Request field 2 true, RecognizerInfo, ClientInfo, UserInfo, and initial parameters. Injected by `knt` on NewS3 (`S/knt.java:74-110`). | Jetson changes routing at `K:87-105`; fixed recognizer booleans and locale/client metadata are not advanced-intent selectors. |
| `iho.j` / `NetworkSpeechRecognizer` (`S/iho.java:224-393`) | Sends initial header(s) at 295-304, then AudioData plus elapsed AudioInfo at 342-365. | A second stream is biasing-related, not Compose/Ramble; see §4. |
| `iho.a` and `iho.b` (`S/iho.java:85-126`) | Obtains current `oxs` editor text asynchronously, then creates a request containing only extension 1203. | Chooses current before/selection/after representation, never an intent or previous session. |
| Static `iho.q` and `iho.g` (`S/iho.java:40-50,161-175`) | Terminal field 2=true/field 3=true and half-close. | No semantic mode. |
| `iho.i` / `iho.k` batch path (`S/iho.java:182-221,401-404`) | Aggregates the same initial, AudioData, and terminal messages into `aamm` for deferred dispatch. | Batching is not evidence of another advanced-mode contract. `jetson_batch_use_unary_recognizer` is declared false by default (`S/muq.java:84`), but no COMPOSE/RAMBLE linkage was found. |

The established RPC descriptor is only `speech.s3.S3StubbyClientConnectorService/S3Bidi`, with `aama` requests and `aamb` responses (`S/aamk.java:16-35`). The batch wrapper does not reveal a Compose/Ramble method. `aama` itself has service field 1 and booleans 2/3 (`S/aama.java:31`); there is no request intent member.

## 2. UI/session selection and every routing choice

### Push-to-Ramble is generic Jetson activation

1. `fio.J` creates the `push_to_ramble` access point (`S/fio.java:149-164`). `uya` maps it to down/up key codes -10224/-10225 (`S/uya.java:758-762`; names in `S/pwk.java:309-310`).
2. `ffh.m` checks active extension, `mut.a(context)`, non-stylus state, and non-null controller/UI before down/up invoke the same continuation (`F:1788-1860`; `S/ffd.java:38-40`). `ffh.z` additionally requires its controller and idling UI state before onboarding/continuation (`S/ffh.java:1006-1034`; `S/fij.java:17-20`).
3. `ffh.y` and `fgr` emit the ordinary start event carrying `ffh.u(kmx)` (`F:2503-2521`; `S/fgr.java:97-102`). `ffh.u` writes `session_type="jetson"`, optional integer `force_offline`, and optional `user_preferences`; it writes no intent or Ramble flag (`F:2000-2079`).
4. `kla.s` recognizes exactly `session_type="jetson"` and sets `kmy.n` (`jetsonVoiceInput`) at line 247 (`S/kla.java:183-219,234-253`). `force_offline` is decoded as a local `kmx` fallback reason, not an S3 field (`S/kla.java:192-205`). `knt.b` copies `kmy.n` to `iic.d` and constructs the ordinary NewS3 session (`S/knt.java:59-110`).

The initial `klb.g(oak)` body is unavailable (`S/klb.java:191-205`), so this is a closed downstream-producer result, not an unbounded claim about every possible caller.

### Service and routing strings

- `ihv.a` selects **`beyond-a2a-recognizer`** when `iic.d` is true, **`multi-recognizer`** when `iic.a` is true, otherwise **`recognizer`** (`S/ihv.java:22-30`). Rambler's `session_type="jetson"` reaches the first through `kla`/`knt`; there is no Ramble-specific service string.
- Only when `kmy.n` is true, `knv` writes RecognizerInfo **field 50, string**, from `muq.A`; the declared value is **`gboard_gemini_v3_streaming_voice_edit_mul`** for `jetson_experiment_recognizer_routing_key` (`K:87-97`; `S/muq.java:35`; schema `S/aapf.java:29,44`). MAIN already sends this field and default value (`R:40,148`).
- In that same Jetson branch, RecognizerInfo **field 64, string**, is written only if `muq.B` (`jetson_secondary_routing_key`) is nonempty (`K:98-105`; `S/muq.java:36`). Its declared default is empty, so no second key value can be derived. MAIN can already emit a caller-supplied nonempty field 64 (`R:26,149-151`); there is no evidence-based advanced-mode value to test.
- Outside Jetson, `knv` uses generic `sba.r` or removes field 50 (`K:360-375`). That is not an alternate Rambler mode.
- RecognizerInfo **field 7 INT32** is 0/2 from `kmy.h`, **field 40 BOOL** comes from `sba.s`, and **fields 1.4=false, 5/13/20/41=true** are fixed (`K:35-86`; `S/aapf.java:44`). MAIN already encodes the same fixed values and exposes 7/40 (`R:138-147`). None is a voice-mode enum.

## 3. Editor state, application context, and history

### (a) Fields MAIN already sends or can send

| Wire item | Android source and exact path | MAIN status / advanced relevance |
|---|---|---|
| InputBox extension **1203**, **field 4 message**, nested **1/2/3 strings** | With `z=true`, `iho.b` calls `iia.a(oxs)` for current before/selected/after (`S/iho.java:96-115`; schema `S/aamo.java:13-31`, `S/aalu.java`). `kla.s` derives cursor awareness from `jetson_enable_cursor_awareness` (`muq.P`, default false) only for Jetson (`S/kla.java:250-253`; `S/muq.java:50`). | Already encoded at `R:242-246`. This is current editor state, not RAMBLE_HISTORY. |
| InputBox **field 1 string**, legacy alternative | With `z=false`, `iho.b` sends `oxs.n()`; if `muq.Q` is true and `oxs.o()` reports a selection, it instead sends `oxs.l()` (`S/iho.java:116-123`). Q is `jetson_send_only_selected_text_if_present`, default false (`S/muq.java:51`). | Already encoded at `R:247-252`; selection-only projection is `before=selection`, selected/after empty. No mode bit accompanies it. |
| InputBox **field 3 repeated string** | Initial generic biasing/contact content from `iic.e` and `igz` branches (`S/ihv.java:31-93`). | Already `bias_terms` at `R:172-179`; generic hints are not Compose instructions. |
| InputBox **field 5 repeated string** | `ffh.u` adds app information under `muq.X` and suitable `EditorInfo`, and custom instruction under `muq.Y`, to `user_preferences` (`F:2010-2077`; declarations `S/muq.java:58-59`). `kla.s` filters that list (`S/kla.java:207-219`), `knt` passes it to `iic.g` (`S/knt.java:98-100`), and `ihv.a` appends it to `aamo.e` (`S/ihv.java:94-100`). | Already `preferences` at `R:172-180`. Missing Android collection/formatting is platform integration, not a missing wire field. |
| Initial InputBox **field 1 or 4** | If `muq.a` is false and `oxs` is non-null, `ihv.a` can put current context in the initial request (`S/ihv.java:102-146`). Otherwise the traced Rambler path sends it after audio through `iho.a/b`. | MAIN sends the same fields late, not in init (`R:237-253`). This is a timing difference, not another field or advanced intent. |
| UserInfo **field 2 primary locale**, **field 4 repeated secondary locales** | Built from `kmy.b/c` by `knv` (`K:171-186,259-311`). Jetson chooses its secondary-language list in `kla.s` (`S/kla.java:223-233`). | Already encoded at `R:155-160`; no conversation/session identifier is added. |

The `EditorInfo.fieldName` is collected into `kmy.e` (`S/kla.java:258-263`; member names in `S/kmy.java:65-68`), but `knv` does **not** write ClientInfo field 17. It therefore is not a hidden request hint in this path. Likewise `muq.Z` (`agentic_dictation_rewrite_compose_rules`) is only a declaration at `S/muq.java:60`; no consumer in the request writers or a demonstrated wire destination was found. It must not be injected as a preference on the strength of its name.

### (b) Concrete field MAIN never sends but Android sets

**ClientInfo extension 294000, field 16, string `triggerApplicationId`.** `kla.s` conditionally takes the actual `EditorInfo.packageName` at `S/kla.java:258-261`; it becomes `kmy.d`, then `knv` writes `aaly.h` at `K:143-149`. The metadata maps that member to **field 16** (`S/aaly.java:25,43`). MAIN's ClientInfo currently contains only fields 2, 8, and 26 (`R:152-154`). This is distinct from field 2 application ID, which MAIN already sends and which Android sets to `gboard` or a separate configured ID (`K:132-149,316-324`).

There is **no static evidence that field 16 enables any advanced intent**. It is simply the sole missing editor-derived request value with a complete source-to-wire trace.

**Minimal falsification test for field 16 (not an activation recipe):**

1. Use two otherwise identical en-US sessions and the same prerecorded utterance: “Hey Gboard, write a short message to Alex saying I will arrive 20 minutes late.” Supply `bias_terms=["Gboard"]` in both arms to preserve the already-observed recognition of the invocation; keep preferences and late editor text empty and all other fields unchanged.
2. Baseline: current request, field 16 absent. Test arm: add only field 16 containing the **real package name of the owned test editor**, corresponding to Android's `EditorInfo.packageName`; do not substitute another application's identity.
3. Compare raw final VoiceEditingResult intent, active oneof member, transcript, edited text/context, and ASR text. A discriminating positive would be generated content and/or a different raw intent only in the field-16 arm, against the established baseline intent 2/literal instruction. If both remain intent 2/literal, field 16 alone is not sufficient for this utterance/profile; if both change, it is not necessary. Do not infer semantics solely from 4/5/6 and do not add device/account flags after a negative.

This is a two-session necessary-condition test with no retry, not a prediction that field 16 is a Compose switch.

**Not advanced-mode (b) candidates:** Android also writes generic ClientInfo field 9 (`Build.DISPLAY`), field 10 (application major.minor, fallback `-1`), and field 11 (`Build.MODEL`) at `K:151-164,325-343`; MAIN omits them. They are constant device/application metadata, not values selected by Compose/Ramble UI or editor mode, and no advanced response predicate is linked to them. No activation testing with device identities is justified. Account-state, credential, experiment/eligibility, and attestation branches were excluded as requested.

### No S3 previous-turn history

The complete writers above contain no previous utterance, prior draft list, or conversation/session ID. InputBox carries the **current** editor snapshot; field 5 carries explicit preference strings. The response name `EDIT_HISTORY`/`RAMBLE_HISTORY` does not create a request-history field.

Jetson Lite does have **local** prompt construction with current text, app/screen/dictionary context, custom rules, and a conversation-history placeholder (`S/fgn.java:29-87`, method `d`). In the recovered body the history list starts empty, the empty case becomes “No previous turns in this session,” and the nonempty branch is incomplete (`S/fgn.java:44,70-80`). This does not establish operational previous-turn history, and none of it is serialized into S3. It goes to the local AICore request at `S/fgn.java:82-87`.

## 4. Second streams, alternate backends, and local generation

- A second **S3Bidi** stream is opened when `igz.h` is enabled and `iic.e` biasing terms are nonempty (`S/iho.java:257-277`). Both streams receive initial headers and the same audio (`S/iho.java:295-304,342-365`). This is generic biasing infrastructure, not an intent-4/5/6 channel.
- Backend enum MUSE/MUSE_LITE does not supply a hidden remote Compose provider in this APK: `ffi` falls back to S3 when the provider is absent (`S/ffi.java:21-30`), and the injected map is empty (`S/erz.java:1895`; `S/wmr.java:11`).
- Lite fallback selection is separate from S3 (`S/kkj.java:23-46`). Its ASR is bound to Android Speech Services (`S/ohq.java:69-80`) and generation to AICore (`S/enm.java:38-40`; invocation in `S/fgn.java:82-87`), not a second portable S3 method.
- `fgn.b` uses Lite version/edit-history gates and computes numeric ASR feature 13 or 4 at line 247 (`S/fgn.java:216-255`; flags `S/muq.java:32-34`). That numeric 4 is passed to the ASR adapter, not `INTENT_COMPOSE`.
- Lite command/V2 shortcuts and LLM results remain text wrappers (`S/fgw.java:163-180`; `S/ews.java:175-197`; `S/hmk.java:198-219`). `fgt.m` then explicitly creates local `VoiceAction` **INTENT_EDIT_HISTORY=2** at `S/fgt.java:178-186` and emits it with the result text at 187-218. There is no input-dependent selection of 4/5/6.

The local invocation phrases in `S/igs.java:6` and prompt replacement in `S/fgn.java:63` therefore concern Lite prompting only. No proofread/magic/compose string becomes an S3 field in any writer listed in §1.

## 5. Purely local response/post-processing (c)

- The NewS3 response helper reads `aamq.e` but accepts only intents **2/3**, with oneof **3/7**, to produce an edited candidate (`H:183-220`). Otherwise it falls back to ASR text at `H:224`; it creates a local `VoiceAction` only if that candidate exists (`H:255-286`). Consequently, even a raw 4/5/6 response would not acquire Compose/Ramble editor semantics through this helper.
- PixelBoard's later filter rewrites an already-existing local intent 4 to 0, preserves intent 5 in its non-command normalization, and normalizes 6 to 0 (`T:174-197`; command recognition only for 1/2/3 at `T:232-264`). These are local transformations, not request activation or proof that the S3 helper created such an action.
- Lite's generated text followed by hardcoded local intent 2 (`S/fgt.java:178-218`) is also local result construction, not wire intent evidence.

## Bottom line

The request contains **no advanced intent toggle to add**. Rambler UI activation, Jetson S3 routing, Lite generation, and returned `VoiceEditingResult.field4` are four different layers. Intents 5/6 are not tied by this evidence to an Android history payload sent to S3. Apart from the isolated ClientInfo-16 falsification test above, the useful next artifact remains a sanitized version-matched Android request/response trace for a real advanced result; guessed routing values, prompt preferences, device/account metadata, or a request-side intent enum are not evidence-based substitutes.
