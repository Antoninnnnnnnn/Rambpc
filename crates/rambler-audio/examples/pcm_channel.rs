//! Offline public-API smoke: synthetic PCM producer -> bounded channel -> Opus/Ogg.
//! No microphone, files, credentials, network, pacing or service request.
//! Run: cargo run -p rambler-audio --example pcm_channel --locked --offline
use bytes::Bytes;
use rambler_audio::{EncoderConfig, Pcm16Opus, PcmInputError, pcm_channel};
use rambler_engine::{AudioSource, SourceError};
use std::io::Cursor;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    const SAMPLES: usize = 1017;
    let pcm = Bytes::from(
        (0..SAMPLES)
            .flat_map(|index| ((index as i16 % 97) * 100 - 4800).to_le_bytes())
            .collect::<Vec<_>>(),
    );
    let (mut writer, source) = pcm_channel();
    let mut encoded = Pcm16Opus::new(source, EncoderConfig::default());
    let producer = async move {
        // Odd byte boundaries are allowed; sample alignment is checked at EOF.
        for chunk in pcm.chunks(17) {
            let owned = Bytes::copy_from_slice(chunk);
            // Cancellation of this capacity wait does not take `owned`.
            let permit = writer.reserve().await?;
            permit.send(owned).map_err(|error| error.kind())?;
        }
        // Queue EOF only. This is NOT acknowledgement of encoding or RPC success.
        writer.finish()
    };
    let consumer = async {
        let result: Result<Vec<u8>, SourceError> = async {
            let mut bytes = Vec::new();
            while let Some(page) = encoded.next().await? {
                bytes.extend_from_slice(&page);
            }
            Ok(bytes)
        }
        .await;
        // Always await cleanup, including after a primary encoding failure.
        let cleanup = encoded.close().await;
        result.and_then(|bytes| cleanup.map(|()| bytes))
    };
    let (produced, encoded) = tokio::join!(producer, consumer);
    let produced: Result<(), PcmInputError> = produced;
    produced.expect("synthetic producer must finish");
    let encoded = encoded.expect("synthetic encoding and cleanup must succeed");

    // Independent Ogg reader checks actual page CRC/framing. The Opus decoder
    // checks sample coverage after the header pre-skip and final granule trim.
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(encoded));
    let mut decoder = opus::Decoder::new(48000, opus::Channels::Mono).unwrap();
    let mut packet_count = 0;
    let mut decoded_samples = 0;
    let mut skip = 0;
    let mut eos_granule = None;
    let mut serial = None;
    while let Some(packet) = reader.read_packet().expect("valid generated Ogg") {
        assert!(eos_granule.is_none(), "no packets after EOS");
        let expected_serial = *serial.get_or_insert_with(|| packet.stream_serial());
        assert_eq!(packet.stream_serial(), expected_serial);
        match packet_count {
            0 => {
                assert!(packet.first_in_stream());
                assert!(packet.data.starts_with(b"OpusHead"));
                skip = u16::from_le_bytes(packet.data[10..12].try_into().unwrap()) as u64;
            }
            1 => assert!(packet.data.starts_with(b"OpusTags")),
            _ => {
                let mut frame = [0_i16; 960];
                decoded_samples += decoder.decode(&packet.data, &mut frame, false).unwrap();
            }
        }
        if packet.last_in_stream() {
            eos_granule = Some(packet.absgp_page());
        }
        packet_count += 1;
    }
    let granule = eos_granule.expect("explicit finish must flush one EOS");
    assert!(packet_count >= 3);
    assert!(skip > 0);
    assert_eq!(granule - skip, SAMPLES as u64 * 3);
    assert!(decoded_samples as u64 >= granule);
    assert!(decoded_samples as u64 - granule < 960);
    println!(
        "offline PCM channel: input_samples={SAMPLES}, decoded_samples_48k={}, ogg_eos=1, service_calls=0",
        granule - skip
    );
}
