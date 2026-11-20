# Cross-platform desktop application — architecture recommendation

Date: November 13, 2026.

**Status: recommendation, not a migration decision or implementation commitment.**

The Windows product decisions of November 20 supersede the provider and
distribution options considered in this earlier cross-platform recommendation.
Sambler for Windows uses Rambler/Jetson exclusively and targets a per-user
installer and a portable ZIP from one build. [Windows architecture](../windows/07-architecture-application.md) · [Distribution](../windows/09-distribution-et-version-portable.md)

## 1. Goal

Evolve Sambler into a dictation application for **Windows first, then macOS and Linux**, with most code shared.

The envisioned uses are:

- resident application with a status indicator;
- microphone capture triggered from the interface or a global shortcut;
- recognition and editing through the Rambler engine;
- displaying, copying and, where possible, inserting the result into another application;
- installation and use without a Python environment to configure.

These uses are design goals, not capabilities already implemented or validated in the repository.

## 2. Recommendation

**Rust for the engine, Tauri 2 for the desktop application, TypeScript + React for the interface.**

| Layer | Recommended choice | Responsibility |
|---|---|---|
| Independent engine | Rust | Protocol, sessions, results, cancellation and resource limits |
| Concurrency and networking | Tokio + tonic, to be validated against the current protocol | Bidirectional gRPC transport and orchestration |
| Native audio | CPAL, explicit conversion, libopus and Ogg muxing | Microphone, format adaptation and progressive encoding |
| Desktop shell | Tauri 2 | Windows, native commands, distribution and interface integration |
| Interface | TypeScript + React, with Vite | Settings, dictation state, errors and optional history |
| System adapters | Rust and OS-specific APIs | Shortcuts, permissions, active application and insertion |
| Research and conformance | Existing Python | Fixtures, service exploration and implementation comparison |

The precise libraries remain to be confirmed by a prototype. The reconstructed protocol may require a suitable codec or unknown-field handling: do not assume standard protobuf generation will automatically preserve every useful behavior.

React is a practical ecosystem choice, not a fundamental constraint. Svelte would also be feasible. The structural choice is separation between native engine, system adapters and interface.

## 3. Why this stack

### A native engine suited to a resident application

Rust allows control over resources, buffers and task lifetimes. It suits a pipeline coordinating capture, encoding, network transmission, reception and cancellation.

Expected benefits primarily concern distribution, resource consumption and engine robustness. They must be measured; the language alone does not guarantee a lightweight or correct application.

### A productive interface without developing everything in Rust

TypeScript and React make screens easier to build and evolve. Tauri connects this interface to the native engine and uses system WebViews.

This avoids always bundling a complete Chromium distribution, but requires tests across WebViews and platforms. Final size will also depend on native libraries and packaging.

### A product without Python installed by the user

The final application can run without an embedded Python runtime. Python would remain useful in the repository for research and validation tools.

This choice does not necessarily imply a single standalone executable: codecs, system components and installation formats must be handled per platform.

## 4. What Rust does not solve

### No guaranteed recognition gain

Sambler is a client of a remote service. Rust will not speed up the model running on the server.

The Python engine already uses native components for gRPC transport and audio encoding. Existing measurements primarily identified audio pipeline buffering as a lever, not a demonstrated Python language bottleneck.

See [measurements and their limits](../performance.md).

### No automatic protocol fix

The port must preserve, among other things:

- useful protocol messages and fields;
- sending context at the right time;
- flushing the last samples;
- the distinction between normal stop and cancellation;
- late errors after receiving results;
- validation of application completion and gRPC completion;
- cautious interpretation of editing results.

Memory safety and typing do not guarantee these functional properties.

### No guarantee of service longevity

The internal Rambler service may still change or become inaccessible. This early study considered a replaceable provider interface. The subsequent Windows design isolates the Rambler SDK for testing and suspends dictation on loss of access. [Windows outage handling](../windows/06-confidentialite-et-continuite.md)

The technical choice does not authorize redistribution of access metadata. Access and distribution terms must be checked separately.

## 5. Proposed architecture

### Portable core independent of Tauri

- Session model and events.
- Protocol and transport.
- Encoding and result processing.
- Deadlines, cancellation and errors.
- Interface between the session and the recognition SDK.

The core must be exercisable from a diagnostic CLI and tests, without a graphical interface.

### System adapters

- Microphone acquisition and device changes.
- Permissions.
- Global shortcuts.
- Target application identification.
- Optional reading of selection and context.
- Insertion or replacement proposal.

These capabilities must be exposed explicitly: a function unavailable on one system must not be presented as universal.

### Tauri interface

The interface displays state and passes commands. Microphone, encoding, networking and insertion remain on the native side.

Exchanges with the interface must be limited to necessary commands and data. Credentials must not be exposed to the frontend; however, secure native storage does not make a shared secret inviolable on the client machine.

## 6. System-specific constraints

| System | Points to anticipate |
|---|---|
| Windows | Shortcuts, notification area, capture, elevated applications, keyboard simulation restrictions and accessibility |
| macOS | Microphone/accessibility permissions, signing, notarization and target hardware architectures |
| Linux | Distribution dependencies, desktop differences, X11 and Wayland, portals and compositor capabilities |

**The window is cross-platform; interaction with other applications is not automatically so.**

On Wayland in particular, do not promise universal insertion or context reading. Provide clipboard copying or manual copying when automatic integration is unavailable.

Each system needs its own builds and tests. Plan for signing and update verification; notarization is specific to macOS.

## 7. Insertion and editing: two distinct levels

### Simple insertion

Inserting a new dictation at the cursor is a reasonable first target. Possible mechanisms, such as the clipboard or keyboard simulation, have their own limits and side effects.

A solution temporarily using the clipboard should respect its contents as far as possible and handle concurrent changes, rather than blindly restore an old value.

### Modifying existing text

A Rambler result may contain an entire context window. It must not automatically be appended to existing text.

Preserve the context and revision checks already planned in the engine. Checking before writing does not necessarily make a change in an external application atomic.

**If ambiguous or context has changed: preview or copy, not automatic replacement.**

See [the result API and its limits](../refinement.md).

## 8. Alternatives considered

| Alternative | When preferable | Tradeoff for Sambler |
|---|---|---|
| C# + Avalonia | Strong .NET expertise or lasting Windows priority | Very credible; macOS/Linux integrations remain specific |
| Flutter + Dart | Consistent interface and potential mobile extension | Plugins and native code still needed for advanced system functions |
| Electron + TypeScript | Priority on rapid development of a rich interface | Bundled Chromium, generally larger distribution and native integrations to maintain |
| Python + PySide6 | Quickly add an interface to the existing engine | Runtime and native dependency packaging to maintain |
| C++ + Qt | Strong Qt experience and need for its mature ecosystem | Maintenance complexity and licensing/distribution terms to examine |
| Entirely Rust interface | Particular need or team experience | Not necessarily the best tradeoff for a polished and accessible interface |

There is no universally best language. With comparable skills, Rust/Tauri is recommended for the envisioned cross-platform dictation utility. Strong prior experience in another stack may change this choice.

## 9. Gradual transition

### Step 1 — Establish the conformance reference

Keep Python and its tests. Define behaviors to preserve, particularly protocol, Unicode, UTF-16 positions, ambiguous results, late errors and cancellation.

Fixtures and loopback tests should verify the port without constantly depending on the real service.

### Step 2 — Port a minimal slice to Rust

First validate: **audio file → service → confirmed final result**, from a CLI.

Compare behavior with Python before adding the microphone. Measure startup, RAM, CPU and latency with the same inputs, without attributing network variation to the language.

### Step 3 — Validate the Windows microphone pipeline

Build the complete chain: **shortcut → microphone → audio adaptation → Opus/Ogg → service → confirmed result → controlled insertion**.

Do not assume all devices directly produce mono 16 kHz PCM. Test available formats, resampling and device changes.

Include disconnection, sleep, network loss, immediate stop and active-window change scenarios. Do not automatically replay audio after an error.

### Step 4 — Build the interface and distribution

Add settings, microphone selection, status indicator, shortcuts, understandable errors and installer. Plan for permissions, signing and updates.

Do not retain audio or text history by default without an explicit need; treat this data as sensitive.

### Step 5 — Complete macOS and Linux

Plan system interfaces from the beginning and build/test early on all three OSes. Then finalize macOS adapters, followed by Linux with an explicit X11/Wayland capability matrix.

Do not wait until Windows development is complete to check dependency portability.

## 10. Independent opinion and conclusion

An Opus agent was asked for a recommendation with a neutral request, without being told the initially proposed choice. After examining the repository and consulting official documentation, it also recommended **Rust + Tauri 2 + TypeScript/React**.

The opinions converge on the independent engine, keeping Python as a reference, OS-specific adapters and no promise of latency gain from changing language alone.

This convergence is an architectural opinion, not a comparative benchmark or feasibility validation of every system integration.

**Conclusion: favor this stack for the envisioned product, but validate a small functional slice before committing to a complete rewrite.** Budget, available skills and exact first-version features remain to be specified.

## References

- [Project README](../../README.md)
- [Performance measurements](../performance.md)
- [Results and editing proposals](../refinement.md)
- [Tauri 2 documentation](https://v2.tauri.app/start/)
- [Windows SendInput restrictions](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [Linux RemoteDesktop portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
