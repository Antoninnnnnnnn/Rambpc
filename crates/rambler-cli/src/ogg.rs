//! Bounded, byte-preserving Ogg framing validation for the CLI's mono/16 kHz
//! Opus profile. This is not an Opus decoder: compressed audio, duration, and
//! audible quality are deliberately not certified here.
use bytes::Bytes;
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use std::path::Path;
use tokio::{fs::File, io::AsyncReadExt};

const MAX_PAGE_BYTES: usize = 27 + 255 + 255 * 255;
// Explicit application policy: 64 KiB accommodates the 48 * 1275-byte
// unpadded single-stream Opus frame payload plus framing. Legal Opus padding
// (or metadata) beyond this limit is intentionally unsupported. Never trust
// untrusted lacing to terminate a packet. Tags have the same policy limit.
const MAX_PACKET_BYTES: usize = 65_536;
const MAX_TAG_BYTES: usize = 65_536;

#[derive(Clone, Copy)]
enum Phase {
    Header,
    Lacing,
    Body,
}

/// One validated Ogg page per chunk; the first OpusHead page is preflighted
/// before returning from open. Later corruption remains a source error, never
/// clean EOF. No seek, whole-file prebuffer, transcoding, or invented EOS.
/// Pending reads and into_std cleanup survive dropped next/close futures.
pub(crate) struct OggAudio {
    file: Option<File>,
    closing: Option<SourceFuture<'static, ()>>,
    page: Vec<u8>,
    filled: usize,
    phase: Phase,
    ready: Option<Bytes>,
    serial: Option<u32>,
    sequence: u32,
    packet_bytes: usize,
    tags: Vec<u8>,
    headers: u8,
    pre_skip: u16,
    last_audio_granule: Option<u64>,
    audio_seen: bool,
    eos: bool,
    eof_checked: bool,
    closed: bool,
    failure: Option<&'static str>,
}
impl OggAudio {
    pub(crate) async fn open(path: impl AsRef<Path>) -> Result<Self, SourceError> {
        let mut file = File::open(path).await.map_err(SourceError::new)?;
        // Also cap Tokio's internal blocking-read staging allocation.
        file.set_max_buf_size(MAX_PAGE_BYTES);
        let mut source = Self {
            file: Some(file),
            closing: None,
            page: vec![0; 27],
            filled: 0,
            phase: Phase::Header,
            ready: None,
            serial: None,
            sequence: 0,
            packet_bytes: 0,
            tags: Vec::new(),
            headers: 0,
            pre_skip: 0,
            last_audio_granule: None,
            audio_seen: false,
            eos: false,
            eof_checked: false,
            closed: false,
            failure: None,
        };
        if let Err(error) = source.prepare_page().await {
            source.close_file().await;
            return Err(invalid(error));
        }
        Ok(source)
    }

    fn begin_close(&mut self) {
        if self.closing.is_none()
            && let Some(file) = self.file.take()
        {
            self.closing = Some(Box::pin(async move {
                drop(file.into_std().await);
            }));
        }
    }
    async fn close_file(&mut self) {
        self.begin_close();
        if let Some(closing) = self.closing.as_mut() {
            closing.await;
        }
        self.closing = None;
    }

    async fn prepare_page(&mut self) -> Result<(), &'static str> {
        loop {
            while self.filled < self.page.len() {
                // read (not read_exact) is cancellation safe. Every successful
                // read's progress is recorded before the next suspension point.
                let count = self
                    .file
                    .as_mut()
                    .ok_or("closed Ogg source")?
                    .read(&mut self.page[self.filled..])
                    .await
                    .map_err(|_| "Ogg read failed")?;
                if count == 0 {
                    return Err("truncated Ogg stream or missing EOS");
                }
                self.filled += count;
            }
            match self.phase {
                Phase::Header => {
                    if &self.page[..4] != b"OggS" || self.page[4] != 0 {
                        return Err("invalid Ogg capture or version");
                    }
                    self.page.resize(27 + usize::from(self.page[26]), 0);
                    self.phase = Phase::Lacing;
                }
                Phase::Lacing => {
                    let body: usize = self.page[27..].iter().map(|&n| usize::from(n)).sum();
                    let size = self.page.len() + body;
                    debug_assert!(size <= MAX_PAGE_BYTES);
                    self.page.resize(size, 0);
                    self.phase = Phase::Body;
                }
                Phase::Body => break,
            }
        }
        self.validate_page()?;
        self.ready = Some(std::mem::take(&mut self.page).into());
        self.page = vec![0; 27];
        self.filled = 0;
        self.phase = Phase::Header;
        Ok(())
    }

    fn validate_page(&mut self) -> Result<(), &'static str> {
        let page = &self.page;
        if crc(page) != le32(&page[22..26]) {
            return Err("invalid Ogg CRC");
        }
        let flags = page[5];
        if flags & !7 != 0 {
            return Err("reserved Ogg flags");
        }
        let serial = le32(&page[14..18]);
        let sequence = le32(&page[18..22]);
        let laces = &page[27..27 + usize::from(page[26])];
        let body = &page[27 + laces.len()..];
        if self.serial.is_none() {
            // OpusHead must be the sole, complete packet of the BOS page.
            if flags != 2 || sequence != 0 || laces != [19] || page[6..14] != [0; 8] {
                return Err("invalid Opus BOS page");
            }
            validate_head(body)?;
            self.pre_skip = u16::from_le_bytes([body[10], body[11]]);
            self.serial = Some(serial);
            self.sequence = 1;
            self.headers = 1;
            return Ok(());
        }
        if self.serial != Some(serial) || flags & 2 != 0 {
            return Err("chained or multiplexed Ogg is unsupported");
        }
        if sequence != self.sequence {
            return Err("invalid Ogg page sequence");
        }
        self.sequence = self.sequence.wrapping_add(1);
        if (flags & 1 != 0) != (self.packet_bytes != 0) {
            return Err("invalid Ogg continuation flag");
        }
        let is_tags = self.headers == 1;
        // RFC 7845 section 3: Tags may span pages, but must start on page 2.
        if is_tags && self.packet_bytes == 0 && laces.is_empty() {
            return Err("OpusTags must begin on the second page");
        }
        let completed = laces.iter().any(|&lace| lace != 255);
        let granule = u64::from_le_bytes(page[6..14].try_into().expect("granule field"));
        if (!completed && granule != u64::MAX)
            || (completed && is_tags && granule != 0)
            || (completed && !is_tags && granule > i64::MAX as u64)
        {
            return Err("invalid Ogg granule position");
        }
        if completed && !is_tags {
            // RFC 7845 sections 4/4.4: an EOS trim may equal, but may not
            // precede, the previous completed-audio page's position.
            if self
                .last_audio_granule
                .is_some_and(|previous| granule < previous)
            {
                return Err("regressing Opus audio granule position");
            }
            // Section 4.5: this special lower bound applies only when the
            // first page completing audio is also EOS. Pre-skip may otherwise
            // span multiple pages; no codec duration is inferred here.
            if self.last_audio_granule.is_none()
                && flags & 4 != 0
                && granule < u64::from(self.pre_skip)
            {
                return Err("initial Opus EOS granule precedes pre-skip");
            }
            self.last_audio_granule = Some(granule);
        }
        if is_tags && self.tags.capacity() == 0 {
            self.tags.reserve_exact(MAX_TAG_BYTES);
        }
        let mut offset = 0;
        for (index, &lace) in laces.iter().enumerate() {
            let size = usize::from(lace);
            let bound = if is_tags {
                MAX_TAG_BYTES
            } else {
                MAX_PACKET_BYTES
            };
            self.packet_bytes = self
                .packet_bytes
                .checked_add(size)
                .ok_or("oversized Opus packet")?;
            if self.packet_bytes > bound {
                return Err("oversized Opus packet or tags");
            }
            if is_tags {
                // The single fixed-capacity header buffer is bounded even
                // when malicious lacing never terminates this packet.
                self.tags.extend_from_slice(&body[offset..offset + size]);
            }
            offset += size;
            if lace != 255 {
                if is_tags {
                    if index + 1 != laces.len() {
                        return Err("OpusTags must finish its page");
                    }
                    validate_tags(&self.tags)?;
                    self.tags = Vec::new();
                    self.headers = 2;
                } else {
                    if self.packet_bytes == 0 {
                        return Err("empty Opus audio packet");
                    }
                    self.audio_seen = true;
                }
                self.packet_bytes = 0;
            }
        }
        if flags & 4 != 0 {
            // RFC 3533 section 4 permits a nil EOS page. Its granule must be
            // -1 (checked above), and headers/audio must already be complete.
            if self.headers != 2 || !self.audio_seen || self.packet_bytes != 0 {
                return Err("invalid or incomplete Ogg EOS");
            }
            self.eos = true;
        }
        Ok(())
    }

    async fn step(&mut self) -> Result<Option<Bytes>, &'static str> {
        if self.closed {
            self.close_file().await;
            return Ok(None);
        }
        if self.ready.is_none() {
            if self.eof_checked {
                return Ok(None);
            }
            self.prepare_page().await?;
        }
        if self.eos && !self.eof_checked {
            // Do not report EOF (or send the final page) before checking for
            // appended bytes, chaining, or multiplexing. Retain ready across
            // cancellation during this read and during asynchronous cleanup.
            let mut extra = [0];
            if self
                .file
                .as_mut()
                .ok_or("closed Ogg source")?
                .read(&mut extra)
                .await
                .map_err(|_| "Ogg read failed")?
                != 0
            {
                return Err("bytes after Ogg EOS");
            }
            self.eof_checked = true;
        }
        if self.eos {
            self.close_file().await;
        }
        Ok(self.ready.take())
    }
}
impl AudioSource for OggAudio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if self.failure.is_none() {
                match self.step().await {
                    Ok(chunk) => return Ok(chunk),
                    Err(error) => {
                        self.failure = Some(error);
                        self.ready = None;
                        self.page.clear();
                        self.tags.clear();
                    }
                }
            }
            self.close_file().await;
            Err(invalid(self.failure.unwrap_or("invalid Ogg source")))
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.closed = true;
        self.ready = None;
        self.page.clear();
        self.tags.clear();
        self.failure = None;
        self.begin_close();
        Box::pin(async move {
            self.close_file().await;
            Ok(())
        })
    }
}
impl Drop for OggAudio {
    fn drop(&mut self) {
        let closing = self.closing.take().or_else(|| {
            self.file.take().map(|file| {
                Box::pin(async move {
                    drop(file.into_std().await);
                }) as SourceFuture<'static, ()>
            })
        });
        if let Some(closing) = closing
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(closing);
        }
    }
}
fn invalid(message: &'static str) -> SourceError {
    SourceError::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    ))
}
fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().expect("four-byte field"))
}
fn validate_head(head: &[u8]) -> Result<(), &'static str> {
    if head.len() != 19
        || &head[..8] != b"OpusHead"
        || head[8] != 1
        || head[9] != 1
        || le32(&head[12..16]) != 16_000
        || head[18] != 0
    {
        return Err("unsupported OpusHead (requires v1 mono 16 kHz mapping 0)");
    }
    Ok(())
}
fn validate_tags(tags: &[u8]) -> Result<(), &'static str> {
    if tags.len() < 16 || &tags[..8] != b"OpusTags" {
        return Err("invalid OpusTags");
    }
    fn field<'a>(data: &mut &'a [u8]) -> Result<&'a [u8], &'static str> {
        if data.len() < 4 {
            return Err("truncated OpusTags field");
        }
        let length = le32(data) as usize;
        *data = &data[4..];
        let value = data.get(..length).ok_or("oversized OpusTags field")?;
        *data = &data[length..];
        std::str::from_utf8(value).map_err(|_| "non-UTF8 OpusTags")?;
        Ok(value)
    }
    let mut remaining = &tags[8..];
    field(&mut remaining)?;
    if remaining.len() < 4 {
        return Err("truncated OpusTags comment count");
    }
    let count = le32(remaining) as usize;
    remaining = &remaining[4..];
    if count > remaining.len() / 4 {
        return Err("oversized OpusTags comment count");
    }
    for _ in 0..count {
        field(&mut remaining)?;
    }
    // RFC 7845 allows arbitrary padding after the comments. Preserve it.
    Ok(())
}
fn crc(page: &[u8]) -> u32 {
    let mut crc = 0_u32;
    for (index, &byte) in page.iter().enumerate() {
        let byte = if (22..26).contains(&index) { 0 } else { byte };
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = (crc << 1)
                ^ if crc & 0x8000_0000 != 0 {
                    0x04c1_1db7
                } else {
                    0
                };
        }
    }
    crc
}

#[cfg(test)]
#[path = "ogg_tests.rs"]
mod tests;
