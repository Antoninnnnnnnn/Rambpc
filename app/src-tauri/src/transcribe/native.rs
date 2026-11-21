//! Native Windows dialog and local Media Foundation reader. All COM objects
//! are created, used and released on their own blocking worker thread.

use super::decode::AudioError;
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::{ERROR_CANCELLED, HWND},
        Media::MediaFoundation::*,
        System::Com::{
            StructuredStorage::{PropVariantClear, PropVariantToUInt64},
            *,
        },
        UI::Shell::{Common::COMDLG_FILTERSPEC, *},
    },
    core::{HSTRING, Interface, PCWSTR, w},
};

struct Apartment;
impl Apartment {
    fn open(mode: COINIT) -> windows::core::Result<Self> {
        // Both S_OK and S_FALSE require a matching CoUninitialize on this thread.
        unsafe {
            CoInitializeEx(None, mode).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub(super) fn pick(owner: isize) -> windows::core::Result<Option<PathBuf>> {
    let _apartment = Apartment::open(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)?;
    let filter = HSTRING::from(super::decode::file_filter());
    // SAFETY: the COM apartment and filter strings outlive the dialog; HWND is
    // the borrowed owner supplied by Tauri. GetDisplayName allocates with CoTaskMem.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(
            dialog.GetOptions()?
                | FOS_FORCEFILESYSTEM
                | FOS_FILEMUSTEXIST
                | FOS_PATHMUSTEXIST
                | FOS_DONTADDTORECENT,
        )?;
        dialog.SetTitle(w!("Transcrire un fichier audio"))?;
        dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
            pszName: w!("Fichiers audio"),
            pszSpec: PCWSTR(filter.as_ptr()),
        }])?;
        match dialog.Show(Some(HWND(owner as *mut _))) {
            Ok(()) => (),
            Err(error) if error.code() == ERROR_CANCELLED.to_hresult() => return Ok(None),
            Err(error) => return Err(error),
        }
        let result = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = OsString::from_wide(result.as_wide());
        CoTaskMemFree(Some(result.0.cast()));
        Ok(Some(PathBuf::from(path)))
    }
}

struct Foundation;
impl Foundation {
    fn start() -> windows::core::Result<Self> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        }
        Ok(Self)
    }
}
impl Drop for Foundation {
    fn drop(&mut self) {
        let _ = unsafe { MFShutdown() };
    }
}

const AUDIO_STREAM: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
const MAX_BUFFER_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct MediaFile {
    reader: IMFSourceReader,
    pub rate: u32,
    channels: usize,
    eof: bool,
    pub duration_ms: Option<u64>,
    // Field order releases the reader before MFShutdown and CoUninitialize.
    _foundation: Foundation,
    _apartment: Apartment,
}

impl MediaFile {
    pub fn open(path: &Path, extension: &str) -> Result<Self, AudioError> {
        Self::open_inner(path, extension).map_err(|_| AudioError::Unsupported)
    }

    fn open_inner(path: &Path, extension: &str) -> windows::core::Result<Self> {
        let apartment = Apartment::open(COINIT_MULTITHREADED)?;
        let foundation = Foundation::start()?;
        let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let hint = HSTRING::from(format!("audio.{extension}"));
        // SAFETY: the paths are NUL terminated, and the COM and MF guards outlive
        // the reader. Hand MF a read-only local IStream, not a resolvable URL.
        unsafe {
            let file = SHCreateStreamOnFileEx(
                PCWSTR(name.as_ptr()),
                (STGM_READ | STGM_SHARE_DENY_WRITE).0,
                0,
                false,
                None::<&IStream>,
            )?;
            let stream = MFCreateMFByteStreamOnStream(&file)?;
            stream
                .cast::<IMFAttributes>()?
                .SetString(&MF_BYTESTREAM_ORIGIN_NAME, &hint)?;
            let reader = MFCreateSourceReaderFromByteStream(&stream, None::<&IMFAttributes>)?;
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(AUDIO_STREAM, true)?;
            let desired = MFCreateMediaType()?;
            desired.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            desired.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            desired.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            reader.SetCurrentMediaType(AUDIO_STREAM, None, &desired)?;
            let (rate, channels) = Self::format(&reader)?;
            let duration_ms = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .ok()
                .and_then(|mut value| {
                    let duration = PropVariantToUInt64(&value).ok().map(|ticks| ticks / 10_000);
                    let _ = PropVariantClear(&mut value);
                    duration.filter(|&ms| ms != 0)
                });
            Ok(Self {
                reader,
                rate,
                channels,
                eof: false,
                duration_ms,
                _foundation: foundation,
                _apartment: apartment,
            })
        }
    }

    fn format(reader: &IMFSourceReader) -> windows::core::Result<(u32, usize)> {
        // SAFETY: all interfaces are owned and the calling COM thread is live.
        unsafe {
            let actual = reader.GetCurrentMediaType(AUDIO_STREAM)?;
            let rate = actual.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)?;
            let channels = actual.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)?;
            if rate == 0
                || channels == 0
                || channels > u32::from(u16::MAX)
                || actual.GetGUID(&MF_MT_SUBTYPE)? != MFAudioFormat_PCM
                || actual.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE)? != 16
            {
                return Err(windows::core::Error::from_hresult(
                    windows::Win32::Foundation::E_FAIL,
                ));
            }
            Ok((rate, channels as usize))
        }
    }

    pub fn read(&mut self, mono: &mut Vec<f32>) -> Result<bool, AudioError> {
        if self.eof {
            return Ok(false);
        }
        // SAFETY: these are synchronous MF calls on the creating COM thread.
        unsafe {
            let mut flags = 0;
            let mut sample = None;
            self.reader
                .ReadSample(
                    AUDIO_STREAM,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )
                .map_err(|_| AudioError::Read)?;
            if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                return Err(AudioError::Read);
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0
                && Self::format(&self.reader).ok() != Some((self.rate, self.channels))
            {
                return Err(AudioError::Invalid);
            }
            self.eof = flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0;
            let Some(sample) = sample else {
                return Ok(!self.eof);
            };
            if sample.GetTotalLength().map_err(|_| AudioError::Read)? as usize > MAX_BUFFER_BYTES {
                return Err(AudioError::Invalid);
            }
            let buffer = sample
                .ConvertToContiguousBuffer()
                .map_err(|_| AudioError::Read)?;
            let mut ptr = std::ptr::null_mut();
            let mut len = 0;
            buffer
                .Lock(&mut ptr, None, Some(&mut len))
                .map_err(|_| AudioError::Read)?;
            let lock = BufferLock(&buffer);
            let frame = self.channels * 2;
            if len as usize > MAX_BUFFER_BYTES
                || !(len as usize).is_multiple_of(frame)
                || (ptr.is_null() && len != 0)
            {
                return Err(AudioError::Invalid);
            }
            if len != 0 {
                // Lock guarantees `len` readable bytes for the life of this borrow.
                let bytes = std::slice::from_raw_parts(ptr, len as usize);
                let scale = 32768.0 * self.channels as f32;
                mono.extend(bytes.chunks_exact(frame).map(|frame| {
                    frame
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&pair| f32::from(i16::from_le_bytes(pair)))
                        .sum::<f32>()
                        / scale
                }));
            }
            drop(lock);
            Ok(true)
        }
    }
}

struct BufferLock<'a>(&'a IMFMediaBuffer);
impl Drop for BufferLock<'_> {
    fn drop(&mut self) {
        let _ = unsafe { self.0.Unlock() };
    }
}
