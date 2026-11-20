use crate::ports::{Clipboard, ClipboardError};
use std::{
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{ERROR_SUCCESS, GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, SetLastError},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
            GetClipboardOwner, GetClipboardSequenceNumber, GetOpenClipboardWindow, OpenClipboard,
            RegisterClipboardFormatW, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        Ole::{
            CF_BITMAP, CF_DSPBITMAP, CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT, CF_ENHMETAFILE,
            CF_METAFILEPICT, CF_OWNERDISPLAY, CF_PALETTE, CF_PRIVATEFIRST, CF_PRIVATELAST,
            CF_UNICODETEXT,
        },
    },
    UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP},
};
use windows::core::{PCWSTR, w};
use zeroize::{Zeroize, Zeroizing};

#[derive(Default)]
pub struct WinClipboard;

impl WinClipboard {
    pub fn new() -> Self {
        Self
    }
}

struct Owner(HWND);
impl Owner {
    fn new() -> Result<Self, ClipboardError> {
        // Opening with a null HWND leaves EmptyClipboard without an owner,
        // in which case SetClipboardData cannot transfer the memory.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
        }
        .map(Self)
        .map_err(|_| ClipboardError::Other)
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

struct Opening;
impl Opening {
    fn new(owner: &Owner) -> Result<Self, ClipboardError> {
        for attempt in 0..8 {
            if unsafe { OpenClipboard(Some(owner.0)) }.is_ok() {
                return Ok(Self);
            }
            if attempt < 7 {
                thread::sleep(Duration::from_millis(10));
            }
        }
        Err(ClipboardError::Busy)
    }

    fn close(self) -> Result<(), ClipboardError> {
        unsafe { CloseClipboard() }.map_err(|_| ClipboardError::Other)?;
        std::mem::forget(self);
        Ok(())
    }
}
impl Drop for Opening {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

struct GlobalBlock {
    handle: Option<HGLOBAL>,
    len: usize,
    plaintext: bool,
}

impl GlobalBlock {
    fn new<T: Copy>(data: &[T], plaintext: bool) -> Result<Self, ClipboardError> {
        let len = std::mem::size_of_val(data);
        let handle =
            unsafe { GlobalAlloc(GMEM_MOVEABLE, len) }.map_err(|_| ClipboardError::Other)?;
        let block = Self {
            handle: Some(handle),
            len,
            plaintext,
        };
        unsafe {
            let dest = GlobalLock(handle);
            if dest.is_null() {
                return Err(ClipboardError::Other);
            }
            // GlobalAlloc returns suitably aligned memory; copy only initialized bytes.
            dest.cast::<T>()
                .copy_from_nonoverlapping(data.as_ptr(), data.len());
            SetLastError(ERROR_SUCCESS);
            // False with ERROR_SUCCESS means the lock count reached zero normally.
            if GlobalUnlock(handle).is_err() && GetLastError() != ERROR_SUCCESS {
                return Err(ClipboardError::Other);
            }
        }
        Ok(block)
    }

    fn transfer(&mut self, format: u32) -> Result<(), ClipboardError> {
        let handle = self.handle.as_ref().ok_or(ClipboardError::Other)?;
        unsafe { SetClipboardData(format, Some(HANDLE(handle.0))) }
            .map_err(|_| ClipboardError::Other)?;
        self.handle = None; // Windows owns the allocation after success.
        Ok(())
    }
}

impl Drop for GlobalBlock {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            unsafe {
                if self.plaintext {
                    let ptr = GlobalLock(handle).cast::<u8>();
                    if !ptr.is_null() {
                        std::slice::from_raw_parts_mut(ptr, self.len).zeroize();
                        let _ = GlobalUnlock(handle);
                    }
                }
                let _ = GlobalFree(Some(handle));
            }
        }
    }
}

struct PrivateText {
    privacy: [(u32, GlobalBlock); 3],
    text: GlobalBlock,
}

impl PrivateText {
    fn new(terminated_utf16: &[u16]) -> Result<Self, ClipboardError> {
        fn exclusion(name: PCWSTR) -> Result<(u32, GlobalBlock), ClipboardError> {
            let format = unsafe { RegisterClipboardFormatW(name) };
            if format == 0 {
                return Err(ClipboardError::ExclusionFailed);
            }
            Ok((format, GlobalBlock::new(&[0u32], false)?))
        }

        Ok(Self {
            privacy: [
                exclusion(w!("ExcludeClipboardContentFromMonitorProcessing"))?,
                exclusion(w!("CanIncludeInClipboardHistory"))?,
                exclusion(w!("CanUploadToCloudClipboard"))?,
            ],
            text: GlobalBlock::new(terminated_utf16, true)?,
        })
    }

    fn publish(&mut self, _opening: &Opening) -> Result<(), ClipboardError> {
        for (format, block) in &mut self.privacy {
            block
                .transfer(*format)
                .map_err(|_| ClipboardError::ExclusionFailed)?;
        }
        self.text.transfer(u32::from(CF_UNICODETEXT.0))
    }
}

impl Clipboard for WinClipboard {
    fn copy_private(&self, text: &str) -> Result<(), ClipboardError> {
        if text.contains('\0') {
            return Err(ClipboardError::Other);
        }
        let utf16 = Zeroizing::new(text.encode_utf16().chain([0]).collect::<Vec<_>>());
        let mut contents = PrivateText::new(&utf16)?;
        let owner = Owner::new()?;
        let opening = Opening::new(&owner)?;
        unsafe { EmptyClipboard() }.map_err(|_| ClipboardError::Other)?;
        contents.publish(&opening)?;
        opening.close()
    }
}

const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
const MIN_PASTE_WAIT: Duration = Duration::from_millis(120);
const MAX_PASTE_WAIT: Duration = Duration::from_millis(400);
const READER_QUIET: Duration = Duration::from_millis(40);
const READER_POLL: Duration = Duration::from_millis(8);

struct Snapshot {
    formats: Vec<(u32, GlobalBlock)>,
    sequence: u32,
}

impl Snapshot {
    fn read(_opening: &Opening) -> Result<Self, ClipboardError> {
        let mut ids = Vec::new();
        let mut format = 0;
        loop {
            unsafe { SetLastError(ERROR_SUCCESS) };
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                if unsafe { GetLastError() } != ERROR_SUCCESS {
                    return Err(ClipboardError::Other);
                }
                break;
            }
            ids.push(format);
        }

        let mut remaining = MAX_SNAPSHOT_BYTES;
        let mut formats = Vec::with_capacity(ids.len());
        for format in ids {
            if [
                CF_BITMAP,
                CF_PALETTE,
                CF_ENHMETAFILE,
                CF_METAFILEPICT,
                CF_DSPBITMAP,
                CF_DSPENHMETAFILE,
                CF_DSPMETAFILEPICT,
            ]
            .iter()
            .any(|f| format == u32::from(f.0))
            {
                // These contain GDI handles, including the nested handle in
                // METAFILEPICT. A byte copy cannot preserve their lifetime.
                continue;
            }
            if format == u32::from(CF_OWNERDISPLAY.0)
                || (u32::from(CF_PRIVATEFIRST.0)..=u32::from(CF_PRIVATELAST.0)).contains(&format)
            {
                // These depend on behavior of the original clipboard owner.
                return Err(ClipboardError::Other);
            }
            let source = HGLOBAL(
                unsafe { GetClipboardData(format) }
                    .map_err(|_| ClipboardError::Other)?
                    .0,
            );
            let size = unsafe { GlobalSize(source) };
            if size == 0 || size > remaining {
                return Err(ClipboardError::Other);
            }
            let address = unsafe { GlobalLock(source) }.cast::<u8>();
            if address.is_null() {
                return Err(ClipboardError::Other);
            }
            // The source block remains allocated and locked for this copy.
            let bytes = unsafe { std::slice::from_raw_parts(address, size) };
            let saved = GlobalBlock::new(bytes, true);
            unsafe { SetLastError(ERROR_SUCCESS) };
            if unsafe { GlobalUnlock(source) }.is_err()
                && unsafe { GetLastError() } != ERROR_SUCCESS
            {
                return Err(ClipboardError::Other);
            }
            formats.push((format, saved?));
            remaining -= size;
        }
        // In particular, measure *after* any successful delayed rendering.
        Ok(Self {
            formats,
            sequence: sequence(),
        })
    }

    fn restore(&mut self, _opening: &Opening) {
        // Try each already allocated block even if a single format is refused.
        if unsafe { EmptyClipboard() }.is_ok() {
            for (format, block) in &mut self.formats {
                let _ = block.transfer(*format);
            }
        }
    }
}

fn sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

pub(super) struct PasteClipboard {
    owner: Owner,
    snapshot: Snapshot,
    ours: u32,
}

impl PasteClipboard {
    pub(super) fn stage(terminated_utf16: &[u16]) -> Result<Self, ClipboardError> {
        let mut text = PrivateText::new(terminated_utf16)?;
        let owner = Owner::new()?;
        let opening = Opening::new(&owner)?;
        // Keep the same opening through snapshot and publication: a later
        // writer cannot slip in between saving and replacing the contents.
        let mut snapshot = Snapshot::read(&opening)?;
        if sequence() != snapshot.sequence {
            return Err(ClipboardError::Other);
        }
        unsafe { EmptyClipboard() }.map_err(|_| ClipboardError::Other)?;
        if let Err(error) = text.publish(&opening) {
            snapshot.restore(&opening);
            return Err(error);
        }
        let closed = opening.close();
        // USER32 can bump the sequence at CloseClipboard when it adds
        // synthesized formats (e.g. CF_TEXT for CF_UNICODETEXT). Do not
        // mistake that increment for a new writer. is_current checks the
        // HWND as well, in case another owner won immediately after close.
        let staged = Self {
            owner,
            snapshot,
            ours: sequence(),
        };
        closed?;
        Ok(staged)
    }

    pub(super) fn is_current(&self) -> bool {
        // A delayed-rendering writer may change the owner before advancing
        // the sequence, so verify both. Cork restoration with OpenClipboard.
        sequence() == self.ours
            && unsafe { GetClipboardOwner() }.is_ok_and(|owner| owner == self.owner.0)
    }

    pub(super) fn wait_for_paste(&self) {
        // Windows has no notification for reads of eagerly rendered data.
        // Give the message queue and renderer at least 120 ms, extending
        // while a named opener is seen. Readers may open with a null HWND;
        // this is a bounded heuristic, not a paste acknowledgement.
        let start = Instant::now();
        let deadline = start + MAX_PASTE_WAIT;
        let mut quiet_until = start + MIN_PASTE_WAIT;
        loop {
            let now = Instant::now();
            if now >= deadline || !self.is_current() {
                break;
            }
            if unsafe { GetOpenClipboardWindow() }.is_ok() {
                quiet_until = quiet_until.max(now + READER_QUIET);
            } else if now >= quiet_until {
                break;
            }
            thread::sleep(READER_POLL.min(deadline.saturating_duration_since(now)));
        }
    }
}

impl Drop for PasteClipboard {
    fn drop(&mut self) {
        if self.is_current()
            && let Ok(opening) = Opening::new(&self.owner)
            && self.is_current()
        {
            // Check again inside the opening. A concurrent writer must win,
            // including one that owned the clipboard while we waited to open.
            self.snapshot.restore(&opening);
        }
    }
}
