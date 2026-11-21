use super::{
    opus_file::{OPUS_RATE, OpusFile},
    resample::Resampler,
    wav::Wav,
};
use bytes::Bytes;
use std::{fs::OpenOptions, path::Path};
use tokio_util::sync::CancellationToken;

pub(super) const RATE: u32 = 16_000;
const MAX_SECONDS: u64 = 2 * 60 * 60;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const BLOCK_SAMPLES: usize = 1600;
const WAV: &[&str] = &["wav", "wave"];
const OPUS: &[&str] = &["ogg", "opus", "oga"];
#[cfg(windows)]
const MEDIA: &[&str] = &["mp3", "m4a", "aac", "wma", "flac"];
#[cfg(not(windows))]
const MEDIA: &[&str] = &[];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum AudioError {
    #[error("Format non pris en charge : convertissez en WAV ou Ogg")]
    Unsupported,
    #[error("Le fichier audio est illisible ou incomplet.")]
    Invalid,
    #[error("Impossible de lire le fichier audio.")]
    Read,
    #[error("Le fichier audio dépasse la taille maximale de 2 Gio.")]
    Size,
    #[error("Le fichier audio dépasse la durée maximale de 2 heures.")]
    Duration,
    #[error("Le fichier ne contient aucun échantillon audio exploitable.")]
    Empty,
    #[error("La lecture du fichier a été interrompue.")]
    Stopped,
}

fn extension(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

pub(super) fn supported(path: &Path) -> bool {
    extension(path).is_some_and(|ext| {
        [WAV, OPUS, MEDIA]
            .iter()
            .any(|group| group.contains(&ext.as_str()))
    })
}

#[cfg(windows)]
pub(super) fn file_filter() -> String {
    [WAV, OPUS, MEDIA]
        .into_iter()
        .flatten()
        .map(|ext| format!("*.{ext}"))
        .collect::<Vec<_>>()
        .join(";")
}

pub(super) fn sample(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-1.0, 1.0)
    }
}

pub(super) fn check_duration(frames: u64, rate: u32) -> Result<(), AudioError> {
    if frames > MAX_SECONDS * u64::from(rate) {
        Err(AudioError::Duration)
    } else {
        Ok(())
    }
}

pub(super) fn samples_ms(frames: u64) -> u64 {
    frames * 1000 / u64::from(RATE)
}

enum Decoder {
    Wav(Wav),
    Opus(OpusFile),
    #[cfg(windows)]
    Media {
        decoder: super::native::MediaFile,
        _anchor: std::fs::File,
    },
}

impl Decoder {
    fn open(path: &Path, stop: &CancellationToken) -> Result<Self, AudioError> {
        let ext = extension(path).ok_or(AudioError::Unsupported)?;
        if !supported(path) {
            return Err(AudioError::Unsupported);
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            // Keep the selected file read-only and prevent write/delete during decoding,
            // including while the native source reader opens its second read handle.
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1); // FILE_SHARE_READ
        }
        let file = options.open(path).map_err(|_| AudioError::Read)?;
        let metadata = file.metadata().map_err(|_| AudioError::Read)?;
        if !metadata.is_file() {
            return Err(AudioError::Read);
        }
        let size = metadata.len();
        if size > MAX_BYTES {
            return Err(AudioError::Size);
        }
        if size == 0 {
            return Err(AudioError::Empty);
        }
        if stop.is_cancelled() {
            return Err(AudioError::Stopped);
        }
        if WAV.contains(&ext.as_str()) {
            return Ok(Self::Wav(Wav::open(file, size, stop)?));
        }
        if OPUS.contains(&ext.as_str()) {
            return Ok(Self::Opus(OpusFile::open(file, size, stop)?));
        }
        #[cfg(windows)]
        {
            Ok(Self::Media {
                decoder: super::native::MediaFile::open(path, &ext)?,
                _anchor: file,
            })
        }
        #[cfg(not(windows))]
        Err(AudioError::Unsupported)
    }

    fn rate(&self) -> u32 {
        match self {
            Self::Wav(wav) => wav.rate,
            Self::Opus(_) => OPUS_RATE,
            #[cfg(windows)]
            Self::Media { decoder, .. } => decoder.rate,
        }
    }

    fn duration_ms(&self) -> Option<u64> {
        match self {
            Self::Wav(wav) => Some(wav.duration_ms()),
            Self::Opus(opus) => opus.duration_ms,
            #[cfg(windows)]
            Self::Media { decoder, .. } => decoder.duration_ms,
        }
    }

    fn read(&mut self, mono: &mut Vec<f32>, stop: &CancellationToken) -> Result<bool, AudioError> {
        match self {
            Self::Wav(wav) => wav.read(mono),
            Self::Opus(opus) => opus.read(mono, stop),
            #[cfg(windows)]
            Self::Media { decoder, .. } => decoder.read(mono),
        }
    }
}

/// Runs entirely on a blocking worker. The callback backpressures as soon as
/// the SDK's single queue slot is occupied; no audio is spooled to disk.
pub(super) fn decode(
    path: &Path,
    stop: &CancellationToken,
    ready: impl FnOnce(Option<u64>) -> Result<(), AudioError>,
    mut send: impl FnMut(Bytes) -> Result<(), AudioError>,
) -> Result<u64, AudioError> {
    if stop.is_cancelled() {
        return Err(AudioError::Stopped);
    }
    let mut decoder = Decoder::open(path, stop)?;
    let rate = decoder.rate();
    let duration_ms = decoder.duration_ms();
    // WAV checks exact frames up front. The Ogg hint may include a nonzero
    // granule origin, so don't use it to reject a file before decoding.
    #[cfg(windows)]
    if matches!(&decoder, Decoder::Media { .. })
        && duration_ms.is_some_and(|ms| ms > MAX_SECONDS * 1000)
    {
        return Err(AudioError::Duration);
    }
    ready(duration_ms.filter(|&ms| ms <= MAX_SECONDS * 1000))?;
    let mut resampler = (rate != RATE).then(|| Resampler::new(rate));
    let mut mono = Vec::new();
    let mut frames = 0;
    let mut output = Vec::with_capacity(BLOCK_SAMPLES * 2);
    let mut emit = |s: f32| {
        output.extend_from_slice(&((s * 32768.0).round() as i16).to_le_bytes());
        if output.len() == BLOCK_SAMPLES * 2 {
            send(Bytes::copy_from_slice(&output))?;
            output.clear();
        }
        Ok(())
    };
    loop {
        if stop.is_cancelled() {
            return Err(AudioError::Stopped);
        }
        mono.clear();
        if !decoder.read(&mut mono, stop)? {
            break;
        }
        frames += mono.len() as u64;
        check_duration(frames, rate)?;
        for &sample in &mono {
            if let Some(resampler) = &mut resampler {
                resampler.push(sample, &mut emit)?;
            } else {
                emit(sample)?;
            }
        }
    }
    if frames * u64::from(RATE) < u64::from(rate) {
        return Err(AudioError::Empty);
    }
    if let Some(resampler) = &mut resampler {
        resampler.finish(emit)?;
    }
    if !output.is_empty() {
        send(output.into())?;
    }
    Ok(frames * 1000 / u64::from(rate))
}
