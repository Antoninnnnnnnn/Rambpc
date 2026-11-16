#![allow(dead_code)]
use bytes::Bytes;
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::Notify;

#[derive(Default)]
pub struct Stats {
    pub reads: AtomicUsize,
    pub closes: AtomicUsize,
    pub eof: AtomicBool,
}
pub struct Source {
    pub chunks: VecDeque<Bytes>,
    pub stats: Arc<Stats>,
    pub fail_after: Option<usize>,
    pub close_fails: bool,
    pub block_after: Option<usize>,
    pub read_entered: Arc<Notify>,
    pub read_gate: Arc<Notify>,
    pub close_entered: Arc<Notify>,
    pub close_gate: Option<Arc<Notify>>,
}
impl Source {
    pub fn new(chunks: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self {
            chunks: chunks.into_iter().map(Bytes::from).collect(),
            stats: Arc::new(Stats::default()),
            fail_after: None,
            close_fails: false,
            block_after: None,
            read_entered: Arc::new(Notify::new()),
            read_gate: Arc::new(Notify::new()),
            close_entered: Arc::new(Notify::new()),
            close_gate: None,
        }
    }
}
impl AudioSource for Source {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            let reads = self.stats.reads.load(Ordering::SeqCst);
            if self.block_after == Some(reads) {
                self.read_entered.notify_one();
                self.read_gate.notified().await;
            }
            if self.fail_after == Some(reads) {
                return Err(SourceError::new(std::io::Error::other("input failure")));
            }
            if let Some(chunk) = self.chunks.pop_front() {
                self.stats.reads.fetch_add(1, Ordering::SeqCst);
                Ok(Some(chunk))
            } else {
                self.stats.eof.store(true, Ordering::SeqCst);
                Ok(None)
            }
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.stats.closes.fetch_add(1, Ordering::SeqCst);
            self.close_entered.notify_one();
            if let Some(gate) = &self.close_gate {
                gate.notified().await;
            }
            if self.close_fails {
                Err(SourceError::new(std::io::Error::other("close failure")))
            } else {
                Ok(())
            }
        })
    }
}
pub fn pcm(samples: usize) -> Vec<u8> {
    (0..samples)
        .flat_map(|i| {
            let sample = (12000.0 * (2.0 * std::f64::consts::PI * 437.0 * i as f64 / 16000.0).cos())
                .round() as i16;
            sample.to_le_bytes()
        })
        .collect()
}
pub fn fragment(bytes: &[u8], sizes: &[usize]) -> Vec<Vec<u8>> {
    let mut out = vec![];
    let mut offset = 0;
    for size in sizes.iter().cycle() {
        if offset == bytes.len() {
            break;
        }
        let end = (offset + size).min(bytes.len());
        out.push(bytes[offset..end].to_vec());
        offset = end;
    }
    out
}
pub async fn collect(source: &mut impl AudioSource) -> Result<Vec<u8>, SourceError> {
    let mut out = vec![];
    while let Some(chunk) = source.next().await? {
        out.extend(chunk);
    }
    Ok(out)
}
#[derive(Debug)]
pub struct Page {
    pub flags: u8,
    pub granule: u64,
    pub serial: u32,
    pub sequence: u32,
    pub lacing: Vec<u8>,
    pub body: Vec<u8>,
}
pub fn pages(data: &[u8]) -> Vec<Page> {
    let mut pages = vec![];
    let mut offset = 0;
    while offset < data.len() {
        assert_eq!(&data[offset..offset + 4], b"OggS");
        assert_eq!(data[offset + 4], 0);
        let n = data[offset + 26] as usize;
        let header_end = offset + 27 + n;
        let lacing = data[offset + 27..header_end].to_vec();
        let end = header_end + lacing.iter().map(|b| *b as usize).sum::<usize>();
        let u32at = |i| u32::from_le_bytes(data[offset + i..offset + i + 4].try_into().unwrap());
        pages.push(Page {
            flags: data[offset + 5],
            granule: u64::from_le_bytes(data[offset + 6..offset + 14].try_into().unwrap()),
            serial: u32at(14),
            sequence: u32at(18),
            lacing,
            body: data[header_end..end].to_vec(),
        });
        offset = end;
    }
    pages
}
pub fn packets(data: &[u8]) -> Vec<Vec<u8>> {
    // Independent established Ogg reader validates page framing and CRC.
    let mut reader = ogg::reading::PacketReader::new(std::io::Cursor::new(data));
    let mut out = vec![];
    while let Some(packet) = reader.read_packet().unwrap() {
        out.push(packet.data);
    }
    out
}
pub fn assert_duration(data: &[u8], samples: usize) {
    let p = pages(data);
    let packets = packets(data);
    assert!(p.len() >= 3);
    assert_eq!(p[0].flags, 2);
    assert_eq!(
        p.iter().map(|p| p.sequence).collect::<Vec<_>>(),
        (0..p.len() as u32).collect::<Vec<_>>()
    );
    assert!(p.iter().all(|page| page.serial == p[0].serial));
    assert!(p[..p.len() - 1].iter().all(|p| p.flags & 4 == 0));
    assert_eq!(p.last().unwrap().flags, 4);
    assert!(packets[0].starts_with(b"OpusHead"));
    assert_eq!(packets[0].len(), 19);
    assert_eq!(&packets[0][8..10], &[1, 1]);
    assert_eq!(
        u32::from_le_bytes(packets[0][12..16].try_into().unwrap()),
        16000
    );
    assert_eq!(&packets[0][16..], &[0, 0, 0]);
    assert!(packets[1].starts_with(b"OpusTags"));
    let skip = u16::from_le_bytes(packets[0][10..12].try_into().unwrap()) as u64;
    assert!(skip > 0);
    assert_eq!(p.last().unwrap().granule - skip, samples as u64 * 3);
    for pair in p[2..].windows(2) {
        assert!(pair[0].granule <= pair[1].granule);
    }
    // Packet decoding is separate from our encoder/mux. At 48kHz each packet
    // decodes 960 samples; verify enough real decoded audio spans skip+tail.
    let mut decoder = opus::Decoder::new(48000, opus::Channels::Mono).unwrap();
    let mut decoded = vec![];
    for packet in &packets[2..] {
        let mut frame = [0_i16; 960];
        let n = decoder.decode(packet, &mut frame, false).unwrap();
        assert_eq!(n, 960);
        decoded.extend_from_slice(&frame[..n]);
    }
    let end = p.last().unwrap().granule as usize;
    assert!(decoded.len() >= end);
    assert!(decoded.len() - end < 960);
    let actual = &decoded[skip as usize..end];
    assert_eq!(actual.len(), samples * 3);
    if samples >= 80 {
        assert!(actual.iter().any(|s| *s != 0));
    }
}
pub fn wav(data: &[u8]) -> Vec<u8> {
    let mut out = b"RIFF".to_vec();
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(16000_u32.to_le_bytes());
    out.extend(32000_u32.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}
