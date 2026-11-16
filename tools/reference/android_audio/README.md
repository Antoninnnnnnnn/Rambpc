# Controlled Android audio prototype — development only

**Experimental, not a working dictation harness. Do not attach/arm this blindly.**
On the measured Android API34/arm64 device, installing constructor/read hooks in
an active PixelBoard IME caused an unarmed ART SIGSEGV. The source is retained for
reproducibility and investigation, **not qualified as a safe working injector**.
No controlled phrase has yet been injected; no Android/native parity is claimed.
[Measured controls and limitations](../../../docs/research/android-reference-validation.md).

These are original project sources, not third-party bridge code:

- `pcm-core.js`: bounded three-arm PCM engine, pacing, exclusive read leases,
  cancellation and finite EOF. Only preapproved hash/size pairs can be armed.
- `frida-agent.js`: exact APK/package/version/class pin, fail-closed unarmed
  constructor replacement and controlled reads. Its superclass-only constructor
  initialization is **not verified on the actual source with PCM**.
- `ui-dispatch.js`: normal, guarded IME start/stop dispatch to the inert editor;
  no direct backend calls. A dispatch acknowledgment is not a recognition result
  or proof of onboarding/quota/consent gate coverage.
- `CONTRACT.txt` and `UI-CONTRACT.txt`: original prototype contracts. Their stated
  behavior is a design contract, **not evidence of successful device execution**.
- Mock/unit tests: no device, network, credentials or real microphone.

The native Rust engine does not import or require these tools. No alternative
ASR or public Gemini API is involved.

## Offline tests

From the repository root, using an existing Node.js installation:

```sh
node --test tools/reference/android_audio/pcm-core.test.js tools/reference/android_audio/frida-agent.mock.test.js tools/reference/android_audio/ui-dispatch.test.js
python -I tools/reference/android_audio/test_assemble.py
```

These cannot establish ART/JNI compatibility or actual capture containment.

## Rebuild the SDK source locally — no attachment

An existing Frida17.17.0 installation supplies the measured Java bridge asset.
The assembler accepts only its exact SHA256, performs no installation/download,
and refuses to overwrite output or metadata. Python `-I` avoids importing from
untrusted input locations; the bridge is read as data, not executed on the host.

```sh
python -I tools/reference/android_audio/assemble.py --output tools/reference/android_audio/build/reference.js
```

`--bridge /trusted/installed/frida_tools/bridges/java.js` can select that same
pinned asset explicitly. Generated output contains third-party bridge code and
is ignored by git. Do not publish it, APKs, signing keys, `.env`, device identifiers,
private snapshots, audio/base64 or service/credential metadata.

No generic device/root/HTTP controller or automatic attach/arm command is shipped
here. The prior lab controller and its53 mocks remain a separate development
artifact, not a delivered portable harness. Future live work needs a reviewed
construction strategy, explicit target/editor validation, bounded cleanup and
independent `RECORD_AUDIO=ignore` containment until the app is force-stopped.
Never recover by enabling ambient capture, changing auth/TLS/safety gates,
replaying a session or bypassing the approved three-arm budget.
