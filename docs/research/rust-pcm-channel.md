# Bounded native PCM input and explicit normal stop

## Role

`rambler_audio::pcm_channel()` provides a new `(PcmSender, PcmInput)` pair,
without microphone, Android, file, credentials or connection. The integrator
produces mono 16 kHz PCM16LE and composes `Pcm16Opus::new(input, config)` before
passing this **Ogg/Opus-encoded** source to the existing engine. This is not a new
ASR backend or activation of an additional Rambler intent.

Normal stop was already possible by finishing a custom `AudioSource`. This helper
makes that contract available without reimplementing a queue and its lifecycle in
every integration. No change to codec profile, bitrate, quality, default page,
transport, context, result or safety policy.

## Producer and capacity

The producer is single and non-cloneable; each channel has independent state and
queue. Only one entry can be queued. Each chunk is bounded by `MAX_INPUT_BYTES`
(8 MiB of **logical length**), with empty or odd-length fragments accepted. The
encoder checks the last sample's integrity at EOF.

```rust,ignore
let owned_pcm: bytes::Bytes = /* PCM obtained by the integrator */;
// `owned_pcm` remains with the producer if this wait is cancelled.
let permit = writer.reserve().await?;
// No await between handing over the chunk and submitting it.
permit.send(owned_pcm)?;
```

Chunk handover is synchronous after reservation. Dropping a permit releases its
slot. Rejection of an oversized chunk or already closed source retains the bytes
in `PcmSendError::into_chunk()`; `kind()` gives a fixed reason. `Debug`, `Display`
and the error chain do not expose the payload. No automatic retry, replay,
resampling or pacing.

The queue does not bound memory held by the producer, the chunk already taken by
the encoder or a large root allocation retained by a `Bytes` slice. It is not a
global 8 MiB RSS bound. Producer and consumer must be driven **concurrently**:
submitting several chunks before starting the consumer would legitimately wait
for capacity.

## Optional PCM observation — local qualification

`Pcm16Opus::new_with_progress(source, config)` returns the encoder and a
`PcmProgressHandle`; `handle.snapshot()` gives a consistent and monotonic
`PcmProgress`, whose counters remain available after closure:

- `input_bytes: u64` counts bytes of upstream accepted chunks, including those
  still buffered without encoding at early closure; rejected oversized chunks do
  not count.
- `encoded_input_samples: u64` counts actual PCM samples in encoded and counted
  frames, without zero padding or lookahead-only flush.
- `encoded_frames: u64` counts codec frames, including padding and flush.
- `input_eof: bool` indicates only a natural `None` from the upstream source.
- `output_eof: bool` indicates natural encoded EOF after upstream cleanup and mux
  drain, never merely explicit close, error or Drop.

```rust,ignore
let (encoder, progress) = Pcm16Opus::new_with_progress(input, config);
// Drive encoder, then await its termination or cleanup.
let snapshot = progress.snapshot();
```

The `Pcm16Opus::new` constructor remains without observer allocation or lock; no
zero CPU cost or optimal performance is claimed. These local measurements are
neither PCM hashes, server acknowledgments, request terminal flags nor proof
against all loss or replay. Engine encoded metrics remain distinct. The
[local A/B qualification](rust-build-validation.md#pcm-progress-and-strict-admission--local-checkpoint-2026-11-15)
and its [evidence](../evidence/rust-pcm-progress-admission.json) distinguish full
suites and the focused rerun after a Clippy failure.

## Normal completion, abandonment and cancellation

- `writer.finish()` consumes the producer and announces intentional EOF. The
  source drains already accepted chunks before `None`. The encoder then closes its
  upstream and flushes the tail/lookahead through its actual EOS.
- Destroying the producer **without `finish()`** drains accepted chunks then
  returns `AudioError::InputAbandoned`, even for an empty channel. The error stays
  latched until explicit close; the encoder fabricates no successful EOS.
- `PcmInput::close()` and Drop immediately close the source, intentionally discard
  the queue and wake reservations. The transition occurs even if the future
  returned by close is never polled. A permit held before close rejects a submission
  that subsequently observes closure.
- A submission concurrent with close may be accepted and then discarded by that
  cancellation. **Acceptance and `finish()` are not acknowledgments of consumption,
  upload or successful dictation.** Always await the existing final conditions:
  audio/cleanup, S3 END, complete upload and gRPC OK.

`AudioError::InputAbandoned` is an additive public variant of the Rust 0.1
prototype; integrators with an exhaustive match must add it. No new wire field
or dependency.

## Actually executable offline example

```sh
cargo run -p rambler-audio --example pcm_channel --locked --offline
```

The example produces 1017 synthetic samples in 17-byte fragments, drives
producer/encoder with `tokio::join!`, then awaits cleanup. An independent Ogg
reader validates framing/CRC, the libopus decoder covers final granule and pre-skip:
**3051 samples at 48 kHz after trim**, one EOS. No audio content is displayed,
no device or service contacted. Opus remains lossy: this duration check does not
claim to reproduce input PCM identically.

This smoke is use of the public package, not remote dictation. Fidelity before/after
fragmentation is also tested by Opus packet equality with the existing memory
source, 20/1000 ms pages and independent decoded duration.

## Validation and limits

The 19 new tests are divided between channel contract and composition with the
encoder: capacity, wakeups verified without sleeps, cancelled reservations, unpolled
close, limits/return, isolation, tail, abandonment, truncation and empty EOF. Sol's
independent state-machine review is static, not a formal proof, Opus review or
exhaustive exploration of schedules.

[Build/test evidence](../evidence/rust-pcm-channel.json). The initial check failure
without `cmake` on PATH remains preserved; validation uses existing local CMake via
an explicit process variable, without global installation/configuration. No new
latency, endurance or remote quality gain is claimed. Android parity and
authentication limits remain distinct.

## Qualification through the engine and HTTP/2 peer

Optional audio CLI tests compose the real PCM channel, `Pcm16Opus`, `Engine` and
`GrpcTransport` against a loopback tonic server. Existing helpers are reused from
engine/audio tests; no dependency or substitute engine implementation is added.
The peer's S3 responses are synthetic, not recognition by the real service.

The five tests complement each other without attributing their assertions to every case:

- `pcm_channel_engine.rs` (2 tests) checks Ogg bytes actually received, CRC/framing
  via an independent Ogg reader, libopus decoded duration with trim, Opus packet
  equality with the memory source and init/audio/context/END order. The fragmented
  case uses 1607 samples and 20/1000 ms pages; the late context case uses 967
  samples. Audio EOF/cleanup precedes the provider call, but not EOF of the entire
  RequestSource. S3 END is insufficient while awaiting gRPC status; terminal
  PermissionDenied remains an error.
- `pcm_engine_failure.rs` (3 tests) checks typed producer abandonment, reservation
  actually Pending on a full queue then awakened by engine cancellation without
  transferring caller bytes, and early END/OK with producer still open classified
  `IncompleteSession`. Cleanup and failure context suppression are checked.
  **The abandonment case establishes only arrival of an audio payload beginning
  with OggS: an OpusHead page suffices, without proof of arrival of a sample packet,
  CRC or decoded duration in this case.**

### Committed base matrix, published November 15, 2026

This qualification belongs to integrated base `90a773a`, **not the uncommitted
local reliability batch**. Its counts and hashes are historical; no final rerun
of the new batch is inferred from the results below.

Runs remain dated **November 14, 2026**, not publication day. On each of Windows
stable, Windows/MSVC 1.88 and Linux/WSL 1.88: **310 workspace tests pass, 0 failed,
1 ignored**. These are 305 base tests plus the five tests above, not 930 distinct
tests or five remote dictations. The `session_reuse` example passes 10 tests and
the CLI without audio 30, on both OSes. Windows also validates strict Clippy,
formatting and release CLI with/without audio; Linux validates release audio CLI.
PCM example build and release smoke pass on both OSes: 1017 input samples, 3051
after trim at 48 kHz, 1 EOS, 0 service calls. No new production behavior or
dependency.

At publication, the 96 Windows/Linux manifest entries were strictly identical and
their hashes matched the qualified snapshot. The evidence publishes the complete
manifest's canonical digest and selected hashes of the **two actual CLI files**,
reused helpers and relevant library inputs. Report, log and Linux section hashes
had been rechecked without rerunning builds/tests/binaries. Files changed since
require new hashes after final validation, not implicit carryover of that old match.

The two final static counter-reviews identify no concrete blocking defect: blocked
reservations/early OK absent from the normal file are covered by the failure file;
healthy codec/order absent from the failure file are covered by the normal file.
The abandonment header-only limit remains. A first review was blocked by nonexistent
`pcm_engine_finish.rs` target, then corrected by resuming the workflow: neither
production failure nor initial review success. The author had named the failure file
`pcm_input_failures.rs` before renaming it `pcm_engine_failure.rs`; targeted logs
and all three workspace logs execute the **final names**, without inferring their
execution from old author commands. These reviews are static, without certified
attribution to the Opus model or formal proof of exhaustiveness.

[Separate integration evidence](../evidence/rust-pcm-engine-loopback.json).
It does not reattribute old channel/encoder tests to remote dictation, turn a
simulated peer into Google qualification or establish new Android parity, intent,
access availability or WAN performance. macOS remains not run; neither test counts
nor reviews prove a perfect engine or complete Gboard parity.

## Local reliability contract — qualification distinct from the committed base

Work on November 15 strengthens the source/transport boundary without changing
`writer.finish()` meaning: it announces producer EOF, not acknowledged dictation.
For supplied audio sources, abandonment, truncation or read error remains `Err` on
subsequent `AudioSource::next()` calls until explicit close; the encoder does not
fabricate healthy EOS after such an error. One-time upstream release and fault
persistence are distinct states. Close retains its own cleanup outcome.

Do not confuse this persistence with `rambler_transport::Session::next()`: that is
intentionally fused after its first terminal error, and its `finish()` retains the
outcome. A `TransportError::PrimaryAndCleanup` pair can preserve both causes; the
engine locally restores the primary and retains distinct cleanup once in its
`AudioCleanup` ledger. Explicit cancellation with failed cleanup remains `Cleanup`
at transport.

The first `source.next()` now waits for metadata, h2 ready and local `send_request`
success. This admission proves neither peer receipt/authentication nor detection
of silent loss behind NAT. A source constructor or independent producer may already
have worked before synchronous `start`. An accepted source is cleaned up once even
without a first chunk consumed. Synchronous raw transport rejection before
`Ok(Session)` only performs ordinary Drop; `Engine::start` supervises audio cleanup
even after rejection for invalid configuration or closed engine. Then await
`Engine::close().await`.

`SessionOptions.finalize_timeout: Option<Duration>` (default `None`, CLI
`--finalize-timeout`) provides a budget starting only after EOF **and successful
audio cleanup**. It bounds context, upload completion and status/delivery under
the hard 30 s deadline, which **includes speech**. `Some(Duration::ZERO)` is
rejected and a huge budget is capped at the hard deadline; no false EOF, `max_audio`
or automatic source stop is added. Cleanup remains cooperatively awaited beyond
deadlines.

All fallible waits now precede the private irreversible terminal commit: a published
healthy EOF no longer becomes an error and `eof_seconds` dates that successful
commit, not the last sample. Dedicated regressions and Windows/Linux status are
separate from the old 12 scenarios in the [reliability contract](rust-reliability-lifecycle.md);
the historical 310 tests above do not qualify this batch. No automatic replay/reconnect,
keepalive or performance gain is claimed, nor new Android/ART, authentication,
COMPOSE, macOS or real service qualification.

## Native preflight of recent code — November 15, 2026

The [preflight evidence](../evidence/rust-native-release-preflight-20261115.json)
records a NO_GO rejection at a real privacy barrier, without a service call. After
fixing the checker, all 11 offline test groups pass; interpreter/source/binary
checks pass, but the ACL check still rejects. Neither permissions nor admission
policy were changed. Live qualification of recent code, particularly terminal/
samples/cleanup and rendering, **was not performed**. No audio or identifier loaded,
no sentinel, reservation or CLI/service launch; a reservation would not prove
server receipt. Descendant confinement was not tested in execution. Old CI and
Windows live evidence remain unchanged; Linux remains NO_GO, phone budget 0/0/0,
without COMPOSE or Android parity claims.

## Native release regression — result distinct from reservation — November 16, 2026

The NO_GO above remains the correct observation of the previous preflight. The
[private root remediation](../evidence/rust-private-capture-root-20261115.json)
subsequently enabled new admission without changing the old ACL or policy; no
administration or settings changes. Protections do not guarantee against a hostile
process of the same user or administrator, content writes or all local races.

Frozen controller `78ce29130fef8ac828d0dac58e7bc2183ddc8861a985ab01c2f9eb753a5bc1f4`
passed 20 offline groups, a preflight, a strengthened locked-command and descendants
sentinel, then supervised local SAPI generation. One opportunity and capture were
consumed, without retry or replay. The controller process exited 0 with
`REGRESSION_HEALTHY_STRICT`; the candidate was qualified only after exit and capture
and provenance guards passed. Supervised cleanup completed. Public input metadata
is synthetic; no audio or raw capture is published.

HEAD at capture is `703f0eee82f6228044115f72be5744aba7c08d6f`, production
association `7327f51b89aeec537e8bf5ebaf6db3b0c25e5cef` and CLI hash
`3d1d2916e337419d4f2987084c2d7002cc02b59c2ad07b6e1e0fc91d7507cb8e`.
This is the release CLI, not the benchmark binary or a CI attestation. Input of
69,440 mono PCM16 samples represents 138,880 bytes and 4.34 s at 16 kHz; the
138,924-byte file adds 44 bytes. Preparation and capture counts agree, with 69,440
samples encoded and EOF/cleanup observed.

The 223 produced requests are **protobuf messages**, not RPCs; server-side RPC
count remains unknown. The 2.2043591 s elapsed concern an unpaced client session,
without WAN speed or quality conclusion. Terminal flags are inferred from the
verified CLI success contract, not an independent wire capture. Intent 2/active
field 7 qualifies a strict literal request, not COMPOSE or generation: retention
booleans are mechanical, semantics of a generated message remain unknown.

The [initial durable reservation](../evidence/rust-native-release-regression-20261115-budget.json)
remains immutable with its pre-execution result `not_observed_at_reservation`; the
[separate evidence](../evidence/rust-native-release-regression-20261116.json)
publishes the observed result without changing that historical fact. This single
synthetic case demonstrates neither representative ASR quality, perfection,
durable provisioning nor Android parity. Linux live, phone, other intents,
revocation and the long incident's cause remain open; historical failures are retained.

## Local CPU profile of the 48 kHz PCM session — November 18, 2026

After FIR symmetrization, the [offline Windows profile](../evidence/rust-native-pcm48-session-cpu-windows-20261118.md)
on 180 s of unpaced synthetic PCM attributes about 56% of session thread cycles to
Opus/Ogg, 17% to the decimator and 26–27% to the remainder (Engine, h2, local peer
and collector). Reading a ready source or through the actual bounded `pcm_channel`
retains this distribution. Table-based Ogg CRC produced the same bytes but won
only 13 CPU pairs out of 20 (threshold set at 15): not retained. These measurements
predict neither network latency nor real-time microphone/service consumption.
