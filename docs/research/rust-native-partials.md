# Native partial-result observation

## Scope

`session_reuse --observe-partials` is an opt-in developer observer of actual
RecognitionResult field4 responses. It calls the native `PartialTranscript`
projection and reports counters and ordered hashes, never transcript, raw payload,
unknown fields or service identifiers. Default collection/reporting is unchanged.
Only this opt-in allows a single-case manifest; the normal reuse validator still
requires2–3 cases. A one-case observation does not establish connection reuse.

The observer retains the strict state1 final-result collection and terminal engine
checks. A partial is never promoted to a final result, nor is a secondary
hypothesis selected. A final response alone is not successful completion: upload,
audio cleanup, S3 END and terminal gRPC status must still succeed. Engine close is
awaited even after early collection failure. No retries or audio replay are added.

## Hash contract and limits

For each of stable, tentative and rendered projections, a SHA256 digest covers
ordered records consisting of the UTF-8 byte length as a little-endian u64 followed
by those bytes. Empty projections participate; missing updates do not. This framing
distinguishes update boundaries, order and empty updates. The previews are not
concatenated into an assembled transcript. Counters and digests reset per session.

A field4 present alongside a state1 final marker can be observed, but is not
proof of pre-final streaming progress. `updates_before_first_state1` counts only
previews strictly before the first state1; `state1_coincident_updates` counts
previews in a state1 response. The latch is updated even when state1 has no field4,
and a later state0 does not reset it. Zero field4 updates are inconclusive for
partial qualification, even if the dictation completes successfully. This
observer does not establish Android UI parity, independent wire equivalence,
latency guarantees, confidence calibration or general speech quality.

## Reproduction

Build/test offline after fetching the locked dependency graph and arranging the
native C/CMake prerequisites described in [build validation](rust-build-validation.md):

```sh
cargo test -p rambler-cli --features audio --example session_reuse --locked --offline
cargo build --release -p rambler-cli --features audio --example session_reuse --locked --offline
```

For an explicitly authorized service observation, configure the three application
metadata environment variables as documented in [the native guide](../rust.md).
Use a controlled manifest with the existing input/PCM-hash/marker schema:

```sh
# Windows; no automatic .env loading by the executable.
target/release/examples/session_reuse.exe manifest.json --allow-live --observe-partials --realtime --timeout 30
```

Do not automatically repeat an inconclusive observation. New audio transmission
must have a distinct bounded purpose. Offline tests do not contact Google.

## Qualification status

The final observer passed10 focused offline example tests, including ordered hash
framing, score threshold/latch and nonfinite values, missing versus empty text,
pre/coincident/post-state1 previews, per-session reset, unique terminal completion,
late errors and result limits. Targeted strict Clippy, formatting and whitespace
checks passed on Windows stable1.99.1; the release example built and its actual
`--help` exposed the opt-in. Production library code/dependencies did not change;
this observer's new example tests were not separately executed on Linux/macOS or1.88.

The [single predeclared live observation](../evidence/rust-native-partials-public.json)
completed with a healthy dictation, all106,535 PCM samples consumed/hash-verified,
source EOF/close and encoded EOF confirmed, one retained final, exit0 and stderr0.
The native child used a Windows-system-only PATH; no Android/phone relay,
Python/FFmpeg runtime, routing/preference/context changes or retries were used.

**20 responses, zero field4 updates**: partial rendering and pre-state1 streaming
remain **inconclusive**. First response (~20.85ms) was not partial-text evidence.
No second call was made. This does not establish a general service limitation,
and does not justify changing unverified routing/settings to force partials.
Earlier captures retained only final summaries and cannot be retroactively
qualified as partial-render evidence. Measured code/binary hashes are preserved;
subsequent documentation edits do not redefine that measured snapshot.
