//! Matched unpaced partitions on one original synthetic clip. Not a codec
//! build comparison, a network benchmark, or a production CPU decomposition.

use super::{
    AudioSource, Bytes, EncoderConfig, FRAME_BYTES, Lifecycle, Pcm16Opus, SourceError,
    SourceFuture, canonical, codec_operation, convert_frames, encode, encoder, mux, proof, timing,
    validate,
};
use rambler_audio::{PcmProgressHandle, pcm_channel};
use std::{
    hint::black_box,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

const SAMPLES: usize = 64_271;
const REPEATS: usize = 12;
const ITERATIONS: usize = 5;
const MUX_ITERATIONS: usize = 40;
const FRAME: &[usize] = &[FRAME_BYTES];
const FRAGMENTED: &[usize] = &[3, 47, 119, 160, 313];
const KIND: &str = "stepped-pulse";

fn clip() -> Bytes {
    // Alternating silence and two deterministic signed integer ramps with a
    // changing envelope. Include a distinctive nonzero last sample.
    (0..SAMPLES)
        .flat_map(|i| {
            let sample = if i + 1 == SAMPLES {
                -12345
            } else if (i / 1249).is_multiple_of(7) {
                0
            } else {
                let ramp = |p: usize| (i % p) as i32 - (p / 2) as i32;
                ((ramp(71) * 95 - ramp(113) * 53) * (1 + (i / 509 % 4) as i32) / 4) as i16
            };
            sample.to_le_bytes()
        })
        .collect::<Vec<_>>()
        .into()
}

struct Chunks {
    pcm: Bytes,
    sizes: &'static [usize],
    index: usize,
    offset: usize,
}
impl Chunks {
    fn new(pcm: Bytes, sizes: &'static [usize]) -> Self {
        Self {
            pcm,
            sizes,
            index: 0,
            offset: 0,
        }
    }
}
impl Iterator for Chunks {
    type Item = Bytes;

    fn next(&mut self) -> Option<Bytes> {
        if self.offset == self.pcm.len() {
            return None;
        }
        let end = (self.offset + self.sizes[self.index]).min(self.pcm.len());
        let chunk = self.pcm.slice(self.offset..end);
        self.offset = end;
        self.index = (self.index + 1) % self.sizes.len();
        Some(chunk)
    }
}
impl AudioSource for Chunks {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move { Ok(Iterator::next(self)) })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async { Ok(()) })
    }
}

// Don't box/wrap each `next`: both paths return exactly their source's future.
struct Tracked<S> {
    source: S,
    life: Arc<Lifecycle>,
}
impl<S: AudioSource> AudioSource for Tracked<S> {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        self.source.next()
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.life.closes.fetch_add(1, Ordering::Relaxed);
        self.source.close()
    }
}
impl<S> Drop for Tracked<S> {
    fn drop(&mut self) {
        self.life.drops.fetch_add(1, Ordering::Relaxed);
    }
}

struct Output {
    ogg: Vec<u8>,
    at_eof: (usize, usize),
    after_close: (usize, usize),
    repeated_eof: bool,
    progress: Option<PcmProgressHandle>,
}

async fn drain(source: impl AudioSource, warmup: bool) -> Output {
    let life = Arc::new(Lifecycle::default());
    let source = Tracked {
        source,
        life: life.clone(),
    };
    let counts = || {
        (
            life.closes.load(Ordering::Relaxed),
            life.drops.load(Ordering::Relaxed),
        )
    };
    let (mut audio, progress) = if warmup {
        let (audio, progress) = Pcm16Opus::new_with_progress(source, EncoderConfig::default());
        (audio, Some(progress))
    } else {
        (Pcm16Opus::new(source, EncoderConfig::default()), None)
    };
    let mut ogg = Vec::new();
    while let Some(page) = audio.next().await.unwrap() {
        if let Some(progress) = &progress
            && page[5] & 4 != 0
        {
            let state = progress.snapshot();
            assert!(state.input_eof && !state.output_eof);
            assert_eq!(counts(), (1, 1), "cleanup must precede EOS visibility");
        }
        ogg.extend_from_slice(&page);
    }
    let at_eof = counts();
    let repeated_eof = audio.next().await.unwrap().is_none();
    audio.close().await.unwrap();
    drop(audio);
    Output {
        ogg,
        at_eof,
        after_close: counts(),
        repeated_eof,
        progress,
    }
}

async fn pipeline(pcm: Bytes, sizes: &'static [usize], use_channel: bool, warmup: bool) -> Output {
    let chunks = Chunks::new(pcm, sizes);
    if !use_channel {
        return drain(chunks, warmup).await;
    }
    let (mut writer, input) = pcm_channel();
    let producer = async move {
        for chunk in chunks {
            writer.reserve().await.unwrap().send(chunk).unwrap();
        }
        writer.finish().unwrap();
    };
    let ((), output) = tokio::join!(producer, drain(input, warmup));
    output
}

fn check(output: Output, packets: &[Vec<u8>], expected: &str) {
    assert_eq!(validate(&output.ogg, packets, SAMPLES), expected);
    assert_eq!(output.at_eof, (1, 1));
    assert_eq!(output.after_close, (1, 1));
    assert!(output.repeated_eof);
    if let Some(progress) = output.progress {
        let state = progress.snapshot();
        assert_eq!(state.input_bytes, SAMPLES as u64 * 2);
        assert_eq!(state.encoded_input_samples, SAMPLES as u64);
        assert_eq!(state.encoded_frames, packets.len() as u64);
        assert!(state.input_eof && state.output_eof);
    }
}

pub(super) fn run() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let pcm = clip();
    let (_, lookahead) = encoder();
    let packets = encode(&pcm);
    let frames = (SAMPLES + usize::try_from(lookahead).unwrap()).div_ceil(super::FRAME_SAMPLES);
    assert_eq!(packets.len(), frames);
    let prepared = convert_frames(&pcm, frames);
    let paths = [
        ("ready_20ms", FRAME, false),
        ("channel_20ms", FRAME, true),
        ("ready_fragmented", FRAGMENTED, false),
        ("channel_fragmented", FRAGMENTED, true),
    ];
    println!(
        "meta,mode=native-ingress,opus={},samples={SAMPLES},frames={frames},repeats={REPEATS},iterations={ITERATIONS},mux_iterations={MUX_ITERATIONS},clock=Instant-wall-elapsed",
        rambler_audio::opus_version()
    );
    println!(
        "partition,frame_bytes={FRAME:?},frame_chunks={},fragment_bytes={FRAGMENTED:?},fragment_chunks={}",
        Chunks::new(pcm.clone(), FRAME).count(),
        Chunks::new(pcm.clone(), FRAGMENTED).count()
    );
    println!(
        "scope,unpaced-single-thread,pipeline=observer-off-source-and-encoder-setup-plus-full-page-drain-EOS-repeated-EOF-close-drop-and-probes,channel=adds-one-slot-producer-reserve-send-finish-join,mux=standalone-native-mux-and-page-collection,native_frames=standalone-padded-preconverted-encode-calls-safe-wrapper-loop-and-result-stores,exclude=generation-validation-decode-hash-printing-runtime-creation-output-Vec-destruction,scopes-not-additive"
    );
    let expected = proof(KIND, &pcm, &packets, SAMPLES, lookahead, 20);
    let reference = mux(&packets, SAMPLES, lookahead, 20);
    let (_, pages, end, _) = canonical(&reference);
    assert_eq!(pages, frames + 2);
    let mut reader = ogg::reading::PacketReader::new(std::io::Cursor::new(&reference));
    for index in 0..pages {
        let packet = reader.read_packet().unwrap().unwrap();
        assert!(packet.first_in_page() && packet.last_in_page());
        assert_eq!(packet.first_in_stream(), index == 0);
        assert_eq!(packet.last_in_stream(), index + 1 == pages);
        let granule = match index {
            0 | 1 => 0,
            _ if index + 1 == pages => end,
            _ => (index as u64 - 1) * 960,
        };
        assert_eq!(packet.absgp_page(), granule);
    }
    assert!(reader.read_packet().unwrap().is_none());

    // Full checked warmup of each path; enable progress here only to check that
    // natural source EOF and cleanup precede EOS, and output EOF follows it.
    codec_operation(&pcm, &prepared, &packets, lookahead);
    for (_, sizes, channel) in paths {
        check(
            runtime.block_on(pipeline(pcm.clone(), sizes, channel, true)),
            &packets,
            &expected,
        );
        check(
            runtime.block_on(pipeline(pcm.clone(), sizes, channel, false)),
            &packets,
            &expected,
        );
    }

    println!("timing,kind,page_ms,repeat,stage,iterations,batch_ns,per_iteration_ns");
    for repeat in 0..REPEATS {
        let stages = paths.len() + 2;
        // Each scope occupies each batch position twice, in opposite rotations.
        for stage in (0..stages).map(|i| {
            let position = (i + repeat) % stages;
            if repeat < stages {
                position
            } else {
                stages - 1 - position
            }
        }) {
            let count = if stage == 1 {
                MUX_ITERATIONS
            } else {
                ITERATIONS
            };
            let mut sum = Duration::ZERO;
            for _ in 0..count {
                if stage == 0 {
                    let measured = codec_operation(&pcm, &prepared, &packets, lookahead);
                    sum += measured[2]; // existing standalone native_frames window
                } else if stage == 1 {
                    let start = Instant::now();
                    let ogg = mux(black_box(&packets), SAMPLES, lookahead, 20);
                    black_box(&ogg);
                    sum += start.elapsed();
                    assert_eq!(validate(&ogg, &packets, SAMPLES), expected);
                } else {
                    let (_, sizes, channel) = paths[stage - 2];
                    let start = Instant::now();
                    let output =
                        runtime.block_on(pipeline(black_box(pcm.clone()), sizes, channel, false));
                    black_box(&output);
                    sum += start.elapsed();
                    check(output, &packets, &expected);
                }
            }
            let name = match stage {
                0 => "native_frames",
                1 => "mux",
                _ => paths[stage - 2].0,
            };
            timing(KIND, repeat, name, count, sum);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partitions_match_the_same_clip_and_cross_sample_and_frame_seams() {
        let pcm = clip();
        assert_eq!(pcm.len(), SAMPLES * 2);
        assert_eq!(&pcm[pcm.len() - 2..], &(-12345_i16).to_le_bytes());
        for (sizes, count) in [(FRAME, 201), (FRAGMENTED, 1003)] {
            let chunks: Vec<_> = Chunks::new(pcm.clone(), sizes).collect();
            assert_eq!(chunks.len(), count);
            assert_eq!(chunks.concat(), pcm);
        }
        assert_eq!(FRAGMENTED[0], 3);
        assert_eq!(FRAGMENTED.iter().sum::<usize>(), FRAME_BYTES + 2);
    }
}
