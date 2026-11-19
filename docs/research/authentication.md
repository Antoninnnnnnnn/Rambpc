# Rambler / PixelBoard authentication and access dependencies

Analysis date: 2026-11-12. Scope: PixelBoard 1.3.4 client, S3 transport used by agentic dictation/Jetson. This note does not document a public Gemini API or propose substituting it for the observed service.

**Experimental update on the same day:** the coordinator obtained 42 complete sessions from Windows with the three observed application metadata fields, without an account, OAuth, Zwieback, phone, or Android relay. See [evidence](../evidence/README.md), including corrections, editing, and native PCM codec. This validates this profile and these credentials/configurations on the test date, not all future restrictions or other authentication branches.

## Documentation supplement — November 17, 2026

Google Cloud pages on [API keys](https://docs.cloud.google.com/docs/authentication/api-keys) and their [restrictions](https://docs.cloud.google.com/api-keys/docs/add-restrictions-api-keys) were consulted without calling the service. A **standard** key associates a request with a project for billing and quotas; it does not identify a user principal. Keys bound to a service account are a separate category, not a mechanism demonstrated for S3/Rambler. Android restrictions identify permitted applications by package and signing fingerprint; API restrictions limit the services that can be used.

These pages document neither third-party access to the internal Rambler service nor a right to reuse a key embedded in another application in a native client. Nor do they demonstrate that a particular configuration is prohibited: **authorization for that configuration remains to be established separately**. Possessing a configuration file, observing headers, or obtaining a server response does not by itself constitute a provisioning, native reuse, or renewal contract. Extracting new third-party credentials, impersonating an application to cross a restriction, or trying random authentication variants is not an acceptable provisioning route.

Technical qualification of the producer, public guard, or launchers therefore does not provide that authorization. A new live trial requires a configuration legitimately authorized for native use, without circumvention; its provenance must be establishable without publishing its values. The presence of the three fields in a local file is not enough to close this gate. No OAuth flow, fabricated GMS identifier, substitution with a public API, or automatic key renewal follows from these general sources.

## Key conclusions

1. **Anonymous access is not sufficient to characterize authentication.** A gRPC call without metadata, with an empty message and half-close, received `PERMISSION_DENIED` (“Permission denied on the service.”) in about 80 ms. This establishes only the rejection of that incomplete call, not that a Google account is mandatory or that a conforming request would be rejected (`docs/evidence/anonymous-empty-rpc.json`).
2. **The client consistently attaches three application metadata fields to the S3 channel**: `X-Goog-Api-Key`, `X-Android-Package`, and `X-Android-Cert`. The package value is `com.google.android.googlequicksearchbox`. The key and fingerprint are embedded application values; their values are deliberately neither reproduced here nor recorded in logs. These elements potentially identify/restrict the client and its quota; **they are not, on their own, proof of user account authentication**.
3. **The Zwieback-related `auth_token` is a conditional protobuf field, not the gRPC `Authorization` header.** In the observed mutator, the gate is `jetsonVoiceInput` **and** `jetson_send_zb=true`. The separate S3 application identifier (`agentic_dictation_use_separate_s3_application_id`) does not condition this gate: when empty, the client uses `gboard` while retaining the ability to add Zwieback. The defaults in this APK are `jetson_send_zb=false` and an empty separate identifier.
4. **The observed Zwieback mechanism is neither OAuth nor a cookie.** Gboard requests a `PseudonymousIdToken` from GMS Core, then locally computes `hex_minuscule(SHA-256(identifiant_en_UTF-8))` and places the result in the authentication field. No Web token-issuance endpoint, expiration, refresh token, or renewal interval appears on this path.
5. **PixelBoard does not provide a standalone authentication mechanism.** Its public patches enable application capabilities and routes; they implement neither OAuth issuance, cookies, accounts, nor attestation, and change neither `jetson_send_zb` nor the examined S3 routing flags.
6. **The tested profile actually works without Android.** Native sessions succeeded without Zwieback. However, if another legitimate server configuration activates the Zwieback branch and the service requires it, the observed initial acquisition depends on GMS Core on the user's device. No documented equivalent public way to issue this identifier outside Android/GMS was found. An identifier must not be fabricated or borrowed from a third party.
7. **No evidence of an account, cookie, OAuth, or Play Integrity was found in the specific S3 path examined.** This absence of evidence is limited to that path and version; it does not prove that the service will never impose an additional check.

## Artifacts and reproducible references

### Public PixelBoard sources

- Repository: <https://github.com/Akshayykadam/PixelBoard>
- Working revision inspected: `80974b4614d15b4e80398f5400867aed366998bc`.
- Tag `1.3.4`: `9a81f8b03d1d26b2ebac07355362865f72452137` (“Bump version to 1.3.4 and update changelog”, 2026-11-10).
- Analyzed APK: <https://github.com/Akshayykadam/PixelBoard/releases/download/1.3.4/PixelBoard-1.3.4.apk>
- APK SHA-256: `a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`.
- Public Rambler policy: [`GboardRambler1803StockPolicy.java`](https://github.com/Akshayykadam/PixelBoard/blob/9a81f8b03d1d26b2ebac07355362865f72452137/pixelboard-patches/extensions/extension/src/main/java/com/akshaykadam/pixelboard/extension/rambler/GboardRambler1803StockPolicy.java), especially lines 38-41 and 52-110.
- Public Rambler selection: [`GboardRambler1803OfficialSelectionRuntime.java`](https://github.com/Akshayykadam/PixelBoard/blob/9a81f8b03d1d26b2ebac07355362865f72452137/pixelboard-patches/extensions/extension/src/main/java/com/akshaykadam/pixelboard/extension/rambler/GboardRambler1803OfficialSelectionRuntime.java).

Commands for establishing provenance, without executing repository code:

```bash
git clone --no-tags https://github.com/Akshayykadam/PixelBoard.git
git ls-remote --tags https://github.com/Akshayykadam/PixelBoard.git refs/tags/1.3.4
sha256sum PixelBoard-1.3.4.apk
```

On Windows, repository checkout required `core.longpaths=true` in the local scratchpad copy. No third-party script was executed.

### JADX output shared by the protocol analysis

The APK was not downloaded or decompiled again for this note. The paths below refer to the existing decompilation supplied by the protocol agent; unobfuscated names come from source strings retained in the bytecode.

- `pixelboard-jadx/sources/defpackage/iha.java` — `S3ConnectionUtil`, lines 8-20: creation of the three headers and attachment to the stub with `abtq`.
- `pixelboard-jadx/sources/defpackage/abtq.java`, lines 17-22, and `abtp.java`, lines 17-21: insertion of metadata into calls.
- `pixelboard-jadx/sources/defpackage/aams.java`, lines 5-16 and 182-214: production host, declared methods and scopes.
- `pixelboard-jadx/sources/defpackage/aamk.java`, lines 16-35: bidirectional `S3Bidi` method.
- `pixelboard-jadx/sources/defpackage/aamr.java`, lines 25-29: association of `S3Bidi` with declared scopes.
- `pixelboard-jadx-knv-simple/sources/defpackage/knv.java` — verified simple reconstruction: `L36`/`L40-L43` (lines 132-137 and 316-324) establish the Jetson gate, including when the separate identifier is empty and the application is `gboard`; `L95` (lines 229-254) checks `muq.D`, requires a successfully completed future, then derives and adds the token. The `pixelboard-jadx-knv/sources/defpackage/knv.java` output with `--show-bad-code`, whose branches were contradictory, is no longer used to infer the conditions.
- `pixelboard-jadx/sources/defpackage/muq.java`, lines 35-39: flags and default values.
- `pixelboard-jadx/sources/defpackage/tjj.java`, lines 8-58: `ZwiebackFetcher`, GMS client, cached future, and one-second timeout.
- `pixelboard-jadx/sources/defpackage/lwa.java`, lines 10-16: `PseudonymousId.API`.
- `pixelboard-jadx/sources/defpackage/lwc.java`, lines 17-22 and 77-83: GMS feature `3901` call.
- `pixelboard-jadx/sources/defpackage/lfo.java`, lines 72-82: Binder `IPseudonymousIdService`.
- `pixelboard-jadx/sources/defpackage/lwd.java`, lines 9-42: `PseudonymousIdToken` containing a string.
- `pixelboard-jadx/sources/defpackage/tji.java`, lines 7-24: wrapper whose `toString()` is `Zwieback`.
- `pixelboard-jadx/sources/defpackage/wuh.java`, `wul.java` lines 71-77 and `wue.java` lines 45-53: SHA-256 and lowercase hexadecimal encoding.
- `pixelboard-jadx/sources/defpackage/aalv.java`, lines 25-33: token protobuf fields; `aamc.java`, lines 15-20 and 28-36: authentication list in the request extension.
- `pixelboard-jadx/sources/defpackage/igz.java`, lines 7-10: default host and `s3_auth_token_for_debugging` flag, empty by default.

Absolute output prefix in this session:

```text
C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/
```

### Independent public source on Zwieback

The Chromium proto preceding commit `7b9af2142151fbb4f38f3abd23ec87541492cf81` defines Zwieback as “the signed-out session identifier” and notes that some clients carry it in-band rather than in an HTTP cookie:

- File at revision `f9f096edfd9a3020626bc2fac9a351c91c150f20`: <https://chromium.googlesource.com/chromium/src/+/f9f096edfd9a3020626bc2fac9a351c91c150f20/components/feed/core/proto/v2/wire/chrome_client_info.proto>
- Diff removing that comment: <https://chromium.googlesource.com/chromium/src/+/7b9af2142151fbb4f38f3abd23ec87541492cf81%5E%21/>

This source defines the general nature of the identifier; it does not document the S3 server, its authorization, or its acquisition by GMS.

## Observed access chain

### 1. Transport and application identification

Observed production target:

```text
speechs3proto2-pa.googleapis.com:443
/speech.s3.S3StubbyClientConnectorService/S3Bidi
```

`iha.a()` constructs the following three headers, then `iha.b()` attaches them to the stub:

| Element | Observation | What is not demonstrated |
|---|---|---|
| `X-Goog-Api-Key` | Embedded application value, present in the S3 channel | User identity, OAuth consent, unlimited access |
| `X-Android-Package` | `com.google.android.googlequicksearchbox` | PixelBoard's actual package or signed-in account |
| `X-Android-Cert` | Embedded application fingerprint | Play Integrity attestation or fresh device proof |

The combined package/certificate presence is compatible with a conventional Android API restriction. It is not an attestation generated for each session: values are static in the analyzed client. For authorized interoperability, a faithful client must be able to provide this metadata without logging it. Injection must remain secret local configuration, never a value copied into git. If reuse is not authorized, official credentials belonging to the user must be obtained rather than bypassing the restriction.

### 2. Zwieback `auth_token`: conditional local issuance

The recovered code conceptually does:

```text
if jetsonVoiceInput
and if jetson_send_zb == true
and if the Zwieback future completed successfully:
    zb = GMS_Core.PseudonymousIdToken       # Android/GMS acquisition
    token = lower_hex(SHA-256(UTF8(zb)))     # local derivation
    authentication.type = 2                  # numeric meaning not established
    authentication.auth_token = token
    request.user_info.auth.append(authentication)
```

The names `user_info` and `type` above are descriptive, not confirmed proto names. Field 2 (`aalv.f`) receives the hash; field 4 (`aalv.c`) receives `2`; the object is added to repeated field 9 (`aamc.c`) of the `UserInfo` extension number 293000 (`aamc.g`). If the future is not complete, the client logs “Zwieback ID not ready yet, skipping auth_token” and continues without this field; an exception produces the same behavior. The code does not construct `Authorization: Bearer ...` or place this hash in a cookie.

**Important configuration point:** when the separate S3 identifier is empty, the mutator uses the application identifier `gboard`, but the Zwieback gate remains `jetsonVoiceInput && jetson_send_zb`. The additional condition “Jetson and nonempty separate identifier and `muq.E`” concerns the quota experiment, not addition of the token. PixelBoard patches force neither `agentic_dictation_use_separate_s3_application_id` nor `jetson_send_zb`. The Phenotype/server values actually received at runtime nevertheless remain unknown: local defaults are insufficient to rule out a different remote configuration.

The `s3_auth_token_for_debugging` flag exists, empty by default, but the JADX search found no explicit consumer outside its declaration. It must not be inferred to allow replacement of the token or used as an access mechanism without evidence.

### 3. Renewal and lifetime

- The API key and fingerprint are static: no renewal is associated with them in the observed code.
- `ZwiebackFetcher` retains a result future and applies a one-second timeout to the GMS call. This timeout bounds the local wait, **not the token's validity period**.
- SHA-256 derivation is deterministic: for the same Zwieback, the hash is identical. No timestamp, nonce, `expires_in`, refresh token, or scheduled rotation appears in `knv`/`tjj`.
- The exact frequency of GMS reacquisition and events that change Zwieback remain unknown. Both the hash and source identifier must be treated as sensitive pseudonymous identifiers and must not be logged.

### 4. Account, OAuth, cookies, and attestation: bounded negative observations

`S3Bidi` declares four generic scopes: `assistant`, `assistant-sdk-prototype`, `googlenow`, and `googleit`. This declaration does not prove that an OAuth flow or `AccountManager` is executed. In the classes on the specific path (`iha`, `abtq`, `aamj`, `aamk`, `aamr`, `knv`, `tjj`), no account acquisition, cookie, bearer token, or attestation was identified. Generic `authorization`/`cookie` occurrences in shared HTTP tables are not evidence of use.

No Web issuance/renewal endpoint, OAuth callback, or Play Integrity/SafetyNet call was found on this path. Another GMS Core or server layer may have its own controls, invisible in this client code.

## Status of access questions

| Question | Status | Evidence needed |
|---|---|---|
| Are the three application metadata fields sufficient with a conforming Rambler request? | **Yes, observed for the tested profile** | 42 complete sessions, S3 end + gRPC OK; not a guarantee of universal or permanent access |
| Is a signed-in Google account necessary? | **Not for the native sessions tested** | No account/OAuth supplied; do not generalize to other configurations |
| Do remote flags activate Zwieback? | Unknown | Runtime value of `jetsonVoiceInput` (`kmy.n`) and redacted value of `muq.D`, or mutator trace without identifier value; `muq.C` only reports the application identifier |
| Does the server require `auth_token` when the branch is active? | Unknown | Response to a valid request in that configuration, without fabricating a token |
| Is the authentication field accepted outside GMS after legitimate provisioning? | Unknown | Controlled reuse of only the user's own identifier, if authorized, without an Android relay at runtime |
| Is additional attestation required server-side? | Not observed client-side | Explicit server error or authorized capture of a legitimate session; no circumvention attempt |
| What is the meaning of `aalv.c = 2`? | Unknown | Corresponding source proto or enum; do not invent “OAuth” or “Zwieback” without evidence |

## Access required to reproduce and unexercised branches

- **Live trials performed:** controlled synthetic corpus and bounded calls, without user intervention, account, or phone. Public tools require `--live` for a non-local endpoint; send content only if its processing is authorized.
- **For application credentials:** an authorized local configuration, injected without display or commit. Do not copy a private token or third-party credentials.
- **Only if Zwieback is actually active:** the PseudonymousIdToken from the user's own GMS device, obtained through the client's normal mechanism, potentially as one-time initial provisioning. The need for an Android device in this case must not be generalized to the default mode. Do not extract the Android accounts database or use another person's identifier.
- **If a PixelBoard reference session becomes necessary:** use the proposed Xiaomi only when available, with the action and expected evidence defined in advance. The phone would serve to observe a development session, never as a mandatory relay for the final engine.

## Non-destructive network mini-experiments

These steps retain the initial investigation plan. **Step 1 was completed successfully and then replicated**; steps 2/3 were unnecessary and were not executed. The Zwieback branch is not provisioned/implemented by the current engine. Any follow-up remains low-volume, without bursts or random authentication variants.

### Step 0 — local test without network

With a public dummy value such as `zwieback-test-fixture`, verify that the implementation produces exactly the lowercase hexadecimal UTF-8 SHA-256 and serializes `aalv.c = 2`, `aalv.f = hash` in the correct field. Never use a real value in a fixture or committed test.

### Step 1 — valid default request (completed, then replicated)

Build the complete minimal Rambler request according to the confirmed schema, with:

- the three legitimately configured application metadata fields;
- application identifier `gboard` if the separate identifier is empty;
- routing keys and parameters actually observed for Jetson;
- no `auth_token` if `jetson_send_zb` is false;
- short authorized test audio, then half-close; no automatic retry.

Record only status, non-sensitive detail, latency, message/byte counts, and redacted configuration hash. **Expected evidence:** distinguish an access denial from a schema error and determine whether the default path works without an account/Zwieback. A new `PERMISSION_DENIED` after a valid request would be significantly more informative than the empty anonymous test, but would still not justify an attempt to circumvent restrictions.

### Step 2 — PixelBoard reference only if step 1 fails

On the available user device and with their consent, run one known Rambler dictation in PixelBoard, without a TLS proxy or security weakening. Collect redacted `GboardS3RMutator`/`NewS3Recognizer` events and the functional result; do not record audio, text, API key, certificate, Zwieback, or hash. **Expected evidence:** establish whether this exact version/configuration works and whether the “skipping auth_token” branch is taken. This does not replace an authorized metadata capture if disagreement persists.

### Step 3 — Zwieback branch only on a legitimately observed configuration

Do not artificially enable `jetson_send_zb` to explore the server. If the user's normal configuration already shows `jetsonVoiceInput=true` and `jetson_send_zb=true`, reproduce a valid request with their own derived token, injected in memory, whether the application identifier is `gboard` or a separate identifier. Compare at most one session to the legitimate reference and stop after an explicit denial. **Expected evidence:** derivation validity and field sufficiency/insufficiency; no rotation or expiration should be assumed without a response or observation over time.

## Stop criteria and privacy rules

- Do not test keys, accounts, cookies, tokens, or fingerprints from public logs or other users.
- Do not substitute the public Gemini API for the observed S3/Rambler protocol.
- Do not turn PixelBoard flags that bypass client quota checks into purported server authorization.
- Do not disable TLS verification, attestation, or access controls.
- Do not log the three credential values, source Zwieback, its hash, audio, or user text. Errors must be redacted before storage.
- After access denial on a conforming request, favor an official/authorized access route and configuration analysis; do not launch authentication fuzzing.
