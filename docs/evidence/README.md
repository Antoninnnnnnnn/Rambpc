# Network observations

## Android development reference

- [android-reference-preflight1-public.json](android-reference-preflight1-public.json):
  Official APK installed and verified, then session lost before any PCM was armed.
  Cause unknown, initial state restored, no dictation or parity qualification.
- [android-reference-sdk-diagnostics-public.json](android-reference-sdk-diagnostics-public.json):
  Two separate 20 s diagnostics without audio, read-only SDK then three hooks
  installed. No execution of the ART constructor or retroactive resolution of the first failure.
- [android-reference-active-ime-isolation-public.json](android-reference-active-ime-isolation-public.json):
  Second active preflight failed, followed by five checks without audio. The minimal
  instrumented constructor reproduces SIGSEGV in `libart.so`, while read-only and a
  read hook alone hold for 20 s; exact cause unknown, restoration successful,
  zero arming. Two alternatives ruled out statically, no qualified dictation.
- [android-reference-editor-public.json](android-reference-editor-public.json):
  Original zero-permission editor source, signed/verified host build and controlled
  device smoke test. Fresh-key rebuild SHA separate from the APK measured on device.
  [Protocol and limits](../research/android-reference-validation.md).

## Rust port

- [Partial results and 48 kHz WAV](interactive-partial-wav48-windows-20261119.md):
  573 workspace tests passed and 2 ignored on Rust stable, plus 28 CLI tests without audio;
  5 targeted Rust 1.88 tests. The actual binary retains exact ranges and does not
  publish dictation in diagnostic mode. Verified on synthetic loopback.

- [Independent audit corrections](rust-integration-outcomes-windows-20261119.md):
  569 Rust tests passed and 2 ignored, plus 28 CLI tests without audio per Windows
  toolchain (stable and 1.88), and 947 Python tests passed. Explicit dictation
  results, rich selection, redacted telemetry and local Python transport exercised
  on fixtures, without another provider call.

- [Capture and integration fixes](rust-capture-fidelity-windows-20261118.md):
  559 tests passed and 2 ignored, plus 24 CLI tests without audio with each Windows
  toolchain (stable and 1.88.0). Rollover with complete PCM, recovery of validated
  segments, SDK/CLI joining, rich selection and explicit TCP recovery.
  Local peers only: no device, microphone or real service.

- [rust-pcm-engine-loopback.json](rust-pcm-engine-loopback.json):
  15 November 2026 publication of 14 November runs: five CLI tests of the actual
  PCM channel/encoder/engine/transport with a synthetic tonic peer. Received
  CRC/duration and Ogg/Opus packets, healthy order/late context, abandonment,
  cancelled blocked reservation and early END/OK. Abandonment proves only arrival
  of an Ogg header/page, not samples. 310 workspace tests pass on each of Windows
  stable/1.88 and Linux 1.88 (0 failed, 1 ignored), with 96 identical source hashes.
  Example 10 and feature-off 30 pass on both OSes; Windows Clippy/fmt and release
  builds/smokes qualified. Zero service/device, no additional remote dictation
  or production/dependency change.
  [Contracts, exact scopes and limits](../research/rust-pcm-channel.md).

- [rust-http2-boundaries.json](rust-http2-boundaries.json):
  Parallel review of 10 scopes, two HTTP/2 fixes reproduced RED/GREEN and independently
  reviewed: mixed-case MIME type and early validation of five forbidden metadata
  names. 305 workspace tests pass on Windows stable/1.88 and Linux 1.88 (one ignored
  oracle), native builds/smokes and identical hashed sources. Zero Rambler/device
  calls. [Contracts and limits](../research/rust-http2-boundaries.md).

- [rust-pcm-channel.json](rust-pcm-channel.json):
  New single-producer PCM input with capacity 1, explicit normal stop,
  abandonment distinct from healthy EOF, cancellable reservations and 19 dedicated
  tests. Offline native example with verified CRC/decoded duration; no service call.
  [Contract and limits](../research/rust-pcm-channel.md).

- [rust-credential-snapshots.json](rust-credential-snapshots.json):
  Snapshot isolation from a shared provider, rotation then removal during three
  RPCs on one loopback TCP connection, exact duplicates/binaries. Pre-send sensitivity
  checked separately; no HPACK inspection or remote renewal. Transport suite:
  63 tests pass on Windows stable/1.88 and Linux 1.88 with identical hashed sources;
  zero service/device calls. [Contract and limits](../research/rust-credential-snapshots.md).

- [rust-native-partials-msrv.json](rust-native-partials-msrv.json):
  10 targeted example tests pass on each of Windows/MSVC 1.88 and Linux 1.88,
  identical hashed source, zero service/device. Initial Windows CMake failure
  in a deeply nested target remains separate from success at the ordinary path.

- [rust-native-partials-public.json](rust-native-partials-public.json):
  One synthetic native real-time-rate trial, complete audio verified and healthy
  final; 20 responses but 0 field4. Partial qualification **inconclusive**,
  zero retries and no second call. [Observer and limits](../research/rust-native-partials.md).

- [rust-backend-decode-differential.json](rust-backend-decode-differential.json):
  6,791 unique inputs, 95,074 comparisons of complete models/errors,
  zero differences from the reference (production limits then instrumented budget).
  Finite Sol test, not proof of universal equivalence or an Opus review.

- [rust-backend-decode-defensive-capacity-profile.json](rust-backend-decode-defensive-capacity-profile.json):
  Bounded final variant, 39→24 allocations/response and favorable local CPU medians.
  Temporary sparse peaks around +18.5% and outliers retained; no WAN gain.
  [Method and limits](../research/rust-backend-decode-defensive-capacity.md).
  [rust-backend-decode-allocations-profile.json](rust-backend-decode-allocations-profile.json)
  retains historical measurements of the **rejected** V2 variant, not the shipped code.

- [rust-recognizer-options-public.json](rust-recognizer-options-public.json):
  Three synthetic calls from the new binary, fields 7/40 omitted then false/true,
  complete PCM source, same hashed text, zero retries. Acceptance on the tested
  profile, not general filter or punctuation effectiveness. Field64 not tested.

- [rust-backend-shared-raw-profile.json](rust-backend-shared-raw-profile.json):
  A/B of immutable sharing of decoded bytes, 100 CPU rows and 40 loopback sessions.
  Logical retention 12 635 008→3 686 912 bytes with identical payload (−70.82%).
  This is not an RSS measurement or WAN gain; `raw`/`active_raw` types migrated
  to `Bytes`, with possible retention of the root by a small child.

- [rust-native-natural-public.json](rust-native-natural-public.json): three licensed
  natural EN/FR excerpts (18.885 s), consolidated Rust backend, PCM actually consumed
  and hashed, three hashed renderings identical to historical Python outputs. Retrospective
  comparison, not a paired benchmark or guarantee of general quality.

- [rust-native-session-reuse-public.json](rust-native-session-reuse-public.json):
  Three controlled cases, one native engine and a single explicit connection call,
  hash of PCM actually consumed, editing with emoji and French, without retry.
  Remote physical TCP connections not counted; one connection is proven on loopback.
- [rust-reuse-initial-marker-failure.json](rust-reuse-initial-marker-failure.json):
  Original batch stopped at the first case, marker `eleven` versus rendering `11`. It remains
  inconclusive; the following qualified batch declared its variants before execution.
- [rust-backend-offline-profile.json](rust-backend-offline-profile.json):
  96 CPU measurements and 40 loopback sessions, reference and candidate byte-identical.
  Local allocation/serialization gain, no WAN gain claimed. The measured snapshot
  predates later changes; [method and limits](../research/rust-backend-profiling.md).

- [rust-incomplete-diagnostic-followup.json](rust-incomplete-diagnostic-followup.json):
  Consolidation with typed diagnostics and independent review (initial model attribution
  not certified; see correction in [validation](../research/rust-build-validation.md)). A 120 s qualification and
  a single 300 s trial pass, without reproducing or resolving the old 282 s incident.
  [Qualification](rust-diagnostic-endurance-120-public.json) ·
  [long diagnostic trial](rust-diagnostic-endurance-300-public.json).
  The old batch remains failed_or_inconclusive; no retroactive third success.

- [rust-pcm-functional-public.json](rust-pcm-functional-public.json): native PCM
  accelerated/progressive with the same text, intent 3 rewrite and selection with emoji;
  four real Windows calls, static libopus, source/lock/binary hashed.
- [rust-endurance-120-public.json](rust-endurance-120-public.json) and
  [rust-endurance-300-public.json](rust-endurance-300-public.json): healthy 120 s, then
  first 288 s healthy but second `incomplete` at 282 s, third not started.
  The long-batch report remains **failed_or_inconclusive**, not corrected to 3/3.
  Each session is a new process/channel; the measurements validate neither channel
  reuse nor general absence of leaks. The `incomplete` category comes from inspection
  of private CLI output; the old projection retains only its nonzero exit. Exact cause
  not established by this capture.
- [rust-windows-ctrl-c.json](rust-windows-ctrl-c.json): real Windows signal in
  an isolated console, loopback client/RPC cancelled, exit 130; no Google. The first
  driver timed out then was corrected, without changing the binary.

- [rust-first-slice-public.json](rust-first-slice-public.json): three real sessions
  of a **Rust** release CLI on Windows, already encoded Ogg/Opus: English,
  context editing and French. The binary works with a system PATH without
  Python/FFmpeg; Python is only a development launcher. TLS verified,
  S3 END and gRPC OK, exit after cleanup, zero retries. Exact hashes of the binary,
  lock and source files before their first integration commit.
  Known synthetic texts were inspected; no new independent binary inspection,
  proof of endurance/native PCM or Rust speedup. Original stdout/stderr captures
  remain in `artifacts/`, outside git.

This directory distinguishes transport diagnostics from actual Rambler responses.
The first two files below sent no audio or user identifier. Functional files added
later use controlled synthetic sentences and licensed public natural excerpts,
and in-memory application metadata whose values are never recorded here. Limits and
comparison conditions are in [performance.md](../performance.md).

## `connectivity.json`

A TCP connection followed by verified TLS to `speechs3proto2-pa.googleapis.com:443`
negotiated TLS 1.3 / ALPN `h2`. `dns_plus_tcp_ms` includes DNS resolution and TCP;
`tls_handshake_ms` covers only the TLS handshake after TCP. Single sample.
No HTTP/gRPC exchange during this check.

Reproduction in principle with standard Python: `socket.create_connection` then
`ssl.create_default_context().wrap_socket(..., server_hostname=host)`, with
`set_alpn_protocols(["h2"])` and `time.perf_counter` on either side. Do not
turn off certificate verification.

## `anonymous-empty-rpc.json`

An empty protobuf message, then half-close, without application metadata or
credentials: gRPC status `PERMISSION_DENIED`, “Permission denied on the service.”
No protobuf response received. This **does not prove** that a request conforming to
the real client would be refused, or that Android is required.

Reproduction (real network, one attempt, no audio data):

```sh
python tools/probe_empty_request.py --timeout 10 --output local/anonymous-probe.json
```

The script refuses to overwrite an existing result file. Install `grpcio` in the
virtual environment beforehand. Python/gRPC version, endpoint, parameters and
duration appear in the observation. The absence of fields from a real request and
client identification metadata greatly limits this test.

## Real functional responses

- `reconstructed-default-session.json`: first complete Ogg/Opus stream from
  Windows, Agentic service/routing, expected text, VoiceEditingResult2, S3 END then
  gRPC OK, without account/Zwieback. Audio duration 6.665 s sent accelerated.
- `functional-cases.json`: spoken correction, context before the cursor and French,
  through the public `RamblerEngine` API on a reused channel. SHA-256 verification
  of input actually consumed. The 33 response payloads were decoded again
  independently, without importing our engine/codec.
- `native-pcm-cli.json`: PCM CLI with PyAV/libopus, without an FFmpeg binary at
  runtime, in a new process; exit 0 and correct text. The report records hashes
  of the clean sources used.
- `performance-baseline.json`: 21 sequential sessions including warmup, comparisons
  of 224/8192 blocks and new/reused API channels. All successful; same texts and
  audio hashes, no proof of a new physical handshake for each API channel.
- `realtime-paging.json`: 6 sessions with FFmpeg `-re` source, 1000/20 ms pages,
  same decoded PCM and texts, separate producer and ending metrics.
- `native-realtime.json`: six native sessions on code 0814813, 20/1000 ms profile,
  complete PCM source, identical re-decoded Ogg across pages, no FFmpeg at runtime,
  code and dependency provenance recorded.
- `lifecycle.json`: selection/deferred context, voluntary cancellation, source
  closure then two isolated concurrent sessions on the same channel.
- `installed-wheel-smoke.json`: real dictation from the installed wheel in a clean
  venv without PyAV, local configuration, no APK/JADX read at runtime.
- `ci-0814813.json`: success of all four Windows/Linux × Python 3.11/3.14 jobs at
  the corresponding milestone; offline/loopback CI, not proof of Google network on Linux.
- `environment.json`: non-sensitive environment and uncontrolled conditions;
  snapshot after first trials, not retroactive code provenance.

Three **original** inspected final responses (not reconstructed) are public as
[fixtures](../../tests/fixtures/live_synthetic_responses.json), with exact SHA,
hex and synthetic sentences. Offline tests verify decoding and retention of unknown
fields. These fixtures contain no headers, credentials, cookies, account identifiers
or personal text. Other captures and audio remain ignored by git. Public text reports
are reserved for controlled corpora or material explicitly publishable under its
license, not personal dictations.

## 2026-11-13 refinement

- `refinement-page-grid.json`: 20 successful transports, five per 20/40/60/100 ms page,
  native source, `muxer` policy, all ordered final updates and decoded PCM identical.
  No raw protobuf capture. Default 20 ms retained; data and outlier to be interpreted
  according to [performance.md](../performance.md).
- `natural-corpus-inputs.json`: provenance, hashes and CC BY 4.0 attribution for
  the first corpus's English LibriSpeech excerpt. Audio not versioned.
- `refinement-capabilities-public.json`: redacted projection of the first five-case
  corpus. Five successful transports, but **4/5 renderings** in the code at the time:
  intent 3 not yet interpreted. This historical result remains unchanged.
- `refinement-long-realtime-public.json`: one 40.564 s PCM dictation at simulated rate,
  fully consumed, corrected text and final reference retained, complete collection
  41.454 s. Same text as accelerated case, not proof of extended endurance.
- `refinement-rewrite-public.json`: new real test after rendering extension,
  intent 3/field 3 rewritten text rendered successfully; no editor replacement.
- `refinement-wheel-smoke.json`: final 0.2.0 wheel built, installed in a clean venv
  without PyAV; imports, module equality with source and derived fixture rendering
  verified. Offline only, no additional Google call.
- [`live_rewrite_response.json`](../../tests/fixtures/live_rewrite_response.json):
  **derived/redacted fixture**, not a fourth original payload. Safe components
  retained, wrapper reconstructed, source and components hashed, extension 74956225
  removed. Independent source inspection: intent 3/field 3 confirmed;
  general replacement scope unknown.

The three `*-public.json` copies identify the source private report's hash/length,
projection tool hash and payload summaries. Raw bytes, unknown values and opaque
service/session identifiers are not published; the library preserves them for the
caller. The SDK code hash used appears in the versions; HEAD alone is insufficient
for the rewrite trial on a modified tree.

## Continuation: natural corpus, endurance and hints

- [reference-0.3-wheel-smoke.json](reference-0.3-wheel-smoke.json): 0.3.0 wheel
  built and installed in a clean venv without PyAV; public imports, modules
  identical to source, derived fixture and emission of hints/secondary locales
  verified offline. No Google call in this smoke test.

- [natural-expansion-inputs.json](natural-expansion-inputs.json): provenance and
  licenses for the ten additional excerpts, seven English and three French.
- [natural-expansion-public.json](natural-expansion-public.json): ten successful
  accelerated transports; [natural-expansion-quality.json](natural-expansion-quality.json)
  summarizes WER 3/109 EN and 3/79 FR, with reference and normalization limits.
- [natural-expansion-realtime-public.json](natural-expansion-realtime-public.json):
  three sessions at simulated rate, texts identical to accelerated trials.
- [endurance-120-public.json](endurance-120-public.json) and
  [endurance-300-public.json](endurance-300-public.json): one 118.682 s session
  then three of 288.352 s; complete source, markers and ordered finals checked.
  Sampled current RSS, CPU/wall and module hashes are reported; measurements
  on 0.2 baseline before hints, without proof of absence of leaks.
- [input-hints-public.json](input-hints-public.json): ten successful transports with
  hashed manifest and summaries; effects of biases, preferences and secondary languages
  limited to observations described in [refinement](../refinement.md).

These 13 + 4 + 10 transports, plus eight composition trials described in
[composition](../research/composition.md), make 35 continuation transports;
this is neither a list of 35 validated capabilities nor an addition to historical counts.
Structured endurance reports contain no raw protobuf, unknown values or service
identifiers; private continuation captures remain under `artifacts/continuation-private/`,
outside git. Published controlled-corpus texts remain subject to their provenance
and privacy review.

The offline tool `tools/sanitize_corpus_report.py` uses a closed allowlist and
refuses any existing output. Text is hashed by default; `--include-text` is used
only after review of a controlled corpus. The tool is not a universal anonymization
guarantee: text, local labels, machine metadata and even hashes may require review.
Do not publish complete reports from `validate_corpus.py --include-results` directly,
even for synthetic audio.
