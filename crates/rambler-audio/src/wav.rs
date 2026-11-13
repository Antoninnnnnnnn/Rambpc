use crate::{AudioError, MAX_INPUT_BYTES};
use bytes::Bytes;
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use std::{path::Path, time::Duration};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_width: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WavConfig {
    pub chunk_ms: u32,
    pub realtime: bool,
}
impl Default for WavConfig {
    fn default() -> Self {
        Self {
            chunk_ms: 40,
            realtime: false,
        }
    }
}
impl WavConfig {
    fn chunk_bytes(self) -> Result<usize, AudioError> {
        let bytes = u64::from(self.chunk_ms)
            .checked_mul(32)
            .ok_or(AudioError::InvalidConfig)?;
        if bytes == 0 || bytes > MAX_INPUT_BYTES as u64 {
            return Err(AudioError::InvalidConfig);
        }
        Ok(bytes as usize)
    }
}
/// Strict classic RIFF/WAVE PCM16LE mono/16k reader, including PCM extensible
/// headers. Unknown RIFF chunks are skipped by seeking, never buffered. One open
/// file descriptor is retained, so path replacement after open cannot swap audio.
/// Explicit close/EOF/errors await Tokio's in-flight file work before releasing
/// the descriptor. Dropped read/close futures retain progress and cleanup state.
pub struct WavAudio {
    file: Option<File>,
    closing: Option<SourceFuture<'static, ()>>,
    pub format: PcmFormat,
    pub frame_count: u64,
    pub duration: Duration,
    data_bytes: u64,
    config: WavConfig,
    chunk_bytes: usize,
    emitted_bytes: u64,
    pending: Vec<u8>,
    filled: usize,
    start: Option<Instant>,
    failure: Option<AudioError>,
}
impl WavAudio {
    pub async fn open(path: impl AsRef<Path>, config: WavConfig) -> Result<Self, AudioError> {
        let chunk_bytes = config.chunk_bytes()?;
        let file = File::open(path).await.map_err(|_| AudioError::Io)?;
        // Establish ownership before any async metadata/header work; Drop can
        // supervise cleanup even if this open future is cancelled mid-parse.
        let mut audio = Self {
            file: Some(file),
            closing: None,
            format: PcmFormat {
                sample_rate: 16000,
                channels: 1,
                sample_width: 2,
            },
            frame_count: 0,
            duration: Duration::ZERO,
            data_bytes: 0,
            config,
            chunk_bytes,
            emitted_bytes: 0,
            pending: Vec::new(),
            filled: 0,
            start: None,
            failure: None,
        };
        let parsed = match audio.file.as_mut() {
            Some(file) => parse_header(file).await,
            None => Err(AudioError::Io),
        };
        match parsed {
            Ok(size) => {
                audio.data_bytes = size;
                audio.frame_count = size / 2;
                let frames = audio.frame_count;
                audio.duration = Duration::from_secs(frames / 16000)
                    + Duration::from_nanos((frames % 16000) * 1_000_000_000 / 16000);
                Ok(audio)
            }
            Err(error) => {
                audio.close_file().await;
                Err(error)
            }
        }
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
    async fn step(&mut self) -> Result<Option<Bytes>, AudioError> {
        if self.file.is_none() {
            self.close_file().await;
            return Ok(None);
        }
        let start = *self.start.get_or_insert_with(Instant::now);
        // data_bytes is private validated metadata, independent of editable
        // descriptive public fields; callers cannot invalidate these counters.
        let remaining = self.data_bytes - self.emitted_bytes;
        if remaining == 0 {
            self.close_file().await;
            return Ok(None);
        }
        let wanted = remaining.min(self.chunk_bytes as u64) as usize;
        if self.pending.is_empty() {
            self.pending.resize(wanted, 0);
            self.filled = 0;
        }
        while self.filled < wanted {
            // read(), unlike read_exact(), is cancellation safe. Progress lives
            // in self so dropping next while reading/pacing never loses samples.
            let file = self.file.as_mut().ok_or(AudioError::Io)?;
            let count = file
                .read(&mut self.pending[self.filled..])
                .await
                .map_err(|_| AudioError::Io)?;
            if count == 0 {
                return Err(AudioError::TruncatedWav);
            }
            self.filled += count;
        }
        if self.config.realtime && self.emitted_bytes > 0 {
            let offset = Duration::from_nanos(self.emitted_bytes * 1_000_000_000 / 32000);
            let deadline = start.checked_add(offset).ok_or(AudioError::InvalidConfig)?;
            tokio::time::sleep_until(deadline).await;
        }
        self.emitted_bytes += wanted as u64;
        self.filled = 0;
        Ok(Some(std::mem::take(&mut self.pending).into()))
    }
}
impl AudioSource for WavAudio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if self.failure.is_none() {
                match self.step().await {
                    Ok(output) => return Ok(output),
                    Err(error) => {
                        self.failure = Some(error);
                        self.pending.clear();
                    }
                }
            }
            self.close_file().await;
            Err(SourceError::new(
                self.failure.take().unwrap_or(AudioError::Io),
            ))
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.pending.clear();
        self.filled = 0;
        self.failure = None;
        self.begin_close();
        Box::pin(async move {
            self.close_file().await;
            Ok(())
        })
    }
}
impl Drop for WavAudio {
    fn drop(&mut self) {
        let pending = self.closing.take().or_else(|| {
            self.file.take().map(|file| {
                Box::pin(async move {
                    drop(file.into_std().await);
                }) as SourceFuture<'static, ()>
            })
        });
        if let Some(pending) = pending
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(pending);
        }
    }
}
async fn parse_header(file: &mut File) -> Result<u64, AudioError> {
    let file_len = file.metadata().await.map_err(|_| AudioError::Io)?.len();
    let mut header = [0; 12];
    exact(file, &mut header).await?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Err(AudioError::InvalidWav);
    }
    let riff_end = 8 + u64::from(u32le(&header[4..8]));
    if riff_end < 12 {
        return Err(AudioError::InvalidWav);
    }
    if riff_end > file_len {
        return Err(AudioError::TruncatedWav);
    }
    let mut position = 12_u64;
    let mut has_format = false;
    // Cap metadata work independently of file length; large opaque chunks are
    // skipped in constant memory, endless tiny chunks are rejected.
    for _ in 0..100_000 {
        if position.checked_add(8).is_none_or(|p| p > riff_end) {
            return Err(AudioError::TruncatedWav);
        }
        let mut chunk = [0; 8];
        exact(file, &mut chunk).await?;
        let size = u64::from(u32le(&chunk[4..]));
        let end = (position + 8)
            .checked_add(size)
            .ok_or(AudioError::InvalidWav)?;
        let padded_end = end.checked_add(size % 2).ok_or(AudioError::InvalidWav)?;
        if padded_end > riff_end || end > file_len {
            return Err(AudioError::TruncatedWav);
        }
        match &chunk[..4] {
            b"fmt " => {
                if has_format || size < 16 {
                    return Err(AudioError::InvalidWav);
                }
                let mut fmt = [0; 40];
                let take = size.min(fmt.len() as u64) as usize;
                exact(file, &mut fmt[..take]).await?;
                validate_format(&fmt, size)?;
                has_format = true;
            }
            b"data" => {
                if !has_format || !size.is_multiple_of(2) {
                    return Err(AudioError::InvalidWav);
                }
                return Ok(size);
            }
            _ => (),
        }
        file.seek(std::io::SeekFrom::Start(padded_end))
            .await
            .map_err(|_| AudioError::Io)?;
        position = padded_end;
    }
    Err(AudioError::InvalidWav)
}
async fn exact(file: &mut File, out: &mut [u8]) -> Result<(), AudioError> {
    file.read_exact(out).await.map(|_| ()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            AudioError::TruncatedWav
        } else {
            AudioError::Io
        }
    })
}
fn u16le(data: &[u8]) -> u16 {
    u16::from_le_bytes([data[0], data[1]])
}
fn u32le(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}
fn validate_format(fmt: &[u8; 40], size: u64) -> Result<(), AudioError> {
    if u16le(&fmt[2..]) != 1
        || u32le(&fmt[4..]) != 16000
        || u32le(&fmt[8..]) != 32000
        || u16le(&fmt[12..]) != 2
        || u16le(&fmt[14..]) != 16
    {
        return Err(AudioError::InvalidWav);
    }
    match u16le(fmt) {
        1 => {
            if size == 17 || (size >= 18 && u64::from(u16le(&fmt[16..])) + 18 > size) {
                return Err(AudioError::InvalidWav);
            }
        }
        0xfffe => {
            const PCM_GUID: [u8; 16] = [
                1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
            ];
            if size < 40
                || u16le(&fmt[16..]) < 22
                || u64::from(u16le(&fmt[16..])) + 18 > size
                || u16le(&fmt[18..]) != 16
                || fmt[24..40] != PCM_GUID
            {
                return Err(AudioError::InvalidWav);
            }
        }
        _ => return Err(AudioError::InvalidWav),
    }
    Ok(())
}
