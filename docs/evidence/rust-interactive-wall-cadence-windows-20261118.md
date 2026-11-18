# Interactive WAV profile at wall-clock cadence — Windows, 2026-11-18

`rambler dictate-wav --interactive` from release source `4e57b00a4c8a67afaf094c8a77c6223600cb9b7b`; executable SHA-256 `2fcfade92de703709bc318c0f504586a7f65d7a6ff43c4fc09b445bbdf44249c`. No live Rambler or microphone. The Windows processes and the Python gRPC peer ran in an owned Job that was empty after successful exit.

Three synthetic mono 16 kHz WAVs ran concurrently against separate owned IPv4 loopback peers. Healthy cases used the default interactive budget. The 300 s WAV has a four-second pause and a distinct terminal tone; the peer replies with one predetermined text per clean stream. The peer observed request END followed by half-close and independently counted trimmed PCM duration from the captured Ogg headers, pre-skip and EOS granule. CLI sample ranges were contiguous and agreed with the peer's counts; Ogg serials were distinct within a capture. Each case used one TCP connection.

| Input | Result | Samples per RPC | Process wall time |
| --- | --- | --- | ---: |
| 31 s, healthy | One retained final | 496,000 | 31.019 s |
| 300 s, healthy | Three ordered finals | 1,920,000 + 1,920,000 + 960,000 | 300.019 s |
| 1 s, peer withheld its final response after upload; `--finalize-timeout 1` | Exit 1, `finalize_timeout`; no retained result | 16,000 | 2.269 s |

The 31 s case passed without overriding the default interactive overall timeout. 300 s of input never occupied one RPC. These timings include file cadence and process startup and are **not** a last-sample-to-Gboard or remote-service latency measurement. The emitter's text is synthetic. The accelerated matrix and diagnostics checks also cover 29, 30, 31, 120 and 300 s of PCM, a ±1 sample rollover and a gated gRPC final status. See the [profile contract and commands](../research/interactive-pcm-dictation.md) and the [separate accelerated packet-equivalence run](rust-long-pcm-loopback-windows-20261118.md).
