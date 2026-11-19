# Partial results and 48 kHz WAV — interactive batch validation

19 November 2026 · audited baseline `debd06e3fb960eaf2254aa8b330b8918df9dff94` · tested source `a1017177ffca1602dbcc6532bfcb8322a7fa544c`.

## What changed

- A mixture of usable segments and segments requiring review or without a result, in a capture whose RPCs completed normally, produces `type: "ambiguous"`, `outcome: "partial"`, `text: null` and exit code `1`. The app retrieves usable parts and ranges requiring review from `completed_segments`, without concatenating across gaps. A segment without a result does not prove that the input was silent. A safety refusal, RPC/close error or cancellation retains its category and priority; refused content remains hidden. [Contract](../research/recoverable-segment-delivery.md)
- `dictate-wav` now accepts mono PCM16LE at 16 or 48 kHz, including in real-time and interactive modes. At 48 kHz, one continuous resampler precedes the encoder or segmentation; segment ranges remain in **16 kHz** samples. The `WavAudio::open` API remains limited to 16 kHz; `open_16_or_48k` is opt-in. Stereo, floating-point, 44.1 kHz or already truncated WAVs are rejected before connection. [Formats and profile](../research/interactive-pcm-dictation.md)

## Verified checks

The [WAV tests](../../crates/rambler-cli/tests/wav.rs) compare Opus packets and Ogg granules with separately encoded references and detect a filter restart at a segment boundary. The [partial-result tests](../../crates/rambler-cli/tests/interactive_partial.rs) check ordering and ranges, including recognized text before **and** after an unusable region, rendering policies and safety refusals. Cancellation supervision is exercised by injecting the signal result, not by another OS interruption of the process.

The [combined check in the actual binary](../../crates/rambler-cli/tests/interactive_48k_partial.rs) supplies 67,200 WAV samples at 48 kHz. It obtains two contiguous ranges covering exactly 22,400 samples after conversion, on one connection, with `partial` and exit code 1. The `--diagnostics-only` variant retains the same statuses and ranges without publishing the usable segment's text.

- Rust stable, Windows/MSVC: **573 workspace tests passed, 2 ignored**, **28 CLI tests without audio passed**, workspace formatting and strict Clippy passed.
- Rust 1.88.0, Windows/MSVC: 5 targeted WAV, partial-result and combined-check tests passed.

The local suite used owned loopback peers and synthetic inputs, without a real service, device or microphone. These results qualify neither repetition transcription nor fidelity in loud noise. The [separate Python close follow-up](python-transport-cancelled-close-windows-20261118.md#independent-audit-follow-up-19-november) did not reproduce the intermittent hang and does not attribute a cause to it.
