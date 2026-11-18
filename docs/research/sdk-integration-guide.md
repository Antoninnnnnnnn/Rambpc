# Integrating the native Rust SDK

The SDK consumes audio supplied by your application and returns a completed session result. You own capture, document state, user consent, and any UI or editor action. The examples below use public APIs; you do not need to assemble protocol messages or drain the responses yourself.

## 1. Create an engine on a live Tokio runtime

Use crates from the same checkout. For example, in your application's `Cargo.toml` (adjust the paths):

```toml
[dependencies]
rambler-engine = { path = "../Rambpc/crates/rambler-engine" }
rambler-transport = { path = "../Rambpc/crates/rambler-transport" }
rambler-audio = { path = "../Rambpc/crates/rambler-audio" } # omit for pre-encoded audio
bytes = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

`rambler-audio` builds bundled native Opus; see the [build prerequisites](../../crates/rambler-audio/BUILDING.md). The engine itself has no microphone or encoder.

- `Engine::connect(transport_config).await` owns the transport. To share an existing `GrpcTransport`, use `Engine::new(transport.clone())` and close that transport separately after its last owner is done. Use only your application's authorized transport and per-session `SessionOptions::credentials` (`Arc<dyn MetadataProvider>`). `TransportConfig::tls` takes a `Host[:port]` authority, not a URL. For a fixture that you control, `TransportConfig::loopback` accepts an explicit numeric loopback address. The SDK does not provision service access.
- Set your locale in `SessionOptions::config.language`. Keep the audio configuration at Ogg/Opus, 16 kHz input, and one channel, as in `ProtocolConfig::default()`. Start each utterance with its own audio source, options, and any per-session context provider.

### Warm the connection before recording

`Engine::connect` is eager: it resolves the endpoint, establishes TCP and verified TLS (except for explicit loopback), and initializes the HTTP/2 client, without starting an RPC. For a confirmed warm start, use `Engine::connect_ready(transport_config).await` during app startup. It also waits for one HTTP/2 PING acknowledgement within the same connection timeout, without an RPC or credential lookup. When sharing a transport, use `GrpcTransport::connect_ready` before `Engine::new`.

Keep the returned engine and use it for the first and later dictations; don't reconnect on every utterance. The probe adds one round trip to warm-up, not to session startup. It is not periodic keepalive, auth validation, or a guarantee that the peer will still be reachable or accept an RPC later. A dead or closed connection never reconnects automatically; explicitly create a replacement for new sessions.

## 2. Feed ordered caller audio

For a local Ogg/Opus file, give `Box::new(rambler_engine::OggAudio::open(path).await?)` to either collector. This is the same bounded, byte-preserving source used by `rambler dictate`; it needs no native Opus dependency. It preflights the first page and validates the remaining pages as they are read, without real-time pacing. A late framing or read error fails the session rather than pretending the input ended normally. See [`OggAudio`'s file policy](../../crates/rambler-engine/src/ogg.rs).

For other already encoded audio, supply a `Box<dyn AudioSource>` over **one continuous Ogg/Opus stream**, with bytes in order; do not concatenate independently encoded clips or pass PCM as encoded audio. `StreamAudio::new` wraps a `Stream<Item = Result<bytes::Bytes, SourceError>>` when synchronous Drop is sufficient. If the source needs asynchronous release, implement `AudioSource`: `next()` must tolerate being dropped; return `Ok(None)` only for normal EOF; make `close()` eventually complete. Keep Tokio alive for cleanup.

For PCM16 little-endian, 16 kHz, mono:

1. Call `rambler_audio::start_pcm_dictation(&engine, options, limits)` for `(mut writer, completed)`. Use `start_pcm_dictation_with_observer(&engine, options, limits, &mut observe)` if you want previews. Both admit the source before returning, even if you have not polled `completed` yet. Check `completed` for admission and session errors, not the writer.
2. Drive the writer and completion **concurrently**. For each chunk in order, `writer.reserve().await` waits for the one queue slot without taking the bytes; `permit.send(chunk)` transfers the chunk or returns it on rejection. Odd chunk boundaries are fine; input must end on a complete sample. Keep each chunk modest; the hard per-chunk limit is 8 MiB of logical `Bytes` length, not a bound on its backing allocation.
3. Call `writer.finish()` on normal input EOF so the encoder can drain and flush. Drop without finish means abandonment, not successful EOF. A failed or cancelled collector closes the input and wakes blocked reservations. Stop production on `Closed`; do not replay accepted chunks or convert a producer failure into `finish()`. `tokio::join!` works for bounded input that does not stall outside the PCM channel (see the [compile-checked slice example](../../crates/rambler-audio/src/dictation.rs)). For a live producer that also waits on capture, race that producer against `completed` with `tokio::select!` and drop the producer when the session ends, or signal and join it if it runs in a separate task. If the producer stops first, still await `completed`. `join!` alone does not stop a stalled capture wait.

Queue acceptance and `finish()` are not delivery, encoding, or session acknowledgements. These helpers use the default encoder. For custom encoder settings or local PCM progress, compose `pcm_channel()` with `Pcm16Opus` and use an engine collector directly. `Pcm16Opus::new_with_progress` supplies a `PcmProgressHandle::snapshot()`; these are encoder observations, not server consumption. See the [offline PCM example](../../crates/rambler-audio/examples/pcm_channel.rs) for the lower-level pattern.

### Opt in to long dictation

Two paced single-stream runs longer than five minutes lost the entire session: the service returned gRPC `Internal` at about 300 seconds without final text. For long live captures, use [`rambler_audio::collect_long_pcm_dictation`](../../crates/rambler-audio/src/long_dictation.rs) on one shared `Engine`. Feed the receiver from `pcm_channel()` as above, or wrap it once in `Pcm48kTo16k::new(input)` for 48 kHz before handing it to the collector. Do not reset the decimator at segment boundaries. Use `ContextInput::None`.

Drive capture concurrently and keep its handoff bounded: the helper stops pulling PCM while each segment finalizes, then opens the next session only when a full sample is available. Do not wait for `reserve()` in a real-time capture callback. Hand off into a bounded application buffer sized for your expected gap, and treat overflow as a capture failure, not silent audio loss. Race a producer that can stall outside the channel against the collector as above; if collection fails first, stop and join any separate producer task. On intentional stop, have the producer drain buffered PCM and call `finish()` to collect the tail. Dropping the collector cancels it and yields no results.

`LongDictationOptions::default()` cuts by 240 seconds of PCM or elapsed segment time, whichever comes first; the configured cap cannot exceed 280 seconds. During the final five seconds of either budget, it prefers the end of the first 100 ms block with PCM16 RMS at most 256, otherwise cutting at the cap. This is an energy threshold, not speech detection; a hard cut can split a word. Every complete sample belongs to exactly one segment on the healthy path, in a fresh Opus/Ogg stream. The cap ends audio intake, not finalization: use per-session timeouts with room for both (e.g. `timeout: Duration::from_secs(270)` for a 240 s budget, not the 30 s default; optionally set `finalize_timeout`). Waiting for the first sample does not start a session deadline. Bound capture length, between-segment idle waits and total retained results in the app; collector limits apply per segment. The source's `close()` is awaited, even beyond deadlines, so it must cooperate.

Only successfully finalized sessions enter `result.segments`. `result.text()` joins the strict text of every segment with one ASCII space, without trimming or fallback; an unsafe or ambiguous segment makes the projection fail. No editing context is accepted or carried across sessions. A failure returns the cause and earlier results in `error.completed`, without replay. The `_with_observer` variant numbers provisional responses by segment and emits `SegmentCompleted { segment_index, sample_range }` after a successful session. The range gives exact absolute half-open 16 kHz PCM sample offsets (after resampling if used, excluding Opus padding). Clear preview there and on failure or cancellation. Completion and its sample range do not imply successful text projection.

For a paced, bounded WAV check, run the [`long_wav_qualification` example](../../crates/rambler-cli/examples/long_wav_qualification.rs). It reports the completed sample ranges and strict result status in JSON; text is omitted unless explicitly requested. It accepts an owned loopback fixture or an explicit TLS authority for a deployment you are authorized to use, with no default endpoint. When testing fidelity, use synthetic paragraphs and keep any `--include-text` report private. Sample coverage is not a speech fidelity result, and the `eof_minus_segment_audio_seconds` metric is a pacing proxy, not a microphone-stop or peer-delivery measurement. A [separate 609-second localhost qualification](../evidence/rust-long-pcm-loopback-windows-20261118.md) checked sample integrity, per-RPC packet agreement, and retention of clean segments after a gRPC failure.

```sh
cargo test --locked --offline -p rambler-cli --features audio --test long_pcm_dictation
cargo test --locked --offline -p rambler-cli --features audio --example long_wav_qualification
```

## 3. Observe preview and progress without taking over the collector

For an already encoded `AudioSource`, use `collect_dictation(&engine, audio, options, limits)` if you only need the result, or `collect_dictation_with_observer(&engine, audio, options, limits, &mut observe)` for an ordered synchronous callback. The single-session PCM helpers above call these same collectors. They admit the audio immediately and collect within the same bounds; you do **not** call `EngineSession::next()` alongside them.

In the callback, `DictationObservation` borrows a `Presentation` and a `SessionMetrics` snapshot. Match `PresentationContent::Recognition` and inspect `result.partial_result`; `PartialTranscript::from_result` gives the stable-prefix and tentative-suffix presentation of that **one** response. Replace the whole preview on each update; do not append across responses. “Stable” here is a client display rule, not a guarantee; the helper flags non-finite scores. Final hypotheses are alternatives, not text to concatenate. Recognition completion, receipt of S3 END, and passage of a heartbeat are observations, not authorization to display committed text or act on it.

The callback runs in the collecting task before the next response is drained. Keep it quick and nonblocking. If a different consumer needs an update, copy only what it needs into a bounded or replace-latest handoff; never log audio, text, context, raw responses, or metadata. Previews are not safety-screened: keep them separate from committed text, or omit them if your product requires safety-checked text. Never use previews for replacement, SEND, or CLEAR. On either completion or failure, clear the preview; on external cancellation, the outer owner must do this independently.

`responses_received` is a one-based response ordinal, not a segment ID. Audio and payload metrics describe encoded-source observations and produced protobufs, not network acknowledgements. Metrics arrive with each accepted decoded response, including status frames, not on a timer while the server is silent. There is no callback for engine EOF, terminal errors, or the response that exceeds the retention limit. An application error frame can itself be observed before the collector returns its error.

The [compile-checked engine example](../../crates/rambler-engine/src/lib.rs) shows a borrowed observer, replacement previews, and ordinary result handling in one function.

## 4. Wait for commitment and keep ownership explicit

On the healthy path the engine sends initialization, then encoded audio, then optional context after normal audio EOF and successful cleanup, then S3 END and upload EOF. The collector only returns `Ok` after observing S3 END and the engine's unique successful EOF (fully exhausted upload, gRPC OK, and successful cleanup), followed by clean terminal completion. A final-looking candidate or S3 END can still be followed by an error. `Ok` means the session completed, not that it has one displayable final.

`CollectorLimits` defaults to 128 final updates and 8 MiB of original protobuf bytes across those finals. Each recognition-complete response counts, including duplicates and empty markers; partials do not enter `DictationResult::final_updates`. This is a retention bound, not an exact heap limit. The result preserves wire order without merging. If a consumer intentionally needs the other hypothesis containers as well, `collect_segments` has separate retention semantics and does not assemble text.

Set `SessionOptions::timeout` (default 30 s) for the overall session and `context_timeout` (default 2 s) for a late provider. Source cleanup is awaited and can outlast session budgets; it must cooperate. Size limits, timeouts, upload failures, late gRPC status, and S3 errors are failures, not partial success.

Admission takes ownership even if the future is never polled; a rejected source still gets cleanup. Dropping the collector signals cancellation of its session but cannot await cleanup. For a shared engine, keep it alive for other sessions; do not close it merely to dismiss one preview. At shutdown, stop producers, drop/cancel outstanding collecting futures, then await `Engine::close()` (permanently closes the engine). Check its distinct cleanup/background errors even if you already handled a collection failure. If the transport was supplied to `Engine::new`, close it separately after all its users are done. Do not discard the runtime first.

### Fail fast after audio ends (opt in)

Pass `SessionOptions::default().with_recommended_finalize_timeout()` to either PCM helper or engine collector (or set `finalize_timeout: Some(RECOMMENDED_FINALIZE_TIMEOUT)` on custom options). Healthy native sessions in the measured campaign finished 0.27–0.49 s after audio end; **5 s** gives room for variance and late context without waiting out the full RPC deadline on stalls. This is a starting point for interactive apps, not a service guarantee. Library and CLI defaults remain unchanged: no separate post-audio budget.

The budget starts at normal audio EOF **and successful cleanup** and covers late context, upload END, gRPC completion, and successful EOF publication. It cannot extend the overall timeout, stop the audio phase, or preempt a blocking observer. If it expires first, collection fails with `EngineError::FinalizeTimeout`; discard previews and don't treat prior candidates as success. In either CLI mode, pass whole seconds explicitly, e.g. `rambler dictate --finalize-timeout 5 input.ogg` or `rambler dictate-wav --finalize-timeout 5 input.wav` (with authorized configuration).

### Offer one explicit user retry (opt in)

Before collection, call `RetryAudio::capture(encoded_source, RetryAudioLimits { max_bytes, max_chunks })`; pass the returned wrapped source to the collector and keep the separate handle. For PCM, compose `pcm_channel` with `Pcm16Opus` as in `minimal_app`, and wrap the encoder's output, not its PCM input. The first upload behaves normally even if either retention cap is exceeded; on overflow the SDK discards the capture and never exposes a truncated retry. Normal source EOF and successful cleanup are required, including when the first RPC fails.

After a **failed collection** and only on the user's request, consume the handle with `retained.into_source()`. If it returns audio, pass it to a new collector with fresh options and current editor context (or confirm the original context is still valid). The encoded audio bytes and chunk boundaries are identical; timing, context, metadata and the RPC itself are new. Use a replacement engine explicitly if the old connection died. No started RPC is resumed or automatically replayed. A failed collection does not prove the server processed no audio.

Only the retained encoded audio is copied, in process memory; the SDK neither persists nor securely erases it. The caps count encoded bytes and nonempty chunks, not allocator overhead. Drop the handle on success, cancellation or dismissal and bound how long your app offers the retry. The handle is single-use; if consumed, the retry source owns the bytes until consumed or dropped.

### Retain safe terminal diagnostics

To diagnose failures without logging payloads, start a session with `let session = engine.start(audio, options)?;`, clone its `let diagnostics = session.diagnostics();`, then await `collect_session(session, limits)`. The retained `diagnostics.final_snapshot()` works even when the collector returns an error or after the engine has finished cleanup following a dropped collector. With a manually driven session, await terminal `next()` or `close()` first. Reading the snapshot neither acknowledges errors nor keeps the RPC alive.

The CLI emits the same structural counters and offsets under `timeline` on error; it is `null` before a session starts. All times are relative to engine session start. The upload dates mark complete protobuf messages and HTTP/2 `END_STREAM` accepted by **local h2**, not peer receipt. Compare produced and submitted protobuf counts to distinguish a backed-up upload from a loss of responses after END. The first and last response times reflect local gRPC deframing for messages handed to the engine; `responses_deframed` can be greater than `responses_received` if cancellation prevents draining the queue. The last response's numeric S3 status (if present) is distinct from the gRPC terminal code and from recognition finality. `last_partial_response_seconds` records the last response carrying decoded partial hypotheses, not heartbeats. `recognition_finals_received` counts recognition-state-1 responses; the separate `saw_s3_end` flag records global S3 completion. A partial after local END, without either completion marker, is distinguishable from silence. The timeline contains no audio, text, context content, headers, credentials, or session ID and does not authorize replay.

## 5. Render or propose only after `Ok`

- Use `DictationResult::text()` for strict text projection, or `render(RenderPolicy::AsrOnly)` for recognition text only. These require a unique final hypothesis and refuse blocked or uninterpreted safety status across retained results; do not bypass errors by falling back to raw text. Strict projected text can include edit text; rendering is never permission to edit.
- For a replacement, capture the *actual sent editor window* (`before`, `selected`, `after`) together with a document identity, range anchor, and revision in one editor read transaction. Set `EditingContext::cursor_aware` when supplying nonempty `selected` or `after`. For `ContextInput::Static`, pair the revision with the supplied snapshot. For `ContextInput::Provider`, use a fresh per-session provider and store the pair at its late read, not at session start. It is invoked once on normal audio EOF and completed cleanup; `Ok(None)` omits context, while a provider error or timeout fails the session. The result carries the captured text window, not the application's revision or range identity.
- Call `replacement_proposal(&completed, Some(captured_revision))`. It refuses unsafe, ambiguous, unsupported, or context-free results. The strict proposal is a replacement of the **captured window**, not a text delta or a whole-document replacement. `selection_utf16_range()` is available when the proposal contains a replacement context; its offsets are local to the replacement string. Treat an empty replacement as data requiring the same approval, not as a command.
- The user/application must explicitly approve an edit. In the *same* editor transaction as applying it, verify the document identity and window anchor, read the current window and revision, then call `proposal.validate_current(&current_window, Some(&current_revision))`. If anything changed, discard the proposal. Use revisions that change even if text is edited and restored; window text alone cannot detect that race. Do not coerce integer and string revisions or reuse a revision from a different snapshot. The [engine example](../../crates/rambler-engine/src/lib.rs) shows pairing a late window with a revision and validating a proposal for review; it intentionally does not mutate anything.

### Opt in to selection rewrites

On 2026-11-18, one native CLI session against the real service, with a selected sentence and a single synthetic utterance (“Make this more polite.”), returned a polite rewrite as VoiceEdit intent 3 (`REWRITE`), active field 7 (`edited_context`). This is one observation, not broader parity evidence. Strict projection still treats this combination as ambiguous.

For selection rewrites, use `render(RenderPolicy::S3ClientCompatible)` for display and `replacement_proposal_with_policy(&completed, Some(captured_revision), RenderPolicy::S3ClientCompatible)` for an editor proposal. Field 7 proposes a replacement for the **captured window**, not just the selected substring. Keep the same approval and document identity, anchor, and revision validation above; don't use rendered text or an ASR fallback as an edit. Both APIs still refuse blocked or uninterpreted safety status.

For inspection, `rambler dictate` and `rambler dictate-wav` accept `--result-policy strict|asr-only|s3-client-compatible` (choose one value). Omitting it preserves strict behavior and the existing JSON shape. Explicit choices add `provenance` to successful output: `hypothesis_text`, `voice_edit_text`, or `voice_edit_context`.

### Opt in to a single local text splice

If the host needs one range replacement instead of the whole captured window, use `proposal.derive_scalar_splice(&current_window, &current_revision)` or opt in to `proposal.derive_grapheme_splice(&current_window, &current_revision)` on the **unmodified proposal** returned by the chosen policy. Use the strict proposal by default; the [minimal WAV application](#minimal-wav-application) explicitly opts in to a separate compatible review. Stop on proposal errors; do not build a proposal from rendered text, a preview, or an ASR fallback. Both helpers repeat exact `before`/`selected`/`after` and revision validation and refuse an unversioned proposal with `ContextConflict`. The same text after an intervening edit still conflicts if the revision has advanced. Identical text and revision in a *different document or window* are not proof of identity; the host must verify the original document and anchor and derive again in the transaction for any explicitly approved apply.

`Ok(Some(splice))` describes the half-open `original_utf16_range()` measured from the **original window start**, with exact `inserted_text()` for that range. The scalar variant preserves the longest equal prefix first (it wins ties on repeated text), then the longest non-overlapping equal suffix. For `The kite is blue. Marker stays.` → `The kite is green. Marker stays.`, both variants return `12..16` and `green`; never pass the full replacement as insertion at the cursor. `Ok(None)` means no text change; an empty range is an insertion, and empty text with a nonempty range is a deletion. The optional `selection_utf16_range()` stays separate, relative to the **resulting window**, and may change even when there is no text splice.

The scalar variant never splits a surrogate pair but may split a grapheme. The grapheme variant widens that same splice to boundaries shared by the two complete captured windows; a cluster may span the `before`/`selected`/`after` split. Neither variant normalizes text. The engine uses the small, maintained, dependency-free [`unicode-segmentation`](https://github.com/unicode-rs/unicode-segmentation) crate for UAX #29 extended grapheme boundaries rather than implementing a partial set of rules.

If the host requires grapheme alignment, verify both splice boundaries in the original and proposed *documents* with the host's segmentation rules. Text outside the capture can affect even interior boundaries (for example, in a run of regional indicators). If boundary validation is unavailable or fails, do not use the splice; do not round offsets. The SDK never performs editor/system effects. Command classification is separate, opt-in, and descriptive only; a splice has no SEND or CLEAR semantics, even when it contains those words.

## Minimal WAV application

[`minimal_app`](../../crates/rambler-cli/examples/minimal_app.rs) feeds a conventional mono PCM16LE RIFF/WAVE file (16 or 48 kHz, up to 3 MB) through `pcm_channel`, then `Pcm48kTo16k` only for 48 kHz, then `Pcm16Opus`. The observer replaces one provisional preview slot and clears it when collection ends. After successful collection the app prints strict text if available, and separately asks `S3ClientCompatible` for a read-only rewrite proposal against an invented selection. A context rewrite can be eligible for compatible review even when strict text is withheld; no ASR fallback is used. The splice is window-local, the resulting selection is separate, and nothing is applied.

```sh
# Offline: synthetic WAVs at both rates and an owned IPv4 localhost fixture; no credentials.
cargo +stable test --locked --offline -p rambler-cli --features audio --example minimal_app

# Optional app usage: provide your authorized TLS authority, input, and credentials.
cargo +stable run --locked --offline -p rambler-cli --features audio --example minimal_app -- --authority YOUR_HOST:443 input.wav
```

The app warms the connection and opts into the recommended 5 s finalization budget. To demonstrate a user-triggered retry, add `--offer-retry`: the example retains up to 1 MiB and 4096 nonempty encoded chunks in memory and, on collection failure with complete retained audio, asks once at a terminal. Only typing `y` starts a new session. Without the flag it never retains retry audio.

The app resolves credentials only through `EnvironmentCredentials` from `RAMBLER_API_KEY`, `RAMBLER_ANDROID_PACKAGE`, and `RAMBLER_ANDROID_CERT`; it has no default service and does not read a `.env` file. The sample WAV reader buffers a small local file to support both input rates; for streaming a 16 kHz file, see [`WavAudio`](../../crates/rambler-audio/src/wav.rs).

For microphone integration, replace the WAV producer with your host's capture backend. Feed ordered mono PCM16LE at a known 16 or 48 kHz into the same bounded channel from an async producer, not a capture callback. Use a bounded handoff, cancel capture when collection ends, and call `finish()` only on intentional EOF. No microphone dependency or capture is included.

## Local verification

From the workspace root, the public-API tests and examples below need no microphone or real service. The collector test uses only invented audio/response fixtures on localhost. The PCM example exercises actual Opus encoding offline.

```sh
cargo +stable test --locked --offline -p rambler-engine --test observed_dictation
cargo +stable test --locked --offline -p rambler-engine --test app_reliability
cargo +stable test --locked --offline -p rambler-engine --test splice
cargo +stable test --locked --offline -p rambler-engine --doc
cargo +stable test --locked --offline -p rambler-cli --features audio --test pcm_sdk
cargo +stable test --locked --offline -p rambler-audio --doc
cargo +stable run --locked --offline -p rambler-audio --example pcm_channel
```
