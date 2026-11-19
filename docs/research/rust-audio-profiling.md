# Offline Rust PCM → Opus/Ogg profiling

Measurement from 2026-11-13, independent of the network and service. No `.env`,
credentials, private audio, external corpus or remote call is used. The three
inputs are synthetic and generated in memory by the versioned benchmark.

## Protocol

Benchmark: `crates/rambler-audio/examples/profile_pipeline.rs`, executable only
with `--release`. Three deterministic mono 16k PCM16LE signals: silence, a
combination of periodic triangles with silence, and xorshift32 noise with seed
`0x12345678`. The generator uses integer arithmetic. These signals are not a
natural speech corpus and do not measure recognition quality.

Campaign parameters: `7 5 100 80017`: 7 repetitions, 5 encoder/total operations
per repetition, 100 mux operations, 80017 samples (5.0010625 s, 160034 PCM
bytes) per input; 20 and 1000 ms pages. The 1000 ms pages are a control, **not**
a proposal to change the 20 ms default. One warmup per stage/configuration is
excluded. The three stages rotate between repetitions; signal/page order remains
fixed, which is a limitation.

- **Encoder**: create the same native libopus, `Application::Voip`, mono16k,
  VBR 24000 bit/s, 320-sample/20 ms frames, PCM LE conversion, encoding, padding
  to cover input + lookahead, collection of all packets and encoder destruction.
  No complexity/quality setting is changed.
- **Mux**: create the actual private mux compiled from `src/ogg.rs`, headers,
  pre-encoded packets, granules/EOS, collect each page in a `Vec<u8>`, destroy.
  It does not re-encode audio.
- **Total**: actual public `Pcm16Opus`, memory source in 640-byte chunks,
  current-thread Tokio runtime, cooperative yields retained, full drain, page
  collection and `close().await`. Runtime created outside measurement; codec
  setup included. No WAV reader, pacing, disk or transport.

Each operation is timed with `Instant`, then its result is validated **outside the
timer**. `batch_ns` is the **sum** of operation durations for the repetition,
not continuous duration including validation. Each result is consumed in vectors
and passed to `black_box`; no packet/page is ignored. Output allocations and
copies are included, freeing the result vector after return excluded. Hashing,
decoding and fidelity assertions are excluded. The three times are not exact
additive components: in particular the encoder benchmark keeps packet vectors,
while total works in streaming mode.

The clocks measure **offline elapsed time**, not cycle counters or isolated
process CPU. No network, service, microphone or recognition latency can be
inferred from these measurements.

## Conditions and provenance

Starting revision: `4fdf29e8197e76d4ea9fdf004ff74bbc7dd35be1`.
Windows 11, supplied host version 10.0.26300, `x86_64-pc-windows-msvc`;
`PROCESSOR_IDENTIFIER=Intel64 Family 6 Model 154 Stepping 3, GenuineIntel`,
12 logical processors reported by the environment. Rust 1.99.1
(`58cf8f9c35b355199727627d07ae34a4e2846d67`, LLVM 23.1.1), Cargo 1.99.1
(`5f94df478`), CMake 3.31.10, Visual Studio 17 2022/x64 generator,
libopus 1.6.1 statically bundled (`OPUS_BUILD_SHARED_LIBRARY=OFF`, Release).
No Cargo profile override: standard optimized workspace release profile. Cargo
directory specific to this worktree; no reuse of the parent target. Power plan,
CPU affinity and frequencies are not controlled.

Measurement windows were reserved with the coordinator after compilation, with
no concurrent compilation announced. This does not guarantee absence of all
OS/editor/antivirus activity. Caches were not cleared, explicit warmups used:
this is not a cold-start measurement.

Provenance SHA-256:

```text
Cargo.lock       0fcd2757af343aeebabd8797a9a22771fb09ecb20d86853a15c3036d051d1628
encode.rs        5db989fa18b3a1126108d961d6baa638a6f2fdbd16d5095c7d223940c986a424
measured bench      f8319c6dcdd787b873d5ebe620b30ec292b2ac29f637f4575c4ccee4ef393194
delivered bench       58ecc07212914d66bcd2bb621e13f172459c9586fdf86e79e8d8515fa79bf053
ogg.rs before     9581bd5655771e39fd40f649954499ea529cddbac7555b31d37af6b360f9e63e
ogg.rs candidate  3733f05758211fc59899f0f51e95ca6741d337792cf37135d5ebc149f8a0d93e
baseline.exe     79a550592f3982e8964231f2e7e2100fc9aea9489749862f494ba4e43ddde1b1
candidate.exe    7adeb1404a051075892caaee78e6e252b135eb2db4e9007e47ddab92222e8e7e
```

Hashes of actual bytes: `ogg.rs` and `encode.rs` use CRLF in this worktree,
the benchmark LF. The appendix C candidate block, reinserted with CRLF into the
initial `ogg.rs`, exactly reproduces the candidate source hash above.

## Measurements and decision

**No production optimization retained.** The candidate only replaced the eight
bitwise Ogg CRC steps with a constant table of 256 u32 (1 KiB). Fidelity is
identical, but the mux gain is neither robust nor uniform; total cost does not
justify a refactor. `src/ogg.rs` and `src/ogg_tests.rs` were restored exactly to
the starting revision. Codec profile, quality, VBR, bitrate, default frames/pages,
EOF, cleanup and bounds are unchanged.

Actual order A1 → candidate compilation (1.81 s, outside measurement) → B1 → B2
→ A2. A = preserved original binary, B = CRC candidate; A2 reruns exactly A1,
not a recompilation. UTC times on 2026-11-13:

| Run | Start | End |
|---|---|---|
| A1 | 21:46:04 | 21:46:46 |
| B1 | 21:47:54 | 21:48:34 |
| B2 | 21:48:34 | 21:49:16 |
| A2 | 21:49:16 | 21:49:55 |

Each per-run column is the median of the 7 per-repetition averages. Grouped
columns combine 14 repetitions, **not** the average of the two medians; they
include minimum and maximum of those averages. Unit: ms per clip/operation.
Rounded to six decimal places; this presentation does not claim nanosecond
precision. Raw integer values are retained in the appendix.

| Signal | Pages ms | Stage | A1 ms | B1 ms | B2 ms | A2 ms | A grouped ms (min–max) | B grouped ms (min–max) |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| silence | 20 | encode | 104.024680 | 88.722660 | 99.603820 | 88.821280 | 90.367790 (87.628160–116.908060) | 92.770420 (87.330860–109.822060) |
| silence | 20 | mux | 0.053154 | 0.050643 | 0.061088 | 0.050492 | 0.051553 (0.047738–0.078135) | 0.059595 (0.047330–0.071049) |
| silence | 20 | total | 96.599260 | 87.943240 | 94.690280 | 89.718260 | 91.792440 (88.139460–108.620980) | 89.922410 (87.019420–102.196200) |
| silence | 1000 | encode | 99.109260 | 101.302500 | 89.582120 | 89.196740 | 94.014260 (86.545200–103.876860) | 91.541950 (88.550420–123.487080) |
| silence | 1000 | mux | 0.014742 | 0.013178 | 0.014153 | 0.014400 | 0.014494 (0.011337–0.022981) | 0.013958 (0.012696–0.018043) |
| silence | 1000 | total | 102.039660 | 100.915980 | 89.406120 | 90.374600 | 97.807810 (87.796660–103.928800) | 90.047640 (86.970620–107.840940) |
| harmonic | 20 | encode | 45.694620 | 46.570480 | 44.667300 | 45.430440 | 45.565560 (44.459280–52.581420) | 46.245490 (44.376840–52.166280) |
| harmonic | 20 | mux | 0.077348 | 0.092767 | 0.080259 | 0.077021 | 0.077185 (0.073384–0.081126) | 0.081568 (0.076015–0.098439) |
| harmonic | 20 | total | 45.912180 | 50.075720 | 46.437400 | 46.135800 | 46.023990 (44.935840–51.178220) | 48.042570 (44.654060–56.517880) |
| harmonic | 1000 | encode | 45.205500 | 53.554220 | 51.137640 | 45.660520 | 45.433010 (43.844240–48.586920) | 51.425620 (45.143420–65.413520) |
| harmonic | 1000 | mux | 0.048370 | 0.052354 | 0.042897 | 0.050696 | 0.049815 (0.044109–0.053906) | 0.051075 (0.039839–0.066753) |
| harmonic | 1000 | total | 46.374040 | 52.218420 | 50.340120 | 46.738320 | 46.391100 (44.836620–49.977680) | 50.889190 (44.861640–56.505720) |
| noise | 20 | encode | 139.711000 | 122.034200 | 141.389380 | 121.117740 | 129.014940 (120.169420–144.440800) | 129.983980 (119.272380–156.232460) |
| noise | 20 | mux | 0.082164 | 0.080331 | 0.080130 | 0.078110 | 0.080816 (0.073164–0.102103) | 0.080231 (0.075930–0.095993) |
| noise | 20 | total | 148.460900 | 123.805320 | 145.943420 | 122.716000 | 133.868220 (120.317620–154.243300) | 130.107770 (119.283760–173.458100) |
| noise | 1000 | encode | 127.022060 | 121.424920 | 133.158500 | 124.099260 | 125.560660 (120.842060–149.113340) | 124.877480 (119.786980–162.775820) |
| noise | 1000 | mux | 0.043189 | 0.039980 | 0.042384 | 0.045549 | 0.044047 (0.038397–0.052948) | 0.042170 (0.038415–0.045333) |
| noise | 1000 | total | 128.504320 | 122.568120 | 126.154740 | 126.450540 | 128.082200 (121.511380–146.178560) | 124.559670 (120.329900–157.959320) |

20 ms pages: grouped mux A→B = 51.553→59.595 µs (silence, **+15.6%**),
77.1845→81.5675 µs (harmonic, **+5.7%**), 80.816→80.231 µs (noise, −0.7%).
At 1000 ms pages: −3.7%, +2.5%, −4.3%; overlapping distributions. No case
supports a general claim of improvement. The **unchanged** encoder itself varies
by approximately −2.6% to +13.2% between groups: total variations (−7.9% to
+9.7%) cannot be attributed to CRC.

On A, mux is approximately 14–81 µs versus about 45–134 ms for total (grouped
medians). The ratio of these separate measurements remains below 0.2%; this is
not an exact additive decomposition. No codec quality or complexity change was
made to seek an artificial gain. No other speculative refactor was attempted.
No percentage gain in process CPU, network or Rust/Python is established.

After measurement, only two Clippy `allow` attributes and their comments were
added to the benchmark (measured `chunks_exact` iterator and runtime rejection
of debug mode). No executed statement, generator or timed loop was changed. The
measured and delivered benchmark hashes differ and are stated above; results are
not presented as a new post-lint run.

### Tests and final verification

- Candidate: `cargo test --release --locked -p rambler-audio`: **30 passed,
  1 ignored**, 7 suites. The additional temporary test compared table CRC to the
  original CRC, all 256 possible bytes, deterministic prefixes up to 64 KiB,
  empty input and vector `123456789 → 0x89a1897f`; removed with the candidate.
- Restored final tree: same command: **29 passed, 1 ignored**, 7 suites,
  1.00 s suite execution (excluding compilation).
- `cargo clippy --release --locked -p rambler-audio --all-targets -- -D warnings`:
  passed; `cargo fmt -p rambler-audio -- --check`: passed.
- An initial Clippy pass flagged two lints in the new benchmark, then corrected
  with local attributes without changing the workload. No functional test failure.
- Existing tests cover fragmentation/EOF, granules, independent CRC, limits,
  cancellation and idempotent closure. The ignored independent PyAV oracle was
  **not** run for this task; no Python/PyAV/FFmpeg is involved in measured encoding.

## Fidelity checks

For **each** timed encoder operation: exact equality of every packet with the
deterministic reference. For **each** mux/total operation: reading by the
independent `ogg` crate (actual CRC and framing validation), equality of all Opus
packets, page sequence and serial uniqueness, BOS/EOS flags, final granule,
canonical Ogg hash. Only serial and CRC field are zeroed in the canonical hash,
**after** validating the actual CRC; serial variations between streams do not
represent an audio change.

Packets are hashed with each length prefixed as u32 LE; 48k decoded PCM is hashed
after removing pre-skip and trimming to the final granule, exactly
`80017 * 3 = 240051` samples. Opus PCM is lossy: fidelity is checked **before/after
optimization**, never declared identical to original input.

## Reproduction

From a clean build checkout/worktree with C/CMake toolchain available:

```sh
# target under this checkout only; do not share with a concurrent build.
export CARGO_TARGET_DIR="$PWD/target/audio-profile"
# Windows if CMake is not on PATH: export CMAKE='/chemin/absolu/cmake.exe'
cargo build --release --locked -p rambler-audio --example profile_pipeline
# Wait for ALL compilation to finish, then run the binary directly.
"$CARGO_TARGET_DIR/release/examples/profile_pipeline.exe" 7 5 100 80017 > baseline.csv
# On Unix, remove only the .exe suffix.
# Preserve/copy this binary BEFORE compiling a candidate.
# Recompile the candidate, again wait for all compilation to finish.
# Run the preserved binaries sequentially, never concurrently.
```

Compare all `proof,` lines between outputs, not just times. `timing,` lines contain
operation count, nanosecond sum and average per operation. Retain outliers; small
n and fixed order do not permit estimating a production long tail.

## Appendix A — exact evidence common to all four runs

The six lines below were automatically compared between A1/B1/B2/A2: strict
equality, no line omitted. Per configuration/version: 35 encodes, 700 muxes and
35 totals timed; over two runs per version, respectively 70, 1400 and 70 fully
verified outputs. No sample excluded.

```csv
proof,kind=silence,page_ms=20,pcm_bytes=160034,pcm_sha256=b5e0410caf0728b0a46afb14c1eb4935d2cd0ae6fb5cd350d2df8818ec80b201,audio_packets=251,packet_bytes=2009,packet_sha256=5b2cd5e02bf347600031a29d8762ff7fa1e2bc685becf3ad192ca2efe94da518,ogg_bytes=9141,pages=253,pre_skip=312,final_granule=240363,canonical_ogg_sha256=0cc292f0ceeed3354055fe42f1995cd395af483a519eb9f84c7271d096a9e87e,decoded_pcm48k_sha256=9271e177675df2fcdbcb2d44fbb9e21a74382b36f3d5b6cf9829c1fc9e8dce40
proof,kind=silence,page_ms=1000,pcm_bytes=160034,pcm_sha256=b5e0410caf0728b0a46afb14c1eb4935d2cd0ae6fb5cd350d2df8818ec80b201,audio_packets=251,packet_bytes=2009,packet_sha256=5b2cd5e02bf347600031a29d8762ff7fa1e2bc685becf3ad192ca2efe94da518,ogg_bytes=2526,pages=8,pre_skip=312,final_granule=240363,canonical_ogg_sha256=38cf20f7e141b6d46d656e1bac7a4eca2d6c17a7d2abb4998cae55ec6074be42,decoded_pcm48k_sha256=9271e177675df2fcdbcb2d44fbb9e21a74382b36f3d5b6cf9829c1fc9e8dce40
proof,kind=harmonic,page_ms=20,pcm_bytes=160034,pcm_sha256=c554e95f4010e21ba354933148262aa41d6247d1910996740ea00e45b15ae24c,audio_packets=251,packet_bytes=13419,packet_sha256=c3bb3cd84699fe6f8bb3f07e4c0a3282aa3c6e02ca955678f966b3c706011697,ogg_bytes=20551,pages=253,pre_skip=312,final_granule=240363,canonical_ogg_sha256=709e7cc7cccc7cad7b542bd844a9a07da97642e62055de5b0b035a4e449d7577,decoded_pcm48k_sha256=69da05665588f7b56fcd5c3eec30a97eb79349882565c27283b0a7d10af3f887
proof,kind=harmonic,page_ms=1000,pcm_bytes=160034,pcm_sha256=c554e95f4010e21ba354933148262aa41d6247d1910996740ea00e45b15ae24c,audio_packets=251,packet_bytes=13419,packet_sha256=c3bb3cd84699fe6f8bb3f07e4c0a3282aa3c6e02ca955678f966b3c706011697,ogg_bytes=13936,pages=8,pre_skip=312,final_granule=240363,canonical_ogg_sha256=be0e055a556db7e1bf0ee7ed973bf094222252a0fcbe1dae90fee125fecbbae0,decoded_pcm48k_sha256=69da05665588f7b56fcd5c3eec30a97eb79349882565c27283b0a7d10af3f887
proof,kind=noise,page_ms=20,pcm_bytes=160034,pcm_sha256=8afee5c59bf517ab814636fb6e8b2c7d8925f81a6f902ce9193a7c2041f89107,audio_packets=251,packet_bytes=13460,packet_sha256=7be0bfc5488dfd66a19ec08d14351978dc08dff5e4f44fd056308bd22929ec02,ogg_bytes=20592,pages=253,pre_skip=312,final_granule=240363,canonical_ogg_sha256=9789fe5766f6e12940f26e3c98e98b3d650b621cddae05853ba4bcf94ecf9480,decoded_pcm48k_sha256=643f3c1d92f2c40f5c1254b6eca5c72c4670e34b59fe687dfb3a178d8b811dfd
proof,kind=noise,page_ms=1000,pcm_bytes=160034,pcm_sha256=8afee5c59bf517ab814636fb6e8b2c7d8925f81a6f902ce9193a7c2041f89107,audio_packets=251,packet_bytes=13460,packet_sha256=7be0bfc5488dfd66a19ec08d14351978dc08dff5e4f44fd056308bd22929ec02,ogg_bytes=13977,pages=8,pre_skip=312,final_granule=240363,canonical_ogg_sha256=8bd05ef6cb0f949de9e4582d6c4011c5482ae8e07ae98fa418091d59883c662d,decoded_pcm48k_sha256=643f3c1d92f2c40f5c1254b6eca5c72c4670e34b59fe687dfb3a178d8b811dfd
```

## Appendix B — raw nanosecond sums per repetition

Each line provides the seven integer `batch_ns` values in repeat0..6 order. Divide
each sum by `iterations`, then by 1e6 for ms/operation. Each run contains 126
timing lines and six proof lines (134 lines with metadata and header). This appendix
retains every repetition, not just medians. Sums are measured elapsed durations,
not process CPU.

SHA-256 of original CSVs, unmodified stdout output:

```text
a1.csv e0486fc5893391e9226f70889d0c5c741c986238796ea1efceddcfd0cd1ecec0
b1.csv 418dc5978a4b89902acad2653ffeafe59cf46031566c6d301667a93c41d50ddd
b2.csv b51cd99b59ae829ea29b460d0655c4062431b609f9cae0490a528bd7105fba74
a2.csv 27c10624c93bce14f8968e01ad94d2ad2c4c7cc44db4bca9232238d5a433e041
```

```csv
run,signal,page_ms,stage,iterations,repeat0_ns,repeat1_ns,repeat2_ns,repeat3_ns,repeat4_ns,repeat5_ns,repeat6_ns
a1,silence,20,encode,5,520123400,541585000,448025800,454747800,462518600,584540300,532216700
a1,silence,20,mux,100,5472200,7696300,4773800,5315400,4854000,7813500,5117600
a1,silence,20,total,5,541668700,543104900,477538800,482996300,468651400,462883900,535870800
a1,silence,1000,encode,5,495546300,501866900,450566600,496714400,502604000,443294600,432726000
a1,silence,1000,mux,100,1484800,1675200,1133700,1474200,2298100,1134400,1375700
a1,silence,1000,total,5,512145600,519644000,514245400,494403200,510198300,483674900,449637700
a1,harmonic,20,encode,5,227182500,225285500,228473100,224195900,229332700,258359000,241302400
a1,harmonic,20,mux,100,7912900,8089800,7734800,7556200,7908900,7660500,7338400
a1,harmonic,20,total,5,226995600,236307400,228379700,232794600,229070500,229560900,247038500
a1,harmonic,1000,encode,5,226027500,219221200,225627100,239662200,220820900,238375000,236017300
a1,harmonic,1000,mux,100,4533800,4644800,4837000,4410900,5077900,5306900,5294500
a1,harmonic,1000,total,5,228283700,224183100,231870200,235786300,232040800,240132500,227742900
a1,noise,20,encode,5,621279600,662126100,722204000,710233800,705540300,670973000,698555000
a1,noise,20,mux,100,7316400,8216400,7847300,8412500,8069400,10210300,8640800
a1,noise,20,total,5,656742700,742304500,771216500,665042300,703789000,742334700,749643500
a1,noise,1000,encode,5,745566700,668866700,606010500,604210300,620461000,635110300,739190800
a1,noise,1000,mux,100,4882100,4318900,3839700,4182000,4079000,5294800,4411900
a1,noise,1000,total,5,717563300,691797800,638300400,621471600,642521600,626836700,727650800
b1,silence,20,encode,5,472374100,441878700,443613300,440988100,448844100,436654300,549110300
b1,silence,20,mux,100,4773500,4947400,5064300,5076200,4733000,6267500,6193900
b1,silence,20,total,5,439716200,435097100,444713900,438792800,447109900,436512100,510981000
b1,silence,1000,encode,5,522001400,617435400,506512500,494252500,446237500,509064100,494655000
b1,silence,1000,mux,100,1299200,1269600,1311400,1571000,1446900,1317800,1410300
b1,silence,1000,total,5,504579900,538049900,539204700,455645100,444637800,446483300,509240700
b1,harmonic,20,encode,5,255084700,226411400,238773100,244850400,232852400,227228100,232319700
b1,harmonic,20,mux,100,9666500,9276700,9843900,8774000,7860600,8177400,9403400
b1,harmonic,20,total,5,282589400,246618600,236291100,258443200,267597800,233081900,250378600
b1,harmonic,1000,encode,5,225717100,281693300,250610400,277953200,257109000,270955800,267771100
b1,harmonic,1000,mux,100,5036800,4507100,6539400,5235400,5178100,6675300,6105200
b1,harmonic,1000,total,5,233389300,242677800,282528600,261092100,262220300,256578900,271992000
b1,noise,20,encode,5,694422900,596361900,605312400,669242500,610171000,634260600,604517300
b1,noise,20,mux,100,8911800,7833400,8110100,7793700,7867600,8287000,8033100
b1,noise,20,total,5,661035300,619170100,640042400,608664900,609504700,619026600,596418800
b1,noise,1000,encode,5,616775600,624550100,600980900,610208700,607124600,601280500,598934900
b1,noise,1000,mux,100,4247000,4533300,3998000,4503600,3980600,3841500,3887800
b1,noise,1000,total,5,611990000,649647600,612840600,643387200,601649500,628458900,602238900
b2,silence,20,encode,5,455330100,446916200,540003700,499234500,498019100,496470400,533467000
b2,silence,20,mux,100,6251900,6345000,5921400,6108800,5469900,7104900,5997600
b2,silence,20,total,5,438690800,452114200,459138500,497624700,488154300,499368300,473451400
b2,silence,1000,encode,5,461523300,442752100,445723400,450156500,443200100,447910600,453896200
b2,silence,1000,mux,100,1370500,1804300,1415300,1482100,1327700,1381400,1427300
b2,silence,1000,total,5,447030600,466588000,439752400,434853100,450774300,446608700,449702100
b2,harmonic,20,encode,5,222013500,221914700,223336500,230135200,260831400,221884200,233735000
b2,harmonic,20,mux,100,8135400,7819000,8136100,8432000,7601500,7917100,8025900
b2,harmonic,20,total,5,236399800,223270300,228334300,225039100,232187000,244025900,281683600
b2,harmonic,1000,encode,5,257147200,235466300,225989400,227695400,255688200,327067600,272283100
b2,harmonic,1000,mux,100,3983900,4137100,4130800,4508700,4289700,5757200,5450100
b2,harmonic,1000,total,5,243900000,251700600,224308200,252313000,236675800,276512700,259833100
b2,noise,20,encode,5,706946900,716199200,715517100,665579200,781162300,621096900,607108900
b2,noise,20,mux,100,9599300,9131000,8360300,7752800,7593000,8013000,7681100
b2,noise,20,total,5,729717100,745929900,711099000,611453700,867290500,672699500,758905600
b2,noise,1000,encode,5,795218200,624224700,736379100,626084300,813879100,638534400,665792500
b2,noise,1000,mux,100,4195600,4171200,4345000,4354300,4075100,4238400,4436000
b2,noise,1000,total,5,606222900,630773700,748064800,613428200,617137800,789796600,719862700
a2,silence,20,encode,5,508112200,448930100,439164300,439138600,444106400,438140800,447940600
a2,silence,20,mux,100,5193000,5049200,5615500,4850800,5246700,5014100,4785600
a2,silence,20,total,5,455040500,445979600,448591300,449326800,440697300,448923400,442282500
a2,silence,1000,encode,5,436093100,445983700,435272100,441203400,519384300,489576000,508611500
a2,silence,1000,mux,100,1464100,1384600,1347500,1380400,1440000,1458800,1724900
a2,silence,1000,total,5,438983300,453821500,451873000,451859900,439820000,497001300,498333500
a2,harmonic,20,encode,5,262907100,222742300,234243900,231407200,227152200,222296400,224272500
a2,harmonic,20,mux,100,8112600,7699900,7487100,7922000,7702100,7538400,7784900
a2,harmonic,20,total,5,255891100,230679000,225198800,224679200,231961200,229345900,239388900
a2,harmonic,1000,encode,5,236541500,225989300,234962900,228302600,242934600,223412700,224916500
a2,harmonic,1000,mux,100,4720400,4433400,5138000,5069600,4893400,5390600,5313400
a2,harmonic,1000,total,5,237234200,226630600,242732800,233691600,227195400,249888400,229012300
a2,noise,20,encode,5,605588700,600847100,628023300,605387600,677923800,615856500,604454100
a2,noise,20,mux,100,7811000,7639300,8760100,7617300,9036200,7753300,8093800
a2,noise,20,total,5,613580000,601588100,608952800,673639900,701601700,621736600,609560500
a2,noise,1000,encode,5,620496300,605583300,608866800,616070400,647888300,675706600,678737100
a2,noise,1000,mux,100,4554900,4185700,4023100,4397500,4757600,4908100,4738200
a2,noise,1000,total,5,621428400,622407800,607556900,632252700,679152900,727623900,730892800
```

## Appendix C — CRC candidate tested then rejected

To reproduce the rejected experiment, replace **only** the `crc` function in
`src/ogg.rs` with this block, keep absolutely all codec settings and the same
benchmark, then rebuild and preserve a second binary. This block is not applied
in the final tree. The table is built at compile time, not runtime.

```rust
// Ogg uses the non-reflected 0x04c11db7 polynomial, initial value zero,
// and no final xor. A byte table replaces eight bit steps without changing CRCs.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0_u32; 256];
    let mut index = 0;
    while index < table.len() {
        let mut value = (index as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000_0000 != 0 {
                (value << 1) ^ 0x04c1_1db7
            } else {
                value << 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
};

fn crc(bytes: &[u8]) -> u32 {
    let mut crc = 0_u32;
    for byte in bytes {
        let index = usize::from((crc >> 24) as u8 ^ *byte);
        crc = (crc << 8) ^ CRC_TABLE[index];
    }
    crc
}
```

Do not infer from this result that every table CRC is intrinsically slower or
that another architecture would have the same results. Here it suffices to
**not retain** this optimization without evidence of a useful gain. A future
campaign could instrument native setup/encode or process CPU separately, with
sampling profiling; no such attribution is invented here.

> Separate follow-up of November 16: [Opus Release Windows](rust-opus-release-build.md),
> fresh baseline, cross-build fidelity and a four-run budget. Local gains and VS17
> qualification do not rewrite the CRC rejection or historical measurements above.

## Additional qualification — synthetic codec attribution of 2026-11-15

The [typed evidence and raw measurements](../evidence/rust-codec-attribution-20261115.json)
cover Windows, offline, only the three synthetic signals above. Measured source:
base `781ae03` and an uncommitted change to the benchmark alone; the 174-file
manifest is identical before/after. The subsequently added CI gate is separate,
outside this manifest; remote CI pending. The real CLI binary is unchanged; no
service or device call, credentials or real audio.

All 13 local phases passed: format, 8 example tests on local Rust stable 1.99.1
and 1.88.0 (including 5 benchmark and 3 Ogg tests), 61 `rambler-audio` tests
passed and 1 ignored on stable, strict audio Clippy and release build. The six
legacy proofs were validated; the three proofs of each attribution run equal the
20 ms proofs of the same build and the three historical 20 ms lines above.
Assertions cover all packets, CRC, sequence and granules, decoded hash after trim,
actual PCM progress, source EOF, closure and destruction. The decoder uses the
same libopus: it is not independent. This is not evidence of ASR, natural quality,
WAN, endurance or general parity.

Each run has 7 repetitions of 5 operations for each of the 3 signals; the 14
integer `batch_ns` sums per signal and stage are retained in JSON. Median, minimum
and maximum cover the 14 per-operation averages, without excluding outliers or
claiming significance. A validated warmup precedes each signal; batch order
rotates, signal order is fixed. Untimed validations influence caches; power,
frequency and affinity are uncontrolled; very short windows may be dominated by
the clock.

Five **standalone** windows measure native setup, host conversion and padding,
all native encoding calls with loops/wrapper, host packet collection, then native
destruction. Preallocated buffers and some host destruction are excluded. They
are not additive to the two public pipeline totals, which include draining, yields,
collection, closure and lifecycle probes, with progress observer disabled or
enabled. No residual subtraction, process CPU, cycles, allocation count or
production component ratio. Off/on ranges overlap: observation overhead cannot
be isolated from noise.

For a 5.0010625 s clip, grouped elapsed off medians are 103.40125 ms (silence),
53.53518 ms (harmonic), 140.80281 ms (noise); corresponding standalone
`native_frames` windows are 102.16344, 50.68323 and 141.12359 ms. Standalone
native setup is about 0.017 ms. These measurements justify no production change:
**NO_CANDIDATE**. Sources and an earlier Windows CMake cache indicate an
acceleration configuration, but attest neither the ISA actually linked nor the
dispatch executed.

Indicative reproduction, without concurrent compilation, with dependencies
already available locally; wait for the build to finish before the two sequential
runs and retain their outputs to compare proofs and times:

```sh
cargo +stable test --offline --locked -p rambler-audio --example profile_pipeline
cargo +stable build --offline --locked --release -p rambler-audio --example profile_pipeline
./target/release/examples/profile_pipeline.exe 3 1 1 80017
./target/release/examples/profile_pipeline.exe --codec-attribution --repeats 7 --iterations 5 --samples 80017
./target/release/examples/profile_pipeline.exe --codec-attribution --repeats 7 --iterations 5 --samples 80017
```

On Unix, omit `.exe`; the binary path assumes the default Cargo target. Use
`cargo +1.88.0 test` for the corresponding MSRV check.

## Remote CI qualification of the benchmark — 2026-11-15

Since the local snapshot above, both workflows for source commit
`72fd4affa561619745e9d642500467d41364458f` completed successfully: eight native
Rust jobs (Windows, Linux, macOS ARM and Intel × 1.88/stable), one Rust lint and
four Python jobs. Each of the eight steps of the new gate runs the eight example
tests (five benchmark and three Ogg); they do not run the timing campaign. Each
workspace reports 402 tests passed on Windows, 403 elsewhere, zero failed and two
ignored. Replayed groups count 10, 8, 1, 3 and 38 passes per job, without adding
them to distinct tests. The four pytest runs report 663/71 ignored on Python 3.11
or 733/1 on 3.14, with 335 subtests reported separately.

[CI evidence, steps, counts and hashes](../evidence/rust-codec-attribution-ci-20261115.json) ·
[separate local Windows measurements](../evidence/rust-codec-attribution-20261115.json).
The “remote CI pending” statement above describes the historical publication time
of the local set. This CI qualifies only tests, builds and checks of this source;
it proves neither speed or accuracy gain, real ASR, general parity nor live
authorization. No production optimization; production functionality is unchanged
and the linked native ISA is not attested. The historical preflight rejection is
retained. A [separate local remediation](../evidence/rust-private-capture-root-20261115.json)
created a private directory accepted by the original check, without changing
existing permissions: 30 synthetic tests and the real locking check pass. This
qualifies no new call; launcher integration and native budgets remain separate.
