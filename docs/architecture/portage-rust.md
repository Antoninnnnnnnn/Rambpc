# Porting the Rambler engine to Rust

User decision of **November 13, 2026**: port the engine to Rust. Close the Python
batch already underway, then use Python as a conformance reference and research
tool, **without a Python runtime dependency in the final engine**.

The [desktop recommendation](application-desktop-multiplateforme.md) remains
relevant, but Tauri, React, microphone, shortcuts and system insertion are later
steps. Porting the core does not authorize an alternative recognition engine,
public Gemini API or Android relay.

## Implementation order

1. **Python 0.3.0 reference**: documented results and limits, fixtures,
   offline/loopback tests, real-service evidence and closing commit. Do not wait
   for universal service perfection to establish this contract.
2. **Independent Rust slice**: Ogg/Opus file CLI → real S3Bidi Rambler → confirmed
   result after successful S3 **and** gRPC completion. No microphone or GUI.
3. **Robustness and audio parity**: sessions, deferred context, cancellation,
   late errors, bounded collection, optional native PCM encoder and metrics.
4. **Controlled comparisons**: same sources and configurations, client time and
   CPU/RSS separated; do not attribute server/network variation to Rust.
5. **Desktop integrations** only after standalone engine validation.

## Contract to preserve

- Real service endpoint/method and routing documented in the
  [protocol](../research/protocol.md), verified TLS and locally supplied
  credentials, never embedded in a binary or exposed to an interface.
- Init → continuous Ogg/Opus stream → late context → `10 01 18 01` → half-close →
  drain through terminal status. No automatic audio replay.
- Optional hints: bias/preferences in `InputBoxInfo` **at init**, secondary
  languages in `UserInfo`. Do not copy these lists into late context, invent flags
  or claim that every preference is honored by the service.
- Protobuf: presence distinct from defaults, repeated-field order, proto2 merging
  of singular messages and the same oneof member, reset on member change. Preserve
  unknown enums/fields and raw bytes; do not concatenate malformed fragments to
  make them artificially valid. Size/field limits.
- Standard protobuf generation is not assumed to preserve unknown extensions.
  Validate the codec before choosing a generated model that would lose them.
- Supervised and bounded producer/encoder/cleanup. Normal stop with complete flush
  distinct from cancellation; no success on incomplete upload, missing END, S3
  error or late gRPC error.
- Preserve all ordered finals without arbitrarily chosen concatenation/deduplication.
  Distinguish hypothesis, transcript and edited text. Preserve explicit ambiguity
  rejection and the rendering policy for verified intents.
- Side-effect-free edit proposals, snapshot/revision and UTF-16 units. Rendering
  a rewrite is insufficient to authorize automatic replacement.
- No Android import, no FFmpeg process for the normal pipeline. Corpus preparation
  tools remain separate development tools.

## Reference and verification

- [`tests/fixtures/live_synthetic_responses.json`](../../tests/fixtures/live_synthetic_responses.json):
  three original payloads inspected.
- [`tests/fixtures/live_rewrite_response.json`](../../tests/fixtures/live_rewrite_response.json):
  **derived/redacted** fixture, never presented as a fourth original.
- [`tests/test_protocol_hints.py`](../../tests/test_protocol_hints.py): existing
  init goldens and exact emission of optional fields.
- [`tests/fixtures/result_policy_conformance.json`](../../tests/fixtures/result_policy_conformance.json):
  19 Python oracle cases for rendering, ambiguities, empty deletions, conflicts,
  typed arbitrary-size revisions and UTF-16. The 20 associated Python tests check
  this corpus; it is not proof of a complete S3 session.
- Python protocol, transport, engine, results and encoding suites: cases to port,
  not only happy paths. Rust offline/loopback tests before live access.
- [Natural corpus](../evidence/natural-expansion-public.json),
  [endurance](../evidence/endurance-300-public.json) and
  [hints](../evidence/input-hints-public.json): reference measurements with their
  hashes, conditions and limits. Do not confuse the measured code versions.

To compare requests byte for byte, provide the same explicit configuration values.
The real Rust client must declare an honest `Rust` platform, not `Python` or a
fictitious Android device; this expected difference must not be mistaken for a
wire regression.

Report module hashes describe the local files actually measured. Git may normalize
their CRLF/LF line endings: check for this difference alone before concluding that
code changed. The wheel smoke test checks exact equality of installed modules with
local files; this issue does not affect binary protobuf request goldens.

## First slice achieved November 13, 2026

The Rust release CLI completed three real Windows sessions: English, context editing
and French, without Python/FFmpeg in the process PATH. The
[inspected observations](../evidence/rust-first-slice-public.json) establish code/binary
hashes and limits. 121 integrated Rust tests and 643 Python reference tests pass;
no remote latency gain is inferred from these trials.

The real transport is `h2` + `tokio-rustls`/system roots, with `tonic` types and
tests. This choice replaces the prototype's tonic/hyper Channel, not the service:
the blocked-upload test required the ability to reset one session without cutting
off others or allowing its upload to resume after cancellation. HTTP/RST cases
were compared with the tonic client. The native PCM port and extended Rust
measurements remain separate from this Ogg file evidence. [Integration guide](../rust.md).

## First Rust batch success criterion

A standalone Rust CLI receives a response from real Rambler, without Python/Android
at runtime, and the selected conformance tests pass. Remaining gaps are precisely
identified; no unmeasured speed, memory or portability gain is claimed. Python
remains in the repository for research and comparisons, not as a hidden
implementation called by the Rust binary.
