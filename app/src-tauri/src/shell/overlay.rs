//! Floating dictation bar: a transparent, borderless, always-on-top WebView
//! window (`overlay.html`) that never takes the focus and lets clicks through.
//! Its content is drawn by the web UI from the `sambler://state` and
//! `sambler://level` events, so it looks the same on every platform.
//!
//! Showing and placing it goes through Win32 directly (`SW_SHOWNOACTIVATE`,
//! `SWP_NOACTIVATE`): Tauri's `show()` activates the window, which would move
//! the caret away from the field being dictated into.

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const OVERLAY_WINDOW: &str = "overlay";
/// Logical size of the transparent window; the pill is drawn inside it.
const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 72.0;
/// Gap between the bar and the bottom of the work area, in logical pixels.
const MARGIN: f64 = 16.0;

pub struct Overlay<R: Runtime> {
    window: WebviewWindow<R>,
}

impl<R: Runtime> Clone for Overlay<R> {
    fn clone(&self) -> Self {
        Self {
            window: self.window.clone(),
        }
    }
}

/// Create the hidden overlay window. Must use the same WebView2 data folder
/// as the main window (one WebView2 environment per folder and options).
pub fn create<R: Runtime>(
    app: &AppHandle<R>,
    webview_dir: std::path::PathBuf,
) -> Option<Overlay<R>> {
    let window = WebviewWindowBuilder::new(app, OVERLAY_WINDOW, WebviewUrl::App("overlay.html".into()))
        .title("Sambler")
        .data_directory(webview_dir)
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()
        .ok()?;
    let _ = window.set_ignore_cursor_events(true);
    Some(Overlay { window })
}

impl<R: Runtime> Overlay<R> {
    pub fn show(&self) {
        #[cfg(windows)]
        if let Ok(hwnd) = self.window.hwnd() {
            win::show_at_active_monitor(hwnd.0 as isize, WIDTH, HEIGHT, MARGIN);
            return;
        }
        let _ = self.window.show();
    }

    pub fn hide(&self) {
        #[cfg(windows)]
        if let Ok(hwnd) = self.window.hwnd() {
            win::hide(hwnd.0 as isize);
            return;
        }
        let _ = self.window.hide();
    }
}

/// Look the overlay up again (e.g. from a command or another task).
#[allow(dead_code)]
pub fn get<R: Runtime>(app: &AppHandle<R>) -> Option<Overlay<R>> {
    app.get_webview_window(OVERLAY_WINDOW)
        .map(|window| Overlay { window })
}

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
        SWP_SHOWWINDOW, SetWindowPos, ShowWindow,
    };

    pub fn show_at_active_monitor(raw: isize, width: f64, height: f64, margin: f64) {
        let hwnd = HWND(raw as *mut _);
        unsafe {
            let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(monitor, &mut info).as_bool() {
                let (mut dx, mut dy) = (96u32, 96u32);
                let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
                let scale = dx as f64 / 96.0;
                let w = (width * scale).round() as i32;
                let h = (height * scale).round() as i32;
                let work = info.rcWork;
                let x = work.left + ((work.right - work.left) - w) / 2;
                let y = work.bottom - h - (margin * scale).round() as i32;
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    x,
                    y,
                    w,
                    h,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }

    pub fn hide(raw: isize) {
        unsafe {
            let _ = ShowWindow(HWND(raw as *mut _), SW_HIDE);
        }
    }
}
