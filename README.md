# Sambler — cross-platform dictation engine

Sambler is a dictation and text editing engine reconstructed from PixelBoard
1.3.4. It communicates directly with the **Rambler/Jetson service** over gRPC.
It runs without Android, a phone, an emulator, or the public Gemini API. The
application handles the interface, microphone capture, and text insertion.

- The **Rust core** includes the SDK (`rambler-*` crates) and the `rambler` CLI
  for file input. Use it as the foundation for an application.
- **Python 0.3** serves as the conformance reference. The `sambler-engine`
  distribution provides the `rambler` module.

The [Windows application dossier (in French)](docs/windows/README.md) describes
a free Windows 11 x64 app that uses Rambler/Jetson for all speech recognition.
The planned release includes a per-user installer and a portable ZIP.
See the [product settings](docs/windows/03-produit-et-priorites.md),
[acceptance criteria](docs/windows/08-plan-de-construction.md) and
[packaging guide](docs/windows/09-distribution-et-version-portable.md).

Original Sambler code and documentation are available under the
[MIT License](LICENSE.md). Third-party components retain their own licenses
and copyright notices, including the [audio dependency notices](crates/rambler-audio/THIRD_PARTY_LICENSES).
Access to the hosted Rambler/Jetson service is governed separately.

## At a glance

| Area | Status | Details |
|---|---|---|
| Rust core, SDK, and file-based CLI | Offline tests on Windows, Linux, and macOS. Cross-compilation for Android ARM64 and iPhone, with tests on an iOS simulator. | [Getting started](app/README.md), [Rust guide](docs/rust.md) |
| Live service | Responses observed on small synthetic and natural speech datasets from November 12 to 18, 2026. | [Observations](#live-service-observations) |
| Editing | Prepare a replacement, check its context, then validate it before applying it. | [SDK guide](docs/research/sdk-integration-guide.md) |
| PixelBoard parity | Partial. Advanced modes need further validation. | [Parity matrix](docs/research/rust-feature-parity.md) |
| Performance | Speed improvements measured locally. | [Measurements](docs/performance.md) |

## Rust quick start

Install Rust and Cargo 1.88 (the validated version) and the platform's native
compiler. On Windows, use MSVC. The build uses a C compiler for `ring` and
libopus, and the audio feature also requires CMake. See the
[build and test instructions](docs/rust.md#build-and-test-offline).

```sh
# Offline tests without a microphone or live service
cargo test --workspace --all-features --locked

# CLI binary with optional WAV/PCM input
cargo build --release --locked -p rambler-cli --features audio
```

The binary is `target/release/rambler.exe` on Windows and
`target/release/rambler` on Linux and macOS. To run the following example, load
the environment variables described in the
[authentication notes](docs/research/authentication.md). The CLI reads these
variables from its environment.

```sh
# Windows with Git Bash. Drop .exe on Linux and macOS.
set -a; source .env; set +a
target/release/rambler.exe dictate input.ogg --language fr-FR
```

`dictate` expects a continuous mono Ogg/Opus stream with a 16 kHz source. FFmpeg
can prepare an input file:

```sh
ffmpeg -i input.wav -ac 1 -ar 16000 -c:a libopus -b:a 24k -application voip input.ogg
```

## Integrate the SDK into an application

The [getting started guide in `app/`](app/README.md) shows how to run the
offline example and what the test suite covers on each platform.

Add the dependencies by path, adjusting the paths to match the location of the
Sambler checkout:

```toml
[dependencies]
rambler-engine = { path = "../Sambler/crates/rambler-engine" }
rambler-transport = { path = "../Sambler/crates/rambler-transport" }
rambler-audio = { path = "../Sambler/crates/rambler-audio" } # Omit for pre-encoded Ogg.
bytes = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

1. Create an `Engine` with `Engine::connect`, or use `Engine::new` to reuse a
   transport. Supply the language and credentials per session.
2. Supply audio in order. Use `OggAudio::open` for an Ogg/Opus file, or an
   `AudioSource`. Use `rambler-audio` to encode PCM (see [Audio inputs](#audio-inputs)).
3. Call `collect_dictation` or its observer variant. Read the result after the
   call returns `Ok`.
4. For an edit, call `replacement_proposal` on the result and obtain explicit
   user approval. Call `validate_current` within the same editor transaction
   as the replacement.

[`crates/rambler-engine/src/lib.rs`](crates/rambler-engine/src/lib.rs)
contains a compiled example. The
[SDK integration guide](docs/research/sdk-integration-guide.md) covers each step.

For [long dictation](docs/research/sdk-integration-guide.md#opt-in-to-long-dictation),
the PCM collector splits audio across sessions, preferably at pauses. For
application capture callbacks, [`capture_channel`](docs/research/rust-pcm-capture.md)
accepts blocks without waiting and explicitly reports overflow.
[`stream_long_pcm_dictation`](docs/research/recoverable-segment-delivery.md)
delivers each validated segment to the app before the next one. Segments
already delivered remain available if a later session fails or is cancelled.
Audio replay and sharing editor context across segments are disabled.

- `Ok` marks the end of a session. Inspect the result before displaying text as final.
- Keep errors separate from `finish()`. The engine never replays audio automatically.
- Use previews for display only. Exclude them from edits, submissions, and deletions.
- Keep audio, text, context, raw responses, and metadata out of logs.
- Treat edit results as possible replacements for the full history. Appending
  them to existing text can duplicate content.
- Reserve `Engine::close()` for application shutdown. Use the session controls
  to dismiss a preview.

## Audio inputs

| Input | Entry point | Notes |
|---|---|---|
| Continuous mono Ogg/Opus, 16 kHz | `dictate` or `OggAudio` | Primary validated input. The reader validates pages as it reads them without a native codec or real-time pacing. |
| Mono PCM16LE WAV at 16 or 48 kHz | `dictate-wav` with the `audio` feature | The engine resamples 48 kHz to 16 kHz continuously before encoding or `--interactive` segmentation. Opus uses 24 kbit/s, 20 ms pages by default, and a final flush. `--realtime` simulates progressive input from a file. |
| Mono PCM16LE at 48 kHz | `pcm_channel` → `Pcm48kTo16k` → `Pcm16Opus`, then `collect_dictation` (SDK) | Optional. The 16 kHz input path remains available. See [section 2 of the guide](docs/research/sdk-integration-guide.md) and [BUILDING](crates/rambler-audio/BUILDING.md). |

`dictate-wav` rejects stereo, floating-point PCM, and other sample rates,
including 44.1 kHz. The `pcm.input_bytes` and `pcm.encoded_input_samples`
counters refer to the encoder's 16 kHz PCM16LE input. In interactive mode,
`sample_range` also uses 16 kHz sample indices after any resampling.

Provide **a single continuous Ogg/Opus stream**. Raw PCM labelled as Opus and
concatenated clips encoded separately are invalid inputs.

## Results and editing

`--result-policy` selects how the final result is interpreted.

| Value | Effect |
|---|---|
| `strict` (default) | Conservative SDK projection. |
| `asr-only` | Returns recognition text only. |
| `s3-client-compatible` | Also accepts a rewrite of a captured window (intent 3, field 7). |

An explicit choice adds `provenance` to an accepted projection
(`hypothesis_text`, `voice_edit_text`, or `voice_edit_context`).

`DictationResult::interpret(policy)` distinguishes `UsableText`, `NoResult`,
`NeedsReview`, `VerifiedDeletion`, `SafetyBlocked`, and `UnrecognizedSafety`.
`NoResult` can mean there is no final response or text, or that the hypothesis
is explicitly empty. This alone cannot establish whether the input was silent.
The older `render(policy)` and `text()` methods keep their existing behavior.
A verified deletion requires an explicitly empty edit eligible to replace a
nonempty captured window. To apply it, prepare a proposal bounded to that window
and revalidate the context and revision.

In the CLI JSON, `outcome` is `usable_text` or `verified_deletion` with exit
code 0 for a single session (`type: result`). `no_result` and `needs_review`
exit with code 1 (`type: ambiguous`, `text: null`). `no_result_reason`
identifies the reason per segment (`no_finals`, `missing_text`, or
`empty_text`). The `safety_blocked` and `unrecognized_safety` errors omit
text, context, and final responses for the affected session.

In `--interactive` mode, the combined text requires usable text from every
session. If a segment is missing or needs review, the CLI exits with a nonzero
code and `text` is `null`. `completed_segments` still contains completed
sessions in order, each with its own `sample_range`, a half-open range of
16 kHz PCM samples. If no session opened, the reason is `no_segments`.
Likewise, `LongDictationResult::text()` only joins segments when each has usable
text. Individual results remain available.

```sh
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --before 'The meeting is tomorrow at ten.'
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --before 'The meeting is tomorrow at ' --selected ten --after '.' --cursor-aware
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --selected 'Send me the report now.' --cursor-aware --result-policy s3-client-compatible
```

For a single local replacement, `derive_scalar_splice` derives a local UTF-16
range and replacement text from an unmodified strict proposal after
revalidating the window and revision. See
[section 5 of the SDK guide](docs/research/sdk-integration-guide.md#opt-in-to-a-single-local-text-splice)
for its scalar boundary rules. See [result handling](docs/refinement.md) and the
[rewrite section](docs/research/sdk-integration-guide.md#opt-in-to-selection-rewrites)
for how to apply rewrites.

## Live service observations

These results come from small synthetic and natural speech datasets.

| Date | Test | Result |
|---|---|---|
| Nov 12 | English dictation | Punctuated text, `VoiceEditingResult` extension, successful S3 and gRPC completion |
| Nov 12 | “...at ten. No, make that eleven.” | `The meeting is tomorrow at 11.` |
| Nov 12 | Context `The meeting is tomorrow at ten.` with “Change ten to eleven.” | `The meeting is tomorrow at eleven.` |
| Nov 12 | Selected `ten`, with the before, selection, and after context supplied later | Selection replaced with `eleven` in a structured three-part response |
| Nov 12 | French dictation | `Bonjour. Ceci est un essai de dictée intelligente. La réunion aura lieu demain à 11h.` |
| Nov 13 | Rewrite of “Send me the report now.” | “Could you please send me the report at your earliest convenience?” |
| Nov 13 | [Python reference with ten read excerpts](docs/evidence/natural-expansion-quality.json), 7 English and 3 French | Differences of 3/109 English words and 3/79 French words after normalization. The figures apply to Python. |
| Nov 18 | [Rust engine with three read excerpts](docs/evidence/rust-native-natural-public.json), 18.885 s total | Three complete dictations from a separate sample of read speech |
| Nov 18 | Dictation of two sentences and “Change blue to green” (synthetic voice) | Exact results `The kite is blue. Marker stays.`, then `The kite is green. Marker stays.` |
| Nov 18 | One sentence in 13 languages (DE, ES, IT, PT, RU, AR, HI, TA, JA, KO, ZH, NL, PL) | [13 final results](docs/evidence/rust-native-multilingual-real-20261118.json). Hindi and Tamil came back in Latin script. |
| Nov 18 | Selected “Send me the report now.” with “Make this more polite.” | A polite rewrite (intent 3, field 7) was accepted with `--result-policy s3-client-compatible`. PixelBoard 1.3.5 did not produce it on the Xiaomi test phone. |

Some tests verified the audio bytes by SHA-256. The reports document the scope
and incidents of those tests. See the [redacted evidence](docs/evidence/),
including the [first Rust call](docs/evidence/rust-first-slice-public.json),
[PCM path](docs/evidence/rust-pcm-functional-public.json),
[natural voices](docs/evidence/rust-native-natural-public.json), and
[PixelBoard comparison](docs/evidence/pixelboard-vs-native-real-20261118.json).
The targeted tests did not observe composition (intent 4). See the
[composition analysis](docs/research/composition.md) and the
[reconstructed protocol](docs/research/protocol.md).

## Limitations

- A future application will handle microphone capture, the interface, and
  system-level text insertion. See the [Rust port contract](docs/architecture/portage-rust.md)
  and the [desktop architecture recommendation](docs/architecture/application-desktop-multiplateforme.md).
- Linux validation covers offline behavior.
- An endurance test recorded an intermittent failure with an undetermined
  cause. See the [status log](docs/STATUS.md) and the
  [diagnostic](docs/evidence/rust-incomplete-diagnostic-followup.json).
- [PixelBoard parity](docs/research/rust-feature-parity.md) is partial and the
  service's internal formats can change.
- [Performance improvements](docs/performance.md) are measured locally.
- The application is responsible for provisioning and revoking its metadata.

## Install and test the Python 0.3 reference

Use Python **3.11 or later**. The example uses Git Bash on Windows. On Linux
and macOS, replace `.venv/Scripts/python` with `.venv/bin/python`.

```sh
python -m venv .venv
.venv/Scripts/python -m pip install -c constraints-tested.txt -e '.[test]'
.venv/Scripts/python -m pytest -q
.venv/Scripts/python -m rambler.cli --help
```

The tests use local loopback gRPC servers. Offline and loopback checks and
builds pass in CI on Windows and Linux with Python 3.11 and 3.14. To build a
wheel, run `python -m pip wheel --no-deps . -w dist`.

The audio extra requires Python **3.12 or later** and adds PyAV with libopus.

```sh
.venv/Scripts/python -m pip install -c constraints-tested.txt -e '.[audio,test]'
.venv/Scripts/python -m rambler.cli dictate-wav input-16k-mono.wav --language fr-FR
.venv/Scripts/python -m rambler.cli dictate-wav input-16k-mono.wav --language fr-FR --realtime
```

The Python CLI emits NDJSON, with lossless Unicode escaping independent of the
Windows code page. It separates results, errors, and metrics. Edit results
can contain a full replacement history. Apply them as replacements after
validation. The codec import adds startup time, so initialize the process
before capture if startup latency matters.

```python
from contextlib import aclosing
from rambler.engine import RamblerEngine
from rambler.credentials import EnvironmentCredentials
from rambler.protocol import ProtocolConfig, EditingContext

async def ogg_chunks(path):
    with open(path, 'rb') as audio:
        while chunk := audio.read(8192):
            yield chunk

async def dictate(path):
    async with RamblerEngine(credentials=EnvironmentCredentials()) as engine:
        async with aclosing(engine.stream(
            ogg_chunks(path),
            config=ProtocolConfig(language='fr-FR'),
            context=EditingContext(),  # Sent after audio, before the end.
            timeout=30,
        )) as events:
            async for event in events:
                if event.response is not None:
                    for hypothesis in event.response.hypotheses:
                        # Also retain voice_edit for the intent and edited text or context.
                        print(hypothesis)
```

Audio producers must cooperate with asyncio cancellation. Use `aclosing` or
consume the stream to completion. Breaking out of the loop leaves an
asynchronous generator open. Inject `MetadataProvider` to supply metadata per
session. For asynchronous PCM, pass `pcm16_to_ogg(source_pcm)` from
`rambler.encoding` to `engine.stream`. The context can come from an async
callback (`context_timeout=2.0`). Context errors propagate to the caller.
`ProtocolConfig` holds biases, preferences, and secondary languages. See
[result handling](docs/refinement.md) for `collect_dictation` and
`replacement_proposal` validation.

## Documentation

- [Status and work log](docs/STATUS.md) · [Roadmap](docs/ROADMAP.md) · [Decisions](docs/DECISIONS.md)
- [Rust engine and CLI](docs/rust.md) · [SDK integration](docs/research/sdk-integration-guide.md) · [Feature parity](docs/research/rust-feature-parity.md)
- [Protocol](docs/research/protocol.md) · [Authentication](docs/research/authentication.md) · [Text editing and results](docs/refinement.md)
- [Test evidence](docs/evidence/) · [Performance](docs/performance.md) · [Validation criteria](docs/ROADMAP.md)
- [Contributing](CONTRIBUTING.md)

Keep keys, tokens, personal audio, raw captures, APK files, and decompiled
sources out of the repository.
