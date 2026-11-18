# Bounded handoff for caller-owned PCM capture

The long-dictation collector stops reading input while it awaits a segment’s final response. The existing `pcm_channel()` intentionally allows one queued chunk and makes the producer wait asynchronously for a permit. That is useful for a file or a producer that can wait, but a capture callback can keep receiving PCM during the pause.

`rambler_audio::capture_channel` is a separate opt-in SDK input. It does not open a microphone, choose a device or service, install a callback, or change the capacity-one channel. Pass its `CaptureInput` to `collect_long_pcm_dictation` for 16 kHz mono PCM16LE, or use [streamed segment delivery](recoverable-segment-delivery.md) to take ownership of each clean result as it arrives. For 48 kHz, put **one** `Pcm48kTo16k` around the input, outside the collector: the converter must retain its history across segment boundaries.

```rust
use rambler_audio::{
    CaptureConfig, CaptureError, CaptureInput, CaptureSender, InteractivePcmProfile,
    LongDictationError, LongDictationResult, Pcm48kTo16k, capture_channel,
    collect_long_pcm_dictation,
};
use rambler_engine::{CollectorLimits, Engine};

fn prepare_48k() -> Result<(CaptureSender, CaptureInput), CaptureError> {
    // Call before starting callbacks. 40 × 20 ms = 800 ms when the queue is empty.
    capture_channel(CaptureConfig { max_chunks: 40, max_chunk_bytes: 1_920 })
}

fn on_capture(sender: &mut CaptureSender, pcm16le: &[u8]) -> Result<(), CaptureError> {
    sender.offer(pcm16le) // No await. On Err, notify the host and stop feeding.
}

async fn dictate_48k(engine: &Engine, input: CaptureInput)
    -> Result<LongDictationResult, LongDictationError>
{
    let profile = InteractivePcmProfile::default();
    collect_long_pcm_dictation(
        engine, Pcm48kTo16k::new(input), profile.session, CollectorLimits::default(),
        profile.segmentation,
    ).await
}
```

The host gives the unique producer to its serial callback and drives the consumer concurrently. After the last callback (including any short, complete-sample tail), reclaim the producer and call `finish()` to drain to normal EOF. Call `abort()` instead to discard unread audio; dropping an unfinished producer also aborts. In either case await the collection and close the engine as usual. `offer(…) -> Ok` and `finish() -> Ok` are not engine or network acknowledgements.

## Bounds and faults

- Each offer contains one nonempty chunk of complete samples. `max_chunk_bytes` is even and positive; each slot holds up to that many bytes, or half as many 16-bit samples. Up to `max_chunks` chunks may be unread. A short chunk still occupies one slot. There are hard per-handoff limits of 4,096 slots and 8 MiB of reserved PCM.
- The handoff reserves `max_chunks * max_chunk_bytes` payload bytes plus a length word per slot and fixed shared state. `offer` copies the borrowed slice into that storage, so it cannot retain a large backing allocation via a small `Bytes` slice. The ring itself does not lock, allocate, wait for capacity, or spawn work. `offer` can call the consumer's waker synchronously; that code belongs to the executor and is outside the ring's latency and allocation guarantees. `next` allocates one independent output chunk on the consumer side. Downstream allocations (converter, encoder, sessions and completed results) are separate. The reservation is freed when both endpoints are gone.
- **Normal finish:** previously accepted chunks drain in order, then clean EOF. **Abort/drop:** unread chunks are discarded and the source reports `CaptureError::Aborted`; a read racing an abort may already have obtained a chunk. **Overflow:** a full queue or an oversized even chunk rejects the entire chunk and latches `CaptureError::Overflow`. Empty or odd-length offers likewise reject and latch `CaptureError::InvalidChunk`. These faults drain the ordered accepted prefix before reporting the typed error (repeated until source close). Neither a retry, freed slot, producer drop nor a later `finish()`/`abort()` can erase the fault. Don’t silently continue or replay missing audio. A collector can still commit a clean rollover whose entire input was in that prefix. It returns a `LongDictationError` whose `completed` contains only cleanly completed segments; the capture fault is available from its `EngineError::Audio` via `SourceError::original()`.
- Consumer close/drop discards the unread queue, releases its ownership and rejects further offers; an offer racing close may have been accepted but not delivered. Stop the callback and drop its handle to free the reservation. If a session or transport fails independently first, that failure can end collection before it observes the capture fault, so also handle errors at the producer.

At 20 ms per chunk, 30 slots hold nominally 600 ms of PCM from an empty ring: 19,200 bytes / 9,600 samples at 16 kHz, or 57,600 bytes / 28,800 samples at 48 kHz. Size for the expected pause **plus** existing backlog, arrival jitter, and callback sizes. The buffer handles finite pauses, not a sustained producer/consumer rate mismatch.

## Reproduce without microphone or provider

From the workspace with the normal [native toolchain](../../crates/rambler-audio/BUILDING.md) and cached locked dependencies:

```sh
cargo fmt --all -- --check
cargo test --offline --locked -p rambler-audio --test capture_handoff
cargo test --offline --locked -p rambler-cli --features audio --test capture_rollover
cargo clippy --offline --locked --workspace --all-targets --all-features -- -D warnings
```

The audio tests check exact synthetic sample order, concurrent wraparound, short tail, overflow, abort, close, wake-up after a cancelled read, and independence from caller `Bytes` backing storage. The CLI crate’s integration tests drive the public SDK against a loopback peer that withholds the first final. They queue 20 ms blocks representing 60, 600, and 900 ms while held, compare Ogg duration and Opus packets against the expected PCM, and verify the continuous 48→16 kHz path and a partial tail. Separate cases abort while held and fill all 54 slots before rejecting an additional block; both check the typed failure, retained clean segments, and cleanup without a successful upload for the failed segment. These are deterministic equivalents of stalled-consumer intervals, not measurements of callback latency, hardware capture, or speech quality.
