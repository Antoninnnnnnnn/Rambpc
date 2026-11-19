# Sambler Rust engine and CLI

The port is separate from the Python 0.3 reference: there is no Python interpreter,
Android, relay phone or FFmpeg process in the Rust path. It uses the same S3Bidi
Rambler service and routing, not a public Gemini API. See the
[porting contract](architecture/portage-rust.md).

## Recent CI validation

The offline CI for `171ef084cc1b1e7f3b82daefdcd6bb0aadafe442` passed on four
native platform families with Rust 1.88.0 and stable. Each of the eight jobs checks
three CLI zero-budget rejections on the release binary and the close regression;
these CLI restrictions do not change the library's zero semantics. The first
bounded close in this test accepts `Ok` or the independent `Connect` error, and
the second must succeed. The three macOS job failures in the previous run
`7327f51` remain historical. [Results and limits](evidence/rust-close-ledger-zero-budgets-ci-20261115.json).

## Build and test offline

From the root, with Rust/Cargo and the platform's native build tools (MSVC on
Windows; a C compiler for `ring` and libopus), as well as CMake for audio and tests:

```sh
cargo fmt --all -- --check
cargo test --workspace --all-features --locked
# This example's tests are not part of the implicit workspace cargo test
cargo test -p rambler-cli --features audio --example session_reuse --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
# Minimal Ogg binary, without an encoder or CMake in its build dependencies
cargo build --release --locked -p rambler-cli
# Binary with native WAV/PCM input
cargo build --release --locked -p rambler-cli --features audio
```

The workspace and test dev-dependencies include libopus even when the CLI feature
is disabled. Only the normal Ogg binary build can do without it. If CMake is not
on PATH, set `CMAKE` to the absolute path of its executable. See the
[audio guide](../crates/rambler-audio/BUILDING.md) for the validated toolchain,
the isolated build tool without global installation, and redistribution licenses.
Libopus 1.6.1 is statically linked; system DLLs/MSVC CRT are still required, but
an Opus DLL, Python and FFmpeg are not.

Rust **1.88.0 was run on Windows x86_64/MSVC and Linux x86_64/GCC**. Exact
snapshots and limits of each revalidation are in the
[backend validation](research/rust-backend-validation-2026-11-14.md). CI for commit
`4264126852b2c03e25fc231c60f9582569a84ae9` successfully ran Windows, Linux,
macOS ARM64 and native macOS Intel under 1.88.0 and stable. PCM/admission results,
counts and limits are in the [build validation](research/rust-build-validation.md)
and its [CI evidence](evidence/rust-pcm-admission-expanded-ci-20261115.json).
Tests in this batch use only fixtures and loopback servers, with no real service
call. The initial download requires Internet; `--locked` fixes versions, not their
local availability.

The binary is `target/release/rambler.exe` on Windows and
`target/release/rambler` on Linux/macOS. No Python virtual environment is needed.
The presence of Python sources in the repository does not make them a binary
dependency.

## Ogg/Opus file CLI

Provide the three [authentication](research/authentication.md) variables in the
environment: `RAMBLER_API_KEY`, `RAMBLER_ANDROID_PACKAGE` and
`RAMBLER_ANDROID_CERT`, all present and nonempty. On the normal endpoint, the
CLI first opens the input, then validates the three fields before `Engine::connect`;
a missing field, partial configuration, invalid Unicode value or invalid metadata
fails locally and input closure is awaited. The immutable snapshot is passed to
`SessionOptions` without rereading; snapshots and diagnostics are redacted when
displayed. No secret is supplied in a command argument; the CLI does not implicitly
load `.env`. Explicit loading in Git Bash is described in the README.

```sh
# Windows / Git Bash; remove .exe on Linux/macOS
target/release/rambler.exe dictate input.ogg --language fr-FR
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --before 'The meeting is tomorrow at ten.'
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --before 'The meeting is tomorrow at ' --selected ten --after '.' --cursor-aware
```

Input must be a **continuous Ogg/Opus stream, mono, 16 kHz source**, version 1
and mapping 0. The CLI emits one validated page per chunk without changing bytes:
CRC, sequence, single serial number, continuation, headers, granules and EOS.
The first `OpusHead` page is checked before the RPC; later corruption is a source
error, never successful EOF. The EOS page is returned only after checking that
there are no extra bytes. Pages are limited to 65,307 bytes, packets and `OpusTags`
to 64 KiB: a larger legal extension/padding remains **outside the supported
profile**, not universally invalid. This is not an Opus decoder: codec duration,
audio payload and audible quality are not certified. It does not convert WAV or
simulate real time. Do not label PCM as Opus. The SDK `AudioSource` remains a
contract for caller-supplied encoded bytes; this file validation belongs to the CLI.

File commands require **regular files** (links to a regular file are allowed), not
FIFOs, pipes, consoles or devices. On Unix, nonblocking open checks the descriptor
without waiting for a producer; on Windows, its type is queried before any read.
Live streams remain possible through an asynchronous `AudioSource` implementation,
not by presenting a pipe as a file. The RPC timeout does not bound every possible
filesystem or failing-disk stall. See [preflight and regressions](research/rust-file-inputs.md).

Options: `--timeout` (30 s, **including speech**), `--context-timeout` (2 s),
`--finalize-timeout` (optional, absent by default), and repeatable lists
`--bias-term`, `--preference`, `--secondary-language`. The optional finalization
budget starts after EOF and successful audio cleanup; it bounds context, upload
completion and status/delivery without exceeding the hard `--timeout`. Zero is
rejected; a huge budget is capped at the hard deadline. No `max_audio` or false
EOF is added; cleanup is still awaited cooperatively even after expiration. The
irreversible terminal commit, its regressions and Windows/Linux qualification
status are distinguished in [reliability and finalization](research/rust-reliability-lifecycle.md).
For example, `--timeout 150 --finalize-timeout 10` illustrates at most 10 s after
EOF/cleanup within 150 s total, not a measured optimal setting or 150 s of speech
plus a guaranteed 10 s. Lists are emitted at init; text context is sent after
audio. Their encoding does not prove their effect on the server. Do not put
personal text in arguments if shell history or the process list can be inspected.

For `--timeout`, `--context-timeout` and `--finalize-timeout`, zero is rejected
at argument parsing, before input opening, environment credential reading or
network connection: redacted JSON `type=error`, `category=arguments`, exit code 2.
This does not guarantee rejection of every invalid configuration before connection.
In the library, zero `SessionOptions` durations remain `EngineError::InvalidConfig`
errors; the caller must await `Engine::close().await` for cooperative cleanup,
even after rejection. [Local validation and limits](evidence/rust-zero-cli-budgets-20261115.json);
the new CI step also checks the release binary on native platforms.

`ProtocolConfig` also exposes `block_offensive_words: Option<bool>`,
`auto_punctuation: Option<bool>` and `secondary_routing: Option<String>`.
All three default `None` values preserve the historical init byte for byte.
The CLI accepts `--block-offensive-words true|false`,
`--auto-punctuation true|false` and `--secondary-routing VALEUR`. These are
verified S3 client fields, not proof of their server effect or a COMPOSE trigger.
The offensive words preference never changes response safety checks.
[Schema, conditions and limits](research/rust-recognizer-options.md).

Output is one UTF-8 JSON line after collection **and** cleanup. It contains ordered
known finals, context and metrics, but not raw bytes, opaque fields, result IDs or
credentials. A valid deletion remains `"text":""`. Ambiguity preserves finals
and produces a nonzero exit code; no last hypothesis is chosen arbitrarily. Text
content remains sensitive: output is not an automatically anonymized log. Finals
not retained as usable text may still contain dictation or private context in this
detailed output.

To connect the CLI to telemetry, use **`--diagnostics-only`** with `dictate` or
`dictate-wav`. The projection removes text, hypotheses and context, including in
completed segments in interactive mode. It retains only known categories,
durations, counters and sample ranges and carries the `"diagnostics_only":true`
marker after valid argument parsing. The exit code remains that of the dictation.
This option does not turn output into a recognition result, change data sent to
the service or mask arguments that the system or shell may retain.

`--loopback 127.0.0.1:PORT` is reserved for local plaintext tests. It rejects
non-loopback addresses and does **not** use environment credentials. The normal
path uses verified TLS with system roots and HTTP/2 ALPN. The Python reference
also restricts `GrpcTransport(..., secure=False)` to numeric loopback addresses
with an explicit port (`127.0.0.1:PORT` or `[::1]:PORT`), without name resolution
or a proxy. Use TLS elsewhere.

## Optional native PCM CLI

After building with `--features audio`:

```sh
target/release/rambler.exe dictate-wav input-16k-mono.wav --language fr-FR
target/release/rambler.exe dictate-wav input-16k-mono.wav --language fr-FR --realtime
# Explicit batching comparison, without changing 20 ms codec frames
target/release/rambler.exe dictate-wav input-16k-mono.wav --language fr-FR --page-duration-ms 60
```

WAV must be mono PCM16LE at 16 or 48 kHz. At 48 kHz, a single 48 → 16 kHz
converter processes the entire file, including before interactive segmentation.
Rates other than 16 and 48 kHz, stereo and floating-point PCM are rejected. The
source is read progressively (20 ms CLI chunks at the file rate), encoded as
VoIP/VBR24kbps and muxed into continuous Ogg; flush/lookahead and the EOS granule
preserve the last samples without extending decoded duration with padding.
`--realtime` simulates a file source, not a microphone; the first chunk does not
wait artificially. `--page-duration-ms` accepts multiples of 20 between 20 and
1000, default 20. Other context, language and metadata options are the same as
`dictate`. A build without the audio feature does not offer this command.
On final output, `dictate-wav` adds a `pcm` object when the observer exists:
`input_bytes`, `encoded_input_samples`, `encoded_frames`, `input_eof` and
`output_eof`. `input_bytes` and `encoded_input_samples` count PCM16LE at 16 kHz
passed to the encoder, not the bytes or samples of the 48 kHz WAV. Ogg output and
output without the audio feature remain unchanged. These local counters are
distinct from encoded metrics and prove neither server receipt nor absence of
loss or replay. Their definitions and the optional API are in the
[PCM contract](research/rust-pcm-channel.md). The
[local A/B qualification](research/rust-build-validation.md#pcm-progress-and-strict-admission--local-checkpoint-2026-11-15)
clarifies scope; see also the [evidence](evidence/rust-pcm-progress-admission.json).

The [four additional live trials](evidence/rust-pcm-functional-public.json)
validate accelerated/progressive PCM with the same final text, intent 3 rewriting
and selection with emoji. Separate [native reuse evidence](evidence/rust-native-session-reuse-public.json)
then validates three cases on one engine, with a hash of the PCM actually consumed.
The independent oracle, ignored by default and then passed separately on 57
containers, is distinct from this network evidence. The
[libopus/wrapper notices](../crates/rambler-audio/THIRD_PARTY_LICENSES) accompany
any redistribution of the audio binary.

Ctrl-C during collection or closure cancels and then awaits cleanup, with a JSON
error and exit 130. In interactive dictation, [already validated segments](research/recoverable-segment-delivery.md)
are retained in private output, without global text. A close error is also reported.
Supervision is tested by deterministic simulation and by a
[real Windows console signal on loopback](evidence/rust-windows-ctrl-c.json), in a
new console whose only attached processes are the driver and its child client.
The first driver timed out; console isolation/inheritance was corrected without
changing the binary. Offline reproduction with the test venv and prepared
controlled fixture:

```sh
.venv/Scripts/python -I tools/validate_rust_ctrl_c.py . local/ctrl-c-new.json
```

## Libraries

- `rambler-protocol`: request encoding and bounded protobuf decoding; presence,
  unknowns, order and proto2 merging preserved, exact root bytes.
- `rambler-transport`: shared HTTP/2 connection, gRPC framing, metadata, deadlines
  and cleanup supervisors. `h2` + `tokio-rustls`/`ring` client; `tonic` remains
  in use for gRPC types and test servers. The original tonic/hyper client did not
  provide the reset control needed to test cancellation under backpressure; this
  change does not modify the backend.
- `rambler-engine`: session assembly, asynchronous audio source, deferred context,
  events, bounded collection and result/proposal policy.
- `rambler-audio`: strict WAV source, bounded producer PCM input and static native
  PCM → Ogg/Opus adapter, fragmentation, tail/flush, file pacing and bounded
  cleanup; no microphone.
- `rambler-cli`: minimal file adapter, without microphone, GUI or insertion.

A [Rust integration example compiled by doctests](../crates/rambler-engine/src/lib.rs)
illustrates collection from a caller-supplied source, context and injected
credentials. `Presentation::new(&response)` provides a borrowed view of the
active member (recognition, speech event, opaque variant).
`RecognitionCompletion::Complete` means state 1, **not** global completion.
Presence of containers 3/4/5/7 is independent. `PartialTranscript::from_result`
separates stable prefix and provisional suffix according to the observed client;
never append previews from successive responses. The view does not authorize any
application in the editor. For detailed signatures:

```sh
cargo doc -p rambler-engine -p rambler-transport -p rambler-protocol --no-deps --open
```

`SourceError` masks sensitive details in `Display`, `Debug` and the automatic
error chain. `original()` provides explicit access to the typed error, compatible
with `downcast_ref`; do not log this content blindly.

`Engine::connect(TransportConfig)` owns its transport; `Engine::new(transport)`
uses an injected transport that the caller must still close.
`Engine::start(Box<dyn AudioSource>, SessionOptions)` takes charge of the source
immediately. Before the first `source.next()`, transport waits for valid metadata,
h2 ready and local `send_request` success; this is neither peer acceptance or
authentication nor a health check/silent NAT detector. The source constructor may
already have worked before synchronous `start`. `Engine::start` supervises audio
cleanup even for invalid configuration or a closed engine; await
`Engine::close().await` to finish this cleanup after rejection too. Raw transport
is different: synchronous rejection by `GrpcTransport::start` before
`Ok(Session)` does not transfer the source to the supervisor and falls under
ordinary Rust Drop. After acceptance, the source is cleaned up once even without
successful RPC admission. `StreamAudio::new(stream)` adapts a stream of `Bytes`
fragments. Source implementations must cooperate with cancellation and their
asynchronous `close` must finish.

Initial establishment resolves DNS once and retains all addresses, alternating
families. The Happy Eyeballs-style TCP race retains the normal **250 ms** stagger;
after an observed failure, it accelerates with deadline
`min(scheduled deadline, max(last launch + 10 ms, now))`, without delaying
a scheduled start. Each pinned future receives its first poll with the real waker
before any yield; the last launch is timestamped **after this poll**, not at
construction. These **application API initiation** delays are inspired by
[RFC 8305 §5](https://www.rfc-editor.org/rfc/rfc8305.txt): recommended default
250 ms, recommended minimum 100 ms (`SHOULD`), absolute floor **10 ms** (`MUST NOT`).
They are **neither a measurement nor proof of kernel SYN spacing**, nor full
RFC 8305 compliance: there is no AAAA/A DNS race or complete RFC 6724 implementation.
No performance gain is claimed. The race yields after initiation to avoid hiding
the timeout and closes losers before TLS. The default 10 s connection ceiling
covers DNS/TCP/TLS/h2 and preserves `Timeout`; approximately 41 blocked attempts
under this default is not a bound for an arbitrary user timeout. The original TLS
name, system roots and ALPN remain unchanged; a TLS fault does not trigger another
destination. This is not session retry/replay/reconnect and no keepalive/PING is
added. The [first-poll fix and its 19 targeted tests](research/rust-connection-lifecycle.md#targeted-tcp-pacing-correction--november-15-2026)
are distinct from the [integrated Windows/Linux results](research/rust-build-validation.md#november-15-reliability-batch).
The 13 older tests whose oracle recorded future construction do not qualify the
first poll. The h2 receive window of 1 MiB per connection / 65,535 bytes per stream
and capacity-one application queues provide finite headroom, not isolation for
arbitrary N streams or a global RSS bound.

For progressively produced PCM, `rambler_audio::pcm_channel()` provides a single
producer and a source to compose with `Pcm16Opus`. Reserving before handing over
bytes avoids losing them when a capacity wait is cancelled. `PcmSender::finish()`
announces normal completion; Drop without finish is an erroneous abandonment, not
successful EOF. Run producer and collection concurrently, then await actual engine
termination. [Contract and executable offline example](research/rust-pcm-channel.md).
This helper adds no microphone capture and is not an upload acknowledgment.

`SessionOptions` groups configuration, deadlines, an
`Option<Arc<dyn MetadataProvider>>` and `ContextInput::{None, Static, Provider}`.
`timeout` defaults to 30 s from startup, including metadata/speech/upload/context;
`finalize_timeout: Option<Duration>` defaults to `None` and starts only after audio
EOF and successful cleanup. Its deadline never exceeds the hard deadline, even for
a huge duration; `Some(Duration::ZERO)` is invalid. A context provider is called
once after this cleanup, under the minimum of its context budget and remaining
deadlines. Credentials can be injected per session; `EnvironmentCredentials` is
a supplied implementation and remains permissive if all three variables are
absent. To require them, use `required_snapshot()` or `required_snapshot_with(...)`;
the normal CLI uses a strict snapshot, unlike explicit loopback. This check does
not prove remote authentication or renewable provisioning; its local qualification
and limits are described in the [snapshot contract](research/rust-credential-snapshots.md).
The terminal commit of this budget and its evidence are described in the
[reliability contract](research/rust-reliability-lifecycle.md).

Migration from the Rust 0.1 prototype: an exhaustive `SessionOptions` literal must
add `finalize_timeout: None` or use `..SessionOptions::default()`. Exhaustive
matches must also cover `EngineError::FinalizeTimeout` and
`TransportError::PrimaryAndCleanup`. An absent budget preserves the 30 s default;
new causes remain structured and redacted when displayed.

`collect_dictation(&engine, source, options, limits)` returns a future whose
ownership is established on the call, even if it is never polled. By default it
retains at most 128 finals and 8 MiB of associated raw responses. To process
events, consume `EngineSession::next()` until `None` and explicitly call `close()`
if stopping early.

`collect_segments` / `collect_segment_session` also retain responses with a
hypothesis container 3/5/7 regardless of state, as well as state 1 markers.
Preview 4 alone is not retained. `SegmentResult.updates` preserves each response
and its client ordinal (including responses not retained), without concatenation,
deduplication or implicit selection. Limits are shared with the strict collector
and measure retained wire bytes, not RSS.

Containers and their fallbacks are structurally decoded; `all_hypotheses()`
iterates them in canonical 3/5/7 order, not a ranking or global wire order. The
`hypotheses()` helpers and rendering remain limited to the primary container.
`RecognitionResponse.language` and `spoken_language` are fields 3/4, not result
identifiers. See [schema and provenance](research/rust-response-containers.md).

### Shared raw bytes: migration from Rust 0.1 prototype

The decoded models' `raw` fields and `RecognitionResponse.active_raw` now use
`bytes::Bytes`, not `Vec<u8>`. Function signatures and wire `Field`/`Value` types
do not change. Use `raw.as_ref()` to borrow bytes, `raw.to_vec()` when an
independent `Vec` is needed, and `raw: vec![...].into()` in manual model construction.

The decoder copies the bounded root payload once, then shares child views; it does
not retain a large transport buffer outside the message. A real merge of multiple
occurrences retains its logical concatenation. A small child can nevertheless
retain its entire root (at most 8 MiB): for prolonged isolated retention, explicitly
detach its `raw` with `bytes::Bytes::copy_from_slice(child.raw.as_ref())`. Text,
presence and unknown fields are not removed. The [memory profile](research/rust-backend-profiling.md)
measures −70.82% logical retention on its benchmark, not a guaranteed RSS reduction.

## Completion, cancellation and results

- Successful completion requires S3 END, fully exhausted/half-closed upload,
  closed source and gRPC OK status. A final received before that is not success.
- An S3 error does not interrupt the draining needed to detect a late gRPC error.
  No failure triggers automatic audio replay.
- `EngineError::IncompleteSession(Arc<IncompleteSessionDiagnostic>)` exposes S3
  END, transport counters/flags and final metrics without text/raw/ID, recorded
  after cleanup. The CLI projects these into `error.incomplete`; they distinguish
  missing END from interrupted upload without proving the server cause. The typed
  payload replaces the unit variant of the initial 0.1 prototype. `source_exhausted`
  refers to the entire RequestSource (init/audio/context/END), not directly PCM EOF;
  `source_closed` means cleanup completed, even early. `upload_eof_polled` remains
  a local END_STREAM submission, not a server acknowledgment, and `saw_s3_end`
  alone does not guarantee the audio tail.
- Normal stop: finish the source and drain responses. Cancellation:
  `request_cancel()` then `close().await`; this is not dictation finalization.
- `Drop` signals cancellation but cannot await asynchronous cleanup. Keep Tokio
  alive and explicitly await session/engine closure. Cooperative cleanup can exceed
  the network deadline.
- `Engine::close()` and `GrpcTransport::close()` await cleanup, then deliver at
  most one previously unobserved error per call. If there are several, call
  `close()` again until `Ok` to collect them all; they are not silently acknowledged
  together. A close future interrupted before its result must not acknowledge an
  error it did not deliver. An observed primary audio error does not acknowledge
  a distinct secondary cleanup error: `Engine::close()` may therefore still return
  `AudioCleanup`, then `Ok` once the other errors have been collected. The owning
  engine also preserves the driver.
- Supplied audio sources retain a typed `Err` fault in `AudioSource::next()` until
  explicit close; no healthy EOF/EOS is fabricated. Conversely,
  `rambler_transport::Session::next()` is intentionally fused after delivery of
  its terminal error and `finish()` retains that outcome.
- `TransportError::PrimaryAndCleanup` preserves a primary and distinct cleanup
  error, with private construction/fields and explicit access to both causes,
  without leakage through formatting/automatic chaining. Explicit cancellation
  with failed cleanup remains `Cleanup`. The engine unwraps this pair only for its
  local adapter and preserves `AudioCleanup` once, without a second close; this
  does not change general public conversion. Transport precedence for an observed
  source fault does not universally erase a deadline/cancellation already chosen
  by the engine.
- Queues are bounded; a stalled consumer can delay reading trailers. Upload counters
  describe client work, not server acknowledgment. All fallible waits, including
  cleanup and queue reservation, precede the private irreversible commit.
  Cancellation and deadlines are rechecked immediately before publication: a
  published healthy EOF cannot become an error, and `metrics.eof_seconds` dates
  this successful commit, not the last PCM sample. See
  [regressions and batch qualification](research/rust-reliability-lifecycle.md).
- `EngineSession::audio_progress()` and `metrics.audio` count nonempty chunks and
  encoded bytes actually returned to the engine, audio EOF and the
  `cleanup_finished`/`cleanup_succeeded` flags. They remain accessible after a
  session error; await termination/closure for a final snapshot. This is neither a
  PCM sample count nor proof of audio received by the server. Audio EOF can be true
  while late context still blocks the RequestSource. The fields are added to the
  0.1 prototype structs.
- `DictationResult::check_safety()` and `SegmentResult::check_safety()` inspect all
  hypotheses in retained responses, including secondary containers/fallbacks,
  without selecting them for rendering. `text()`, `single_hypothesis()` and
  `replacement_proposal()` reject safety 1 candidates (`SafetyBlocked`) and
  uninterpreted field 30 (`UnrecognizedSafety`) before ambiguity. The CLI returns
  an error without text or finals. Structured data remains explicitly available to
  the integrator. Absent/0 unchanged; rejection outside VoiceEdit and on unknowns
  is intentionally conservative. [Scope and static evidence](research/rust-feature-parity.md).
- Simplified text requires a single interpretable final/hypothesis: without
  VoiceEdit, hypothesis text; intent 2, verified edited text/context; intent 3,
  `edited_text` field only. Other cases remain ambiguous.
- A replacement proposal remains limited to intent 2 and changes nothing. Check
  the text/revision snapshot before application; positions are in UTF-16 units,
  not Rust bytes. Text and integer revisions are distinct types; canonical integers
  are not limited to 64 bits.

### Explicitly opt-in rendering and commands

`DictationResult::render(RenderPolicy)` returns text, provenance, intent and any
fallback reason. `Strict` preserves the `text()` contract described above. `AsrOnly`
projects hypothesis text, never the VoiceEdit transcript. `S3ClientCompatible`
renders intents 2/3, members 3/7; otherwise it uses the ASR getter (absence → empty
string). An accepted empty edit remains empty. All policies require uniqueness and
safety; this helper does not reproduce Android selection.

`replacement_proposal_with_policy` remains effect-free: in compatible mode,
intent 2/fields 3 or 7, and intent 3/**field 7 only**. Intent 3/field 3 can be
rendered but does not prove the scope of a replacement; `AsrOnly` produces no
proposal. Scope remains the captured context window.

`classify_command(CommandProfile::S3DefaultPatternsV1, ime_action)` recognizes
standard SEND/CLEAR/CLEAR ALL regexes on a single safe edited final candidate.
CLEAR is recognized but rejected by the client branch; SEND requires IME action 4.
The result exposes preconditions/disposition and `applied=false`, without sending,
deleting or system insertion. This versioned profile does not interpret remotely
configured regexes. It is not a COMPOSE implementation.

## Explicit endurance and private reports

`tools/validate_rust_endurance.py` is a **Python development tool**, not a binary
component. It runs the real executable, measures its child RSS/CPU, bounds output,
duration and sessions, requires markers in rendered text and compares ordered
known finals. It stops at the first failure/inconclusive result, without replay.
Each session is a new process and channel.

After preparing the controlled synthetic corpus and explicitly loading the
authorized environment, Windows / Git Bash example:

```sh
.venv/Scripts/python tools/validate_rust_endurance.py --allow-live --stage 120 \
  --executable target/release/rambler.exe \
  --audio artifacts/endurance-synthetic/endurance-two-minute-en-16k.wav \
  --timeout 150 --sessions 1 --compiler-version "$(rustc --version)" \
  --build-version 'COMMIT;release;features=audio' \
  --marker-beginning 'amber orchard' --marker-middle 'cobalt lantern' \
  --marker-end 'violet harbor' --output local/rust120-new.json \
  --public-output local/rust120-public-new.json
```

The `--stage 300` level requires a healthy `--previous-report` 120 compatible with
the same binary/configuration; use the five-minute file and a 330 s timeout, at
most three sessions. The gate is a local precaution, not an attestation. Recorded
evidence includes a **failure on the second long attempt**; do not present the
command as a guarantee of success. Pre/post file hashes are not independent proof
of every byte consumed.

The full report retains bounded private stdout/stderr; the `--public-output`
projection is separate, allowlisted, and still needs inspection. Compiler/build
versions are caller declarations, not inferred from the binary. Metrics are
sampled, not exact peaks. On Windows, child PATH is limited to system directories;
this policy on Unix does not establish the absence of interpreters in those
directories.

## Validation of multiple native sessions

```sh
cargo run --release -p rambler-cli --features audio --example session_reuse -- \
  manifest.json --allow-live --timeout 30
```

This development example explicitly requires `--allow-live` (or a `--loopback`
endpoint), accepts 2 to 3 controlled WAVs, bounds manifest and files, uses one
explicit engine/connection and stops at the first failure without retry. Each case
declares `input`, `language`, `pcm_sha256`, `markers`; options
`marker_alternatives`, `expected_text_sha256` and context/hints are documented in
[its source](../crates/rambler-cli/examples/session_reuse.rs). Output contains only
hashes, counters and categories, not text/path/credentials. The manifest also
accepts `block_offensive_words`, `auto_punctuation`, `secondary_routing`; values
are not echoed in the report. Loopback tests physically count one TCP connection;
this count is not invented for remote evidence. The first batch with marker
`eleven` instead of `11` remains [inconclusive](evidence/rust-reuse-initial-marker-failure.json);
the next batch declared variants before execution, without rewriting that failure.

## Measured performance

Audio request construction uses one exact allocation and a single payload copy,
after checking all bounds. The [A/B profile](research/rust-backend-profiling.md)
publishes 96 CPU measurements and 40 loopback sessions, hashed source/binary and
snapshots distinguished from later code. Micro-CPU gain established; **no robust
overall WAN or pipeline gain established**. The earlier CRC candidate was rejected,
not retained on intuition alone.

## Scope still distinct

The engine takes pre-encoded Ogg/Opus or PCM through the optional native adapter.
Rust endurance measurements are reported separately from Python evidence, with
their process/channel model; no cross-platform live run is inferred. Intent 4
composition has not been demonstrated on the observed profile. Provisioning and
revocation of application metadata remain external limitations.
