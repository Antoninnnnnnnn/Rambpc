use super::decode::{AudioError, check_duration, sample};
use ogg::{
    Packet,
    reading::{BasePacketReader, PageParser},
};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
};
use tokio_util::sync::CancellationToken;

pub(super) const OPUS_RATE: u32 = 48_000;
const MAX_PACKET: usize = 1024 * 1024;
const MAX_PAGE: u64 = 27 + 255 + 255 * 255;
const MAX_FRAME: usize = 5760; // 120 ms

struct Pages {
    file: BufReader<File>,
    packets: BasePacketReader,
    serial: u32,
    sequence: u32,
    pending: usize,
    eos: bool,
}

impl Pages {
    fn next(&mut self, stop: &CancellationToken) -> Result<Option<Packet>, AudioError> {
        loop {
            if stop.is_cancelled() {
                return Err(AudioError::Stopped);
            }
            if let Some(packet) = self.packets.read_packet() {
                return Ok(Some(packet));
            }
            if self.eos {
                // Do not silently discard additional logical streams.
                if self
                    .file
                    .fill_buf()
                    .map_err(|_| AudioError::Read)?
                    .is_empty()
                {
                    return Ok(None);
                }
                return Err(AudioError::Unsupported);
            }
            let mut header = [0; 27];
            self.file
                .read_exact(&mut header)
                .map_err(|_| AudioError::Invalid)?;
            if &header[..4] != b"OggS" {
                return Err(AudioError::Invalid);
            }
            let flags = header[5];
            let serial = u32::from_le_bytes(header[14..18].try_into().unwrap());
            let sequence = u32::from_le_bytes(header[18..22].try_into().unwrap());
            if self.sequence == 0 {
                self.serial = serial;
            }
            if serial != self.serial {
                return Err(AudioError::Unsupported);
            }
            if sequence != self.sequence
                || (flags & 2 != 0) != (sequence == 0)
                || (flags & 1 != 0) != (self.pending != 0)
            {
                return Err(AudioError::Invalid);
            }
            self.sequence += 1; // File size is bounded well below 2^32 pages.
            let (mut parser, n) = PageParser::new(header).map_err(|_| AudioError::Invalid)?;
            if n == 0 {
                return Err(AudioError::Invalid);
            }
            let mut lacing = vec![0; n];
            self.file
                .read_exact(&mut lacing)
                .map_err(|_| AudioError::Invalid)?;
            for &length in &lacing {
                self.pending += usize::from(length);
                if self.pending > MAX_PACKET {
                    return Err(AudioError::Invalid);
                }
                if length < 255 {
                    self.pending = 0;
                }
            }
            self.eos = flags & 4 != 0;
            if self.eos && self.pending != 0 {
                return Err(AudioError::Invalid);
            }
            let mut bytes = vec![0; parser.parse_segments(lacing)];
            self.file
                .read_exact(&mut bytes)
                .map_err(|_| AudioError::Invalid)?;
            let page = parser
                .parse_packet_data(bytes)
                .map_err(|_| AudioError::Invalid)?;
            self.packets
                .push_page(page)
                .map_err(|_| AudioError::Invalid)?;
        }
    }

    /// Find a complete checksummed EOS page with a bounded look behind EOF.
    /// This is a duration hint; the sequential pass still verifies every page.
    fn end_granule(&mut self, size: u64) -> Result<Option<u64>, AudioError> {
        let position = self.file.stream_position().map_err(|_| AudioError::Read)?;
        let len = size.min(MAX_PAGE) as usize;
        self.file
            .seek(SeekFrom::End(-(len as i64)))
            .map_err(|_| AudioError::Read)?;
        let mut tail = vec![0; len];
        self.file
            .read_exact(&mut tail)
            .map_err(|_| AudioError::Read)?;
        self.file
            .seek(SeekFrom::Start(position))
            .map_err(|_| AudioError::Read)?;
        for i in (0..len.saturating_sub(26)).rev() {
            let header: [u8; 27] = tail[i..i + 27].try_into().unwrap();
            if &header[..4] != b"OggS"
                || header[5] & 4 == 0
                || header[14..18] != self.serial.to_le_bytes()
            {
                continue;
            }
            let Ok((mut parser, n)) = PageParser::new(header) else {
                continue;
            };
            let Some(lacing) = tail.get(i + 27..i + 27 + n) else {
                continue;
            };
            if lacing.last().is_none_or(|&n| n == 255) {
                continue;
            }
            let body = &tail[i + 27 + n..];
            if body.len() == parser.parse_segments(lacing.to_vec())
                && parser.parse_packet_data(body.to_vec()).is_ok()
            {
                return Ok(Some(u64::from_le_bytes(header[6..14].try_into().unwrap())));
            }
        }
        Ok(None)
    }
}

pub(super) struct OpusFile {
    pages: Pages,
    decoder: opus::MSDecoder,
    channels: usize,
    skip: u64,
    gain: f32,
    decoded: u64,
    origin: Option<u64>,
    scratch: Vec<f32>,
    pub duration_ms: Option<u64>,
}

impl OpusFile {
    pub fn open(file: File, size: u64, stop: &CancellationToken) -> Result<Self, AudioError> {
        let mut pages = Pages {
            file: BufReader::new(file),
            packets: BasePacketReader::new(),
            serial: 0,
            sequence: 0,
            pending: 0,
            eos: false,
        };
        let head = pages.next(stop)?.ok_or(AudioError::Invalid)?;
        let header = &head.data;
        if !header.starts_with(b"OpusHead") {
            return Err(AudioError::Unsupported);
        }
        if header.len() < 19
            || !head.first_in_stream()
            || !head.last_in_page()
            || header[8] > 15
            || header[9] == 0
        {
            return Err(AudioError::Invalid);
        }
        let channels = usize::from(header[9]);
        let skip = u64::from(u16::from_le_bytes([header[10], header[11]]));
        let gain =
            10_f32.powf(f32::from(i16::from_le_bytes([header[16], header[17]])) / (256.0 * 20.0));
        let (streams, coupled, mapping) = match header[18] {
            0 if channels <= 2 => (1, (channels - 1) as u8, &[0, 1][..channels]),
            1 | 255 if header.len() >= 21 + channels => {
                (header[19], header[20], &header[21..21 + channels])
            }
            _ => return Err(AudioError::Unsupported),
        };
        let decoder = opus::MSDecoder::new(OPUS_RATE, streams, coupled, mapping)
            .map_err(|_| AudioError::Invalid)?;
        let tags = pages.next(stop)?.ok_or(AudioError::Invalid)?;
        if !tags.data.starts_with(b"OpusTags") || !tags.last_in_page() || tags.last_in_stream() {
            return Err(AudioError::Invalid);
        }
        let duration_ms = pages
            .end_granule(size)?
            .filter(|&n| n < (1 << 63) && n >= skip)
            .map(|n| (n - skip).saturating_mul(1000) / u64::from(OPUS_RATE));
        Ok(Self {
            pages,
            decoder,
            channels,
            skip,
            gain,
            decoded: 0,
            origin: None,
            scratch: vec![0.0; MAX_FRAME * channels],
            duration_ms,
        })
    }

    pub fn read(
        &mut self,
        mono: &mut Vec<f32>,
        stop: &CancellationToken,
    ) -> Result<bool, AudioError> {
        let Some(packet) = self.pages.next(stop)? else {
            return Ok(false);
        };
        if packet.data.is_empty() {
            return Err(AudioError::Invalid);
        }
        let n = self
            .decoder
            .decode_float(&packet.data, &mut self.scratch, false)
            .map_err(|_| AudioError::Invalid)?;
        let before = self.decoded;
        self.decoded += n as u64;
        let mut end = self.decoded;
        if packet.last_in_page() {
            let granule = packet.absgp_page();
            if granule >= (1 << 63) {
                return Err(AudioError::Invalid);
            }
            // A cropped logical stream may start at a non-zero granule offset.
            let origin = *self
                .origin
                .get_or_insert_with(|| granule.saturating_sub(self.decoded));
            end = granule.checked_sub(origin).ok_or(AudioError::Invalid)?;
            if end > self.decoded
                || (!packet.last_in_stream() && end != self.decoded)
                || end < before
            {
                return Err(AudioError::Invalid);
            }
        }
        check_duration(end.saturating_sub(self.skip), OPUS_RATE)?;
        let start = self.skip.max(before).min(end);
        let channels = self.channels;
        mono.extend(
            self.scratch[(start - before) as usize * channels..(end - before) as usize * channels]
                .chunks_exact(channels)
                .map(|frame| sample(frame.iter().sum::<f32>() * self.gain / channels as f32)),
        );
        Ok(true)
    }
}
