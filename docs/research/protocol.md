# S3 / Rambler protocol — PixelBoard 1.3.4

Status: static analysis verified against the published APK, without executing the APK or third-party sources. No network test was performed by this analyst. The descriptive names below are working aliases, not recovered `.proto` names. **The protocol is more than a transcription API: Rambler selects an Agentic service/routing and carries editing context.**

## 1. Provenance and reproducibility

- Release: <https://github.com/Akshayykadam/PixelBoard/releases/tag/1.3.4>.
- APK: `PixelBoard-1.3.4.apk`, 127650866 bytes, SHA-256 **`a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`**.
- Release notes: Gboard base `18.6.3.1002251336-release-arm64-v8a`.
- Tag source archive: root `Akshayykadam-PixelBoard-9a81f8b`; the APK, not patches from another version, is authoritative for wire numbers.
- Tool: JADX 1.5.3, Java 17, `--no-res`, 26 method errors during the general pass; targeted recovery of important methods. JADX ZIP SHA-256 `8280f3799c0273fe797a2bcd90258c943e451fd195f13d05400de5e6451d15ec`; source archive SHA-256 `156146ba76c27fac8b8717e54a5ef5a2772bc414c6e21906e3791515f6c86dff`.
- Independent verification of generated metadata: `C:/Users/FSOS/Documents/project/Rambpc/tools/research/jadx_metadata.py`. Run `python -I` from the repository with Java paths as arguments. The central schemas below were decoded with no metadata left over; this script neither loads nor executes their code.
- Shared artifact directory: `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad`.
- APK: `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-apk-download/PixelBoard-1.3.4.apk`.
- General decompilation: `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx/sources`.
- Targeted recoveries: same scratchpad, `pixelboard-jadx-ihv/sources/defpackage/ihv.java` (init), `pixelboard-jadx-knv/sources/defpackage/knv.java` (mutator), `pixelboard-jadx-ihm/sources/defpackage/ihm.java` (responses). Low-level control-flow check: `pixelboard-jadx-knv-simple/sources/defpackage/knv.java` and `pixelboard-jadx-ffh-simple/sources/defpackage/ffh.java` (`--decompilation-mode simple --no-inline-methods --no-inline-anonymous`).

The `classe.méthode` references below refer to `sources/defpackage` unless otherwise specified. `aaej.T(extendee, default, default, N, MESSAGE)` directly establishes the extension number. The `new aafx(...)` metadata strings establish field numbers and types; **masks such as `b |= 524288` are Java presence bits, not protobuf numbers**.

Warning: targeted `--show-bad-code` output has demonstrably incorrect/inverted conditions. Assignment fields/values are usable, but a doubtful condition is not considered verified. JADX lines are lines of a generated artifact, not Google source lines.

## 2. Transport and metadata

**Statically verified**:

- Service: `speech.s3.S3StubbyClientConnectorService`.
- Bidirectional method: `S3Bidi`, gRPC path `/speech.s3.S3StubbyClientConnectorService/S3Bidi`.
- `aamk.b()` lines 22–30: bidi type, request marshaller **`aama`**, response **`aamb`**.
- Domain present in the APK: `speechs3proto2-pa.googleapis.com`, port 443. The path is binary gRPC, not REST Speech-to-Text or a public Gemini API.
- `iha.a()` / `iha.b()` (`S3ConnectionUtil`) actually attach headers **`X-Goog-Api-Key`**, **`X-Android-Package`**, **`X-Android-Cert`** to the stub. The declared package is `com.google.android.googlequicksearchbox`, not the PixelBoard package. **Key and fingerprint values are not reproduced here; no secret found was used.**
- These application metadata fields do not by themselves prove OAuth/user account/attestation. Their general acceptance and server restrictions remain unknown; native trials in sections 9–10 establish acceptance in the tested configuration.
- `knv.a()` can separately add Zwieback user information; acquisition by `tjj` (`ZwiebackFetcher`) via GMS. Base flags `muq.C` (`agentic_dictation_use_separate_s3_application_id`) and `muq.D` (`jetson_send_zb`) default to empty and false respectively. This does not prove their effective value after configuration/patching.

**Inferred from standard gRPC, not captured on device**: HTTP/2 + TLS, POST, `content-type: application/grpc`, `te: trailers`, message envelope of 1 compression byte + big-endian32 length + protobuf. Do not confuse this framing with a protobuf varint prefix.

## 3. S3 envelopes

### S3Request = `aama`

`aama.a(int,Object)` metadata line 31:

| Field | Wire type | Java member | Established use |
|---|---|---|---|
| 1 | string / 2 | c | service chosen by `ihv.a()` |
| 2 | bool / 0 | d, default **true** | explicitly true in init and end; original semantic name not recovered |
| 3 | bool / 0 | e | final/end-of-data marker in `iho.q` |
| extensions | mostly messages / 2 | inherited `aaeh.w` | audio, parameters, context |

The static terminal request `iho.q` contains **field2=true and field3=true**. Its protobuf is exactly **`10 01 18 01`**; `iho.g()` sends it then calls `onCompleted()` on the client side (half-close). It is neither an empty message nor only field3=true.

### S3Response = `aamb`

`aamb.a(...)` metadata line 32:

| Field | Type | Java member | Use |
|---|---|---|---|
| 1 | enum / varint | c | S3 status, distinct from gRPC status |
| 2 | int32 / varint | d | S3 error code |
| 3 | string | e | S3 error detail |
| extensions | messages | inherited `aaeh.w` | recognition, heartbeat, etc. |

Status values **verified** by `a.aw(int)` lines 825–835 and targeted `ihm.c()` lines 86–108: **0=in progress/normal, 1=end of recognition, 2=error, 3=NOT_STARTED**. The dispatcher handles these branches separately. A protobuf response can be valid despite an unsuccessful S3 status; conversely, `grpc-status != 0` must not be treated as an S3 response.

## 4. Request extensions and initial config

### AudioInfo = `aalt`, S3Request extension **293100**

`aalt` static line 18; metadata line 28:

| Field | Type | Established use |
|---|---|---|
| 2 | **float32**, wire5 | sample rate; init **16000.0f** |
| 3 | enum, wire0 | encoding; observed path **10** |
| 4 | int32, wire0, default 1 | channel count; NewS3 call **1** |
| 9 | int64, wire0 | elapsed time in milliseconds (`wba.c().toMillis()`) |

Targeted `ihv.a()` lines 106–125/147–166 write these values. **Encoding10 corresponds to the Ogg/Opus path**, not raw PCM: `iho.j()` uses `ihp` (`OggOpusInputStream`), which calls `com.google.audio.hearing.common.OggOpusEncoder`.

An encoding enum can allow other values without the Rambler backend accepting them. No usable PCM value has yet been established by this report; do not invent `LINEAR16=1` from Cloud Speech.

### AudioData = `aals`, S3Request extension **293101**

`aals` static line 15 and metadata line 25: **field1 bytes**. The bytes are chunks of a **continuous Ogg/Opus stream**. `iho.j()` normally reads up to **224 bytes** per request; this split is not an Opus packet boundary and does not imply a separate Ogg file per message.

Actual audio messages also add extension 293100 with **only field9 elapsed_ms**, not the entire audio config (`iho.j` lines 342–360). `iho.i()` splits the batch path into 8192-byte blocks, so 224 is not a normative protocol constant.

### RecognizerInfo = `aapf`, S3Request extension **294500**

`aapf` static line 29; metadata line 44. Mutator config `knv.a()`:

| Field | Type | Value/use |
|---|---|---|
| 1 | message `aakn` | sub-config, **field4 bool=false** (`knv` lines 95–116) |
| 5 | bool | true |
| 7 | int32, default 2 | **blockOffensiveWords** (`kmy.h`): true→2, false→0 |
| 13 | bool | true |
| 20 | bool | true |
| 40 | bool | flag `sba.s` **enable_s3_auto_punctuation**, default false |
| 41 | bool | true |
| **50** | **string** | primary routing key |
| **64** | string | optional secondary routing |
| 91 | message `aapa` | field2 bool = absence of the **AdultAccount** component, via `!qfl.e(mtd.d)`; field4 bool exists but is not written here |

**Rambler / Jetson**: `knv.a()` lines 117–135 sets field50 to flag `muq.A`, whose verified base value is **`gboard_gemini_v3_streaming_voice_edit_mul`** (`muq.java` line 35). The flag is `jetson_experiment_recognizer_routing_key`. It is not an HTTP header. Other recognizers use `sba.r` or omit this field.

### ClientInfo = `aaly`, S3Request extension **294000**

`aaly` static and metadata:

| Field | Type | Mutator use |
|---|---|---|
| 2 | string | application id: `gboard`, or nonempty separate Agentic flag |
| 8 | string | platform `Android` |
| 9 | string | `Build.DISPLAY` |
| 10 | string | application major.minor version, fallback `-1` |
| 11 | string | `Build.MODEL` |
| 16 | string | trigger application id (`kmy.d`) |
| 17 | string | use not established |
| 22 | message `aalw`, with presence | field1 BOOL, also with presence: `kmy.m` / `supervisionEnabled`; account-state signal, see below |
| 26 | enum | value 1 assigned by the mutator |

These values describe the observed Android client; no need to falsify model/device for a native port is established.

### UserInfo = `aamc`, S3Request extension **293000**

- field2 message `aalx` = primary locale; `aalx.field1 string` (language tag), field2 int32.
- field4 repeated `aalx` = secondary locales.
- field9 repeated `aalv` = user tokens/information.
- field20 bool = absence of the **UnmanagedAccount** component, via `!qfl.e(mtd.a)` (`mtd` lines 20–23, `knv-simple` lines 188–192). This encodes a local component restriction/absence, not consent to donation.
- `aalv`: field1 string, **field2 string auth_token**, **field4 int32 type** (Zwieback mutator writes 2), field7 bool.

`knv.a()` adds the token only for **jetsonVoiceInput=true AND muq.D=true**, with the Zwieback future already completed successfully. **Nonempty C is not required**: `C=""` only keeps application id `gboard`. Control-flow evidence resolved by the `knv-simple` pass (lines 132–137,193–195,229–258,316–324), correcting inverted branches in the first pseudo-Java. The hash is **lowercase hex of SHA-256(UTF-8(Zwieback identifier))**, placed in `aalv.field2`, with `aalv.field4=2`; `wuh.a -> wul()` chooses SHA-256 and `wue.toString()` writes lowercase hex. Incomplete future or exception: token omitted. No identifier value was used or retained in this report. See also `authentication.md`.

### Other init extensions

- **29735924** -> `aamh`: field1 int32 = **500** in `knv.a()`; likely cadence/timeout but semantic name not established.
- **47096292** -> `aalz`: extensible logging/experiments container.
- Internal extension of `aalz` **229601060** -> `aamg`, repeated int64 field1. A `knv` branch adds an experiment identifier. This is not necessarily an interoperability requirement; do not reproduce a quota experiment to change server controls.
- Donation: extension **307792803** -> `aame`, added by `saz.a()` only with opt-in and local conditions. **The mutator's Jetson path passes false**, so `saz.a()` returns without adding this extension (`knv-simple` lines 196–228; `saz` lines 25–35). Do not add a consent declaration by assumption.

RecognizerInfo7/40/91, ClientInfo22/26, and UserInfo20 fields are explicitly written by the observed path; some depend on local state. Omitting them is not an exact static reproduction. `kmy.toString()` lines 67–68 confirms properties `blockOffensiveWords`, `supervisionEnabled`, and `jetsonVoiceInput`.

### Account-state signals: intentional native non-parity

Static clarification dated 2026-11-16, limited to the APK identified in §1:

- **ClientInfo22 → `aalw.field1`**: message and BOOL each have presence (`aaly.java:25,43`, `aalw.java:23`). `kla.s` initializes the parameter to false (`kla.java:240`), then sets it to true if the **UnsupervisedAccount** component is not positively ready (`kla.java:264–265`, `mtd.java:21`). The check `hky.java:17–36`, labeled **MinorsChecker**, links this signal to minor/supervised account state; it is not a feature switch.
- The parameter passes through `kmw.i` → `kmw.l` → `kmy.m` (`kmw.java:47,117–119`, `kmy.java:68`). For Rambler init without preexisting ClientInfo, the mutator writes presence and value, **including false** (`knv-simple:128–129,344–355`, calls `ihv.java:125,166`). The `ihv` output carries a JADX warning; the low-level `knv-simple` check is the reference for this branch.
- **RecognizerInfo91.2** = absence of **AdultAccount** and **UserInfo20** = absence of **UnmanagedAccount** are two other account-state signals (`knv-simple:115–125,188–192`), not consents or freely chosen options.

The Rust client without a legitimate account-state provider **intentionally omits these three signals**. No true/false API or CLI option is added. Android explicitly emits true for supervision when its state is unknown; native omission is therefore a divergence, **not a reproduction of that default**. Protobuf absence must not be described as a false assertion. Server handling of absence, true, and false remains unknown, particularly without account/Zwieback. This analysis proves neither its safety behavior nor COMPOSE activation; no guessed server trial supplements it.

## 5. Editing context: extension **1203**

InputBoxInfo = `aamo`, S3Request extension **1203**, static line 22. This is **not** a free-form JSON field.

| Field | Type | Use |
|---|---|---|
| 1 | string, oneof | text preceding cursor (or selection depending on flag) |
| 3 | repeated string | bias words / contacts |
| 4 | message `aalu`, same oneof as 1 | context with cursor |
| 5 | repeated string | user preferences / instructions |

`aalu` = text in three parts: **field1 string=before**, **field2 string=selection**, **field3 string=after**. Direct evidence `iia.a()`; the strings are not cursor indices.

`ihv.a()` can put context in init for certain modes. **Observed Rambler path: context sent late after audio**:

1. `ffh.fK()` / `AgenticDictationExtension.onListeningStop` retrieves context when the microphone stops (lines 1186–1213).
2. `knt.b()` installs a `knr` provider for Jetson; `kno.call()` retrieves current texts.
3. `iho.a()` waits up to 2s for the provider; `gwp.apply`, case12, calls `iho.b(context,cursorAware)`.
4. `iho.b()` constructs a new request containing **only extension1203**; `gwp` sends it then `iho.g()` sends the end and half-close.

Therefore **do not close the stream immediately after the last audio if context remains to send**. The choice of `aamo.field1` versus `.field4` depends on cursor-aware activation (`muq.P`, default false); when false, `muq.Q` can select only the selected text.

**SDK 0.3 — init hints.** `ProtocolConfig.bias_terms` and `.preferences` write repeated fields 3 and 5 of 1203 respectively in the initial request, and `.secondary_languages` writes repeated locales `293000.field4`. Before/selection/after text remains separately sent after audio. Static examination of fallback `ihv.java` lines 153–211, `knv-simple/knv.java` lines 259–309, and `pixelboard-jadx/sources/defpackage/zxh.java` lines 243–255 supports this placement and locale construction; full output paths are given in §1. Their presence does not establish general effectiveness. Requests in the ten [hints](../evidence/input-hints-public.json) trials succeeded; effects and limits are reported in [refinement](../refinement.md). Bounds are SDK bounds, not known server limits; init goldens without options remain unchanged.

## 6. Minimal faithful sequence

```text
TLS/HTTP2 + legitimate application metadata
S3Bidi(
  S3Request {
    1: "beyond-a2a-recognizer", 2:true,
    [293100]: {2:float(16000), 3:10, 4:1, 9:elapsed_ms},
    [294500]: {..., 50:"gboard_gemini_v3_streaming_voice_edit_mul"},
    [294000]: ClientInfo, [293000]: UserInfo,
    [29735924]: {1:500}, other parameters depending on path
  }
  S3Request {[293101]:{1:ogg_chunk}, [293100]:{9:elapsed_ms}} ...
  S3Request {[1203]:{1:preceding_text}}  # OR {4:{1:before,2:selected,3:after}}
  S3Request {2:true,3:true}             # protobuf 10 01 18 01
)
client half-close; continue reading responses/trailers
```

**Verified**: `ihv.a()` chooses service **`beyond-a2a-recognizer`** when `iic.d` (Jetson); otherwise **`multi-recognizer`** if multilingual, otherwise **`recognizer`**. This selection + field50 routing distinguish Rambler from simple S3 transcription. Init is therefore not a Google Cloud Speech `RecognitionConfig`.

This skeleton was initially a static reconstruction. **The subset implemented in `src/rambler/protocol.py` is now accepted by the service** (see sections 9–11 and native evidence). Not all fields marked `...` are emitted or declared indispensable; the exact need for each option was not established by ablation. Success in this profile does not guarantee other consents, flags, or server configurations.

## 7. Agentic responses and results

### Main S3Response extension **1253625** = `aapd`

`aapd` static, metadata `a(int,Object)`: **a single oneof** contains the following message alternatives: `1:aaln`, `2:aalf`, `5:aalc`, `6:aale`, `7:aalk`, `8:aapc`, `9:aald`, `10:aapb`, `11:aahy`, `12:aalr`, `13:aaph`, `15:aalh`, `16:aahx`, `18:aall`, `19:aalq`, `20:aapg`, `21:aalb`. Fields **3 string `d`** and **4 string `e`** are independent, outside the oneof. An unknown alternative can therefore replace variant 1/2: respect protobuf semantics, do not retain all variants simultaneously.

- field1 message **`aaln`** = recognition results.
- field2 message **`aalf`** = speech event.
- field3 string `d` = identifier associated with the result (exact name unknown).
- field4 string `e`; other oneof alternatives not exhaustively analyzed.

**Final text result**:

```text
S3Response[1253625]
  .field1 (aaln)
  .field3 (aalp)
  .field3 repeated (aalg)
  .field1 string = hypothesis text
  .field2 float32 = score/confidence
```

`aaln.field1 enum` represents recognition state (**0 intermediate, 1 final** in dispatcher branches). `aaln.field3` (final segment result) and `aaln.field4` (partial) are **independent**, outside oneof. `aaln.field4 (aalj).field1 repeated(aali)` supplies partials: `aali.field1 string` text, `aali.field2 double` stability/confidence. `ihm` concatenates the parts and uses threshold 0.9 to distinguish them. A segment final can arrive before the global end: do not equate field3 presence with half-close.

Additional verified fields, even if unnecessary for the first parser:

- `aaln`: field5 `aalp`, field7 `aalp`, field10 `aalm`.
- `aalp`: field3 repeated `aalg`, field5 `aalo`.
- `aalg`: field1 string, field2 float32, fields3/4/10/21 `aala`, field6 `aakl`, field30 enum.
- `aalj`: field1 repeated `aali`, fields4/5/10 `aala`.

Messages `aala`/`aakl` and enum field30 are not semantically reconstructed here. A valid hypothesis may have no score; preserve presence separately from default 0. Similarly, absent S3 status means protobuf default value 0, while remaining absent on the wire.

`aalf.field1 enum` carries **0=start-of-speech, 1=end-of-speech, 3=end-of-utterance** (`ihm.c` lines 124–139 + `a.aw`). Value 2 is allowed by the validator but does not trigger a callback in this branch.

### VoiceEditingResult = extension **74956224** of each `aalg` hypothesis

This is **not** a direct S3Response extension. `aamq` static line 24, metadata line 36:

| Field | Type | Use |
|---|---|---|
| 1 | string | transcript named by the client; **not necessarily verbatim audio**, see section 10 |
| 2 | string, oneof | text alternative, exact meaning to be confirmed |
| 3 | string, same oneof | edited history text |
| 4 | enum | user intent |
| 7 | `aalu`, same oneof | edited history text with before/selection/after |

Targeted `ihm.c()` contains `VoiceEditResultHelper.maybeGetVoiceEditedText`: for certain intents, it uses field3 or concatenation of the three field7 parts instead of ASR text. The transcript with `<cursor>`/`</cursor>` markers can be normalized. This branch has JADX control-flow defects; wire mapping comes from `aamq.a(...)`.

Enum **`aamp`**, **verified** values:

```text
0 DEFAULT_INTENT_DICTATION
1 INTENT_EDIT
2 INTENT_EDIT_HISTORY
3 INTENT_REWRITE
4 INTENT_COMPOSE
5 INTENT_RAMBLE
6 INTENT_RAMBLE_HISTORY
```

### Local types of the Agentic extension (do not confuse them with S3 wire)

`aaks` = local VoiceText, repeated **field1 `aakw`** (spans).

`aakw`: field1 string text, **field3 bool finalized**, field4 string id, field5 repeated `aaku` annotations, field6 `aakr`, field7 bool edit result, **field8 `aakv` VoiceAction**, field9 double confidence, field10 bool.

`aakv`: **field1 enum intent**, **field4 string transcript**, **field5 `aalu` produced text with cursor** (diffs are calculated locally).

`ffh.j()` (`AgenticDictationExtension.onVoiceText`, lines 670–804) checks for **exactly one span**, finalized and with VoiceAction; partials are not applied by this path. It then handles safety, simple actions, edited content, and context. A reimplementation must distinguish produced text, raw transcript, and editing action; do not blindly paste every hypothesis.

**Verified PixelBoard modification**: `ffh.j()` line 676 calls `GboardRamblerDictationRuntime.filterVoiceText(this, aaks)` **before** validation. The runtime source notably preserves transcripts, rewrites certain COMPOSE intents as dictation, and removes certain diffs. This client filter changes rendering; it is not raw Rambler backend behavior.

### Other response extensions

- **29737259** -> `aami`, empty message: heartbeat recognized by `ihm.c`.
- **171993426** -> `aape`, field2 string and field4 string: logging/diagnostics.
- **443278602** -> `aamf`, field1 bool: consent/donation response (`aamf` static line 15).

### Full text, local diffs, and cursor units

**Verified**: `aamq.field3` / `.field7` describe **produced history text**; `VoiceAction.field5` contains three strings, not a remote numeric patch. `ffh.E()` recomposes `before+selected+after` then `AgenticDictationTextEditor` locally generates a list of diffs between current text and produced text (`rfw.b`, `ffh-simple` lines 141–145,261–301). Local whitespace/newline and context corrections occur (`ffj.e`, `ffq.b/g`). Do not treat this text as a simple delta to append.

**Verified units: Java UTF-16.** `ffq.d()` lines 64–74 calculates selection from `aalu.c.length()` and `aalu.d.length()`, then `owj.F(start,end)`; `ffq.g()` line 94 uses `before.length()`/`after.length()` for replacement. `ffh-simple` uses `String.length`, `charAt`, `subSequence`, and offsets in the same unit. Neither UTF-8 bytes nor Python `str` indices. Example: `"A\U0001F600B"` (Python notation) has Java length 4, Python codepoints 3. No numeric wire offset was established in the context/VoiceEditingResult messages above; the port must retain the three strings and convert to ranges only with an explicit convention.

## 8. Verified, inferred, unknown — architectural consequences

**Statically verified**: bidi service, envelopes and extensions above; distinct Jetson service; protobuf field50 routing; Ogg/Opus16k mono on the NewS3 path; late context; end+half-close; intent model and PixelBoard filter.

**Inferred**: certain descriptive parameter names, semantics of unnamed booleans and 500; absence of an Android constraint in the wire itself. Android dependencies in the client do not establish a backend requirement.

**Unknown / to validate before a product engine**:

1. Effective flag values after patches/remote configuration; exact names of all fields.
2. Server durability/restrictions of metadata and potential need for Zwieback in other profiles; the tested profile actually works with the three metadata fields, without Zwieback.
3. Accepted PCM encoding: not established; the actual path is Ogg/Opus,10.
4. Full semantic mapping of result alternatives and editing types; coverage of different intents beyond the recovered partial helper. Statuses 0/1/2/3 are established.
5. Minimum list of parameters/consents required by the server, application errors/quotas, and reconnection strategies.
6. An Android capture is missing for comparison with app parity, as is broad functional coverage. Three-part selections, including in non-BMP context, are now exercised natively (sections 11–12), without on-device comparison. Selection of a non-BMP character itself has not been tested against the service.

**Recommended decision**: keep bidi transport, protobuf codecs, and editing policy separate; parse/preserve unknown extensions; do not call simple S3 transcription “functional Rambler”. No engine was implemented by the static analyst.

## 9. Coordinator's first native network validation

**Observation supplied by the coordinator, JSON independently reviewed. Bytes from this first session alone were not retained; independent re-decoding covers the following 33 responses, section 10.** Evidence: `C:/Users/FSOS/Documents/project/Rambpc/docs/evidence/reconstructed-default-session.json`, timestamp `2026-11-12T20:10:49.907227+00:00`.

- Target/method above; service **beyond-a2a-recognizer**, routing **gboard_gemini_v3_streaming_voice_edit_mul**.
- Observed application metadata injected in memory by the coordinator, no value recorded; **no OAuth, no Zwieback** in this session.
- Native client, declared platform **Python**, application id `gboard`, language en-US.
- Controlled synthetic speech, Ogg/Opus16k mono24kbit/s,14136 bytes, SHA-256 `5f9e53bb05e954f16517c618c63eb52da16e5289cc454936815354e9af397bc6`.
- Audio 224 bytes/chunk; empty context sent after audio; 67 requests, 13 responses; gRPC **OK**, terminal S3 status **1**, recognition state **1**, event **3** (end-of-utterance).
- Produced hypothesis: `Hello. This is a test of intelligent dictation. The meeting is tomorrow at 11.`
- VoiceEditingResult: **intent2**, nonempty transcript and edited_text identical to the text above; no edited_context. This 2 is a real value, **do not automatically rename it DICTATION0**: in the analyzed client enum 2=INTENT_EDIT_HISTORY.
- Evidence JSON retains presence of absent statuses (`null`) separately from protobuf default 0; this is correct. An absent score is not an error.
- Total duration 3301.479ms, last audio supplied 148.022ms; this is an accelerated sample send, **not** an end-to-end real-time dictation latency measurement.

**Bounded conclusion from this first session alone**: this validates one session of the statically identified Agentic S3 service/routing pipeline and a structured VoiceEditingResult response outside Android, without account/Zwieback token for this configuration. Additional functional correction/context validation is described in section 10. None of these trials demonstrate all languages, durable access, absence of quota, or complete Rambler/PixelBoard parity.

## 10. Correction and context: independent verification of 33 raw responses

The coordinator performed three additional sessions via `RamblerEngine`/`GrpcTransport`. **The analyst then re-decoded the 33 response protobufs with a standalone parser, without importing `rambler.protocol`, `rambler.wire`, or the coordinator's decoder.** Tool: `C:/Users/FSOS/Documents/project/Rambpc/tools/research/inspect_s3_capture.py`, executed with `python -I`.

- Response-only captures, local/ignored by Git: `C:/Users/FSOS/Documents/project/Rambpc/artifacts/live-functional-cases`.
- Redacted evidence: `C:/Users/FSOS/Documents/project/Rambpc/docs/evidence/functional-cases.json`, timestamp `2026-11-12T20:14:44.714250+00:00`.
- The 33 capture hashes, statuses, and texts match the JSON. No header/secret is present in these response captures.
- **Reusable offline corpus**: `C:/Users/FSOS/Documents/project/Rambpc/tests/fixtures/live_synthetic_responses.json` retains the three final results **original byte for byte**, with hex, SHA-256, input audio hash, date/provenance, and expectations. Recursive inspection before publication: the only text fields are duplicated controlled synthetic outputs; other leaves are states/intent and small numeric scalars. No account identifier, token, pseudonym, key, header, or opaque string found. Unknown scalars are retained unchanged without inventing semantics.
- `C:/Users/FSOS/Documents/project/Rambpc/tests/test_live_fixtures.py` verifies original hashes and their decoding by the client, **3 tests passed**. Final results have no explicit S3 status; this corpus does not present them as terminal session messages.

### Results verified in the raw bytes

| Case | Controlled input reported by the coordinator | Final text read directly from the capture |
|---|---|---|
| `correction-en` | sentence followed by “ at ten. No, make that eleven. ”, empty context | `The meeting is tomorrow at 11.` |
| `context-edit-en` | preceding context `The meeting is tomorrow at ten.`; audio `Change ten to eleven.` | `The meeting is tomorrow at eleven.` |
| `plain-fr` | synthetic French speech; empty context | `Bonjour. Ceci est un essai de dictée intelligente. La réunion aura lieu demain à 11h.` |

For each result, exact observed path: **S3Response.field1253625 → field1 → field3 → repeated field3 → field1 text**; in the same hypothesis, **extension74956224**, **field1 string**, **field3 string**, **field4 varint=2**. VoiceEditingResult fields 1 and 3 are identical to the final text above; **field7 absent**, confidence absent. Recognition `.1253625.1.1=1`, event `.1253625.2.1=3`, and terminal status `.1=1` observed in the respective captures. The coordinator reports gRPC OK and complete audio consumption with identical hash for all three sessions; these transport/input facts cannot be inferred from responses alone.

Final captures and SHA-256:

- `C:/Users/FSOS/Documents/project/Rambpc/artifacts/live-functional-cases/correction-en/response-008.pb`,243 bytes: `43fd638823e88612faa6ee8a50614c0ba30ac38bef51f1623fee1178013b9c1e`.
- `C:/Users/FSOS/Documents/project/Rambpc/artifacts/live-functional-cases/context-edit-en/response-006.pb`,267 bytes: `788d7342768e01bc529ef1799182641f10ff954b72b2e21099884672d22f922c`.
- `C:/Users/FSOS/Documents/project/Rambpc/artifacts/live-functional-cases/plain-fr/response-010.pb`,597 bytes: `b8c4e7ce9a84b34f27fd1bebf1201cd1ecd538940962a4b8b2cb243380b70d15`.

### Established consequences and limitations

1. **Strong evidence of Agentic behavior beyond simple ASR**: spoken self-correction is resolved; the contextual command rewrites supplied text rather than literally transcribing “ Change ten to eleven. ”. This corresponds to the statically identified Rambler service/routing, without an Android runtime dependency for these cases.
2. **The field named transcript is not a verbatim guarantee.** In the context case its content is produced history, not the spoken command; in the correction case it does not retain the disfluency. Do not promise recovery of a raw audio transcript from this field alone.
3. **Full text, not delta**: context is included in returned final text. An application appending this result to the end of context would duplicate the sentence; editing/range policy remains necessary.
4. **Unknown envelope field observed: field5 varint**. It appears in all 33 responses, including terminal messages, but has no named field in the APK's optimized `aamb`. Preserve/ignore it as unknown; no sequence/time semantics are demonstrated.
5. These three samples alone cover neither microphone, selection, all intents, quotas, nor general PixelBoard parity. Subsequent selection/concurrency and native encoder validations are separate below and in project status.

## 11. Subsequent selection and deferred-context validation

Coordinator evidence: [lifecycle.json](../evidence/lifecycle.json), engine, transport, and protocol code hashed. Controlled input `Change ten to eleven.`, context supplied by callback **after audio EOF**: before=`The meeting is tomorrow at `, selection=`ten`, after=`.`; `cursor_aware=true`.

The service returns VoiceEditingResult intent2 with `edited_context`:

```text
before:   "The meeting is tomorrow at "
selected: "eleven"
after:    "."
```

Text/transcript also contains `The meeting is tomorrow at <cursor>eleven</cursor>.`. Markers belong to this representation: for integration, preserve structured context rather than pasting this transcript literally. Field3 `edited_text` is absent, replaced by oneof7. S3 end, gRPC OK, and audio integrity were observed. This evidence comes from the engine codec; it is not presented as a new independent raw-payload inspection.

The same report then validates cancellation of a blocked input, source closure, and two concurrent English/French sessions on the same channel, without observed mixing. The [native progressive encoder](../evidence/native-realtime.json) also succeeded in six sessions with complete PCM, source/code hashes, and compared Ogg profiles. This still does not validate all intents, natural voices, long durations, or future service constraints.

## 12. Rewrite and non-BMP context — 2026-11-13

A new controlled case supplies context `Send me the report now.` then audio `Make this sentence more polite, but keep the same meaning.`. The final response was inspected with a standalone parser, without importing `rambler.*`:

- Source response: **1055 bytes**, SHA-256 `7a8c1217362395a706d0626aa69860413a82bed82cfc868f369750f185f4ff92`.
- Recognition `.1253625.1.1=1`, hypothesis under `.1253625.1.3.3`; S3 status absent from this final recognition response, which is not the session terminal.
- Hypothesis `.field1` and `VoiceEditingResult.field1`: context followed by the command.
- `VoiceEditingResult.field4=3`, active oneof **field3**, fields2/7 absent: `Could you please send me the report at your earliest convenience?`.
- `aamp.java` lines 9/29–30 confirms `INTENT_REWRITE(3)`. The `ihm.java` callsite lines 192–212 chooses edited text 3 or context 7 for ordinals 2/3, then line 224 favors it over ASR text. This reading corroborates rendering of the observed case, not a general replacement-scope contract.

The SDK now renders intent3/field3, but replacement proposals remain limited to intent2. A [new follow-up session](../evidence/refinement-rewrite-public.json) confirmed this rendering after modification. The first [redacted report](../evidence/refinement-capabilities-public.json) retains its historically ambiguous state for this session; it is not corrected retroactively.

The source response also contains **extension74956225** with internal text-processing labels and an opaque service identifier, present in two branches. Its schema and full semantics are not established; the SDK preserves it as unknown and takes no action based on these data. The original body remains private. The [fixture](../../tests/fixtures/live_rewrite_response.json) is **derived and redacted**, with reconstructed wrapper, safe components retained and hashed, explicit removal of 74956225. It is not a fourth public original capture.

The same corpus exercised context before=`🙂 The meeting is tomorrow at `, selection=`ten`, after=`.`. Returned intent2/field7: same before, selection=`eleven`, same after. A selection in context containing a non-BMP character is therefore actually exercised outside Android; the emoji is not itself selected. Numeric ranges remain calculated locally in UTF-16, not transmitted over the wire. The first natural English clip and 40,56s dictation are described with their limitations in [refinement](../refinement.md), along with additional natural corpora and subsequent endurance trials.
