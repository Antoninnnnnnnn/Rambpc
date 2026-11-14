//! Offline test fixture: real libopus packets muxed by the independent ogg crate.
//! Neither library is needed by a normal (non-test, non-audio) CLI build.
pub fn fixture(packet_count: usize) -> Vec<u8> {
    use ogg::writing::{PacketWriteEndInfo, PacketWriter};
    assert!(packet_count > 0);
    let mut encoder =
        opus::Encoder::new(16_000, opus::Channels::Mono, opus::Application::Audio).unwrap();
    encoder.set_bitrate(opus::Bitrate::Bits(32_000)).unwrap();
    let mut head = b"OpusHead\x01\x01".to_vec();
    head.extend_from_slice(&((encoder.get_lookahead().unwrap() * 3) as u16).to_le_bytes());
    head.extend_from_slice(&16_000u32.to_le_bytes());
    head.extend_from_slice(&[0, 0, 0]);
    let vendor = "Rambler offline fixture été".as_bytes();
    let mut tags = b"OpusTags".to_vec();
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes());
    let mut writer = PacketWriter::new(Vec::new());
    writer
        .write_packet(head, 0x12345678, PacketWriteEndInfo::EndPage, 0)
        .unwrap();
    writer
        .write_packet(tags, 0x12345678, PacketWriteEndInfo::EndPage, 0)
        .unwrap();
    for index in 0..packet_count {
        let samples: Vec<i16> = (0..320)
            .map(|i| (((index * 320 + i) as f64 * 0.17).sin() * 10_000.0) as i16)
            .collect();
        let mut packet = vec![0; 4000];
        let size = encoder.encode(&samples, &mut packet).unwrap();
        packet.truncate(size);
        writer
            .write_packet(
                packet,
                0x12345678,
                if index + 1 == packet_count {
                    PacketWriteEndInfo::EndStream
                } else {
                    PacketWriteEndInfo::EndPage
                },
                (index as u64 + 1) * 960,
            )
            .unwrap();
    }
    writer.into_inner()
}
