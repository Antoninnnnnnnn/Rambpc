//! Native dictation capsule: a small topmost Win32 window that never takes
//! the focus (WS_EX_NOACTIVATE, SWP_NOACTIVATE, MA_NOACTIVATE) and lets clicks
//! through, so the caret stays in the target field. It shows a text label and
//! a shape for every state (never color alone), plus a level bar while listening.
//! Placed at the bottom center of the work area of the foreground window's
//! monitor, scaled for that monitor's DPI. Actions live in the main window and
//! the tray menu.

use super::relay::Capsule;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, AtomicU8, AtomicU32, Ordering};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, COLOR_HIGHLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor, CLIP_DEFAULT_PRECIS, CreateFontW, CreateRoundRectRgn,
    CreateSolidBrush, DEFAULT_CHARSET, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW,
    Ellipse, EndPaint, FF_DONTCARE, FW_SEMIBOLD, FillRect, GetMonitorInfoW, HGDIOBJ,
    InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, Polygon, Rectangle, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW,
    DispatchMessageW, GetForegroundWindow, GetMessageW, HWND_TOPMOST, MA_NOACTIVATE,
    MSG, PostMessageW, RegisterClassExW, SPI_GETHIGHCONTRAST, SW_HIDE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetLayeredWindowAttributes, SetWindowPos,
    ShowWindow, SystemParametersInfoW, TranslateMessage, WM_APP, WM_MOUSEACTIVATE, WM_PAINT,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP, LWA_ALPHA,
};
use windows::core::{PCWSTR, w};

const WM_CAPSULE_UPDATE: u32 = WM_APP + 1;
const WM_CAPSULE_LEVEL: u32 = WM_APP + 2;

static STATE: AtomicU8 = AtomicU8::new(0);
static LEVEL: AtomicU32 = AtomicU32::new(0);

const ALL: [Capsule; 12] = [
    Capsule::Hidden,
    Capsule::Preparing,
    Capsule::Listening,
    Capsule::Processing,
    Capsule::Inserted,
    Capsule::CheckField,
    Capsule::KeptInSambler,
    Capsule::NoText,
    Capsule::Cancelled,
    Capsule::Interrupted,
    Capsule::Unavailable,
    Capsule::Refused,
];

fn code(c: Capsule) -> u8 {
    ALL.iter().position(|x| *x == c).unwrap_or(0) as u8
}

fn current() -> Capsule {
    ALL.get(STATE.load(Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or(Capsule::Hidden)
}

#[derive(Clone)]
pub struct Indicator {
    hwnd: Arc<AtomicIsize>,
}

impl Indicator {
    pub fn spawn() -> Self {
        let hwnd = Arc::new(AtomicIsize::new(0));
        let slot = hwnd.clone();
        let _ = std::thread::Builder::new()
            .name("sambler-capsule".into())
            .spawn(move || unsafe { run(slot) });
        Self { hwnd }
    }

    fn post(&self, msg: u32) {
        let raw = self.hwnd.load(Ordering::Acquire);
        if raw != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(raw as *mut _)), msg, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn show(&self, capsule: Capsule) {
        STATE.store(code(capsule), Ordering::Relaxed);
        self.post(WM_CAPSULE_UPDATE);
    }

    pub fn level(&self, level: f32) {
        LEVEL.store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        if current() == Capsule::Listening {
            self.post(WM_CAPSULE_LEVEL);
        }
    }
}

unsafe fn run(slot: Arc<AtomicIsize>) {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let class = w!("SamblerCapsule");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            class,
            w!("Sambler"),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            return;
        };
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 242, LWA_ALPHA);
        slot.store(hwnd.0 as isize, Ordering::Release);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

struct Palette {
    background: COLORREF,
    text: COLORREF,
    accent: COLORREF,
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

unsafe fn palette() -> Palette {
    unsafe {
        let mut hc = HIGHCONTRASTW {
            cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        let high_contrast = SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some(&mut hc as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
            && hc.dwFlags.contains(HCF_HIGHCONTRASTON);
        if high_contrast {
            Palette {
                background: COLORREF(GetSysColor(COLOR_WINDOW)),
                text: COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                accent: COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
            }
        } else {
            Palette {
                background: rgb(32, 32, 36),
                text: rgb(245, 245, 247),
                accent: rgb(96, 205, 255),
            }
        }
    }
}

/// Size and position for the current state, in physical pixels.
unsafe fn layout() -> Option<(i32, i32, i32, i32, f32)> {
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let scale = dx as f32 / 96.0;
        let width = (300.0 * scale) as i32;
        let height = (44.0 * scale) as i32;
        let work = info.rcWork;
        let x = work.left + ((work.right - work.left) - width) / 2;
        let y = work.bottom - height - (40.0 * scale) as i32;
        Some((x, y, width, height, scale))
    }
}

static SCALE_BITS: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_CAPSULE_UPDATE => {
                if current() == Capsule::Hidden {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                } else if let Some((x, y, w, h, scale)) = layout() {
                    SCALE_BITS.store(scale.to_bits(), Ordering::Relaxed);
                    let radius = h;
                    let region = CreateRoundRectRgn(0, 0, w + 1, h + 1, radius, radius);
                    SetWindowRgn(hwnd, Some(region), false);
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        x,
                        y,
                        w,
                        h,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    let _ = InvalidateRect(Some(hwnd), None, true);
                }
                LRESULT(0)
            }
            WM_CAPSULE_LEVEL => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

unsafe fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let scale = f32::from_bits(SCALE_BITS.load(Ordering::Relaxed));
        let s = |v: f32| (v * scale) as i32;
        let colors = palette();
        let mut client = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut client);

        let background = CreateSolidBrush(colors.background);
        FillRect(hdc, &client, background);
        let _ = DeleteObject(HGDIOBJ::from(background));

        let state = current();
        // Shape per state (not color alone): filled dot = listening, ring =
        // preparing, three dots = processing, triangle = warning, square = done.
        let accent = CreateSolidBrush(colors.accent);
        let old_brush = SelectObject(hdc, HGDIOBJ::from(accent));
        let cx = s(22.0);
        let cy = client.bottom / 2;
        match state {
            Capsule::Listening => {
                let r = s(7.0);
                let _ = Ellipse(hdc, cx - r, cy - r, cx + r, cy + r);
            }
            Capsule::Preparing => {
                let r = s(7.0);
                let _ = Ellipse(hdc, cx - r, cy - r, cx + r, cy + r);
                let inner = CreateSolidBrush(colors.background);
                SelectObject(hdc, HGDIOBJ::from(inner));
                let r2 = s(4.0);
                let _ = Ellipse(hdc, cx - r2, cy - r2, cx + r2, cy + r2);
                SelectObject(hdc, HGDIOBJ::from(accent));
                let _ = DeleteObject(HGDIOBJ::from(inner));
            }
            Capsule::Processing => {
                for i in -1..=1 {
                    let x = cx + i * s(6.0);
                    let r = s(2.5);
                    let _ = Ellipse(hdc, x - r, cy - r, x + r, cy + r);
                }
            }
            Capsule::Interrupted | Capsule::Unavailable | Capsule::Refused | Capsule::CheckField => {
                let points = [
                    windows::Win32::Foundation::POINT { x: cx, y: cy - s(8.0) },
                    windows::Win32::Foundation::POINT { x: cx + s(8.0), y: cy + s(7.0) },
                    windows::Win32::Foundation::POINT { x: cx - s(8.0), y: cy + s(7.0) },
                ];
                let _ = Polygon(hdc, &points);
            }
            _ => {
                let r = s(6.0);
                let _ = Rectangle(hdc, cx - r, cy - r, cx + r, cy + r);
            }
        }
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(HGDIOBJ::from(accent));

        // Label.
        let font = CreateFontW(
            -s(15.0),
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            FF_DONTCARE.0 as u32,
            w!("Segoe UI Variable Text"),
        );
        let old_font = SelectObject(hdc, HGDIOBJ::from(font));
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, colors.text);
        let mut label: Vec<u16> = state.label().encode_utf16().collect();
        let right = if state == Capsule::Listening {
            client.right - s(80.0)
        } else {
            client.right - s(12.0)
        };
        let mut text_rect = RECT {
            left: s(40.0),
            top: 0,
            right,
            bottom: client.bottom,
        };
        DrawTextW(hdc, &mut label, &mut text_rect, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        SelectObject(hdc, old_font);
        let _ = DeleteObject(HGDIOBJ::from(font));

        // Level bar while listening.
        if state == Capsule::Listening {
            let level = f32::from_bits(LEVEL.load(Ordering::Relaxed)).clamp(0.0, 1.0);
            let left = client.right - s(72.0);
            let width = s(56.0);
            let track = RECT {
                left,
                top: cy - s(3.0),
                right: left + width,
                bottom: cy + s(3.0),
            };
            let track_brush = CreateSolidBrush(rgb(90, 90, 96));
            FillRect(hdc, &track, track_brush);
            let _ = DeleteObject(HGDIOBJ::from(track_brush));
            let fill = RECT {
                right: left + (width as f32 * level) as i32,
                ..track
            };
            let fill_brush = CreateSolidBrush(colors.accent);
            FillRect(hdc, &fill, fill_brush);
            let _ = DeleteObject(HGDIOBJ::from(fill_brush));
        }
        let _ = EndPaint(hwnd, &ps);
        let _ = PCWSTR::null();
    }
}
