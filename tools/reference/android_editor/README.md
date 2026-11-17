# Inert Android reference editor (development only)

Original test fixture, **not an Android dependency of the native engine** and not
a user-facing application. Use only on an explicitly authorized lab device with
controlled synthetic text/audio. It neither records audio nor recognizes speech.

- Package `org.rambpc.referenceprobe`; two exported activities, no intent filters,
  services, receivers, providers or permissions (including network/microphone/storage).
  The separate no-display audio-input inventory activity is described below.
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

The builder runs the standalone catalog and audio-inventory reducer/JSON host tests,
checks zero permissions (including SDK-qualified permission declarations), compiles
only the four explicitly listed original Java sources plus generated `R.java`, aligns
the APK, verifies signing and writes source/tool
hashes under ignored `build/`. It downloads nothing, installs nothing, and never
uses ADB. Run without Python `-O`, so manifest safety assertions remain enabled.
Generated classes/resources are disposable; the local signing identity is retained
across rebuilds. Keep the test-only `build/reference-test-only.p12` private and
out of git. Never use that disposable identity/password for production signing.
A new identity changes APK hashes; do not uninstall/erase existing apps to work
around signer conflicts. Binaries are not bit-for-bit reproducible across new
keys, timestamps or different toolchains.

## Isolated inventory-only APK (bounded passive device observation)

The original editor APK built successfully, but updating the installed
`org.rambpc.referenceprobe` failed with `INSTALL_FAILED_UPDATE_INCOMPATIBLE`.
No inventory activity was launched and no audio experiment was performed.
**Leave that installed app and its data intact.** Do not uninstall, wipe data,
search for another key, or attempt to re-sign it to bypass the conflict.

The separate `scripts/build_inventory.py` path uses the fixed new package
`org.rambpc.audioinventoryprobe`. It cannot update the original package. Its
`inventory-source/AndroidManifest.xml` allows exactly one exported, fully qualified
`org.rambpc.referenceprobe.AudioInputInventoryActivity`: `Theme.NoDisplay`,
`noHistory`, excluded from recents, and `stateAlwaysHidden`. There are zero
permissions, intent filters, services, receivers, providers or other components.
The differing Java namespace is intentional; a relative activity name would be wrong.

Only the existing original `AudioInputInventory.java` and
`AudioInputInventoryActivity.java` are compiled, unchanged, with an empty implicit
source path. No `ReferenceActivity`, `CaseCatalog`, resources or `R.java` are inputs.
All generated artifacts, verification text and `build-metadata.json` go to ignored
`build-inventory/`; the original `source/`, `build.py` and `build/` are not modified.
The **existing** `build/reference-test-only.p12` must be present or the builder
fails. It never creates, replaces, copies, hashes or searches for a signing key;
only the explicitly invoked signing tool later reads that exact file. Do not
run the original builder to generate a missing key as a workaround.

The installed `aapt2 link` help was reviewed and the isolated inventory APK was
built, signed, installed with authorized root, and launched once for passive
input enumeration on 2026-11-16. See the [bounded evidence](../../../docs/evidence/android-passive-input-inventory-20261116.json).
This does not qualify recording, hardware routing or backend behavior. Earlier
failed builds and installation restrictions remain historical observations.
For a future build, review the **installed** tool's help before passing the
confirmation flag; the script has no guessed no-resources option or resource
fallback. Build/signing and device operations still require authorization:

```sh
REFERENCE_ANDROID_SDK="$ANDROID_SDK_ROOT" python -I tools/reference/android_editor/scripts/build_inventory.py --resource-free-link-verified
```

The local SDK/JDK prerequisites and environment overrides are the same as above.
There are no downloads, network/device/ADB operations or key generation in this
builder. Source checks use explicit exceptions (also active with Python `-O`).
Both unsigned and signed APKs must pass a zero-permission dump allowlist and a
compiled-manifest component/attribute allowlist. Unknown dump formats fail closed
and require review, not relaxed checks. Host tests are separate, not an APK build:

```sh
python -I -B tools/reference/android_editor/scripts/test_build_inventory.py
```

These tests read only the builder, new manifest and their own test source, use
mocked missing-key checks, and write no generated outputs or signing files.

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

## Passive audio-input inventory (development only)

`AudioInputInventoryActivity` is a separate original exported activity with
`Theme.NoDisplay`. It finishes synchronously without creating views, showing an
IME, reading launch extras or delivering intents to/resetting the editor. Normal
Android lifecycle transitions may pause an already-open editor and trigger its
existing private snapshot callback; this activity never reads its text/files.
It only calls the public API23+ `AudioManager.getDevices(GET_DEVICES_INPUTS)`,
`AudioDeviceInfo.getType()` and `isSource()` (the APK still requires API26+).
It never creates `AudioRecord`, requests permissions, starts recording, changes
routing, or reads device names, addresses, IDs, serials or other apps' data.

After separately authorized build/signing and installation of the **isolated**
APK, an authorized operator may install it alongside the editor (never as an
editor update), then launch and read only the new app's fixed private file:

```sh
adb install tools/reference/android_editor/build-inventory/audio-inventory-probe.apk
adb shell am start -W -n org.rambpc.audioinventoryprobe/org.rambpc.referenceprobe.AudioInputInventoryActivity
adb exec-out run-as org.rambpc.audioinventoryprobe cat files/audio-input-inventory.json
```

These are documentation only, not permission to build/install/launch now. If the
new package also reports a signer conflict, stop; do not uninstall, wipe or re-sign
either package. Do not substitute the original package in these commands. The
new package has its own app sandbox; the Java class namespace does not determine
the `run-as` package. The editor package and its existing private files stay intact.

No extras, configurable paths, forced restart (`-S`), or editor case launch are
needed. This is documentation, not an automatic device/build/install step.
The activity atomically replaces only `files/audio-input-inventory.json` using
`AtomicFile`. Schema version1 contains a random UUID `run_id`,
`snapshot_time_unix_ms`, boolean `success`, `failure` (null or the fixed code
`UNSUPPORTED_API`, `AUDIO_MANAGER_UNAVAILABLE`, `DEVICE_QUERY_FAILED`),
`device_count`, and `type_counts`. Each count entry is
`{"type":15,"is_source":true,"count":1}`: a numeric Android type and the actual
source flag grouped together, sorted by type then source flag. There are no
individual device identities. Unknown/future numeric types are retained.
`recording_started=false` and `routing_proven=false` are always explicit.
An empty successful list is valid; query failure clears any partial counts.

Read the previous run ID before launch if a file exists, then require a new ID
and a fresh timestamp afterward. A successful `am start` is not evidence of a
successful inventory. Storage failure/process termination can leave an old file
or no file; there are no logs, alternate output paths or fallback probes. Query
errors contain only fixed codes, not raw exception text.

**Input presence is not routing proof.** Enumeration cannot show which input an
IME/backend would use, whether a reported source works, or whether any audio can
be captured. There is no ambient-microphone fallback, recording experiment,
speech recognition or backend qualification. Host tests prove only reducer/JSON
behavior, not Android enumeration, file atomicity, IME behavior or hardware routing.

To run just the pure Java tests without the SDK, APK build or signing, point the
output at a fresh scratch directory outside `build/` (JDK17+):

```sh
javac -encoding UTF-8 -d "$HOST_TEST_CLASSES" \
  tools/reference/android_editor/source/java/org/rambpc/referenceprobe/CaseCatalog.java \
  tools/reference/android_editor/source/java/org/rambpc/referenceprobe/AudioInputInventory.java \
  tools/reference/android_editor/scripts/CaseCatalogTest.java \
  tools/reference/android_editor/scripts/AudioInputInventoryTest.java
java -cp "$HOST_TEST_CLASSES" CaseCatalogTest
java -cp "$HOST_TEST_CLASSES" AudioInputInventoryTest
```

## Isolated public voice reference editor

The separate `org.rambpc.publicvoiceprobe` package has one exported activity,
`org.rambpc.referenceprobe.PublicVoiceReferenceActivity`, no intent filters or
permissions. It is distinct from the installed original editor; leave that app
and its data intact. The original editor sets `NO_PERSONALIZED_LEARNING`; an
incognito hat and grey microphone were observed there. This normal public-text
editor omits that flag to remove an ambiguity, not because it has been proven
responsible for those observations. It is not a privacy bypass, and normal text
mode gives no guarantee about provider learning. Use only public phrases. No
microphone is activated automatically; this fixture makes no recording or paired
audio claim.

The standalone builder reuses the original `CaseCatalog.java` and
`source/res/layout/main.xml`, alongside the isolated activity and generated
package-specific `R.java`. It needs the existing SDK and JDK described above and
`build/reference-test-only.p12`; it never creates, replaces, copies or searches
for a key. The original `build/` and `build-inventory/` remain untouched.
Output and verification metadata are confined to ignored `build-public-editor/`.
Unsigned and signed manifests are checked against fixed zero-permission,
one-activity allowlists; unknown dump formats fail closed. It performs no device
or network operations and downloads nothing. Build only when authorized:

```sh
REFERENCE_ANDROID_SDK="$ANDROID_SDK_ROOT" python -I tools/reference/android_editor/scripts/build_public_editor.py
```

The policy tests are pure and separate from building or device validation:

```sh
python -I -B tools/reference/android_editor/scripts/test_build_public_editor.py
python -I -B -O tools/reference/android_editor/scripts/test_build_public_editor.py
```

The public editor was built and installed in the authorized lab session, followed
by three supervised public voice cases on 2026-11-16. These were bounded UI
observations, not three semantic passes or wire/backend qualification. The
original keyboard and microphone restrictions were restored; the fixtures remain
installed. See the [observations and limits](../../../docs/evidence/android-supervised-public-voice-20261116.json).

### Snapshot and build-output caveats

`latest.json` describes the most recent watcher/callback/lifecycle capture, not
necessarily the live editor at the instant the file is read. Selection-only
changes and `finishComposingText()` after a text update do not themselves trigger
the TextWatcher. Selection/composition offsets may therefore remain stale until
an explicit capture or another snapshot-producing event. Fresh run/sequence
checks do not establish that every offset is current.

The isolated builders sign directly to their documented APK output paths before
final verification. A failed build can leave an unqualified APK there, possibly
beside older metadata. Do not install or reuse those outputs after a failure;
require the successful verification record and matching APK/source hashes from
that build. File existence alone is not qualification.
