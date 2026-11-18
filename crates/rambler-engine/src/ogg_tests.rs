use super::*;
use std::{
    io::ErrorKind,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    task::Poll,
};

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir();
        let builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        loop {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("rambler-ogg-{}-{id}", std::process::id()));
            match builder.create(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create Ogg test directory: {error}"),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
    let dir = TestDir::new();
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

const SILENCE_PACKET: [u8; 3] = [0xf8, 0xff, 0xfe];

fn head_page(pre_skip: u16) -> Vec<u8> {
    let mut head = b"OpusHead\x01\x01".to_vec();
    head.extend_from_slice(&pre_skip.to_le_bytes());
    head.extend_from_slice(&16_000_u32.to_le_bytes());
    head.extend_from_slice(&[0; 3]);
    page(2, 0, 0, &[19], &head)
}

// Hand-authored fixtures keep the engine's tests codec-free. The CLI loopback
// independently muxes libopus packets and checks the actual uploaded bytes.
fn fixture(packet_count: u32) -> Vec<Vec<u8>> {
    assert!(packet_count > 0);
    let vendor = "Rambler offline fixture été".as_bytes();
    let mut tags = b"OpusTags".to_vec();
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0_u32.to_le_bytes());
    let mut pages = vec![head_page(312), page(0, 1, 0, &[tags.len() as u8], &tags)];
    for index in 0..packet_count {
        pages.push(page(
            if index + 1 == packet_count { 4 } else { 0 },
            index + 2,
            (u64::from(index) + 1) * 960,
            &[3],
            &SILENCE_PACKET,
        ));
    }
    pages
}

// Use opaque payloads for long-packet framing tests: the source does not
// validate their codec semantics or derive durations from them.
fn opaque_packet(size: usize) -> Vec<u8> {
    (0..size).map(|byte| byte as u8).collect()
}

// These tests need only two pages: a full 255-lace page and its terminator.
fn two_page_packet(seq: u32, eos: bool, granule: u64, packet: &[u8]) -> [Vec<u8>; 2] {
    let split = 255 * 255;
    let rest = &packet[split..];
    let mut laces = vec![255; rest.len() / 255];
    laces.push((rest.len() % 255) as u8);
    [
        page(0, seq, u64::MAX, &[255; 255], &packet[..split]),
        page(if eos { 5 } else { 1 }, seq + 1, granule, &laces, rest),
    ]
}

// This exact minimal stream is also accepted and decoded by the independent
// PyAV/FFmpeg oracle: one 960-sample Opus silence packet, then an empty EOS.
fn nil_eos_pages() -> Vec<Vec<u8>> {
    vec![
        head_page(0),
        page(0, 1, 0, &[16], b"OpusTags\0\0\0\0\0\0\0\0"),
        page(0, 2, 960, &[3], &SILENCE_PACKET),
        page(4, 3, u64::MAX, &[], &[]),
    ]
}

#[tokio::test]
async fn nil_eos_preserves_bytes_and_requires_physical_eof() {
    let parts = nil_eos_pages();
    let bytes = parts.concat();
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
    let mut parts = fixture(2);
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
    let original = fixture(3);
    for (index, granule) in [(3, 959_u64), (4, 1919_u64)] {
        let mut parts = original.clone();
        parts[index][6..14].copy_from_slice(&granule.to_le_bytes());
        checksum(&mut parts[index]);
        assert!(collect(&parts.concat()).await.is_err());
    }
}

#[tokio::test]
async fn equal_audio_granules_are_allowed_and_incomplete_pages_do_not_reset_them() {
    let mut parts = fixture(2);
    parts[3][6..14].copy_from_slice(&960_u64.to_le_bytes());
    checksum(&mut parts[3]);
    let bytes = parts.concat();
    assert_eq!(collect(&bytes).await.unwrap(), bytes);

    let padded = opaque_packet(510);
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
            assert_eq!(collect(&bytes).await.unwrap(), bytes);
        }
    }
}

#[tokio::test]
async fn first_completed_audio_page_at_eos_must_reach_preskip_even_after_continuation() {
    let parts = fixture(1);
    let skip = u64::from(u16::from_le_bytes(parts[0][38..40].try_into().unwrap()));
    assert!(skip > 0);
    let padded = opaque_packet(510);
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
                assert_eq!(collect(&bytes).await.unwrap(), bytes);
            }
        }
    }
}

#[tokio::test]
async fn preskip_may_span_multiple_non_eos_audio_pages() {
    let mut parts = fixture(3);
    parts[0][38..40].copy_from_slice(&2000_u16.to_le_bytes());
    checksum(&mut parts[0]);
    // First and second complete-audio granules (960, 1920) are below skip.
    // They are not EOS; only the third page (2880) finishes this stream.
    let bytes = parts.concat();
    assert_eq!(collect(&bytes).await.unwrap(), bytes);
}

#[tokio::test]
async fn unicode_path_and_exact_page_chunks() {
    let pages = fixture(12);
    let bytes = pages.concat();
    assert_eq!(collect(&bytes).await.unwrap(), bytes);
    let dir = TestDir::new();
    let path = dir.path().join("audio.ogg");
    std::fs::write(&path, &bytes).unwrap();
    let mut source = OggAudio::open(&path).await.unwrap();
    for expected in pages {
        assert_eq!(source.next().await.unwrap().unwrap().as_ref(), expected);
    }
    assert!(source.next().await.unwrap().is_none());
    assert!(source.next().await.unwrap().is_none());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn continuation_and_zero_terminator_preserve_packet_bytes() {
    let padded = opaque_packet(510);
    let mut parts = fixture(1);
    parts.truncate(2);
    parts.push(page(0, 2, u64::MAX, &[255], &padded[..255]));
    parts.push(page(5, 3, 960, &[255, 0], &padded[255..]));
    let bytes = parts.concat();
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
async fn packet_at_policy_bound_is_preserved_and_one_over_fails() {
    let original = fixture(1);
    for size in [MAX_PACKET_BYTES, MAX_PACKET_BYTES + 1] {
        let [first, last] = two_page_packet(2, true, 960, &opaque_packet(size));
        let bytes = [original[..2].concat(), first, last].concat();
        if size <= MAX_PACKET_BYTES {
            assert_eq!(collect(&bytes).await.unwrap(), bytes);
        } else {
            assert!(collect(&bytes).await.is_err());
        }
    }
}

#[tokio::test]
async fn bounded_multpage_tags_and_optional_padding_are_preserved() {
    for size in [16, MAX_TAG_BYTES, MAX_TAG_BYTES + 1] {
        let mut tags = b"OpusTags\0\0\0\0\0\0\0\0".to_vec();
        tags.resize(size, 0x7e);
        let mut parts = vec![head_page(312)];
        if size == 16 {
            parts.push(page(0, 1, 0, &[16], &tags));
        } else {
            parts.extend(two_page_packet(1, false, 0, &tags));
        }
        parts.push(page(4, parts.len() as u32, 960, &[3], &SILENCE_PACKET));
        let bytes = parts.concat();
        if size <= MAX_TAG_BYTES {
            assert_eq!(collect(&bytes).await.unwrap(), bytes);
        } else {
            assert!(collect(&bytes).await.is_err());
        }
    }
}

#[tokio::test]
async fn crc_and_each_structural_header_corruption_are_errors() {
    let original = fixture(2);
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
        let mut parts = original.clone();
        assert_ne!(parts[page_index][field], value);
        parts[page_index][field] = value;
        checksum(&mut parts[page_index]);
        assert!(
            collect(&parts.concat()).await.is_err(),
            "page {page_index} field {field}"
        );
    }
    let bytes = original.concat();
    for index in [0, 22, 27, 28, bytes.len() - 1] {
        let mut bad = bytes.clone();
        bad[index] ^= 0x80;
        assert!(collect(&bad).await.is_err());
    }
    // A lacing table claiming more bytes cannot silently truncate at EOF.
    let mut parts = original.clone();
    let last = parts.last_mut().unwrap();
    last[27] = 255;
    checksum(last);
    assert!(collect(&parts.concat()).await.is_err());
}

#[tokio::test]
async fn tags_lengths_counts_and_utf8_are_bounded_and_checked() {
    let original = fixture(1);
    for (offset, value) in [(8, u32::MAX), (12, u32::MAX), (12, 1)] {
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&[0; 8]);
        tags[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let mut parts = original.clone();
        parts[1] = page(0, 1, 0, &[tags.len() as u8], &tags);
        assert!(collect(&parts.concat()).await.is_err());
    }
    let tags = b"OpusTags\x01\0\0\0\xff\0\0\0\0";
    let mut parts = original.clone();
    parts[1] = page(0, 1, 0, &[tags.len() as u8], tags);
    assert!(collect(&parts.concat()).await.is_err());
    // Tags and audio sharing a page violate the supported Ogg Opus layout.
    let mut parts = original.clone();
    let body = [b"OpusTags\0\0\0\0\0\0\0\0".as_slice(), &[1]].concat();
    parts[1] = page(0, 1, 0, &[16, 1], &body);
    assert!(collect(&parts.concat()).await.is_err());
}

#[tokio::test]
async fn every_byte_truncation_missing_eos_and_trailing_chaining_fail() {
    let original = fixture(2);
    let bytes = original.concat();
    for end in 0..bytes.len() {
        assert!(
            collect(&bytes[..end]).await.is_err(),
            "truncation at byte {end}"
        );
    }
    let mut parts = original.clone();
    let last = parts.last_mut().unwrap();
    last[5] &= !4;
    checksum(last);
    assert!(collect(&parts.concat()).await.is_err());
    for suffix in [vec![0], bytes.clone()] {
        assert!(collect(&[bytes.clone(), suffix].concat()).await.is_err());
    }
    let mut multiplexed = original;
    multiplexed[2][14..18].copy_from_slice(&0x98765432u32.to_le_bytes());
    checksum(&mut multiplexed[2]);
    assert!(collect(&multiplexed.concat()).await.is_err());
}

#[tokio::test]
async fn endless_packet_continuation_hits_fixed_bound_without_accumulating_audio() {
    let original = fixture(1);
    for tags in [false, true] {
        let mut parts = original.clone();
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
        let dir = TestDir::new();
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
    let bytes = fixture(3).concat();
    let dir = TestDir::new();
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
    let bytes = fixture(1).concat();
    let dir = TestDir::new();
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

fn assert_invalid(error: SourceError, message: &str) {
    let io = error.original().downcast_ref::<std::io::Error>().unwrap();
    assert_eq!(io.kind(), ErrorKind::InvalidData);
    assert_eq!(io.to_string(), message);
    assert_eq!(error.to_string(), "request source failed");
}

#[tokio::test]
async fn appended_bytes_withhold_eos_and_close_the_file() {
    let parts = nil_eos_pages();
    let dir = TestDir::new();
    let path = dir.path().join("trailing.ogg");
    std::fs::write(&path, [parts.concat(), vec![0]].concat()).unwrap();
    let mut source = OggAudio::open(&path).await.unwrap();
    for expected in &parts[..parts.len() - 1] {
        assert_eq!(source.next().await.unwrap().unwrap().as_ref(), expected);
    }
    for _ in 0..2 {
        assert_invalid(source.next().await.unwrap_err(), "bytes after Ogg EOS");
    }
    assert!(source.file.is_none());
    std::fs::remove_file(path).unwrap();
    source.close().await.unwrap();
    assert!(source.next().await.unwrap().is_none());
}

#[tokio::test]
async fn preflight_reads_only_first_page_then_late_crc_stays_source_error() {
    let mut parts = fixture(2);
    parts[2][28] ^= 1;
    let dir = TestDir::new();
    let path = dir.path().join("late.ogg");
    std::fs::write(&path, parts.concat()).unwrap();
    let mut source = OggAudio::open(&path).await.unwrap();
    assert_eq!(source.headers, 1);
    assert_eq!(source.ready.as_ref().unwrap().as_ref(), parts[0]);
    assert!(source.tags.is_empty());
    assert_eq!(source.next().await.unwrap().unwrap().as_ref(), parts[0]);
    assert_eq!(source.next().await.unwrap().unwrap().as_ref(), parts[1]);
    for _ in 0..2 {
        assert_invalid(source.next().await.unwrap_err(), "invalid Ogg CRC");
    }
    std::fs::remove_file(path).unwrap();
}
