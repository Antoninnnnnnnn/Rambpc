use crate::{AudioError, MAX_INPUT_BYTES};
use bytes::Bytes;
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use std::{path::Path, time::Duration};
use tokio::{
    fs::{File, OpenOptions},
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
    fn chunk_bytes(self, sample_rate: u32) -> Result<usize, AudioError> {
        let bytes = u64::from(self.chunk_ms) * u64::from(sample_rate / 1000) * 2;
        if bytes == 0 || bytes > MAX_INPUT_BYTES as u64 {
            return Err(AudioError::InvalidConfig);
        }
        Ok(bytes as usize)
    }
}
/// Strict classic RIFF/WAVE PCM16LE mono reader, including PCM extensible
/// headers. Reads the exact PCM bytes at the validated file rate.
/// Unknown RIFF chunks are skipped by seeking, never buffered. One open
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
    bytes_per_second: u64,
    emitted_bytes: u64,
    pending: Vec<u8>,
    filled: usize,
    start: Option<Instant>,
    failure: Option<AudioError>,
}
impl WavAudio {
    /// Open a 16 kHz mono PCM16LE WAV.
    pub async fn open(path: impl AsRef<Path>, config: WavConfig) -> Result<Self, AudioError> {
        Self::open_with_rates(path.as_ref(), config, false).await
    }

    /// Also accept 48 kHz mono PCM16LE. Chunk durations and realtime pacing
    /// use the original rate; `next` still emits the file's exact PCM bytes.
    /// Use [`crate::Pcm48kTo16k`] before feeding 48 kHz audio to the 16 kHz encoder.
    pub async fn open_16_or_48k(
        path: impl AsRef<Path>,
        config: WavConfig,
    ) -> Result<Self, AudioError> {
        Self::open_with_rates(path.as_ref(), config, true).await
    }

    async fn open_with_rates(
        path: &Path,
        config: WavConfig,
        allow_48k: bool,
    ) -> Result<Self, AudioError> {
        // Reject invalid configuration before opening the file when possible.
        let chunk_bytes = config.chunk_bytes(16_000)?;
        let mut options = OpenOptions::new();
        options.read(true);
        // A FIFO must not block open before its descriptor can be rejected.
        // O_NONBLOCK has no effect on ordinary file reads and avoids a
        // path-metadata/open race when a path is replaced with a FIFO.
        #[cfg(unix)]
        options.custom_flags(libc::O_NONBLOCK);
        let file = options.open(path).await.map_err(|_| AudioError::Io)?;
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
            bytes_per_second: 32_000,
            emitted_bytes: 0,
            pending: Vec::new(),
            filled: 0,
            start: None,
            failure: None,
        };
        let parsed = match audio.file.as_mut() {
            Some(file) => parse_header(file, allow_48k).await,
            None => Err(AudioError::Io),
        };
        match parsed.and_then(|(size, rate)| Ok((size, rate, config.chunk_bytes(rate)?))) {
            Ok((size, sample_rate, chunk_bytes)) => {
                audio.format.sample_rate = sample_rate;
                let rate = u64::from(sample_rate);
                audio.chunk_bytes = chunk_bytes;
                audio.bytes_per_second = rate * 2;
                audio.data_bytes = size;
                audio.frame_count = size / 2;
                audio.duration = Duration::from_nanos(audio.frame_count * 1_000_000_000 / rate);
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
            let offset =
                Duration::from_nanos(self.emitted_bytes * 1_000_000_000 / self.bytes_per_second);
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
            // Cleanup releases the descriptor, but only explicit close clears
            // the primary failure; later reads must not resemble healthy EOF.
            Err(SourceError::new(
                self.failure.clone().unwrap_or(AudioError::Io),
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
#[cfg(windows)]
fn is_disk_file(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_DISK, GetFileType};
    // SAFETY: file owns this live handle throughout the query. GetFileType
    // neither closes it nor retains the handle or any borrowed memory.
    unsafe { GetFileType(file.as_raw_handle()) == FILE_TYPE_DISK }
}
async fn parse_header(file: &mut File, allow_48k: bool) -> Result<(u64, u32), AudioError> {
    // Windows metadata alone can classify a pipe as a file. Query the opened
    // handle before any metadata/header operation that could wait for pipe data.
    #[cfg(windows)]
    if !is_disk_file(file) {
        return Err(AudioError::InvalidWav);
    }
    let metadata = file.metadata().await.map_err(|_| AudioError::Io)?;
    if !metadata.is_file() {
        return Err(AudioError::InvalidWav);
    }
    let file_len = metadata.len();
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
    let mut sample_rate = None;
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
                if sample_rate.is_some() || size < 16 {
                    return Err(AudioError::InvalidWav);
                }
                let mut fmt = [0; 40];
                let take = size.min(fmt.len() as u64) as usize;
                exact(file, &mut fmt[..take]).await?;
                sample_rate = Some(validate_format(&fmt, size, allow_48k)?);
            }
            b"data" => {
                if !size.is_multiple_of(2) {
                    return Err(AudioError::InvalidWav);
                }
                return Ok((size, sample_rate.ok_or(AudioError::InvalidWav)?));
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
fn validate_format(fmt: &[u8; 40], size: u64, allow_48k: bool) -> Result<u32, AudioError> {
    let rate = u32le(&fmt[4..]);
    if !(rate == 16_000 || allow_48k && rate == 48_000)
        || u16le(&fmt[2..]) != 1
        || u32le(&fmt[8..]) != rate * 2
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
    Ok(rate)
}
