# Getting started with Sambler applications

Use `app/` for application interfaces. Each project uses the same Rust
dictation engine in `crates/`. Run the command-line tool and the tests from
the repository root.

For the Windows application, start with the [product and engineering dossier
(in French)](../docs/windows/README.md). The first [implementation milestone](../docs/windows/08-plan-de-construction.md)
connects a real microphone, the existing Rambler SDK and an identified field
in another app. Loopback servers supply controlled responses in tests.
The proposed app has its own Cargo workspace under `app/src-tauri/` and ships
the same dictation features in a per-user installer and a portable ZIP.
See the [application architecture](../docs/windows/07-architecture-application.md)
and [Windows packaging guide](../docs/windows/09-distribution-et-version-portable.md).

## Sambler for Windows (this directory)

Tauri 2 host in Rust (`src-tauri/`, standalone Cargo workspace, SDK crates
linked by path) with a React/TypeScript interface (`src/`). Rambler/Jetson is
the only speech engine; the shipped binary contains no simulated recognizer.

| Need | Command (from `app/`) |
| --- | --- |
| Run in development | `powershell -ExecutionPolicy Bypass -File scripts\dev.ps1` (loads `RAMBLER_*` from the repository's local `.env` into that process only) |
| Interface checks | `npm ci`, `npm run typecheck`, `npm test`, `npm run build` |
| Native tests (offline) | `cd src-tauri` then `cargo test --locked` |
| Installer + portable ZIP + SHA-256 | `powershell -ExecutionPolicy Bypass -File scripts\package.ps1` → `../dist-windows/` |

Prerequisites: Rust (MSVC, 1.90+), Node 24, Visual Studio C++ tools and CMake
for the bundled Opus build (see [BUILDING](../crates/rambler-audio/BUILDING.md)).
End users need only Windows 11 x64 with the WebView2 Evergreen runtime.

Access to Rambler is configured by the person, never built in: the three
`RAMBLER_*` values (and optionally `RAMBLER_AUTHORITY`, otherwise the SDK
default) come from the environment or are entered in the app. The installed
edition can remember them with DPAPI; the portable edition keeps them in
memory only.

Module map (`src-tauri/src/`): `controller/` (take state machine, single
Tokio task), `rambler/` (SDK adapter, capture handoff, watchdog), `text/`
(Naturel/Fidèle rendering, lexicon, expressions), `platform/windows/`
(WASAPI capture, UI Automation target, insertion, private clipboard, DPAPI,
session events), `storage/` and `history/` (data root, settings, protected
rules, encrypted SQLite history), `shell/` (Tauri commands, global shortcut,
tray, native capsule). Shared contracts: `model.rs`, `ports.rs`, `ipc.rs`
(mirrored in `src/ipc/contract.ts`), `settings.rs`.

Acceptance harnesses (not part of the app; they call the real service and
drive a fresh Notepad window):

```text
cd src-tauri
cargo run --example recette_bloc_notes -- <wav> <fr|en> <hold|toggle|switch|cancel|all> [mic]
cargo run --example recette_app -- <wav> <hold|toggle|toggle-escape|silence|switch>
```

`recette_bloc_notes` drives the real controller with a WAV as microphone (or
the real microphone with `mic`, playing the WAV on the speakers);
`recette_app` presses the real global shortcut against a running Sambler.
Offline platform harnesses: `target_harness`, `notepad_harness`, `mic_probe`.

Start with one project in `app/` if the platforms share an interface. A single
project can produce builds for multiple operating systems. If you develop
independent interfaces, create a subdirectory under `app/` for each project and
declare the shared Rust crates as dependencies.

## Test the engine

Install [Rust, CMake, and a native compiler](../crates/rambler-audio/BUILDING.md).
From the repository root, run this test with a generated audio file and a
simulated server on your machine.

```sh
cargo fetch --locked
cargo test --locked --offline -p rambler-cli --features audio --example minimal_app
```

## Connect your application

An application can also share the SDK workspace. If its `Cargo.toml` is in
`app/`, add `app` to `members` in the root `Cargo.toml` and specify these
dependencies.

```toml
[dependencies]
rambler-engine = { path = "../crates/rambler-engine" }
rambler-transport = { path = "../crates/rambler-transport" }
rambler-audio = { path = "../crates/rambler-audio" }
bytes.workspace = true
tokio.workspace = true
```

With this shared-workspace layout, a manifest one level deeper uses paths
starting with `../../crates/`. Add its directory to the workspace members.

For a standalone SDK exercise, follow **Start → Finish → Show text → Copy**
with a WAV file as in the [Rust example](../crates/rambler-cli/examples/minimal_app.rs).
For the Windows product's first milestone, feed the microphone directly.
The application supplies signed 16-bit mono PCM
at 16 or 48 kHz, and the engine encodes it as Opus. Close the audio source
when the user finishes dictating. Handle cancellation, errors, and live
previews separately as described in the [SDK guide](../docs/research/sdk-integration-guide.md).

## What GitHub checks

| Platform | Engine checks |
| --- | --- |
| Windows, Linux, and macOS | Offline Rust tests and a CLI build with Opus. macOS coverage includes Intel and Apple Silicon. |
| Android | ARM64 engine build with Opus and a 16 KiB alignment check on the example binary. See [Android integration](../crates/rambler-transport/ANDROID_TLS.md). |
| iOS | iPhone build and local tests on an Apple Silicon simulator, including rejection of an untrusted certificate. |

Test microphone capture, interruptions, and insertion into another application
on physical devices once the app exists. See the
[Rust workflow](../.github/workflows/rust.yml).
