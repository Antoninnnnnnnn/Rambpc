# Android development reference: validation status

## Role and limits

This laboratory phone is used **only as a client reference**, with explicit authorization for root installation and isolated tests. The Rust engine continues to communicate directly with Rambler without Android, a phone or relay during use. This tool adds no substitute ASR backend.

The status below qualifies preparation and diagnostics, **not Android dictation or an additional service intent**. The three planned sentences have not yet been armed/injected in the observations published here.

## Controlled preparation

- Official PixelBoard release 1.3.4, APK SHA256 `a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`, GitHub digest and signature verified before installation. Its manifest version is `18.6.3.1002251336-release-arm64-v8a`, distinct from the release tag.
- Package `com.akshaykadam.pixelboard`, installed alongside Gboard through the normal root package manager: no deletion of Gboard/data, no change to Magisk, signature, authentication or attestation.
- Android API 34 / arm64-v8a. No serial number, account identifier, token, API key or authentication certificate value published.
- Rambler preference and Jetson selection read as true, without modifying them. English(US) was added only to the PixelBoard profile. The first two SDK diagnostics retained the original keyboard; their framework subtype is therefore **not** a measurement of a Rambler voice session's language.

## Reproducible inert editor

The [original fixture and its builder](../../tools/reference/android_editor/README.md) has zero permissions (including network, microphone and storage). It reads only its own editor; SEND increments a local counter without sending or clearing anything. Private snapshots carry a run ID, sequence, UTF-16 selections and counters to reject stale captures. No arbitrary text intent.

The initial APK `c7e507…b912` passed seven device checks: fixed cases, whole selection, final cursor, simple restart/invalid ID without reset, new runs and zero send callbacks. The new APK `354e1c4363e399ea1978d21544983843b5a423eb6cee8f11f171b641960fb8fe` passed the same checks, then a controlled **non-voice** input `X` verified the TextWatcher, selection at the end of the UI turn and reset without stale writes. It keeps only its foreground window awake, without a global setting. The old measured APK was retained; this evidence is not reattributed between versions.

An `adb push` transfer to a private root directory was initially refused; transfer through shell/tee then failed SHA verification before installation. Binary `adb push` transfer to shell staging, then **root** installation with SHA check, succeeded without bypassing normal signature verification. On this device, nested `su -c run-as …` reading returned empty output; direct `adb exec-out run-as` reading of the fixture's files alone works.

## Initial preflight: failure/inconclusive retained

[Public projection](../evidence/android-reference-preflight1-public.json). The package, APK, class signatures and installation of three targeted hooks passed checks. But the session was no longer available at the first subsequent read-only RPC (`InvalidOperationError`). Only `loaded` and `ready` were observed: **0 arm, 0 capture constructor, 0 PCM EOF**.

The cause remains unknown: the Android `UNKNOWN` observation does not demonstrate a native crash. Settings and IME declare the same default process; the manifest does not support the hypothesis of a distinct wrong Settings process. The initial controller did not capture detachment and its RPC error prevented HTTP shutdown. The initial keyboard, microphone permission and AppOps were restored; the test-owned server and its forward were removed before the host controller stopped.

## Separate diagnostics: limited successes

[Two public observations](../evidence/android-reference-sdk-diagnostics-public.json):

1. Read-only SDK, no hooks: package/Java, Rambler/Jetson preference and subtype getter respond; no unexpected detachment for 20 s.
2. Installation of three hooks, **no arm**: same immediate read available, no constructor called/refused, no unexpected detachment for 20 s.

Each diagnostic restored the initial state and removed its server/forward. These successes **do not retroactively resolve** the first failure. ART construction with initialization of only the InputStream superclass was not exercised. The hardened controller passed 53 mocked host tests (detachment, shutdown despite blocked/failed RPC, timeouts, privacy, persistent cap and bounded UI commands), without device validation attributed to these tests. Diagnostics/fixtures are Sol work; the additional static analysis is Kimi, not an Opus review.

## Hardened active preflight and discriminating checks without audio

A second preflight, with PixelBoard selected as IME in front of the inert editor, reaches `ready` at 2.282 s then observes `script.destroyed` and a `process-terminated` detachment with crash object at 2.810 s. The controller automatically restores the device and exits with an error: **0 arm, 0 voice start, 0 constructor, 0 EOF**. Filtered inspection of the Android log for the target PID alone identified SIGSEGV; the raw log was not retained. The signal was not populated in the published lifecycle object: this distinction between sources remains explicit. This proves neither the cause of the first failure nor that a `script.destroyed` event causes the crash.

Separate studies, in front of the same editor and without audio, then pass:

- Active IME **without Frida**, microphone permission revoked and AppOp `ignore`: same process alive after 20 s; restoration successful.
- Active IME, microphone permission granted but AppOp `ignore`, read-only SDK without method replacement: Rambler/Jetson getters true, same process alive after 20 s; detachment requested only during our cleanup.
- Same containment, creation of an inert laboratory class in the process, then replacement of its only method, which is never called: no PixelBoard method replacement, same process alive after 20 s, zero marker calls.
- Same containment, replacement of only `aaqe.read(byte[],int,int)` with a refusal without reading: same process alive after 20 s, zero calls, no constructor replaced or exercised.

A fifth check adds only the no-argument constructor to the read hook, both replaced with a refusal: SIGSEGV after 0.426 s. The crash object permits retention only of module `libart.so` and relative PC `0x270fd8`, without raw report, memory, registers, path, ID or symbol. Restoration succeeds. Handler-entry counters are lost on process death: this check does not prove zero replacement invocations. System module ELF initialization was read as data for a symbol search; exported symbols do not resolve this PC. No code from this library was executed on the host.

[Isolation evidence](../evidence/android-reference-active-ime-isolation-public.json). These checks narrow the hypotheses but do not yet establish the cause of the three-hook crash. Minimal script bodies and scopes differ from the full preflight; 20 s success is not proof of general stability. Additional Sol static review finds no signature mismatch and notes that internal ART bridge mechanisms have a broader scope than the three replaced application methods. It is not an Opus review.

## Approaches rejected before any new audio

- Retaining the original constructor and neutralizing `AudioRecord.startRecording` is not viable with a true STOPPED state: `aaqe` immediately requires state 3 then throws an IOException without deterministic release on this failure. The proposal is rejected **before implementation and execution**; no simulated state or modified postcondition. Two independent static readings agree.
- `kmy.l` is explicitly `connectOnly`, not an external audio selector. `kmy.audioInputStream` is fixed to null in this build. `knt.l` supplies only `new aaqe()` or null; `iho.j(null, …)` initializes then terminates networking without audio upload. This branch also modifies audio focus handling and other paths: it is neither activated nor presented as external injection. Source: `kmy.java:11,27–42,65–68`, `kmw.java:25–47,90–93`, `kla.java:183–257`, `knt.java:318–344`, `iho.java:250–308` from the same APK.
- Additional Kimi static review finds no normal seam for supplying an external `InputStream`: the session field remains fixed to null, the builder has no setter and `kkv.java:10–13` reads then discards this value. The generic parameter of `iho.j` is not exposed configuration: its S3 caller supplies only `new aaqe()` or null. Jetson Lite (`fom.java:68–89`) and on-device (`kme.java:52–69`) variants also construct `aaqe`; factory selection (`kki.java:63–111`) demonstrates no external input. No device action. `klb.g`, `kjo.run` and `kml.a` remain undecompiled: this finding is not exhaustive instruction-by-instruction proof, nor justification for bypassing these paths or rerunning constructor hooks.

The [original audio/UI prototype sources](../../tools/reference/android_audio/README.md) are retained with 29 passing offline Node tests and a local assembler that passes 5 Python tests. Its actual build passes with the installed bridge exactly hashed, without device action. The assembler does not provide a qualified dictation controller; its new composed SDK is not reattributed to past trials. The retained audio source fragment equals that of the observed preflight SDK. Third-party bridge code, binaries/APKs, keys and private files are not distributed.

## Targeted DEX inspection: scope and flag correction — 15 November 2026

A targeted static extraction of the same APK inspected the DEX structure of `aaqe` and rechecked targeted lifecycle metadata. Source decompilation of `klb.g`, `kjo.run` and `kml.a` remains incomplete: this limitation is no longer a lack of structural DEX inspection, but does not become exhaustive instruction analysis. Two constructors and two `read` overloads are declared; previous checks targeted only the no-argument constructor. This establishes neither a usable external audio path nor the cause of the native crash, still unobserved.

The intermediate reader had mislabeled the bits: `0x10000` is `ACC_CONSTRUCTOR`, not declared-synchronized; `0x20000` is `ACC_DECLARED_SYNCHRONIZED`, not unknown. With `ACC_PUBLIC=0x1`, flags `0x10001` therefore mean public + constructor. The declared-synchronized flag concerns declaration/reflection and has no execution effect by itself, according to the [official DEX reference](https://source.android.com/docs/core/runtime/dex-format#access-flags). Corrected extraction R2 succeeded; erroneous intermediate reports are retained as such, their interpretations removed. Exact comparison of initial and corrected outputs confirms that only three flag-label lists change: numeric values and code hashes are identical. The eight code fingerprints match the parser; no DEX data is modified. Private outputs do not constitute reproducible evidence archived in the repository.

No AOT/JIT/interpreter state or presence on stack is observed by this metadata. A small toy laboratory remains **NO-GO**: it would not discriminate the actual state of the relevant runtime. No new device, audio or service action: **0 PCM arming, 0 voice start, 0 sentences**; the three-sentence budget remains intact. No parity or real-service capability is established; no bypass is authorized.

## Next bounded trial, without anticipating a result

**Blocked at this stage on compatible and contained ART instrumented construction.** No new arming is justified by current checks. Another method requires its own review and audio-free preflight; do not bypass this failure by starting the real microphone, falsifying a state or directly calling a backend instead of the normal chain.

At most three local synthetic sentences, PCM16LE mono 16 kHz, hashes fixed before arming, no automatic retry. Audio replacement is limited to the exact PixelBoard client and `aaqe`; no auth/TLS, safety, response or routing changes. AppOp `RECORD_AUDIO=ignore` must remain enforced before attachment, during reading and until force-stop before restoration, to prevent unintended ambient capture. A missing expected hook, ART/Java error, hash/EOF mismatch or loss of containment ends the trial, without switching to another engine or the real microphone.

Normal keyboard dispatch was established statically: `kla.g("jetson", consumer)` to the active LatinIME instance, `dG(oak)` on the UI thread; STOP uses the same `oak.d(kla.c)` as `kla.h`. DEX confirms `Consumer.accept(Object):void`; JADX's `e(...)` is an artifact. This **does not qualify** undecompiled branches `ffh.m`/`klb.g`, onboarding/quota `ffh.z/y`, or executor `kla.n`. No flag or gate is modified. Any parity proposal must retain these limits.

## Isolated passive inventory of Android inputs — 16 November 2026

[Public inventory projection](../evidence/android-passive-input-inventory-20261116.json). This is a single enumeration on Android API 34, not validation of routing, dictation or a matched reference. The separate fixture `org.rambpc.audioinventoryprobe` starts a non-displayed Activity that queries `AudioManager.getDevices(GET_DEVICES_INPUTS)` and aggregates only type and `isSource`. It atomically writes a private result with freshness markers; an old output must be rejected if writing fails. No product name, address, ID or exception text is read in this inventory. The public projection contains neither private markers nor device identifiers.

The initial local build of the enhanced editor succeeded with zero permissions, without device execution. Its update was then refused with `INSTALL_FAILED_UPDATE_INCOMPATIBLE`: no uninstall or data deletion. For the isolated package, the first two local builds failed before signing on parsing namespace and aapt2 attribute format; the parser was corrected based on a real public manifest dump and mutation tests. The third build succeeded with 16 pure Python tests, zero permissions and a single Activity. Individual source fingerprints and the APK SHA-256 are in the projection; they do not mean these uncommitted sources are already integrated into the repository.

A normal fresh installation of the isolated package failed with `INSTALL_FAILED_USER_RESTRICTED`. After the user's explicit request « installe avec root », an authorized root installation succeeded, limited to this APK, without global or policy change. A single inventory Activity was then launched; result freshness was verified. The original editor package and selected IME remained unchanged.

Five inputs, all declared sources, were inventoried: type 15 twice, types 16, 18 and 25 once each. The [official AudioDeviceInfo reference](https://developer.android.com/reference/android/media/AudioDeviceInfo) names these constants respectively built-in microphone, FM tuner, telephony and remote submix. No USB type 11, 12 or 22 or wired headset type 3 appears in this observation. These are static API categories, **not proof that a stream is routed or received**; this observation establishes neither universal hardware incapability nor impossibility of software injection.

No recording, backend RPC or actual matched-reference trial; microphone permission absent and `routing_proven=false`. A future matched trial requires a legitimate controlled input route, verified end to end: a mere list, stale file or silence does not prove that route. No fallback to a root modification of AudioPolicy or the provider, hooks, state alteration or PCM capture is proposed here.

## Supervised public voice in a separate editor — 16 November 2026

[Public projection and build fingerprints](../evidence/android-supervised-public-voice-20261116.json). This later observation is separate from the instrumented injection plan and passive inventory above: historical mentions of “no audio” and the blocking of that plan remain valid for their respective trials, without describing these new manual trials. Earlier failures, signature constraints and `INSTALL_FAILED_USER_RESTRICTED` are neither erased nor retroactively resolved.

The user selected PixelBoard and its starred microphone through the normal interface, without hooks or alteration, then personally held, started and stopped voice using the built-in microphone; no automated fallback to ambient audio. The separate ordinary editor belongs to the laboratory, requests no permissions and does not force incognito. Removing its own `NO_PERSONALIZED_LEARNING` flag resolved an incognito UI ambiguity; it is not established as the sole cause and promises nothing about provider learning. The original fixture and inventory remain preserved. The new APK and its build sources are pinned by SHA-256 in the projection, without claiming to integrate uncommitted sources.

1. After checking a repeated initial prompt, the planned sentence « The draft is ready. » yields `The draft is ready`: initial prefix not retained and punctuation absent. The cause is not isolated; this is neither proof of context fidelity nor an established native or Android bug.
2. With `Send me the report now.` fully selected, « Rewrite politely. » yields literally `Rewrite politely`: no polite rewrite in this trial.
3. On an empty field, the user first says « Send » and perceives no visible effect; they then report having tried « salut, ça va » and « send the message » and seeing it work. The only final snapshot indicates two SEND callbacks, two editor actions total, last action ID 4, text length 28 UTF-16 units and composition -1/-1. The French text obtained is retained by the projection. No intermediate snapshot allows each callback to be attributed to a sentence: neither failure of « Send » alone, general success of longer commands nor causality is established. SEND is consumed locally and sends nothing.

These observations do not constitute three semantic successes and do not count distinct ASR or RPC calls. No network or wire traffic was captured: neither S3Bidi/ASR route identity, complete Android parity nor backend terminal state is proven. The native Rust backend remains Rambler, without ASR substitution. Gboard and initial microphone restrictions were restored, the owned test window placed in the background; fixtures and captures are retained. The public projection excludes identifiers and private captures.
