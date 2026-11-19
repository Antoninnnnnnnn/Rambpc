# Performance measurements and decisions

Initial measurements from 2026-11-12. The results describe this test setup
and provide no general ranking of languages or guarantee of service latency.
A successful run requires usable text, S3 completion, gRPC OK, and complete
audio input.

## Separate native Rust profiling — 2026-11-13

[Protocol, raw measurements, hashes, and reproducible candidate](research/rust-audio-profiling.md).
The ABBA release benchmark uses 5.001 seconds of synthetic signals and measures
the encoder, muxer, and pipeline separately. No concurrent compilation was
reported. Grouped baseline medians were 45–134 ms total and 14–81 μs for the
muxer. The CRC candidate did not provide a consistent improvement, so
**production stayed unchanged**. Variation in the unchanged encoder prevents
attributing total differences to CRC. The benchmark provides no comparison
of network latency, ASR quality, process CPU, or Rust against Python.
The command `cargo run --release -p rambler-audio --example profile_pipeline -- 7 5 100 80017`
runs offline. Its windows and inputs do not represent natural speech or
establish a global optimum.

## Conditions and clocks

The [environment report](evidence/environment.json) records Windows 11,
Python 3.14.5, grpcio 1.85.1, and 12 logical processors. The test used a
standard Internet connection, uncontrolled machine and network load, a
monotonic client clock, and no server instrumentation. The corpus uses the
same synthetic Zira voice and an English sentence lasting 6.658 s in WAV
and 6.665 s in Ogg/Opus. In the first benchmark, the compressed input was
14,136 bytes.

The first reports predate the consolidation commit and have no exact source
hash. The public tools now record code provenance, versions, and
configuration. Independent checks verified the functional payloads. Git
tracks their hashes and three inspected original captures.

- `first_any_response` can be a heartbeat.
- `first_text` is the first candidate string. A later terminal error would
  invalidate the session.
- S3 completion and gRPC EOF are measured separately. A partial result
  cannot establish the final result.
- `last_audio_yield` and source exhaustion mark the **producer's** progress.
  They cannot establish completion of the last network write or server
  acknowledgment.
- The byte counters measure protobuf payloads. Actual TLS and HTTP/2
  traffic was not captured.
- Without remote instrumentation, timing includes both network and server work.

## 1. Pre-recorded file, accelerated upload

[21 original sessions](evidence/performance-baseline.json) include one warmup
excluded from aggregates and four groups of 5 trials in alternating order,
with concurrency 1. Every session succeeded with the same SHA-256 of consumed
audio, the same text, and the same intent.

| API channel | Ogg block | Median first text | Median EOF | Median request payload | Messages |
|---|---:|---:|---:|---:|---:|
| New | 224 B | 3,114 ms | 3,117 ms | 15,317 B | 67 |
| New | 8,192 B | 3,153 ms | 3,155 ms | 14,327 B | 5 |
| Reused | 224 B | 3,171 ms | 3,174 ms | 15,317 B | 67 |
| Reused | 8,192 B | 3,117 ms | 3,120 ms | 14,327 B | 5 |

Each larger block configuration sent 62 fewer messages and saved 990 protobuf
bytes, or 6.46% of the request payload for this audio. Protobuf overhead
excluding audio went from 1,181 to 191 bytes, a reduction of about 84%. The
minimum gRPC framing of 5 bytes per message would save another 310 bytes.
The benchmark did not capture HTTP/2 or TLS traffic.

The test did not establish faster final results. The direction of the
difference changes between groups, and their distributions overlap. With
five samples, nearest-rank p95 is the group maximum and provides no reliable
tail estimate.

The reused channel stays open, and gRPC C-core can share its subchannels
with new API channels. Initial warmup channel setup took about 51 ms. Median
API channel setup afterwards took about 3 ms, which cannot be used as a
cold TLS handshake measurement. Cold process, DNS, and server behavior
were outside this comparison.

Process CPU medians were 15.625–31.25 ms by group, with visible 15.625 ms
increments on Windows. CPU includes reading, hashing, and instrumentation
as well as the native client. This granularity prevents a reliable CPU
comparison. Process high-water RSS went from about 47.3 to 49.0 MiB across
21 sessions. It gives neither a peak per session nor leak evidence over
longer runs.

**Decision:** expose the 8,192-byte batch setting for available files. Use
smaller chunks for live input instead of waiting to fill 8,192 bytes and
adding buffering latency. Reuse the engine and channel with isolated stream
state.

## 2. Progressive source and Ogg buffering

[Six original sessions](evidence/realtime-paging.json) used three trials per
configuration in alternating order. The shared setup used the same WAV
input, FFmpeg/libopus at 24 kbps, mono 16 kHz, 20 ms frames, `-re`, and
`-flush_packets 1`. Only the page duration varied, between 1,000 and 20 ms.
One channel handled all sessions without retries. Every session returned
the **same text**.

| Ogg page duration | Median first speech event | Median first text after source EOF | Median S3 completion after source EOF | Ogg output |
|---|---:|---:|---:|---:|
| 1,000 ms | 1,663 ms | 935 ms | 937 ms | 14,136 B |
| 20 ms | 216 ms | 325 ms | 325 ms | 22,965 B |

Time from source exhaustion to first text dropped by about 610 ms, or 65%.
S3 completion moved by a similar amount. All six recorded streams decoded
to PCM with the same SHA-256, so there was no audio difference attributable
to page duration.

Shorter pages increase Ogg output by about 62%. The timing gain comes from
input buffering and measures neither the remote model nor its execution
time. Speech markers arrived earlier, but the small corpus produced no
usable text before the end.

The FFmpeg `-re` source was exhausted at about 6.15 s for 6.66 s of encoded
audio. It is a simulated stream from a file. The post-EOF intervals are
comparable within this experiment and include network, remaining buffering,
and service time. They cannot identify the time from the acoustic end.
The external encoder's CPU is separate from engine CPU, which this test
does not establish.

**Decision:** use 20 ms pages, bounded memory, and an explicit final flush
for the optional native PCM-to-Ogg/libopus path. Validate this
implementation with its own tests and live sessions.

## 3. Native profile validation

[Six native sessions](evidence/native-realtime.json) used commit `0814813`
with exact hashes of the modules and benchmark. The benchmark hash was
verified against the commit. The new observation file is included in
`git_dirty`. The recorded hashes identify the measured modules.

The runs used a native WAVE PCM16 reader with mono 16 kHz input in 20 ms
chunks, PyAV/libopus, and three alternating pairs on one ready channel. No
FFmpeg process was involved. All six consumed exactly 213,070 PCM bytes
matching the input hash. All decoded streams have the same length and hash.
Opus is lossy, so the hash checks equality across configurations only.

| Native Ogg pages | Median speech event | First text after PCM reader EOF | S3 completion after PCM reader EOF | Ogg output |
|---|---:|---:|---:|---:|
| 1,000 ms | 2,150 ms | 990 ms | 991 ms | 14,126 B |
| 20 ms | 344 ms | 328 ms | 330 ms | 22,955 B |

Rounded text delays after EOF were 981/1,036/990 ms for 1,000 ms pages and
328/407/324 ms for 20 ms pages. The median improved by about **67%** in this
small corpus on an uncontrolled network. Text and intent were identical.
The result supports the 20 ms default in the shipped native implementation.
The trade-off is about 63% more Ogg bytes and 339 messages instead of 75.
The benchmark splits muxer writes into fragments of at most 224 bytes
without waiting to fill a 224-byte live block.

Native EOF marks exhaustion of the **PCM reader**, followed by the encoder
flush. The FFmpeg benchmark measured EOF of **already encoded stdout**.
Compare each benchmark separately and interpret the intervals as
client-observed. Native PCM ran out at about 6.64 s for 6.66 s of audio
with pacing at the start of each chunk. There was no usable text before
the end on these sentences.

For a sentence lasting 6.66 s, the native 20 ms benchmark took a median
171.875 ms of process CPU, with a range of 93.75–390.625 ms. This includes
encoding, the engine, hashing, and decoding for verification. Windows
granularity and variation prevent a reliable CPU comparison. The Python
encoder bounds its output queue at 64 KiB and each incoming PCM chunk at
8 MiB. The bounds cover neither all native PyAV/gRPC memory nor a full
per-session peak.

## 4. Startup and distribution

[A live PCM CLI run](evidence/native-pcm-cli.json) in a new process took
5.725 s total. About 3.198 s elapsed in the engine stream before text
arrived. The difference includes Python startup, imports, CLI setup,
and connection establishment. The measurements do not isolate these
costs. A single run cannot establish a startup median.

Three later PyAV imports in fresh processes, **with OS caches intact**,
took 115.169, 97.754, and 99.964 ms offline. The native benchmark also
measured 66 ms of setup and 48 ms to open the channel in its process.
These data cannot attribute the first CLI's 2.5 s gap to PyAV alone.
The optional encoder loads on demand. A long-running process can load it
before capture.

[A wheel installed in a fresh venv](evidence/installed-wheel-smoke.json)
completed live dictation through the Ogg CLI, without PyAV or imports
from the checkout. Exit code 0 and the expected result arrived in
3.533 s total. The run used the existing local configuration. It needed
no APK, JADX, or Android at runtime. Compare this single sample separately
from the PCM CLI run.

## 5. Refined native page grid — 2026-11-13

[20 additional sessions](evidence/refinement-page-grid.json) ran at
concurrency 1, with five trials for each page duration of 20, 40, 60,
and 100 ms in rotating then reversed order. They used the same sentence
and source PCM as the historical native benchmark. The measured checkpoint
was `4b96696` and the report records the exact hashes. `git_dirty` includes
new evidence files. The `muxer` policy sends native writes immediately,
as the CLI and SDK do. The old benchmark systematically split them into
224-byte chunks. Keep the two sets of results separate.

All 20 sources were fully consumed. Decoded PCM matched by hash, and
S3 and gRPC completed successfully in each session. The guard
`ordered_all_recognition_state_1_sets_v2` compared every ordered list of
final hypotheses, including empty and repeated lists and without merging
or deduplication. It found no differences.

| Ogg page | Text after PCM EOF, median (min–max) | Median S3 completion | Ogg output | Median pipeline CPU |
|---|---:|---:|---:|---:|
| 20 ms | 348 ms (297–858) | 349 ms | 22,955 B | 469 ms |
| 40 ms | 417 ms (407–489) | 421 ms | 18,446 B | 281 ms |
| 60 ms | 365 ms (348–428) | 366 ms | 16,961 B | 328 ms |
| 100 ms | 422 ms (354–485) | 423 ms | 15,746 B | 203 ms |

**Decision: keep 20 ms as the default.** It had the lowest observed text
median and the earliest median speech marker (322 ms versus 366/413/483 ms).
The 60 ms setting saves **26.1% of Ogg bytes** at a cost of about **17 ms** in
the median final result in this benchmark. The comparison establishes a
bandwidth trade-off for this setup only. Ogg bytes exclude TLS overhead.

The 20 ms outlier at **858 ms** remains in the data. With five samples
and uncontrolled network and host load, the tail is unknown. A favorable
median gives no universal latency ranking or global optimum.

Pipeline CPU excludes verification and decoding. It includes artifact
writes (also measured separately), encoding, transport, hashing, and
instrumentation. Ranges were wide with Windows timing granularity
(297–547 ms at 20 ms and 188–766 ms at 60 ms). The benchmark cannot isolate
engine CPU. It records the Ogg header separately from the first useful
audio page. Encoder setup was **unmeasured**.

A [separate progressive dictation](evidence/refinement-long-realtime-public.json)
used 40.564 s of PCM and completed full collection in 41.454 s, with
complete source input and the same text as the accelerated run. This
single trial is outside the grid. It measures neither post-stop
microphone latency nor long endurance.

## 6. Endurance at simulated pacing — continuation

[One 118.682 s session](evidence/endurance-120-public.json) and then
[three 288.3515625 s sessions](evidence/endurance-300-public.json) ran
sequentially with 20 ms pages and chunks. At the long stage, the same
channel handled 9,227,250 PCM bytes per session, identical ordered sets
of final responses, and observed markers. The benchmark marked all
four sessions healthy.

| Session | Pipeline wall time | Process CPU | RSS start → end (bytes) | Maximum sampled RSS |
|---|---:|---:|---:|---:|
| 118.682 s | 119.880 s | 5.891 s | 58,511,360 → 60,723,200 | 60,907,520 |
| 288.352 s, 1 | 291.238 s | 17.094 s | 58,396,672 → 60,694,528 | 60,796,928 |
| 288.352 s, 2 | 290.908 s | 14.469 s | 60,739,584 → 61,001,728 | 61,120,512 |
| 288.352 s, 3 | 290.747 s | 14.672 s | 61,341,696 → 61,464,576 | 61,661,184 |

RSS is sampled current process usage. An exact peak or high-water mark
cannot be recovered from these data, and four sessions are insufficient to
rule out leaks. CPU includes codec, engine, hashing, and instrumentation.
The source is a paced file. The runs used the 0.2 baseline **before**
integrating the 0.3 hints. The reports contain exact hashes. The
300 s and 360 s limits belong to the benchmark.

## 7. Native Rust — first separate measurements

[Three Ogg calls](evidence/rust-first-slice-public.json) and
[four PCM and editing calls](evidence/rust-pcm-functional-public.json)
validate the native path. The same 6.658 s English PCM returned the same
text in 3.293 s accelerated and 7.106 s at simulated pacing. Each mode
was run once, including startup, TLS, network, and service. Rust
`metrics.eof_seconds` marks consumer completion after draining and
cleanup. The previous benchmarks took their post-EOF timings from the
PCM reader, so those intervals are unavailable here.

[Rust endurance](evidence/rust-endurance-120-public.json) ran 118.682 s of
audio in 119.929 s of process time, with maximum sampled RSS of 11,898,880
bytes and child CPU of 6.766 s at the final sample. The first
[long trial](evidence/rust-endurance-300-public.json) ran 288.352 s of audio
in 290.905 s, with maximum sampled RSS of 11,862,016 bytes and final sampled
CPU of 12.984 s. The second failed as **incomplete after 282.202 s**. The
third did not run. Available counters do not establish the cause.

Measurements cover the Rust child only and exclude the Python driver.
RSS samples reflect current usage, and CPU samples are cumulative. These
samples cannot establish an exact peak or all CPU through exit. The first
sample can precede TLS and codec loading. Each trial uses a fresh process
and channel, while the three long Python trials reused a channel. Codec,
version, instrumentation, load, and network differ. The runs do not
establish relative gains or rule out leaks. The 300 s and 360 s limits
belong to the benchmark.

A [separate diagnostic follow-up](evidence/rust-incomplete-diagnostic-followup.json)
validated the new binary on 118.682 s, then completed one 288.352 s trial
in 290.904 s. The known final results matched. Maximum sampled RSS was
11,792,384 bytes and final sampled CPU was 15.344 s. The original failure
did not recur and remains unresolved. Its flags cannot be recovered
retroactively. An independent review recommended against repeated replays.
The follow-up is separate from the initial batch.

## Reproduce

Prepare synthetic WAV/Opus with `tools/make_fixture.ps1` or use an authorized
corpus. Set the three application metadata variables without logging them.

```sh
# Windows with Git Bash, the venv installed, and a local .env configured
set -a; source .env; set +a
mkdir -p local
.venv/Scripts/python tools/benchmark_engine.py --live --audio artifacts/synthetic-v1/plain-en.ogg --language en-US --samples 5 --chunk-sizes 224,8192 --output local/batch-run.json
# Explicitly prepare the 16 kHz WAV if the original SAPI corpus is at 22.05 kHz
ffmpeg -n -i artifacts/synthetic-v1/plain-en.wav -ac 1 -ar 16000 -c:a pcm_s16le artifacts/synthetic-v1/plain-en-16k.wav
# Requires the audio extra. This native benchmark runs without an FFmpeg binary.
.venv/Scripts/python tools/benchmark_realtime.py --source native --live --wav artifacts/synthetic-v1/plain-en-16k.wav --language en-US --output local/native-run.json --encoded-dir artifacts/new-native-run --pairs 3
# Refined grid with 20 sessions using the SDK/CLI write policy
.venv/Scripts/python tools/benchmark_realtime.py --source native --live --native-chunk-policy muxer --rounds 5 --page-durations 20000,40000,60000,100000 --wav artifacts/synthetic-v1/plain-en-16k.wav --language en-US --output local/grid-run.json --encoded-dir artifacts/new-grid-run
```

On Linux or macOS, replace `.venv/Scripts/python` with `.venv/bin/python`.
The generator's SAPI voices require Windows and must be installed. On
other systems, use your own authorized corpus and record its hash and
test conditions. Use new paths for the 16 kHz WAV and outputs.

The tools reject overwriting existing observations. Keep raw files and
audio local. `--include-results` records known text in the benchmarks.
In `validate_corpus.py`, it also retains full responses and may capture
opaque service identifiers. Before publishing, create a sanitized copy with
`tools/sanitize_corpus_report.py` and inspect it, including when the sentence
is synthetic. Without the flag, benchmark results are hashed. Local tests
run independently of the service. The optional FFmpeg tests cover the
test producer only.

## Measurements still needed

Measure genuinely cold process startup, real microphone/PCM input,
endurance beyond 288 s, degraded networks, isolated per-session
resources, exact network traffic, and representative natural speech
quality (ten additional read clips plus the initial sample). The Rust
observations do not compare languages under controlled conditions or
establish a remote processing gain. The measurements identify the input
pipeline as a major source of latency. A transport rewrite alone would
not have substantiated the observed finalization gain.
