# Long PCM rollover — Windows localhost, 2026-11-18

**Candidate:** `f4e386849faa8648019eee97569d594162c94216`. This is a client-side sample-integrity and lifecycle check, not a Rambler transcription or latency test. No third-party service or credentials used.

The input was a locally synthesized WAV of 17 original prose paragraphs (mono PCM16LE at 16 kHz), with 250 ms silences between paragraphs. Duration: 609.03 s, 9,744,480 samples. Input SHA-256: `2416574525f587ba859ecd0f2546d3e26f1e95f9f23db5100f4a076536b1f679`. Test input used unpaced 137 ms chunks, a 180 s segment budget, and a 4 s pause-search window. The standalone full-duration diagnostic fixture was temporary; the checkout's [shorter integration tests](../../crates/rambler-cli/tests/long_pcm_dictation.rs) exercise the same public SDK without this WAV.

## Observed

- **Successful pass:** four RPCs on the same Engine and TCP connection. Each sent S3 END and a clean HTTP/2 half-close; each received S3 END and completed successfully. All samples were accounted for, with four retained finals.
- **Failure pass, reusing that Engine and TCP connection:** the fixture sent S3 END then gRPC `Internal` on the third RPC. The collector returned the typed transport failure and the first two completed results, despite receiving S3 END on the third. There was no fourth RPC or automatic replay. The source closed once without reading EOF; 1,536 samples past the failed segment's cut were read into the client, not uploaded.
- An independent sample-based pause oracle predicted the ranges below. For all seven uploads over the two passes, the captured Opus packets matched encoding the corresponding PCM slices independently, and decoded duration matched the slice after pre-skip/trim. Each upload used a distinct Ogg serial. Packet agreement tests the client boundary, not lossless Opus audio or text fidelity.

| Segment | 16 kHz sample range (half-open) | Cut |
|---|---:|---|
| 1 | `[0, 2 875 200)` | Low energy |
| 2 | `[2 875 200, 5 692 800)` | Low energy |
| 3 | `[5 692 800, 8 564 800)` | Low energy |
| 4 | `[8 564 800, 9 744 480)` | EOF |

## Preserved attempts and limits

The initial unoptimized Windows run reached its 90 s outer test deadline without a summary (Cargo exit 101, owned Job empty). That attempt is **inconclusive**. One follow-up with stage instrumentation and a 165 s outer backstop passed: unpaced collections took 29.62 s and 26.72 s, the peer closed at 56.69 s, and the subsequent packet oracle and decode checks ran until 85.43 s. This shows the test can approach the old deadline without a deadlock; the first run did not have stage evidence. These measurements are fixture wall times, not service performance.

Separately, the candidate passed the Windows locked offline workspace suite on both stable and Rust 1.88.0 (518 passed and 2 opt-in ignored on each), plus 24 tests of the CLI without audio on each. Stable formatting, strict workspace Clippy and the release qualification-example build passed. This does not substitute for the exact candidate's cross-platform CI or for an authorized real-service qualification of natural speech.
