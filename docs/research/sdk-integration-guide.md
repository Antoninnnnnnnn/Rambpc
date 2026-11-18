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

## 2. Feed ordered caller audio

For a local Ogg/Opus file, give `Box::new(rambler_engine::OggAudio::open(path).await?)` to either collector. This is the same bounded, byte-preserving source used by `rambler dictate`; it needs no native Opus dependency. It preflights the first page and validates the remaining pages as they are read, without real-time pacing. A late framing or read error fails the session rather than pretending the input ended normally. See [`OggAudio`'s file policy](../../crates/rambler-engine/src/ogg.rs).

For other already encoded audio, supply a `Box<dyn AudioSource>` over **one continuous Ogg/Opus stream**, with bytes in order; do not concatenate independently encoded clips or pass PCM as encoded audio. `StreamAudio::new` wraps a `Stream<Item = Result<bytes::Bytes, SourceError>>` when synchronous Drop is sufficient. If the source needs asynchronous release, implement `AudioSource`: `next()` must tolerate being dropped; return `Ok(None)` only for normal EOF; make `close()` eventually complete. Keep Tokio alive for cleanup.

For PCM16 little-endian, 16 kHz, mono:

1. Call `rambler_audio::start_pcm_dictation(&engine, options, limits)` for `(mut writer, completed)`. Use `start_pcm_dictation_with_observer(&engine, options, limits, &mut observe)` if you want previews. Both admit the source before returning, even if you have not polled `completed` yet. Check `completed` for admission and session errors, not the writer.
2. Drive the writer and completion **concurrently**. For each chunk in order, `writer.reserve().await` waits for the one queue slot without taking the bytes; `permit.send(chunk)` transfers the chunk or returns it on rejection. Odd chunk boundaries are fine; input must end on a complete sample. Keep each chunk modest; the hard per-chunk limit is 8 MiB of logical `Bytes` length, not a bound on its backing allocation.
3. Call `writer.finish()` on normal input EOF so the encoder can drain and flush. Drop without finish means abandonment, not successful EOF. A failed or cancelled collector closes the input and wakes blocked reservations. Stop production on `Closed`; do not replay accepted chunks or convert a producer failure into `finish()`. `tokio::join!` works for bounded input that does not stall outside the PCM channel (see the [compile-checked slice example](../../crates/rambler-audio/src/dictation.rs)). For a live producer that also waits on capture, race that producer against `completed` with `tokio::select!` and drop the producer when the session ends, or signal and join it if it runs in a separate task. If the producer stops first, still await `completed`. `join!` alone does not stop a stalled capture wait.

Queue acceptance and `finish()` are not delivery, encoding, or session acknowledgements. These helpers use the default encoder. For custom encoder settings or local PCM progress, compose `pcm_channel()` with `Pcm16Opus` and use an engine collector directly. `Pcm16Opus::new_with_progress` supplies a `PcmProgressHandle::snapshot()`; these are encoder observations, not server consumption. See the [offline PCM example](../../crates/rambler-audio/examples/pcm_channel.rs) for the lower-level pattern.

## 3. Observe preview and progress without taking over the collector

For an already encoded `AudioSource`, use `collect_dictation(&engine, audio, options, limits)` if you only need the result, or `collect_dictation_with_observer(&engine, audio, options, limits, &mut observe)` for an ordered synchronous callback. The PCM helpers above call these same collectors. They admit the audio immediately and collect within the same bounds; you do **not** call `EngineSession::next()` alongside them.

In the callback, `DictationObservation` borrows a `Presentation` and a `SessionMetrics` snapshot. Match `PresentationContent::Recognition` and inspect `result.partial_result`; `PartialTranscript::from_result` gives the stable-prefix and tentative-suffix presentation of that **one** response. Replace the whole preview on each update; do not append across responses. “Stable” here is a client display rule, not a guarantee; the helper flags non-finite scores. Final hypotheses are alternatives, not text to concatenate. Recognition completion, receipt of S3 END, and passage of a heartbeat are observations, not authorization to display committed text or act on it.

The callback runs in the collecting task before the next response is drained. Keep it quick and nonblocking. If a different consumer needs an update, copy only what it needs into a bounded or replace-latest handoff; never log audio, text, context, raw responses, or metadata. Previews are not safety-screened: keep them separate from committed text, or omit them if your product requires safety-checked text. Never use previews for replacement, SEND, or CLEAR. On either completion or failure, clear the preview; on external cancellation, the outer owner must do this independently.

`responses_received` is a one-based response ordinal, not a segment ID. Audio and payload metrics describe encoded-source observations and produced protobufs, not network acknowledgements. Metrics arrive with each accepted decoded response, including status frames, not on a timer while the server is silent. There is no callback for engine EOF, terminal errors, or the response that exceeds the retention limit. An application error frame can itself be observed before the collector returns its error.

The [compile-checked engine example](../../crates/rambler-engine/src/lib.rs) shows a borrowed observer, replacement previews, and ordinary result handling in one function.

## 4. Wait for commitment and keep ownership explicit

On the healthy path the engine sends initialization, then encoded audio, then optional context after normal audio EOF and successful cleanup, then S3 END and upload EOF. The collector only returns `Ok` after observing S3 END and the engine's unique successful EOF (fully exhausted upload, gRPC OK, and successful cleanup), followed by clean terminal completion. A final-looking candidate or S3 END can still be followed by an error. `Ok` means the session completed, not that it has one displayable final.

`CollectorLimits` defaults to 128 final updates and 8 MiB of original protobuf bytes across those finals. Each recognition-complete response counts, including duplicates and empty markers; partials do not enter `DictationResult::final_updates`. This is a retention bound, not an exact heap limit. The result preserves wire order without merging. If a consumer intentionally needs the other hypothesis containers as well, `collect_segments` has separate retention semantics and does not assemble text.

Set `SessionOptions::timeout` (default 30 s) for the overall session and `context_timeout` (default 2 s) for a late provider. Optionally set `finalize_timeout` for the period from normal audio EOF **and successful cleanup** through late context, upload END, and successful EOF publication. It cannot extend the overall timeout, stop the audio phase, or preempt a blocking observer. Source cleanup is awaited and can outlast session budgets; it must cooperate. Size limits, timeouts, upload failures, late gRPC status, and S3 errors are failures, not partial success or permission to replay audio.

Admission takes ownership even if the future is never polled; a rejected source still gets cleanup. Dropping the collector signals cancellation of its session but cannot await cleanup. For a shared engine, keep it alive for other sessions; do not close it merely to dismiss one preview. At shutdown, stop producers, drop/cancel outstanding collecting futures, then await `Engine::close()` (permanently closes the engine). Check its distinct cleanup/background errors even if you already handled a collection failure. If the transport was supplied to `Engine::new`, close it separately after all its users are done. Do not discard the runtime first.

## 5. Render or propose only after `Ok`

- Use `DictationResult::text()` for strict text projection, or `render(RenderPolicy::AsrOnly)` for recognition text only. These require a unique final hypothesis and refuse blocked or uninterpreted safety status across retained results; do not bypass errors by falling back to raw text. Strict projected text can include edit text; rendering is never permission to edit.
- For a replacement, capture the *actual sent editor window* (`before`, `selected`, `after`) together with a document identity, range anchor, and revision in one editor read transaction. Set `EditingContext::cursor_aware` when supplying nonempty `selected` or `after`. For `ContextInput::Static`, pair the revision with the supplied snapshot. For `ContextInput::Provider`, use a fresh per-session provider and store the pair at its late read, not at session start. It is invoked once on normal audio EOF and completed cleanup; `Ok(None)` omits context, while a provider error or timeout fails the session. The result carries the captured text window, not the application's revision or range identity.
- Call `replacement_proposal(&completed, Some(captured_revision))`. It refuses unsafe, ambiguous, unsupported, or context-free results. The strict proposal is a replacement of the **captured window**, not a text delta or a whole-document replacement. `selection_utf16_range()` is available when the proposal contains a replacement context; its offsets are local to the replacement string. Treat an empty replacement as data requiring the same approval, not as a command.
- The user/application must explicitly approve an edit. In the *same* editor transaction as applying it, verify the document identity and window anchor, read the current window and revision, then call `proposal.validate_current(&current_window, Some(&current_revision))`. If anything changed, discard the proposal. Use revisions that change even if text is edited and restored; window text alone cannot detect that race. Do not coerce integer and string revisions or reuse a revision from a different snapshot. The [engine example](../../crates/rambler-engine/src/lib.rs) shows pairing a late window with a revision and validating a proposal for review; it intentionally does not mutate anything.

### Opt in to a single local text splice

If the host needs one range replacement instead of the whole captured window, call `proposal.derive_scalar_splice(&current_window, &current_revision)` on the **unmodified strict proposal**. Stop on proposal errors; do not build a proposal from rendered text, a preview, or an ASR fallback. The helper repeats exact `before`/`selected`/`after` and revision validation and refuses an unversioned proposal with `ContextConflict`. The same text after an intervening edit still conflicts if the revision has advanced. Identical text and revision in a *different document or window* are not proof of identity; the host must verify the original document and anchor and derive again in the transaction for any explicitly approved apply.

`Ok(Some(splice))` describes the half-open `original_utf16_range()` measured from the **original window start**, with exact `inserted_text()` for that range. Preserve the longest equal prefix first (it wins ties on repeated text), then the longest non-overlapping equal suffix. For `The kite is blue. Marker stays.` → `The kite is green. Marker stays.`, the range is `12..16` and the inserted text is `green`; never pass the full replacement as insertion at the cursor. `Ok(None)` means no text change; an empty range is an insertion, and empty text with a nonempty range is a deletion. The optional `selection_utf16_range()` stays separate, relative to the **resulting window**, and may change even when there is no text splice.

This is deliberately a **scalar**, not a grapheme, splice. It never splits a surrogate pair, but may split a grapheme (for example, a base letter and a combining mark), and does not normalize text. If the host requires grapheme-aligned edits, verify both splice boundaries in the original and proposed *document* with the host's segmentation rules; if unknown or unsupported, do not use the splice. Do not round offsets. The SDK never performs editor/system effects. Command classification is separate, opt-in, and descriptive only; a splice has no SEND or CLEAR semantics, even when it contains those words.

## Local verification

From the workspace root, the public-API tests and examples below need no microphone or real service. The collector test uses only invented audio/response fixtures on localhost. The PCM example exercises actual Opus encoding offline.

```sh
cargo +stable test --locked --offline -p rambler-engine --test observed_dictation
cargo +stable test --locked --offline -p rambler-engine --test splice
cargo +stable test --locked --offline -p rambler-engine --doc
cargo +stable test --locked --offline -p rambler-cli --features audio --test pcm_sdk
cargo +stable test --locked --offline -p rambler-audio --doc
cargo +stable run --locked --offline -p rambler-audio --example pcm_channel
```
