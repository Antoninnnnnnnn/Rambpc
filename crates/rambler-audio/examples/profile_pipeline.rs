//! Offline release-only benchmark; no files, network, credentials or private audio.
//! cargo run --release -p rambler-audio --example profile_pipeline --
//!     [repeats=9] [encode/total iterations=10] [mux iterations=100] [samples=160017]
//! Timers exclude generation, validation, hashing and decoding. Each operation
//! includes setup, allocations and consuming every output byte into Vecs.
//! Compile the actual private mux source, without exposing a benchmark-only API.
#[path = "../src/ogg.rs"]
mod native_ogg;

use bytes::Bytes;
use rambler_audio::{
    AudioError, EncoderConfig, FRAME_BYTES, FRAME_SAMPLES, MAX_PENDING_BYTES, Pcm16Opus,
};
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use sha2::{Digest, Sha256};
use std::{hint::black_box, io::Cursor, time::Instant};

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
        let packet_bytes: Vec<u8> = packets
            .iter()
            .flat_map(|p| {
                (p.len() as u32)
                    .to_le_bytes()
                    .into_iter()
                    .chain(p.iter().copied())
            })
            .collect();
        for page_ms in [20, 1000] {
            let reference = mux(&packets, samples, lookahead, page_ms);
            let expected = validate(&reference, &packets, samples);
            let (_, pages, end, skip) = canonical(&reference);
            println!(
                "proof,kind={kind},page_ms={page_ms},pcm_bytes={},pcm_sha256={},audio_packets={},packet_bytes={},packet_sha256={},ogg_bytes={},pages={pages},pre_skip={skip},final_granule={end},canonical_ogg_sha256={expected},decoded_pcm48k_sha256={}",
                pcm.len(),
                sha(&pcm),
                packets.len(),
                packets.iter().map(Vec::len).sum::<usize>(),
                sha(&packet_bytes),
                reference.len(),
                decoded_digest(&packets, skip, end)
            );
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
