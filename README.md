# Sambler

**Voice dictation for Windows.** Hold a shortcut, speak, let go: your text
appears where you were typing, in any application (Word, Outlook, Edge,
Chrome, VS Code…). Speech recognition comes from Google's Rambler/Jetson
service. The interface is in French.

**[Download the latest version](https://github.com/Antoninnnnnnnn/Sambler/releases/latest)**
· Free and open source ([MIT](LICENSE.md))

## Download and install

| File | Use it for |
| --- | --- |
| `Sambler-…-windows-x64-setup.exe` | Installing Sambler for your Windows account. No administrator rights needed. |
| `Sambler-…-windows-x64-portable.zip` | Running Sambler without installing it: extract the `Sambler` folder and open `Sambler.exe`. Its settings stay in the `Data\` folder next to it. |

You need Windows 11 x64 (with WebView2, built into Windows 11) and an Internet
connection. There is nothing to configure: Sambler includes its own access to
the speech service.

The files are not code-signed yet, so SmartScreen may warn you. Click « Informations
complémentaires », then « Exécuter quand même ». You can check each file against
`SHA256SUMS.txt` in the release.

To update, run the new installer. It replaces the previous version in place,
keeps your settings, and closes Sambler if it is open.

## How to use it

1. **The first time**, Sambler asks you to turn on dictation (nothing is sent
   before that), to choose a microphone, and to try a phrase.
2. **Dictate anywhere**: click in a text field, hold **Ctrl+Shift+F2**, speak,
   then let go. The text is inserted at the cursor.
   You can switch to a press-to-start / press-to-stop mode, or change the
   shortcut, in the settings (« Réglages »).
3. **Cancel** with **Escape** while you speak.
4. If the text cannot be inserted (the field changed, or the application does
   not accept it), it is kept in **Recents** (« Récents ») in the Sambler
   window, ready to copy.

You can also dictate directly into Sambler (« Dicter dans Sambler »), without a
shortcut.

## Features

- **Instant insertion**: the text is pasted in one go and your clipboard is
  restored afterwards. A character-by-character typing mode is also available.
- **French and English**, or both in the same sentence (automatic by default),
  with French typography: non-breaking spaces and « » quotation marks.
- **Clean text**: punctuation and capital letters, spoken layout commands
  (« à la ligne », « nouveau paragraphe »…), a personal dictionary, voice
  shortcuts, and a different style per application.
- **Notebook** (« Carnet »): dictated notes, and transcription of audio files
  (WAV, Ogg Opus, MP3, M4A/AAC, WMA, FLAC, up to 2 hours).
- **Recents**, an optional encrypted **history**, and **activity**
  statistics: words per day, streaks, words per application.
- Two themes, « Verre et aurore » and « Doux et rond », each in light or dark,
  and a floating bar that never takes the focus away from what you are typing.

## Privacy

- Your voice is sent to Google's service **only while you dictate**, and only
  after you have turned dictation on. No audio is ever kept.
- Recents stay in memory for a few minutes and are erased when Windows locks.
  The history is off by default; when turned on, it is encrypted for your
  Windows account.
- Text copied by Sambler is excluded from the clipboard history (Win+V).
- Everything stays on your PC: in `%LOCALAPPDATA%\org.sambler.app` when
  installed, or in the `Data\` folder of the portable version.

**About the built-in access.** The access key to the speech service is
included in the application and in this repository, so it is readable by
anyone. If Google revokes or limits it, dictation stops for everyone; you can
then enter another access in Réglages › Accès à Rambler, without reinstalling.
`RAMBLER_*` environment variables take precedence over the built-in access.

## Known limitations

- Brave, right after switching to it, may not expose its text field in time;
  the text then goes to Recents. Starting Brave with
  `--force-renderer-accessibility` avoids it.
- Outlook (classic and new) has not been tested yet.
- For long dictations, the service sometimes adds formatting (`*` bullets),
  which Sambler keeps as is.
- Sambler works on Windows only for now. The engine also builds on Linux, macOS,
  Android and iOS (see below).

Release notes for each version are in [docs/windows](docs/windows/) (in French).

## Build from source

You need Rust (MSVC, 1.90 or later), Node 24, the Visual Studio C++ build tools,
and CMake. From the `app/` folder:

| To | Run |
| --- | --- |
| Start in development mode | `powershell -ExecutionPolicy Bypass -File scripts\dev.ps1` |
| Check the interface | `npm ci`, `npm run typecheck`, `npm test`, `npm run build` |
| Run the native tests | `cd src-tauri`, then `cargo test --locked` |
| Build the installer and the portable ZIP | `powershell -ExecutionPolicy Bypass -File scripts\package.ps1` (output in `dist-windows/`) |

More in the [application guide](app/README.md) and the
[Windows design dossier](docs/windows/README.md) (in French).

## What is in this repository

| Folder | Contents |
| --- | --- |
| [`app/`](app/README.md) | The Windows application: Tauri 2, with a Rust host (`app/src-tauri/`) and a React/TypeScript interface (`app/src/`). |
| `crates/` | The Rust dictation engine and SDK (`rambler-engine`, `rambler-audio`, `rambler-transport`, `rambler-protocol`) and the `rambler` command-line tool. |
| `src/rambler/` | The Python 0.3 reference implementation. |
| [`docs/`](docs/) | Application dossier, protocol research, test evidence, status and roadmap. |

Developers who want to use the engine directly (SDK, command line, Python):
see **[the engine and SDK guide](docs/engine.md)**.

## Credits

Sambler started from **[PixelBoard](https://github.com/Akshayykadam/PixelBoard)**,
an independent Android keyboard by Akshayykadam that uses Rambler for voice
typing. Sambler's engine, and the way it talks to the Rambler/Jetson service,
were rebuilt from PixelBoard 1.3.4, then compared with PixelBoard 1.3.5.
Thank you for that work.

Sambler also relies on [Tauri](https://tauri.app/), [React](https://react.dev/),
the [Opus codec](https://opus-codec.org/), and the Figtree, Fraunces, Geist
Mono, Hanken Grotesk, Mona Sans and DM Mono fonts (via Fontsource), each under
its own license.

Sambler is not affiliated with Google or with PixelBoard.

## License

Sambler's own code and documentation are under the [MIT License](LICENSE.md).
Third-party components keep their own licenses and notices, including the
[audio dependency notices](crates/rambler-audio/THIRD_PARTY_LICENSES). Access
to the hosted Rambler/Jetson service is governed separately.

To contribute, see [CONTRIBUTING](CONTRIBUTING.md). Apart from the built-in
service access, keep keys, tokens, personal audio, raw captures, APK files and
decompiled sources out of the repository.
