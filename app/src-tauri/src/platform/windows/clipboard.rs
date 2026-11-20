use crate::ports::{Clipboard, ClipboardError};
use std::{thread, time::Duration};
use windows::Win32::{
    Foundation::{ERROR_SUCCESS, GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, SetLastError},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW,
            SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::CF_UNICODETEXT,
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

impl Clipboard for WinClipboard {
    fn copy_private(&self, text: &str) -> Result<(), ClipboardError> {
        if text.contains('\0') {
            return Err(ClipboardError::Other);
        }
        let privacy = unsafe {
            [
                RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")),
                RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
                RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
            ]
        };
        if privacy.contains(&0) {
            return Err(ClipboardError::ExclusionFailed);
        }
        let utf16 = Zeroizing::new(text.encode_utf16().chain([0]).collect::<Vec<_>>());
        let mut contents = GlobalBlock::new(&utf16[..], true)?;
        let mut exclude = GlobalBlock::new(&[0u32], false)?;
        let mut history = GlobalBlock::new(&[0u32], false)?;
        let mut upload = GlobalBlock::new(&[0u32], false)?;

        // A real owner is required: EmptyClipboard after OpenClipboard(NULL)
        // leaves the clipboard unowned and SetClipboardData would fail.
        let owner = Owner(
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
            .map_err(|_| ClipboardError::Other)?,
        );
        let opening = Opening::new(&owner)?;
        unsafe { EmptyClipboard() }.map_err(|_| ClipboardError::Other)?;
        for (format, block) in privacy
            .into_iter()
            .zip([&mut exclude, &mut history, &mut upload])
        {
            block
                .transfer(format)
                .map_err(|_| ClipboardError::ExclusionFailed)?;
        }
        contents.transfer(u32::from(CF_UNICODETEXT.0))?;
        // Report a close error too; the guard can retry closing on that path.
        unsafe { CloseClipboard() }.map_err(|_| ClipboardError::Other)?;
        std::mem::forget(opening);
        Ok(())
    }
}
