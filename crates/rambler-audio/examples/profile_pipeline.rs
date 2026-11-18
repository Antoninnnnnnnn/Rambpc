//! Offline release-only benchmark; no files, network, credentials or private audio.
//! cargo run --release -p rambler-audio --example profile_pipeline --
//!     [repeats=9] [encode/total iterations=10] [mux iterations=100] [samples=160017]
//! Additive codec attribution mode (20ms pages only):
//!     --codec-attribution [--repeats 7] [--iterations 5] [--samples 80017]
//! Additive bounded native ingress mode: --native-ingress (fixed workload).
//! Timers exclude generation, validation, hashing and decoding. Legacy operations
//! include setup, allocations and consuming every output byte into Vecs.
//! Attribution scopes and exclusions are emitted as metadata; none are CPU cycles.
//! Compile the actual private mux source, without exposing a benchmark-only API.
#[path = "profile_pipeline/ingress.rs"]
mod ingress;
#[path = "../src/ogg.rs"]
mod native_ogg;

use bytes::Bytes;
use rambler_audio::{
    AudioError, EncoderConfig, FRAME_BYTES, FRAME_SAMPLES, MAX_PENDING_BYTES, Pcm16Opus,
};
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use sha2::{Digest, Sha256};
use std::{
    hint::black_box,
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct MemorySource {
    pcm: Bytes,
    offset: usize,
    closed: bool,
}
impl AudioSource for MemorySource {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            assert!(!self.closed);
            if self.offset == self.pcm.len() {
                return Ok(None);
            }
            let end = (self.offset + FRAME_BYTES).min(self.pcm.len());
            let out = self.pcm.slice(self.offset..end);
            self.offset = end;
            Ok(Some(out))
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.closed = true;
        Box::pin(async { Ok(()) })
    }
}

fn synthetic(kind: &str, samples: usize) -> Bytes {
    let mut seed = 0x1234_5678_u32;
    let mut out = Vec::with_capacity(samples * 2);
    for i in 0..samples {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        // Integer-only, deterministic across hosts. Harmonic is not speech and
        // says nothing about recognition quality or natural-voice fidelity.
        let sample = match kind {
            "silence" => 0,
            "harmonic" => {
                let triangle = |period: usize| {
                    let phase = (i % period) as i32;
                    (phase.min(period as i32 - phase) * 4 - period as i32) * 6000 / period as i32
                };
                let envelope = if (i / 3200) % 5 == 4 { 0 } else { 1 };
                ((triangle(80) + triangle(37) / 3) * envelope) as i16
            }
            "noise" => (seed >> 16) as i16 / 3,
            _ => unreachable!(),
        };
        out.extend(sample.to_le_bytes());
    }
    out.into()
}

fn configured_encoder() -> Result<(opus::Encoder, i32), opus::Error> {
    let mut encoder = opus::Encoder::new(16000, opus::Channels::Mono, opus::Application::Voip)?;
    encoder.set_bitrate(opus::Bitrate::Bits(24000))?;
    encoder.set_vbr(true)?;
    let lookahead = encoder.get_lookahead()?;
    Ok((encoder, lookahead))
}

fn encoder() -> (opus::Encoder, u64) {
    let mut encoder =
        opus::Encoder::new(16000, opus::Channels::Mono, opus::Application::Voip).unwrap();
    encoder.set_bitrate(opus::Bitrate::Bits(24000)).unwrap();
    encoder.set_vbr(true).unwrap();
    let lookahead = encoder.get_lookahead().unwrap() as u64;
    assert!(lookahead > 0);
    (encoder, lookahead)
}

// Keep the measured iterator form: lint-only changes must not alter this workload.
#[allow(clippy::chunks_exact_to_as_chunks)]
fn encode(pcm: &[u8]) -> Vec<Vec<u8>> {
    let (mut encoder, lookahead) = encoder();
    let frames = (pcm.len() / 2 + lookahead as usize).div_ceil(FRAME_SAMPLES);
    let mut packets = Vec::new();
    for index in 0..frames {
        let mut samples = [0_i16; FRAME_SAMPLES];
        let start = index * FRAME_BYTES;
        if start < pcm.len() {
            let end = (start + FRAME_BYTES).min(pcm.len());
            for (sample, pair) in samples.iter_mut().zip(pcm[start..end].chunks_exact(2)) {
                *sample = i16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        let mut packet = [0_u8; 1275];
        let size = encoder.encode(&samples, &mut packet).unwrap();
        packets.push(packet[..size].to_vec());
    }
    packets
}

fn mux(packets: &[Vec<u8>], samples: usize, lookahead: u64, page_ms: u16) -> Vec<u8> {
    let mut mux = native_ogg::Ogg::new(page_ms, (lookahead * 3) as u16).unwrap();
    let mut out = Vec::new();
    while let Some(page) = mux.pop() {
        out.extend_from_slice(&page);
    }
    for (index, packet) in packets.iter().enumerate() {
        let eos = index + 1 == packets.len();
        let granule = if eos {
            (samples as u64 + lookahead) * 3
        } else {
            (index as u64 + 1) * 960
        };
        mux.packet(packet, granule, eos).unwrap();
        while let Some(page) = mux.pop() {
            out.extend_from_slice(&page);
        }
    }
    out
}

async fn total(pcm: Bytes, page_ms: u16) -> Vec<u8> {
    let mut audio = Pcm16Opus::new(
        MemorySource {
            pcm,
            offset: 0,
            closed: false,
        },
        EncoderConfig {
            page_duration_ms: page_ms,
            ..Default::default()
        },
    );
    let mut out = Vec::new();
    while let Some(page) = audio.next().await.unwrap() {
        out.extend_from_slice(&page);
    }
    audio.close().await.unwrap();
    out
}

fn sha(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

// Ogg serials intentionally differ per stream. Zero ONLY serial and checksum
// for the canonical digest, after independent PacketReader validated real CRCs.
fn canonical(data: &[u8]) -> (Vec<u8>, usize, u64, u16) {
    let mut normalized = data.to_vec();
    let mut offset = 0;
    let mut pages = 0;
    let mut last_granule = 0;
    let mut pre_skip = 0;
    let serial = &data[14..18];
    while offset < data.len() {
        assert_eq!(&data[offset..offset + 4], b"OggS");
        assert_eq!(&data[offset + 14..offset + 18], serial);
        assert_eq!(
            u32::from_le_bytes(data[offset + 18..offset + 22].try_into().unwrap()),
            pages
        );
        let laces = data[offset + 26] as usize;
        let header_end = offset + 27 + laces;
        let size = 27
            + laces
            + data[offset + 27..header_end]
                .iter()
                .map(|n| *n as usize)
                .sum::<usize>();
        last_granule = u64::from_le_bytes(data[offset + 6..offset + 14].try_into().unwrap());
        assert_eq!(
            data[offset + 5],
            if pages == 0 {
                2
            } else if offset + size == data.len() {
                4
            } else {
                0
            }
        );
        if pages == 0 {
            pre_skip =
                u16::from_le_bytes(data[header_end + 10..header_end + 12].try_into().unwrap());
        }
        normalized[offset + 14..offset + 18].fill(0);
        normalized[offset + 22..offset + 26].fill(0);
        offset += size;
        pages += 1;
    }
    assert_eq!(offset, data.len());
    (normalized, pages as usize, last_granule, pre_skip)
}

fn read_packets(data: &[u8]) -> Vec<Vec<u8>> {
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(data));
    let mut packets = Vec::new();
    while let Some(packet) = reader.read_packet().unwrap() {
        packets.push(packet.data);
    }
    assert!(packets[0].starts_with(b"OpusHead"));
    assert!(packets[1].starts_with(b"OpusTags"));
    packets.drain(..2);
    packets
}

fn validate(data: &[u8], packets: &[Vec<u8>], samples: usize) -> String {
    assert_eq!(read_packets(data), packets); // independent Ogg framing/CRC reader
    let (normalized, _, end, skip) = canonical(data);
    assert_eq!(end - u64::from(skip), samples as u64 * 3);
    sha(&normalized)
}

fn decoded_digest(packets: &[Vec<u8>], skip: u16, end: u64) -> String {
    let mut decoder = opus::Decoder::new(48000, opus::Channels::Mono).unwrap();
    let mut decoded = Vec::new();
    for packet in packets {
        let mut frame = [0_i16; 960];
        assert_eq!(decoder.decode(packet, &mut frame, false).unwrap(), 960);
        decoded.extend(frame);
    }
    assert!(decoded.len() >= end as usize);
    assert!(decoded.len() - (end as usize) < 960);
    let bytes: Vec<u8> = decoded[skip as usize..end as usize]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    sha(&bytes)
}

fn proof(
    kind: &str,
    pcm: &[u8],
    packets: &[Vec<u8>],
    samples: usize,
    lookahead: u64,
    page_ms: u16,
) -> String {
    let packet_bytes: Vec<u8> = packets
        .iter()
        .flat_map(|p| {
            (p.len() as u32)
                .to_le_bytes()
                .into_iter()
                .chain(p.iter().copied())
        })
        .collect();
    let reference = mux(packets, samples, lookahead, page_ms);
    let expected = validate(&reference, packets, samples);
    let (_, pages, end, skip) = canonical(&reference);
    println!(
        "proof,kind={kind},page_ms={page_ms},pcm_bytes={},pcm_sha256={},audio_packets={},packet_bytes={},packet_sha256={},ogg_bytes={},pages={pages},pre_skip={skip},final_granule={end},canonical_ogg_sha256={expected},decoded_pcm48k_sha256={}",
        pcm.len(),
        sha(pcm),
        packets.len(),
        packets.iter().map(Vec::len).sum::<usize>(),
        sha(&packet_bytes),
        reference.len(),
        decoded_digest(packets, skip, end)
    );
    expected
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CodecArgs {
    repeats: usize,
    iterations: usize,
    samples: usize,
}

fn parse_codec_args(args: &[&str]) -> Result<CodecArgs, String> {
    let mut options = CodecArgs {
        repeats: 7,
        iterations: 5,
        samples: 80017,
    };
    let mut seen = [false; 3];
    let mut args = args.iter();
    while let Some(&name) = args.next() {
        let (index, slot, min, max) = match name {
            "--repeats" => (0, &mut options.repeats, 3, 100),
            "--iterations" => (1, &mut options.iterations, 1, 10000),
            "--samples" => (2, &mut options.samples, 1, 16000000),
            _ => return Err(format!("unknown codec-attribution argument: {name}")),
        };
        if seen[index] {
            return Err(format!("duplicate argument: {name}"));
        }
        seen[index] = true;
        let text = args
            .next()
            .ok_or_else(|| format!("missing value for {name}"))?;
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("{name} requires an unsigned decimal integer"));
        }
        let value = text
            .parse::<usize>()
            .map_err(|_| format!("{name} integer overflow"))?;
        if !(min..=max).contains(&value) {
            return Err(format!("{name} must be in {min}..={max}"));
        }
        *slot = value;
    }
    Ok(options)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Preparation {
    pcm_bytes: usize,
    frames: usize,
}

// Check arithmetic before generation/allocation, including codec delay padding.
fn preparation(samples: usize, lookahead: usize) -> Result<Preparation, &'static str> {
    if samples == 0 || lookahead == 0 {
        return Err("samples and lookahead must be positive");
    }
    let pcm_bytes = samples.checked_mul(2).ok_or("PCM size overflow")?;
    let target = samples.checked_add(lookahead).ok_or("padding overflow")?;
    let frames = (target / FRAME_SAMPLES)
        .checked_add(usize::from(!target.is_multiple_of(FRAME_SAMPLES)))
        .ok_or("frame count overflow")?;
    for bytes in [
        pcm_bytes,
        frames
            .checked_mul(FRAME_BYTES)
            .ok_or("frame storage overflow")?,
        frames.checked_mul(1275).ok_or("packet storage overflow")?,
    ] {
        if bytes > isize::MAX as usize {
            return Err("storage exceeds addressable allocation");
        }
    }
    Ok(Preparation { pcm_bytes, frames })
}

fn convert_frames(pcm: &[u8], frames: usize) -> Vec<[i16; FRAME_SAMPLES]> {
    let mut out = vec![[0_i16; FRAME_SAMPLES]; frames];
    for (input, frame) in pcm.chunks(FRAME_BYTES).zip(&mut out) {
        for (pair, sample) in input.as_chunks::<2>().0.iter().zip(frame) {
            *sample = i16::from_le_bytes([pair[0], pair[1]]);
        }
    }
    out
}

const CODEC_STAGES: [&str; 5] = [
    "native_setup",
    "host_conversion",
    "native_frames",
    "host_packet_collection",
    "native_drop",
];

// Disjoint windows in a standalone staged workload, NOT a decomposition of total.
// Output buffers and Result slots are allocated/zeroed before these windows.
fn codec_operation(
    pcm: &[u8],
    prepared: &[[i16; FRAME_SAMPLES]],
    packets: &[Vec<u8>],
    lookahead: u64,
) -> [Duration; 5] {
    let mut buffers = vec![[0_u8; 1275]; prepared.len()];
    let mut sizes: Vec<Result<usize, opus::Error>> = (0..prepared.len()).map(|_| Ok(0)).collect();
    let mut times = [Duration::ZERO; 5];

    let start = Instant::now();
    let setup = configured_encoder();
    black_box(&setup);
    times[0] = start.elapsed();
    let (mut encoder, delay) = setup.unwrap();
    assert_eq!(u64::try_from(delay).unwrap(), lookahead);

    let start = Instant::now();
    let frames = convert_frames(black_box(pcm), prepared.len());
    black_box(&frames);
    times[1] = start.elapsed();
    assert_eq!(frames, prepared);

    let start = Instant::now();
    for ((frame, buffer), size) in black_box(&frames).iter().zip(&mut buffers).zip(&mut sizes) {
        *size = encoder.encode(frame, buffer);
    }
    black_box((&buffers, &sizes));
    times[2] = start.elapsed();
    // Check native results only after stopping the clock. No unwrap/assert in
    // the timed encode loop; the safe opus wrapper's own error handling remains.
    let sizes: Vec<usize> = sizes.into_iter().map(Result::unwrap).collect();
    for size in &sizes {
        assert!((1..=1275).contains(size));
    }

    let start = Instant::now();
    let mut actual = Vec::new();
    for (buffer, size) in buffers.iter().zip(&sizes) {
        actual.push(buffer[..*size].to_vec());
    }
    black_box(&actual);
    times[3] = start.elapsed();

    let start = Instant::now();
    drop(encoder);
    times[4] = start.elapsed();
    assert_eq!(actual, packets);
    times
}

#[derive(Default)]
struct Lifecycle {
    eof: AtomicBool,
    closes: AtomicUsize,
    drops: AtomicUsize,
}

// Same frame-sized chunks as MemorySource, with cleanup evidence recorded, not
// asserted, inside the pipeline. These identical off/on probes cost wall time.
struct AttributionSource {
    pcm: Bytes,
    offset: usize,
    lifecycle: Arc<Lifecycle>,
}
impl AudioSource for AttributionSource {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if self.offset == self.pcm.len() {
                self.lifecycle.eof.store(true, Ordering::Relaxed);
                return Ok(None);
            }
            let end = (self.offset + FRAME_BYTES).min(self.pcm.len());
            let out = self.pcm.slice(self.offset..end);
            self.offset = end;
            Ok(Some(out))
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.lifecycle.closes.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Ok(()) })
    }
}
impl Drop for AttributionSource {
    fn drop(&mut self) {
        self.lifecycle.drops.fetch_add(1, Ordering::Relaxed);
    }
}

struct PipelineOutput {
    ogg: Vec<u8>,
    lifecycle: Arc<Lifecycle>,
    progress: Option<rambler_audio::PcmProgressHandle>,
}

async fn attribution_total(pcm: Bytes, observe: bool) -> Result<PipelineOutput, SourceError> {
    let lifecycle = Arc::new(Lifecycle::default());
    let source = AttributionSource {
        pcm,
        offset: 0,
        lifecycle: lifecycle.clone(),
    };
    let (mut audio, progress) = if observe {
        let (audio, progress) = Pcm16Opus::new_with_progress(source, EncoderConfig::default());
        (audio, Some(progress))
    } else {
        (Pcm16Opus::new(source, EncoderConfig::default()), None)
    };
    let mut ogg = Vec::new();
    while let Some(page) = audio.next().await? {
        ogg.extend_from_slice(&page);
    }
    audio.close().await?;
    drop(audio);
    Ok(PipelineOutput {
        ogg,
        lifecycle,
        progress,
    })
}

fn validate_pipeline(actual: &PipelineOutput, packets: &[Vec<u8>], samples: usize, expected: &str) {
    assert_eq!(validate(&actual.ogg, packets, samples), expected);
    assert!(actual.lifecycle.eof.load(Ordering::Relaxed));
    assert_eq!(actual.lifecycle.closes.load(Ordering::Relaxed), 1);
    assert_eq!(actual.lifecycle.drops.load(Ordering::Relaxed), 1);
    if let Some(progress) = &actual.progress {
        let state = progress.snapshot();
        assert_eq!(state.input_bytes, samples as u64 * 2);
        assert_eq!(state.encoded_input_samples, samples as u64);
        assert_eq!(state.encoded_frames, packets.len() as u64);
        assert!(state.input_eof && state.output_eof);
    }
}

fn timing(kind: &str, repeat: usize, stage: &str, count: usize, elapsed: Duration) {
    println!(
        "timing,{kind},20,{repeat},{stage},{count},{},{:.3}",
        elapsed.as_nanos(),
        elapsed.as_nanos() as f64 / count as f64
    );
}

fn codec_attribution(options: CodecArgs) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (_, lookahead) = encoder();
    assert!(lookahead <= u64::from(u16::MAX) / 3);
    let layout = preparation(options.samples, usize::try_from(lookahead).unwrap()).unwrap();
    println!(
        "meta,mode=codec-attribution,opus={},profile=voip-mono16k-vbr24k-frame20ms,lookahead16k={lookahead},repeats={},iterations={},samples={},frames={},allocation_counts=unavailable,clock=Instant-wall-elapsed",
        rambler_audio::opus_version(),
        options.repeats,
        options.iterations,
        options.samples,
        layout.frames
    );
    println!(
        "scope,native_setup=create-configure-get_lookahead-safe-wrapper,native_frames=all-native-encode-calls-plus-safe-wrapper-loop-result-stores,host_conversion=whole-clip-PCM16LE-plus-zero-padding-and-Vec-allocation,host_packet_collection=all-packet-copies-and-Vec-allocations,native_drop=encoder-destructor"
    );
    println!(
        "scope,codec_excludes=preallocated-packet-buffers-result-slots-and-output-Vec-destruction,totals=public-pipeline-frame-chunks-640-default-pages-20-all-yields-setup-production-checks-collect-close-drop-plus-source-lifecycle-probes,total_observer_on=includes-handle-creation-and-updates-no-snapshot,all_exclude=generation-oracles-hashing-decoding-printing-runtime-creation-output-destruction,stages=separate-scopes-not-additive-to-total,no-residual-subtraction,no-process-CPU-or-cycle-or-allocation-count-claim"
    );
    println!("timing,kind,page_ms,repeat,stage,iterations,batch_ns,per_iteration_ns");
    for kind in ["silence", "harmonic", "noise"] {
        let pcm = synthetic(kind, options.samples);
        assert_eq!(pcm.len(), layout.pcm_bytes);
        let prepared = convert_frames(&pcm, layout.frames);
        let packets = encode(&pcm);
        assert_eq!(packets.len(), layout.frames);
        let expected = proof(kind, &pcm, &packets, options.samples, lookahead, 20);
        // One complete warmup of every scope, fully checked, excluded from CSV.
        codec_operation(&pcm, &prepared, &packets, lookahead);
        for observe in [false, true] {
            let actual = runtime
                .block_on(attribution_total(pcm.clone(), observe))
                .unwrap();
            validate_pipeline(&actual, &packets, options.samples, &expected);
        }
        for repeat in 0..options.repeats {
            // Rotate standalone codec / total-off / total-on batch order. Codec
            // windows retain dependency order; signals retain fixed order.
            for stage in (0..3).map(|n| (n + repeat) % 3) {
                let mut elapsed = [Duration::ZERO; 5];
                for _ in 0..options.iterations {
                    if stage == 0 {
                        let measured = codec_operation(&pcm, &prepared, &packets, lookahead);
                        for (sum, time) in elapsed.iter_mut().zip(measured) {
                            *sum += time;
                        }
                    } else {
                        let start = Instant::now();
                        let actual =
                            runtime.block_on(attribution_total(black_box(pcm.clone()), stage == 2));
                        black_box(&actual);
                        elapsed[0] += start.elapsed();
                        let actual = actual.unwrap();
                        validate_pipeline(&actual, &packets, options.samples, &expected);
                    }
                }
                if stage == 0 {
                    for (name, elapsed) in CODEC_STAGES.iter().zip(elapsed) {
                        timing(kind, repeat, name, options.iterations, elapsed);
                    }
                } else {
                    let name = if stage == 1 {
                        "total_observer_off"
                    } else {
                        "total_observer_on"
                    };
                    timing(kind, repeat, name, options.iterations, elapsed[0]);
                }
            }
        }
    }
}

fn arg(index: usize, default: usize) -> usize {
    std::env::args()
        .nth(index)
        .map(|s| s.parse().expect("positive integer"))
        .unwrap_or(default)
}

// Runtime rejection keeps ordinary debug test builds possible, but not timings.
#[allow(clippy::assertions_on_constants)]
fn main() {
    assert!(!cfg!(debug_assertions), "benchmark requires --release");
    if std::env::args().nth(1).as_deref() == Some("--native-ingress") {
        if std::env::args_os().nth(2).is_some() {
            eprintln!("--native-ingress takes no arguments");
            std::process::exit(2);
        }
        ingress::run();
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--codec-attribution") {
        let args: Vec<String> = std::env::args().skip(2).collect();
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        match parse_codec_args(&refs) {
            Ok(options) => codec_attribution(options),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
        return;
    }
    let repeats = arg(1, 9);
    let iterations = arg(2, 10);
    let mux_iterations = arg(3, 100);
    let samples = arg(4, 160017);
    assert!((3..=100).contains(&repeats));
    assert!((1..=10000).contains(&iterations));
    assert!((1..=10000).contains(&mux_iterations));
    assert!((1..=16000000).contains(&samples));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (_, lookahead) = encoder();
    println!(
        "meta,opus={},profile=voip-mono16k-vbr24k-frame20ms,lookahead16k={},repeats={},iterations={},mux_iterations={},samples={}",
        rambler_audio::opus_version(),
        lookahead,
        repeats,
        iterations,
        mux_iterations,
        samples
    );
    println!("timing,kind,page_ms,repeat,stage,iterations,batch_ns,per_iteration_ns");
    for kind in ["silence", "harmonic", "noise"] {
        let pcm = synthetic(kind, samples);
        let packets = encode(&pcm);
        for page_ms in [20, 1000] {
            let expected = proof(kind, &pcm, &packets, samples, lookahead, page_ms);
            // Warm every stage once; not included in samples. Full production
            // packets must match the standalone encoder before collecting times.
            assert_eq!(encode(&pcm), packets);
            assert_eq!(
                validate(
                    &runtime.block_on(total(pcm.clone(), page_ms)),
                    &packets,
                    samples
                ),
                expected
            );
            for repeat in 0..repeats {
                // Rotate stage order to avoid always favouring the same stage.
                for stage in (0..3).map(|n| (n + repeat) % 3) {
                    let count = if stage == 1 {
                        mux_iterations
                    } else {
                        iterations
                    };
                    let mut elapsed = std::time::Duration::ZERO;
                    for _ in 0..count {
                        if stage == 0 {
                            let start = Instant::now();
                            let actual = encode(black_box(&pcm));
                            black_box(&actual);
                            elapsed += start.elapsed();
                            assert_eq!(actual, packets);
                        } else {
                            let start = Instant::now();
                            let actual = if stage == 1 {
                                mux(black_box(&packets), samples, lookahead, page_ms)
                            } else {
                                runtime.block_on(total(black_box(pcm.clone()), page_ms))
                            };
                            black_box(&actual);
                            elapsed += start.elapsed();
                            assert_eq!(validate(&actual, &packets, samples), expected);
                        }
                    }
                    let name = ["encode", "mux", "total"][stage];
                    println!(
                        "timing,{kind},{page_ms},{repeat},{name},{count},{},{:.3}",
                        elapsed.as_nanos(),
                        elapsed.as_nanos() as f64 / count as f64
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_defaults_and_explicit_bounds() {
        assert_eq!(
            parse_codec_args(&[]).unwrap(),
            CodecArgs {
                repeats: 7,
                iterations: 5,
                samples: 80017,
            }
        );
        for (args, expected) in [
            (
                ["--repeats", "3", "--iterations", "1", "--samples", "1"],
                CodecArgs {
                    repeats: 3,
                    iterations: 1,
                    samples: 1,
                },
            ),
            (
                [
                    "--samples",
                    "16000000",
                    "--repeats",
                    "100",
                    "--iterations",
                    "10000",
                ],
                CodecArgs {
                    repeats: 100,
                    iterations: 10000,
                    samples: 16000000,
                },
            ),
        ] {
            assert_eq!(parse_codec_args(&args).unwrap(), expected);
        }
    }

    #[test]
    fn codec_rejects_zero_overflow_unknown_missing_duplicate_and_bad_numbers() {
        for name in ["--repeats", "--iterations", "--samples"] {
            for value in ["0", "-1", "+1", "1.5", "", "184467440737095516160", " 3"] {
                assert!(parse_codec_args(&[name, value]).is_err(), "{name} {value}");
            }
            assert!(parse_codec_args(&[name]).is_err());
        }
        for args in [
            vec!["--repeats", "2"],
            vec!["--repeats", "101"],
            vec!["--iterations", "10001"],
            vec!["--samples", "16000001"],
            vec!["--unknown", "3"],
            vec!["--samples=3"],
            vec!["3"],
            vec!["--samples", "3", "--samples", "4"],
            vec!["--samples", "3", "extra"],
        ] {
            assert!(parse_codec_args(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn checked_padding_covers_boundaries_without_extra_frames() {
        for (samples, frames) in [(1, 1), (216, 1), (217, 2), (320, 2), (80017, 251)] {
            assert_eq!(
                preparation(samples, 104).unwrap(),
                Preparation {
                    pcm_bytes: samples * 2,
                    frames,
                }
            );
        }
        for (samples, lookahead) in [
            (0, 104),
            (1, 0),
            (usize::MAX, 1),
            (1, usize::MAX),
            (isize::MAX as usize / 2, 1),
        ] {
            assert!(preparation(samples, lookahead).is_err());
        }
    }

    #[test]
    fn conversion_preserves_le_samples_tail_and_lookahead_zeros() {
        for samples in [1, 216, 217, 320, 321, 640] {
            let layout = preparation(samples, 104).unwrap();
            let input: Vec<i16> = (0..samples)
                .map(|i| (i as i16).wrapping_mul(-137))
                .collect();
            let pcm: Vec<u8> = input
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect();
            let frames = convert_frames(&pcm, layout.frames);
            let flat: Vec<i16> = frames.into_iter().flatten().collect();
            assert_eq!(&flat[..samples], &input);
            assert!(flat[samples..].iter().all(|sample| *sample == 0));
            assert!(flat.len() >= samples + 104);
            assert!(flat.len() - (samples + 104) < FRAME_SAMPLES);
        }
    }

    #[test]
    fn synthetic_inputs_retain_published_integer_only_fingerprints() {
        for (kind, expected) in [
            (
                "silence",
                "b5e0410caf0728b0a46afb14c1eb4935d2cd0ae6fb5cd350d2df8818ec80b201",
            ),
            (
                "harmonic",
                "c554e95f4010e21ba354933148262aa41d6247d1910996740ea00e45b15ae24c",
            ),
            (
                "noise",
                "8afee5c59bf517ab814636fb6e8b2c7d8925f81a6f902ce9193a7c2041f89107",
            ),
        ] {
            assert_eq!(sha(&synthetic(kind, 80017)), expected);
        }
    }
}
