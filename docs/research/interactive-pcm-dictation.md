# Interactive PCM dictation and end-of-capture timing

`rambler_audio::InteractivePcmProfile` is the opt-in companion to `collect_long_pcm_dictation` and `collect_long_pcm_dictation_with_observer`. Use it for context-free dictation whose PCM arrives as the person speaks. It is not a change to `SessionOptions::default()`, editor commands, or the ordinary one-session CLI.

```rust,ignore
let profile = rambler_audio::InteractivePcmProfile::default();
// `source` is host-owned PCM16LE mono at 16 kHz (e.g. PcmInput driven
// concurrently by PcmSender); `engine` is the host's connected engine.
let result = rambler_audio::collect_long_pcm_dictation(
    &engine, source, profile.session, CollectorLimits::default(), profile.segmentation,
).await;
match result {
    Ok(complete) => { /* render `complete.segments` or use `complete.text()` */ }
    Err(failure) => { /* keep `failure.completed`; report `failure.cause` */ }
}
```

- **Speech and pauses.** The default segment accepts at most 120 seconds of 16 kHz samples or 120 seconds on the local segment clock, whichever ends intake first. During the last five seconds of either budget, a quiet 100 ms block can cut it earlier. This is a PCM energy heuristic, not speech endpoint detection. Every admitted sample belongs to exactly one segment; a cut does not end the capture or insert a new empty RPC at source EOF. An optional `Pcm48kTo16k` adapter must precede this collector for 48 kHz mono. Sample ranges refer to the PCM after that adapter.
- **Budgets.** The overall default is 135 s **per RPC**: segment budget + 10 s for setup/drain + 5 s for finalization. The separate 5 s starts at clean encoded-source EOF and successful source cleanup, and includes request END, terminal gRPC status and engine EOF commitment. It is not a promise of five seconds from mic stop. The earlier overall deadline still wins if it is reached first. Cleanup is awaited even past a deadline. `InteractivePcmProfile::new` derives the overall budget for other segment and finalization values; any explicit `SessionOptions` overrides retain their normal meaning. The standard 2 s context timeout is unchanged, but this segmented route rejects non-`None` context.
- **Rollover and stopping.** Source capture can continue through successive segments on the same engine; no single RPC needs to span a five-minute capture. The next RPC waits for the current one to finish successfully. Hand PCM off from the capture callback to a bounded queue; plan for backpressure during rollover and decide how to stop/report overflow rather than dropping samples. The host must bound the whole capture and any idle wait before the next segment's first sample. At a voluntary stop the producer should enqueue its final samples and call `PcmSender::finish` (or return normal EOF from its own source), not drop the sender. Cancellation is an abort, not a way to flush the capture.
- **Results.** Only transport/protocol-validated segments enter the result. A later failure returns the earlier clean segments, not a complete joined transcript or an automatic replay. A clean segment is not necessarily renderable: its text must still pass safety and projection policy. Concatenation does not reconcile grammar or context across segment boundaries.

## CLI

Build the CLI with the `audio` feature. `rambler dictate-wav INPUT.wav --interactive` uses paced 16 kHz mono PCM WAV and the profile above. `--segment-seconds` sets the cap (6–280; five-second pause search stays fixed); `--finalize-timeout` and `--timeout` are optional explicit seconds per RPC. Without an explicit `--timeout`, the derived default uses the effective finalization budget. `--interactive` is separate from `--realtime`, which retains the single-session 30 s default, and is not compatible with editor context or an explicit `--page-duration-ms`.

Interactive stdout is one JSON object. It contains `completed_segments` in capture order, with half-open `[start, end]` `sample_range` pairs in 16 kHz input samples. If every segment projects usable text, `type: "result"` and `outcome: "usable_text"` carry joined `text` (exit 0).

A clean capture with both usable text and unusable segments now reports `type: "ambiguous"`, `outcome: "partial"`, `text: null` (exit 1). This replaces `no_result` or `needs_review` **only** for that mixed case. In `completed_segments`, consume `text` only from entries with `outcome: "usable_text"`; retain their `sample_range` and use the other entries' ranges and outcomes to locate gaps. Usable ranges may be non-contiguous. A `no_result` entry describes a projection, not acoustic silence; `needs_review` means no text was accepted for that range. Do not join across gaps or apply independently projected segments as edits. Captures with no usable segments still report `no_result` or `needs_review`, and empty input reports `no_result` with reason `no_segments`.

RPC, safety and cleanup errors remain errors (exit 1) even when usable results were already collected. Ctrl-C aborts the active collection instead of draining it (exit 130). Neither path emits joined `text`; both retain [earlier completed segments](recoverable-segment-delivery.md), with safety-rejected candidates withheld as before. Treat the whole JSON as private application data, including on error. `--diagnostics-only` keeps the outcome labels and sample ranges, but omits transcript and raw candidates. Non-interactive CLI output is unchanged.

## Independent SDK timing seam

The audio host owns the sample clock. For a single app-owned `EngineSession`, take `let diagnostics = session.diagnostics()` before passing it to `collect_session`. Timestamp the actual last captured sample (not stop-click, enqueue, encoder EOF, or elapsed file duration) on the same process monotonic clock as `std::time::Instant`. On successful collection, calculate:

```rust,ignore
let sample_to_validated_eof = diagnostics.validated_eof_at()
    .and_then(|eof| eof.checked_duration_since(last_sample_at));
```

`validated_eof_at()` is recorded at the engine's commitment after normal upload completion, S3 END, gRPC OK, cleanup and event queue reservation. It is absent on failure or before commitment. It deliberately does not authorize rendering or measure delivery to the editor. This seam does not modify the producer, consume replay audio, or invent a microphone time. The long collector currently reports completed sample ranges, not a per-segment absolute EOF instant. Do not substitute another segment's clock or derive capture timing by subtracting a nominal audio duration.

For measurement, bracket `Engine::connect_ready` with the same host clock and label cold versus reused connections. Record first/last sample at the source and first UI-eligible preview under the app's preview policy; `first_response_seconds` can be just control traffic. Session diagnostics already distinguish upload END, half-close, response times and final commitment. Keep text/audio outside metrics, and report failed sessions separately from latency observations.

Local qualification is synthetic: the accelerated cadence tests feed actual 29/30/31/120/300-second PCM amounts with pauses and a tail witness, decode the resulting Ogg/Opus independently, and exercise the 120-second cut ±1 sample. A separately gated loopback delays gRPC completion after S3 END, covering a released gate, a finalization expiry and an earlier overall deadline. The executable acceptance test uses paced WAV and confirms that a later timeout does not erase an earlier segment. The [release CLI at wall-clock pace](../evidence/rust-interactive-wall-cadence-windows-20261118.md) separately completed 31 s and 300 s WAVs against an owned loopback peer, and reported `finalize_timeout` when that peer withheld its final response. None of these test recognition or latency on a remote service, with a microphone, or on a mobile network.

Windows x64/MSVC, Rust 1.99.1: `cargo fmt --all --check` and `cargo clippy --locked --offline --workspace --all-features --all-targets -- -D warnings` passed. The focused Cargo run below passed 33 tests across the CLI parser, legacy finalization, PCM rollover and engine diagnostics. Validation used offline build tools, an isolated target, fresh scratch directories and only loopback traffic.

```sh
cargo test --locked --offline --all-features -p rambler-cli -p rambler-engine \
  --bin rambler --test long_pcm_dictation --test finalization_cli --test diagnostics
```
