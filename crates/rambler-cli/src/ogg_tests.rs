use super::*;
use std::{io::Cursor, task::Poll};
#[path = "../tests/support/ogg_fixture.rs"]
mod ogg_fixture;

fn pages(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut offset = 0;
    let mut output = Vec::new();
    while offset < bytes.len() {
        let segments = usize::from(bytes[offset + 26]);
        let body: usize = bytes[offset + 27..offset + 27 + segments]
            .iter()
            .map(|&n| usize::from(n))
            .sum();
        let end = offset + 27 + segments + body;
        output.push(bytes[offset..end].to_vec());
        offset = end;
    }
    output
}
fn checksum(page: &mut [u8]) {
    let checksum = crc(page);
    page[22..26].copy_from_slice(&checksum.to_le_bytes());
}
fn page(flags: u8, sequence: u32, granule: u64, laces: &[u8], body: &[u8]) -> Vec<u8> {
    assert_eq!(
        laces.iter().map(|&n| usize::from(n)).sum::<usize>(),
        body.len()
    );
    let mut output = b"OggS\0".to_vec();
    output.push(flags);
    output.extend_from_slice(&granule.to_le_bytes());
    output.extend_from_slice(&0x12345678u32.to_le_bytes());
    output.extend_from_slice(&sequence.to_le_bytes());
    output.extend_from_slice(&[0; 4]);
    output.push(laces.len() as u8);
    output.extend_from_slice(laces);
    output.extend_from_slice(body);
    checksum(&mut output);
    output
}
async fn collect(bytes: &[u8]) -> Result<Vec<u8>, SourceError> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio été 音声.ogg");
    std::fs::write(&path, bytes).unwrap();
    let mut source = OggAudio::open(&path).await?;
    let mut output = Vec::new();
    while let Some(chunk) = source.next().await? {
        assert!(chunk.len() <= MAX_PAGE_BYTES);
        output.extend_from_slice(&chunk);
    }
    source.close().await?;
    // Windows refuses deletion while the async file still owns its descriptor.
    std::fs::remove_file(path).unwrap();
    Ok(output)
}
fn assert_decodable(bytes: &[u8], expected_packets: usize) {
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(bytes));
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
    let mut decoder = opus::Decoder::new(16_000, opus::Channels::Mono).unwrap();
    let mut packets = 0;
    while let Some(packet) = reader.read_packet().unwrap() {
        let mut samples = [0; 1920];
        assert!(decoder.decode(&packet.data, &mut samples, false).unwrap() > 0);
        packets += 1;
    }
    assert_eq!(packets, expected_packets);
}

// This exact minimal stream is also accepted and decoded by the independent
// PyAV/FFmpeg oracle: one 960-sample Opus silence packet, then an empty EOS.
fn nil_eos_pages() -> Vec<Vec<u8>> {
    let mut head = b"OpusHead\x01\x01".to_vec();
    head.extend_from_slice(&0_u16.to_le_bytes());
    head.extend_from_slice(&16_000_u32.to_le_bytes());
    head.extend_from_slice(&[0; 3]);
    vec![
        page(2, 0, 0, &[19], &head),
        page(0, 1, 0, &[16], b"OpusTags\0\0\0\0\0\0\0\0"),
        page(0, 2, 960, &[3], &[0xf8, 0xff, 0xfe]),
        page(4, 3, u64::MAX, &[], &[]),
    ]
}

#[tokio::test]
async fn nil_eos_preserves_bytes_and_requires_physical_eof() {
    let parts = nil_eos_pages();
    let bytes = parts.concat();
    assert_decodable(&bytes, 1);
    assert_eq!(collect(&bytes).await.unwrap(), bytes);
    let eos_start = bytes.len() - parts.last().unwrap().len();
    for end in eos_start..bytes.len() {
        assert!(collect(&bytes[..end]).await.is_err(), "truncation at {end}");
    }
    for suffix in [vec![0], bytes.clone()] {
        assert!(collect(&[bytes.clone(), suffix].concat()).await.is_err());
    }
}

#[tokio::test]
async fn nil_eos_requires_both_headers_audio_no_partial_packet_and_unknown_granule() {
    let parts = nil_eos_pages();
    // An empty EOS cannot replace Tags or the first audio packet.
    for prefix_len in [1, 2] {
        let bytes = [
            parts[..prefix_len].concat(),
            page(4, prefix_len as u32, u64::MAX, &[], &[]),
        ]
        .concat();
        assert!(collect(&bytes).await.is_err());
    }
    let partial_packet = [
        parts[..3].concat(),
        page(0, 3, u64::MAX, &[255], &[0; 255]),
        page(5, 4, u64::MAX, &[], &[]),
    ]
    .concat();
    assert!(collect(&partial_packet).await.is_err());
    for granule in [0, 959, 960, 961] {
        let bytes = [parts[..3].concat(), page(4, 3, granule, &[], &[])].concat();
        assert!(collect(&bytes).await.is_err());
    }
    let bad_continuation = [parts[..3].concat(), page(5, 3, u64::MAX, &[], &[])].concat();
    assert!(collect(&bad_continuation).await.is_err());
}

#[tokio::test]
async fn tags_must_start_on_the_second_page_not_after_an_empty_page() {
    let mut parts = pages(&ogg_fixture::fixture(2));
    for part in &mut parts[1..] {
        let sequence = le32(&part[18..22]) + 1;
        part[18..22].copy_from_slice(&sequence.to_le_bytes());
        checksum(part);
    }
    parts.insert(1, page(0, 1, u64::MAX, &[], &[]));
    assert!(collect(&parts.concat()).await.is_err());
}

#[tokio::test]
async fn completed_audio_granules_cannot_regress_on_normal_or_eos_pages() {
    let original = ogg_fixture::fixture(3);
    for (index, granule) in [(3, 959_u64), (4, 1919_u64)] {
        let mut parts = pages(&original);
        parts[index][6..14].copy_from_slice(&granule.to_le_bytes());
        checksum(&mut parts[index]);
        assert!(collect(&parts.concat()).await.is_err());
    }
}

fn padded_packet(original: &[u8]) -> Vec<u8> {
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(original));
    reader.read_packet().unwrap();
    reader.read_packet().unwrap();
    let packet = reader.read_packet().unwrap().unwrap().data;
    let mut padded = vec![0; 510];
    padded[..packet.len()].copy_from_slice(&packet);
    opus::packet::pad(&mut padded, packet.len()).unwrap();
    padded
}

#[tokio::test]
async fn equal_audio_granules_are_allowed_and_incomplete_pages_do_not_reset_them() {
    let original = ogg_fixture::fixture(2);
    let mut parts = pages(&original);
    parts[3][6..14].copy_from_slice(&960_u64.to_le_bytes());
    checksum(&mut parts[3]);
    let bytes = parts.concat();
    assert_eq!(collect(&bytes).await.unwrap(), bytes);

    let padded = padded_packet(&original);
    parts.truncate(3); // retain the first completed-audio page at granule 960
    parts.push(page(0, 3, u64::MAX, &[255], &padded[..255]));
    for granule in [959, 960, 1920] {
        let bytes = [
            parts.concat(),
            page(5, 4, granule, &[255, 0], &padded[255..]),
        ]
        .concat();
        if granule < 960 {
            assert!(collect(&bytes).await.is_err());
        } else {
            assert_decodable(&bytes, 2);
            assert_eq!(collect(&bytes).await.unwrap(), bytes);
        }
    }
}

#[tokio::test]
async fn first_completed_audio_page_at_eos_must_reach_preskip_even_after_continuation() {
    let original = ogg_fixture::fixture(1);
    let parts = pages(&original);
    let skip = u64::from(u16::from_le_bytes(parts[0][38..40].try_into().unwrap()));
    assert!(skip > 0);
    let padded = padded_packet(&original);
    for granule in [skip - 1, skip, skip + 1] {
        let mut direct = parts.clone();
        direct[2][6..14].copy_from_slice(&granule.to_le_bytes());
        checksum(&mut direct[2]);
        let continued = [
            parts[..2].concat(),
            page(0, 2, u64::MAX, &[255], &padded[..255]),
            page(5, 3, granule, &[255, 0], &padded[255..]),
        ]
        .concat();
        for bytes in [direct.concat(), continued] {
            if granule < skip {
                assert!(collect(&bytes).await.is_err());
            } else {
                assert_decodable(&bytes, 1);
                assert_eq!(collect(&bytes).await.unwrap(), bytes);
            }
        }
    }
}

#[tokio::test]
async fn preskip_may_span_multiple_non_eos_audio_pages() {
    let mut parts = pages(&ogg_fixture::fixture(3));
    parts[0][38..40].copy_from_slice(&2000_u16.to_le_bytes());
    checksum(&mut parts[0]);
    // First and second complete-audio granules (960, 1920) are below skip.
    // They are not EOS; only the third page (2880) finishes this stream.
    let bytes = parts.concat();
    assert_decodable(&bytes, 3);
    assert_eq!(collect(&bytes).await.unwrap(), bytes);
}

#[tokio::test]
async fn real_opus_oracle_unicode_and_exact_page_chunks() {
    let bytes = ogg_fixture::fixture(12);
    assert_decodable(&bytes, 12);
    assert_eq!(collect(&bytes).await.unwrap(), bytes);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audio.ogg");
    std::fs::write(&path, &bytes).unwrap();
    let mut source = OggAudio::open(path).await.unwrap();
    for expected in pages(&bytes) {
        assert_eq!(source.next().await.unwrap().unwrap().as_ref(), expected);
    }
    assert!(source.next().await.unwrap().is_none());
    assert!(source.next().await.unwrap().is_none());
}

#[tokio::test]
async fn continuation_and_zero_terminator_preserve_real_padded_packet() {
    let original = ogg_fixture::fixture(1);
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(&original));
    reader.read_packet().unwrap();
    reader.read_packet().unwrap();
    let packet = reader.read_packet().unwrap().unwrap().data;
    let mut padded = vec![0; 510];
    padded[..packet.len()].copy_from_slice(&packet);
    opus::packet::pad(&mut padded, packet.len()).unwrap();
    let mut parts = pages(&original);
    parts.truncate(2);
    parts.push(page(0, 2, u64::MAX, &[255], &padded[..255]));
    parts.push(page(5, 3, 960, &[255, 0], &padded[255..]));
    let bytes = parts.concat();
    assert_decodable(&bytes, 1);
    assert_eq!(collect(&bytes).await.unwrap(), bytes);
    let mut bad = parts.clone();
    bad[3][5] &= !1;
    checksum(&mut bad[3]);
    assert!(collect(&bad.concat()).await.is_err());
    let mut bad = parts;
    bad[2][5] |= 1;
    checksum(&mut bad[2]);
    assert!(collect(&bad.concat()).await.is_err());
}

#[tokio::test]
async fn real_padded_audio_packet_at_policy_bound_is_preserved_and_one_over_fails() {
    use ogg::writing::{PacketWriteEndInfo as End, PacketWriter};
    let original = ogg_fixture::fixture(1);
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(&original));
    let head = reader.read_packet().unwrap().unwrap().data;
    let tags = reader.read_packet().unwrap().unwrap().data;
    let packet = reader.read_packet().unwrap().unwrap().data;
    for size in [MAX_PACKET_BYTES, MAX_PACKET_BYTES + 1] {
        let mut padded = vec![0; size];
        padded[..packet.len()].copy_from_slice(&packet);
        opus::packet::pad(&mut padded, packet.len()).unwrap();
        let mut writer = PacketWriter::new(Vec::new());
        writer
            .write_packet(head.clone(), 0x12345678, End::EndPage, 0)
            .unwrap();
        writer
            .write_packet(tags.clone(), 0x12345678, End::EndPage, 0)
            .unwrap();
        writer
            .write_packet(padded, 0x12345678, End::EndStream, 960)
            .unwrap();
        let bytes = writer.into_inner();
        // Both are codec-valid; rejecting the latter is our documented bound,
        // not a claim that all valid Opus packets fit this application profile.
        assert_decodable(&bytes, 1);
        if size <= MAX_PACKET_BYTES {
            assert_eq!(collect(&bytes).await.unwrap(), bytes);
        } else {
            assert!(collect(&bytes).await.is_err());
        }
    }
}

#[tokio::test]
async fn bounded_multpage_tags_and_optional_padding_are_preserved() {
    use ogg::writing::{PacketWriteEndInfo as End, PacketWriter};
    let original = ogg_fixture::fixture(1);
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(&original));
    let head = reader.read_packet().unwrap().unwrap().data;
    reader.read_packet().unwrap();
    let packet = reader.read_packet().unwrap().unwrap().data;
    for size in [16, MAX_TAG_BYTES, MAX_TAG_BYTES + 1] {
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&0u32.to_le_bytes());
        tags.extend_from_slice(&0u32.to_le_bytes());
        tags.resize(size, 0x7e);
        let mut writer = PacketWriter::new(Vec::new());
        writer
            .write_packet(head.clone(), 0x12345678, End::EndPage, 0)
            .unwrap();
        writer
            .write_packet(tags, 0x12345678, End::EndPage, 0)
            .unwrap();
        writer
            .write_packet(packet.clone(), 0x12345678, End::EndStream, 960)
            .unwrap();
        let bytes = writer.into_inner();
        if size <= MAX_TAG_BYTES {
            assert_decodable(&bytes, 1);
            assert_eq!(collect(&bytes).await.unwrap(), bytes);
        } else {
            assert!(collect(&bytes).await.is_err());
        }
    }
}

#[tokio::test]
async fn crc_and_each_structural_header_corruption_are_errors() {
    let original = ogg_fixture::fixture(2);
    for (page_index, field, value) in [
        (0, 0, b'x'),
        (0, 4, 1),
        (0, 5, 0),
        (0, 5, 6),
        (0, 18, 1),
        (0, 27, 18),
        (0, 28, b'x'),
        (0, 36, 16),
        (0, 37, 2),
        (0, 40, 0),
        (0, 46, 1),
        (1, 5, 2),
        (1, 5, 1),
        (1, 5, 4),
        (1, 5, 8),
        (1, 14, 0),
        (1, 18, 0),
        (1, 28, b'x'),
        (2, 5, 1),
        (2, 18, 4),
    ] {
        let mut parts = pages(&original);
        assert_ne!(parts[page_index][field], value);
        parts[page_index][field] = value;
        checksum(&mut parts[page_index]);
        assert!(
            collect(&parts.concat()).await.is_err(),
            "page {page_index} field {field}"
        );
    }
    for index in [0, 22, 27, 28, original.len() - 1] {
        let mut bad = original.clone();
        bad[index] ^= 0x80;
        assert!(collect(&bad).await.is_err());
    }
    // A lacing table claiming more bytes cannot silently truncate at EOF.
    let mut parts = pages(&original);
    let last = parts.last_mut().unwrap();
    last[27] = 255;
    checksum(last);
    assert!(collect(&parts.concat()).await.is_err());
}

#[tokio::test]
async fn tags_lengths_counts_and_utf8_are_bounded_and_checked() {
    let original = ogg_fixture::fixture(1);
    for (offset, value) in [(8, u32::MAX), (12, u32::MAX), (12, 1)] {
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&[0; 8]);
        tags[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let mut parts = pages(&original);
        parts[1] = page(0, 1, 0, &[tags.len() as u8], &tags);
        assert!(collect(&parts.concat()).await.is_err());
    }
    let tags = b"OpusTags\x01\0\0\0\xff\0\0\0\0";
    let mut parts = pages(&original);
    parts[1] = page(0, 1, 0, &[tags.len() as u8], tags);
    assert!(collect(&parts.concat()).await.is_err());
    // Tags and audio sharing a page violate the supported Ogg Opus layout.
    let mut parts = pages(&original);
    let body = [b"OpusTags\0\0\0\0\0\0\0\0".as_slice(), &[1]].concat();
    parts[1] = page(0, 1, 0, &[16, 1], &body);
    assert!(collect(&parts.concat()).await.is_err());
}

#[tokio::test]
async fn every_byte_truncation_missing_eos_and_trailing_chaining_fail() {
    let bytes = ogg_fixture::fixture(2);
    for end in 0..bytes.len() {
        assert!(
            collect(&bytes[..end]).await.is_err(),
            "truncation at byte {end}"
        );
    }
    let mut parts = pages(&bytes);
    let last = parts.last_mut().unwrap();
    last[5] &= !4;
    checksum(last);
    assert!(collect(&parts.concat()).await.is_err());
    for suffix in [vec![0], bytes.clone()] {
        assert!(collect(&[bytes.clone(), suffix].concat()).await.is_err());
    }
    let mut multiplexed = pages(&bytes);
    multiplexed[2][14..18].copy_from_slice(&0x98765432u32.to_le_bytes());
    checksum(&mut multiplexed[2]);
    assert!(collect(&multiplexed.concat()).await.is_err());
}

#[tokio::test]
async fn endless_packet_continuation_hits_fixed_bound_without_accumulating_audio() {
    let original = ogg_fixture::fixture(1);
    for tags in [false, true] {
        let mut parts = pages(&original);
        parts.truncate(if tags { 1 } else { 2 });
        let sequence = parts.len() as u32;
        parts.push(page(0, sequence, u64::MAX, &[255; 255], &vec![0; 65_025]));
        parts.push(page(
            1,
            sequence + 1,
            u64::MAX,
            &[255; 255],
            &vec![0; 65_025],
        ));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.ogg");
        std::fs::write(&path, parts.concat()).unwrap();
        let mut source = OggAudio::open(path).await.unwrap();
        loop {
            assert!(source.tags.len() <= MAX_TAG_BYTES);
            assert!(source.tags.capacity() <= MAX_TAG_BYTES);
            assert!(source.page.len() <= MAX_PAGE_BYTES);
            assert!(source.packet_bytes <= MAX_PACKET_BYTES);
            if source.next().await.is_err() {
                break;
            }
        }
        assert!(source.file.is_none());
        assert!(source.next().await.is_err()); // sticky error, not EOF
    }
}

#[tokio::test]
async fn cancel_each_pending_read_resumes_without_lost_or_repeated_bytes() {
    let bytes = ogg_fixture::fixture(3);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cancel.ogg");
    std::fs::write(&path, &bytes).unwrap();
    let mut source = OggAudio::open(&path).await.unwrap();
    source.file.as_mut().unwrap().set_max_buf_size(1);
    let mut output = Vec::new();
    let mut cancelled = 0;
    loop {
        let polled = {
            let mut next = source.next();
            std::future::poll_fn(|cx| {
                Poll::Ready(match next.as_mut().poll(cx) {
                    Poll::Pending => None,
                    Poll::Ready(result) => Some(result),
                })
            })
            .await
        }; // drop the pending next future at every suspension
        match polled {
            None => {
                cancelled += 1;
                tokio::task::yield_now().await;
            }
            Some(Ok(Some(chunk))) => output.extend_from_slice(&chunk),
            Some(Ok(None)) => break,
            Some(Err(error)) => panic!("{error:?}"),
        }
    }
    assert!(cancelled > 0);
    assert_eq!(output, bytes);
    assert!(source.file.is_none());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn close_unpolled_or_cancelled_is_resumable_and_idempotent() {
    let bytes = ogg_fixture::fixture(1);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("close.ogg");
    std::fs::write(&path, bytes).unwrap();
    let mut source = OggAudio::open(&path).await.unwrap();
    source.next().await.unwrap();
    // Queue or complete a file operation and immediately abandon its future.
    {
        let mut next = source.next();
        std::future::poll_fn(|cx| {
            let _ = next.as_mut().poll(cx);
            Poll::Ready(())
        })
        .await;
    }
    drop(source.close()); // beginning cleanup must not depend on polling close
    assert!(source.file.is_none());
    assert!(source.closing.is_some());
    {
        let mut close = source.close();
        std::future::poll_fn(|cx| {
            let _ = close.as_mut().poll(cx);
            Poll::Ready(())
        })
        .await;
    }
    source.close().await.unwrap();
    source.close().await.unwrap();
    assert!(source.closing.is_none());
    assert!(source.next().await.unwrap().is_none());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn preflight_reads_only_first_page_then_late_crc_stays_source_error() {
    let mut parts = pages(&ogg_fixture::fixture(2));
    parts[2][28] ^= 1;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("late.ogg");
    std::fs::write(&path, parts.concat()).unwrap();
    let mut source = OggAudio::open(&path).await.unwrap();
    assert_eq!(source.headers, 1);
    assert_eq!(source.ready.as_ref().unwrap().as_ref(), parts[0]);
    assert!(source.tags.is_empty());
    assert_eq!(source.next().await.unwrap().unwrap().as_ref(), parts[0]);
    assert_eq!(source.next().await.unwrap().unwrap().as_ref(), parts[1]);
    assert!(source.next().await.is_err());
    assert!(source.next().await.is_err());
    std::fs::remove_file(path).unwrap();
}
