use super::*;

#[test]
fn maximum_packets_and_255_byte_multiples_keep_valid_lacing_and_crc() {
    for size in [255, 510, 1274, 1275] {
        let mut mux = Ogg::new(1000, 312).unwrap();
        let packet = vec![0x55; size];
        let mut bytes = vec![];
        for index in 0..200 {
            mux.packet(&packet, (index + 1) * 960, index == 199)
                .unwrap();
            while let Some(page) = mux.pop() {
                assert!(page.len() <= MAX_PENDING_BYTES);
                assert!(page[26] > 0);
                bytes.extend(page);
            }
        }
        let mut reader = ogg::reading::PacketReader::new(std::io::Cursor::new(bytes));
        assert!(
            reader
                .read_packet()
                .unwrap()
                .unwrap()
                .data
                .starts_with(b"OpusHead")
        );
        assert!(
            reader
                .read_packet()
                .unwrap()
                .unwrap()
                .data
                .starts_with(b"OpusTags")
        );
        for index in 0..200 {
            let decoded = reader.read_packet().unwrap().unwrap();
            assert_eq!(decoded.data, packet);
            assert_eq!(decoded.last_in_stream(), index == 199);
        }
        assert!(reader.read_packet().unwrap().is_none());
    }
}
#[test]
fn pending_output_has_hard_limit_and_bad_packet_sizes_fail() {
    let mut mux = Ogg::new(20, 312).unwrap();
    assert_eq!(mux.packet(&[], 0, false), Err(AudioError::OutputLimit));
    assert_eq!(
        mux.packet(&[0; 1276], 0, false),
        Err(AudioError::OutputLimit)
    );
    let mut overflow = false;
    for index in 0..100 {
        if mux.packet(&[0; 1275], (index + 1) * 960, false).is_err() {
            overflow = true;
            break;
        }
        assert!(mux.output_bytes <= MAX_PENDING_BYTES);
    }
    assert!(overflow);
    assert!(mux.output_bytes <= MAX_PENDING_BYTES);
    let total: usize = std::iter::from_fn(|| mux.pop()).map(|p| p.len()).sum();
    assert!(total <= MAX_PENDING_BYTES);
    assert_eq!(mux.output_bytes, 0);
}
#[test]
fn sequence_overflow_is_an_error_not_a_panic() {
    let mut mux = Ogg::new(20, 312).unwrap();
    mux.sequence = u32::MAX;
    assert_eq!(
        mux.packet(&[1], 960, false),
        Err(AudioError::CounterOverflow)
    );
}
