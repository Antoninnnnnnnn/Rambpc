# Decisions

## D001 — Evidence before optimization (2026-11-12)

**Decision.** Track four levels separately: reachable transport, a service
that understands the S3 request, usable S3 speech recognition, and observed
Rambler behavior. Each level requires its own evidence. Report remote
performance only for successful requests with recorded test conditions.

## D002 — Isolate third-party artifacts (2026-11-12)

**Decision.** Download third-party APKs and sources to new directories in the
scratchpad. Run Python analysis in isolation (`-I`) and leave extracted code
unexecuted. Keep only notes, reconstructed schemas needed for
interoperability, original scripts, and redacted results in Git. Keep
decompiled code out of redistributed material.

## D003 — Explicit authentication (2026-11-12)

**Decision.** A copied token does not establish independent authentication.
Document observed issuance, expiry, renewal, and dependencies. Use authorized
credentials and respect access denials.

## D004 — Language and architecture (2026-11-12)

**Decision for the first proof-of-concept client.** Use Python >= 3.11 and
`grpc.aio` (native gRPC C-core), with no Android dependency. Field numbers
still need to be reconstructed, so the ability to change and inspect messages
quickly takes priority. Consider Rust if measured local costs justify it.
Compare alternatives before claiming optimal performance. Keep byte transport
separate from the codec, authentication, and audio/result API. Connections can
be reused. Streams have isolated state. Retries must never replay dictation
automatically.

## D005 — Diagnostic security and privacy (2026-11-12)

**Decision.** Keep secrets and user content out of default logs. Run live
service tests explicitly, with limits and separately from local tests.
Avoid automatic audio replay after ambiguous errors to prevent duplicates and
keep sessions distinct.

## D006 — Preserve editing semantics (2026-11-12)

**Decision verified against three inspected live responses.** Results can
contain the entire edited history. Even the field named `transcript` may
differ from a verbatim transcript of the command. Expose text, intent, and
edited context separately. Require an explicit edit decision before inserting a
response. Convert Java UTF-16 ranges before using them as Python indices.
Preserve Rambler behavior by default and make any PixelBoard rendering policy
explicit.

## D007 — Explicit completion and late errors (2026-11-12)

**Decision.** Treat partial results as provisional. Wait for S3 completion
and the terminal gRPC status. Give late gRPC errors priority. A server that
closes before the upload finishes cannot confirm the truncated input.
Cancel and close session resources while leaving injected channels owned by
other components available. Producers must cooperate with asyncio cancellation.

## D008 — Distinct optimizations for batch and live input (2026-11-12)

**Measured on the synthetic corpus.** With an available file, 8,192-byte
blocks instead of 224-byte blocks reduced requests from 67 to 5 and saved
about 6.5% of the protobuf payload. The test did not establish a clear gain
in final response latency. Waiting for 8,192 bytes on a live source would
add delay.

With the same FFmpeg `-re` source, reducing Ogg pages from 1,000 ms to 20 ms
reduced the median time from source exhaustion to text from 935 ms to 325 ms
over three trials per setting. Decoded PCM was identical and the Ogg payload
grew. The measurement captures input buffering. Use the 20 ms low-latency
profile for the optional native encoder. Keep the comparisons, limitations,
and trade-offs in the documentation.

## D009 — Optional native PCM encoder (2026-11-12)

**Implemented and validated.** An optional PyAV dependency with bundled
libopus loads on demand and converts mono 16 kHz PCM16LE to Ogg/Opus at
24 kbps. The engine runs without an FFmpeg process. PyAV 19 needs
Python >= 3.12 and the pre-encoded input path supports Python >= 3.11.
Tests check the last samples and Ogg end marker, memory limits, and cleanup
after cancellation. A live PCM CLI run returned the expected text and
`VoiceEditingResult`, followed by S3 completion and gRPC OK. See
`evidence/native-pcm-cli.json`. The FFmpeg measurements and the native
encoder validation cover different setups.
