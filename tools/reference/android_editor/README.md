# Inert Android reference editor (development only)

Original test fixture, **not an Android dependency of the native engine** and not
a user-facing application. Use only on an explicitly authorized lab device with
controlled synthetic text/audio. It neither records audio nor recognizes speech.

- Package `org.rambpc.referenceprobe`; one exported activity, no intent filters,
  services, receivers, providers or permissions (including network/microphone/storage).
- Three fixed integer case IDs, never arbitrary intent text: repeated-context
  dictation, fully selected polite rewrite, and empty-editor `Send.`.
- `IME_ACTION_SEND` is consumed locally: only counters/snapshots change; nothing is
  sent and the editor is not cleared. No personalized learning or autofill.
- Own foreground window keeps the screen on; no global power/keyboard settings changed.
- Own TextWatcher writes only latest private editor state, deferred to the end of
  the UI turn. Reset suppresses stale writes and establishes a fresh run ID.
- Snapshots include own text, UTF-16 selection/composition and callback counters.
  Treat them as private. Never use personal input; never publish raw captures
  without review. A run ID/sequence check is necessary to reject stale files.

## Build without Gradle or downloads

Prerequisites: Python3.11+, JDK17+, an **existing** Android SDK with platform
`android-37.0` and build-tools `36.0.0`. Other compatible installed versions may
be selected explicitly with `REFERENCE_PLATFORM` / `REFERENCE_BUILD_TOOLS`.
Windows was tested; non-Windows executable-name selection is implemented, not
runtime-validated. Target API34 / minimum runtime API26.

```sh
REFERENCE_ANDROID_SDK="$ANDROID_SDK_ROOT" python -I tools/reference/android_editor/scripts/build.py
```

The builder runs the standalone catalog tests, checks zero permissions, compiles
only this original source, aligns the APK, verifies signing and writes source/tool
hashes under ignored `build/`. It downloads nothing, installs nothing, and never
uses ADB. Run without Python `-O`, so manifest safety assertions remain enabled.
Generated classes/resources are disposable; the local signing identity is retained
across rebuilds. Keep the test-only `build/reference-test-only.p12` private and
out of git. Never use that disposable identity/password for production signing.
A new identity changes APK hashes; do not uninstall/erase existing apps to work
around signer conflicts. Binaries are not bit-for-bit reproducible across new
keys, timestamps or different toolchains.

## Authorized-device contract

Installation, keyboard changes, microphone/network use and root require separate
explicit authorization. The following example only launches **this fixture**;
it does not activate voice input:

```sh
adb shell am start -W -n org.rambpc.referenceprobe/.ReferenceActivity --ei case_id 1
adb exec-out run-as org.rambpc.referenceprobe cat files/case-1-latest.json
```

Use integer IDs1–3. Plain relaunch preserves an existing case; explicit valid IDs
reset it. Invalid IDs preserve an already-running case. Initial launch without
an ID defaults to1. Stable accessibility descriptions are `reference_editor`,
`reference_case_1/2/3`, `reference_reset`, `reference_capture`, `reference_status`.
Explicit capture preserves `files/case-N-capture.json`; watcher/callback/lifecycle
updates go to `files/case-N-latest.json`. On the tested device, nested
`su -c run-as …` yielded an empty result; direct app-scoped `adb exec-out run-as`
worked. Root is unnecessary for reading this debuggable fixture's own files.

No Android dictation or backend intent is qualified by the editor smoke alone.
[Reference evidence and limitations](../../../docs/research/android-reference-validation.md).
