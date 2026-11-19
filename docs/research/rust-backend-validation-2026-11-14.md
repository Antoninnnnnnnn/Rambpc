# Rust backend validation — 14 November 2026 milestone

This document describes a milestone, not a backend declared perfect or finished. Consolidation and validation continue; verified containers and the Ogg validator are now integrated. Each item of evidence below has its own scope; together they do not guarantee a general absence of bugs.

## Follow-up: shared raw and request options

The R6 snapshot then incorporates immutable raw sharing, regular-file preflight and RecognizerInfo 7/40/64 options. **274 workspace tests** pass on Windows stable/1.88 and Linux 1.88; four example tests, 30 feature-off, Clippy/format and release builds pass. The 262 counts below describe the previous milestone, not a rewritten result. Tests remain offline/loopback. Three distinct network trials then completed on the new binary, with fields 7/40 omitted then explicitly false/true: complete hashed PCM, three identical hashed texts, no error and no retry. This is evidence of acceptance, not preference effectiveness; [closed report](../evidence/rust-recognizer-options-public.json). The extended runner was also revalidated in Linux snapshot R7 (same counts).

[Memory profile](rust-backend-profiling.md): −70.82% logical retention with identical payloads, not an RSS measurement; raw API migrated to `Bytes` and a small child potentially retaining its root are documented. [Files](rust-file-inputs.md): blocking FIFOs/pipes are refused before RPC. [Options](rust-recognizer-options.md): identical omitted defaults, server effect not demonstrated and safety unchanged.

## Results and presentation

- In `RecognitionResponse`, protobuf fields 3 and 4 are respectively `language` and `spoken_language`, not `result_id` and `alternative_text`. This breaks the Rust 0.1 source API, without changing wire numbers or the frozen Python oracle. See [field analysis](rust-response-language.md).
- Explicit rendering offers `AsrOnly` and `S3ClientCompatible` among others; the latter is a deliberate text-compatibility choice, not Android candidate selection or proof of server behavior. It accepts intents 2/3 and VoiceEdit members 3/7; otherwise it falls back to ASR. Provenance, intent and any fallback reason are exposed. Safety and uniqueness checks remain applied; an accepted empty edit does not become ASR text.
- In compatible mode, a proposal requires intent 2/field 3 or 7, or intent 3/field 7 only. Intent 3/field 3 remains renderable but produces no proposal; text alone does not prove replacement scope. The proposal concerns the captured context window. No rendering policy authorizes effects on the editor or system: commands remain without automatic effect.
- Segment collection retains responses containing a 3/5/7 container, regardless of state, and state 1 markers, in order with client ordinal. Partial 4 alone remains an unretained preview. No global assembly or deduplication; an edited candidate may already include history. State 1 alone does not prove session success.
- Parts of a partial result are presented within their response, never concatenated across responses. The observed rule tests `score < 0.9`: NaN and +inf can therefore enter the stable prefix, -inf triggers the tentative part; non-finite scores are flagged, without normalization. Absent confidence is 0; absent text is ignored, whereas present empty text participates in locking the tentative part.

## Performance and local coverage

The [offline profile](rust-backend-profiling.md) compares identical bytes and establishes, for audio request construction measured on Windows, a change from 8 allocations and 3 reallocations to one allocation with no reallocation. This is a measured local CPU gain, not proof of WAN or remote dictation latency gain; local pipeline distributions overlap and do not support a robust overall gain.

On the consolidated tree, the workspace passes **262 tests** (one oracle ignored by default) on Windows stable 1.99.1 and 1.88.0 and on Linux 1.88. The `session_reuse` example passes 4 tests, the feature-off CLI 27; Clippy all targets/features and formatting pass on Windows stable. Windows release builds with/without audio pass, including the normal build without audio with a deliberately nonexistent `CMAKE`. The independent audio oracle separately passes 57 containers; the Python reference/tools pass 656 tests in 13.82 s, without changing their frozen policy. The CI matrix covers 1.88/stable on Windows/Linux/macOS and the explicit example, but has not been run remotely.

Linux snapshot **R5** contains containers, the Ogg validator and engine error-retention fixes. It passes 262 workspace tests + 4 example + 27 feature-off and the audio release build, on WSL2 Ubuntu 24.04, Rust 1.88, GCC 13.3 and CMake 3.31.10. These are native Linux ELF executables, not a Windows cross-check or Android. The first snapshot timed out on 8 MiB in 5 s; `TCP_NODELAY` in the custom TCP fixture fixed it without increasing the timeout. An old test improperly discarded a driver error and was corrected for the conservative contract. These intermediate failures remain distinct from R5 success. macOS was not run.

## Real sessions and interpretation limits

The [public evidence](../evidence/rust-native-session-reuse-public.json) describes three controlled sessions on Windows, in one `Engine`, with a single explicit connection call, in approximately 8.3 s total, without retry. All three PCM inputs were consumed through EOF, closed and hash-verified; cases include editing with emoji with exact rendering and French. The physical number of remote TCP connections was not independently observed. Markers and the exact hash demonstrate the stated assertions, not general recognition quality.

The [first batch](../evidence/rust-reuse-initial-marker-failure.json), stopped after one session, remains retained as a failure or inconclusive result: the expected marker differed between `eleven` and `11`. It does not validate reuse. The later batch declared this numeric alternative before execution; it does not retroactively requalify the first.

### Natural voices on consolidated code

[Three clips already licensed CC BY 4.0](../evidence/rust-native-natural-public.json), two English and one French (18.885 s), pass in one native engine, one explicit connection call, zero retries, empty stderr. PCM actually consumed matches provenance hashes. Markers were declared before execution; text hashes also match historical Python outputs, a retrospective comparison separate from pass criteria. Source/lock/binary are hashed. The Python launcher is not part of the runtime; child PATH is system-only. This small sample of read books proves neither general WER, endurance nor WAN gain.

## Ogg and error retention

The CLI file source progressively validates the Ogg/Opus mono 16k/v1/mapping0 profile, one page per chunk, without decoding audio or modifying bytes. It checks CRC, continuity/serial/sequence, headers, granules and EOS then physical EOF; packets/Tags limited to 64 KiB. A valid empty EOS (RFC3533) was also verified with PyAV, and the start of OpusTags is required on the second page (RFC7845). Codec payload and quality are not certified; legal padding beyond the limits is outside the profile.

Two error losses were reproduced before correction: `Engine::close` sometimes overwrote an already acknowledged driver error with a session error, and a primary audio error could mask a distinct secondary cleanup error. The primary remains unchanged, the secondary remains available through explicit close, even after observation/reaping; no implicit grouped acknowledgment. Cancelled/concurrent closes, deadline, real loopback GOAWAY and publication stress are covered by 13 new tests. Audio deduplication only by original object identity, never by redacted text/code. No public API change.

## Transport and remaining limits

The [lifecycle contract](rust-connection-lifecycle.md) provides no automatic reconnection, retry or replay. An independent driver error may be reported after a session gRPC error, even if they may share a cause. This is a conservative fallback: the public h2 API exposes no identity allowing reliable correlation of these errors; exact deduplication is not guaranteed. Tested scenarios do not prove absence of concurrency defects.

The tested authentication profile works without account/OAuth/Zwieback, with application metadata injectable per session. Automatic provisioning, recovery after revocation and future access durability are not demonstrated. No unestablished COMPOSE behavior is assumed. Measurements and tests guarantee neither WAN endurance, behavior behind all proxies nor unexecuted macOS compatibility.
