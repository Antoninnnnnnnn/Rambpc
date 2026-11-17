mod common;

use common::*;
use rambler_audio::{EncoderConfig, FRAME_BYTES, FRAME_SAMPLES, Pcm16Opus};
use rambler_engine::AudioSource;
use std::sync::atomic::Ordering;

// Independent sample-to-frame oracle: typed samples bypass the adapter's byte
// assembly, pending buffer, EOF accounting and mux. The fixed public profile is
// VoIP/16kHz/mono, 24kbps VBR with 20ms frames (src/lib.rs, src/encode.rs).
fn native_reference(samples: &[i16]) -> (usize, Vec<Vec<u8>>) {
    let mut encoder =
        opus::Encoder::new(16000, opus::Channels::Mono, opus::Application::Voip).unwrap();
    encoder.set_bitrate(opus::Bitrate::Bits(24000)).unwrap();
    encoder.set_vbr(true).unwrap();
    let lookahead = usize::try_from(encoder.get_lookahead().unwrap()).unwrap();
    let mut padded = samples.to_vec();
    padded.resize(
        (samples.len() + lookahead).div_ceil(FRAME_SAMPLES) * FRAME_SAMPLES,
        0,
    );
    let (frames, tail) = padded.as_chunks::<FRAME_SAMPLES>();
    assert!(tail.is_empty());
    let packets = frames
        .iter()
        .map(|frame| {
            let mut packet = [0_u8; 1275];
            let size = encoder.encode(frame, &mut packet).unwrap();
            packet[..size].to_vec()
        })
        .collect();
    (lookahead * 3, packets)
}

#[tokio::test]
async fn signed_samples_and_nonzero_tails_match_native_frames_across_partitions() {
    // Existing encoding.rs checks duration at these boundaries, but compares
    // partitioned packet content only against another adapter on a long cosine.
    // Here an independent typed-sample oracle checks content, including extrema,
    // byte order and the final nonzero sample. Six streams x three partitions.
    for count in [1, 216, 217, 319, 320, 321] {
        let mut samples: Vec<i16> = (0..count)
            .map(|i| (i as u16).wrapping_mul(40503).wrapping_add(0x1234) as i16)
            .collect();
        for (sample, edge) in
            samples
                .iter_mut()
                .zip([i16::MIN, i16::MAX, -1, 0, 1, 0x0102, 0x0201, -0x0102])
        {
            *sample = edge;
        }
        *samples.last_mut().unwrap() = -12345;
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let (pre_skip, expected) = native_reference(&samples);

        for (partition, sizes) in [
            ("whole", vec![bytes.len()]),
            ("bytewise", vec![1]),
            ("frame seams", vec![FRAME_BYTES - 1, 1, 1, FRAME_BYTES - 1]),
        ] {
            // Empty chunks must not terminate input or lose a pending low byte.
            let mut chunks = vec![vec![]];
            for chunk in fragment(&bytes, &sizes) {
                chunks.push(chunk);
                chunks.push(vec![]);
            }
            assert_eq!(chunks.concat(), bytes);
            let source = Source::new(chunks);
            let stats = source.stats.clone();
            let mut audio = Pcm16Opus::new(source, EncoderConfig::default());
            let encoded = collect(&mut audio).await.unwrap();

            // Serial numbers differ between streams: compare complete audio
            // packets, not normalized Ogg bytes. The shared independent Ogg
            // reader checks CRC; assert_duration checks granules and decoded trim.
            assert_duration(&encoded, count);
            let actual = packets(&encoded);
            assert_eq!(
                usize::from(u16::from_le_bytes(actual[0][10..12].try_into().unwrap())),
                pre_skip,
                "samples={count}, partition={partition}"
            );
            assert_eq!(
                &actual[2..],
                expected.as_slice(),
                "samples={count}, partition={partition}"
            );
            assert!(stats.eof.load(Ordering::SeqCst));
            assert!(audio.next().await.unwrap().is_none());
            audio.close().await.unwrap();
            assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
        }
    }
}
