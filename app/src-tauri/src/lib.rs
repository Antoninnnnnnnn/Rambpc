//! Sambler for Windows: dictation at the cursor with Rambler/Jetson.
//!
//! Module ownership (see docs/windows/07-architecture-application.md):
//! - `model`, `ports`, `ipc`, `settings`: shared contracts.
//! - `controller`: take state machine and its single task.
//! - `rambler`: the SDK adapter (the only speech engine).
//! - `text`: deterministic Naturel/Fidèle rendering, lexicon, expressions.
//! - `platform`: Windows capture, target, insertion, clipboard, DPAPI, events.
//! - `storage`: data directory, settings file, protected values.
//! - `shell`: Tauri windows, commands, shortcuts, tray and indicator.

pub mod controller;
pub mod ipc;
pub mod model;
pub mod platform;
pub mod ports;
pub mod rambler;
pub mod settings;
pub mod shell;
pub mod storage;
pub mod text;

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running Sambler");
}
