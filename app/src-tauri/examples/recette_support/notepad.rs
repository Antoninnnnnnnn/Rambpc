//! Fresh Windows 11 Notepad windows for the acceptance harness. Adapted from
//! `examples/notepad_harness.rs`: only windows created by the harness are
//! read, reset and closed; a pre-existing Notepad window is never touched.

use std::process::Command;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::Win32::UI::Controls::EM_SETMODIFY;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, w};

fn class(hwnd: HWND) -> String {
    let mut class = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut class) }.max(0) as usize;
    String::from_utf16_lossy(&class[..len])
}

fn notepads() -> Vec<isize> {
    unsafe extern "system" fn each(hwnd: HWND, list: LPARAM) -> BOOL {
        if class(hwnd).eq_ignore_ascii_case("Notepad")
            && unsafe { IsWindowVisible(hwnd) }.as_bool()
        {
            unsafe { &mut *(list.0 as *mut Vec<isize>) }.push(hwnd.0 as isize);
        }
        BOOL(1)
    }
    let mut list = Vec::new();
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut list as *mut Vec<isize> as isize)) };
    list
}

fn until<T>(timeout: Duration, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = f() {
            return Some(v);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut _)
}

fn foreground() -> isize {
    unsafe { GetForegroundWindow() }.0 as isize
}

fn foreground_notepad() -> Option<isize> {
    let fg = foreground();
    class(hwnd(fg)).eq_ignore_ascii_case("Notepad").then_some(fg)
}

fn chord(keys: &[(VIRTUAL_KEY, bool)]) -> bool {
    let batch: Vec<INPUT> = keys
        .iter()
        .map(|&(key, up)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                    ..Default::default()
                },
            },
        })
        .collect();
    (unsafe { SendInput(&batch, std::mem::size_of::<INPUT>() as i32) }) as usize == batch.len()
}

fn modifiers_held() -> bool {
    [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|key| unsafe { GetAsyncKeyState(key.0.into()) } < 0)
}

/// Ctrl+Shift+N from a foreground Notepad window opens a fresh window.
fn new_window(from: isize) -> Result<isize, String> {
    let previous = notepads();
    if foreground() != from || modifiers_held() {
        return Err("Notepad not foreground or modifiers held; nothing sent".into());
    }
    let n = VIRTUAL_KEY(b'N'.into());
    let keys = [
        (VK_CONTROL, false),
        (VK_SHIFT, false),
        (n, false),
        (n, true),
        (VK_SHIFT, true),
        (VK_CONTROL, true),
    ];
    if !chord(&keys) {
        return Err("new-window shortcut not accepted".into());
    }
    until(Duration::from_secs(5), || {
        foreground_notepad().filter(|h| !previous.contains(h))
    })
    .ok_or_else(|| "no fresh Notepad window".to_string())
}

struct Doc {
    editor: isize,
    text: IUIAutomationTextPattern,
}

fn doc(automation: &IUIAutomation, window: isize) -> Option<Doc> {
    if foreground() != window {
        return None;
    }
    let element = unsafe { automation.GetFocusedElement() }.ok()?;
    let editor = unsafe { element.CurrentNativeWindowHandle() }.ok()?;
    if !class(editor).to_ascii_lowercase().starts_with("richedit")
        || unsafe { GetAncestor(editor, GA_ROOT) }.0 as isize != window
    {
        return None;
    }
    let text = unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) }.ok()?;
    Some(Doc {
        editor: editor.0 as isize,
        text,
    })
}

fn read(doc: &Doc) -> Result<String, String> {
    // Only harness-created test windows are ever read.
    let range = unsafe { doc.text.DocumentRange() }.map_err(|_| "UIA range")?;
    let text = unsafe { range.GetText(4096) }
        .map_err(|_| "UIA read")?
        .to_string();
    Ok(text.trim_end_matches(['\r', '\n']).to_owned())
}

fn send(raw: isize, msg: u32, w: WPARAM, l: LPARAM) -> bool {
    unsafe {
        SendMessageTimeoutW(
            hwnd(raw),
            msg,
            w,
            l,
            SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
            200,
            None,
        )
    }
    .0 != 0
}

fn reset_and_close(doc: &Doc, window: isize) -> bool {
    let cleared = send(doc.editor, WM_SETTEXT, WPARAM(0), LPARAM(w!("").as_ptr() as isize))
        && send(doc.editor, EM_SETMODIFY, WPARAM(0), LPARAM(0));
    let _ = unsafe { PostMessageW(Some(hwnd(window)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    let closed = until(Duration::from_secs(3), || {
        (!unsafe { IsWindow(Some(hwnd(window))) }.as_bool()).then_some(())
    })
    .is_some();
    cleared && closed
}

pub struct Fixture {
    automation: IUIAutomation,
    boot: Option<isize>,
    window: isize,
    first: Doc,
    second: Mutex<Option<(isize, Doc)>>,
}

// UIA interfaces from CUIAutomation8 are free-threaded in the MTA.
unsafe impl Send for Fixture {}
unsafe impl Sync for Fixture {}

impl Fixture {
    pub fn open() -> Result<Self, String> {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| "UIA")?;
        let (boot_window, launched) = match foreground_notepad() {
            Some(h) => (h, false),
            None => {
                Command::new("notepad.exe")
                    .spawn()
                    .map_err(|_| "cannot launch Notepad")?;
                let h = until(Duration::from_secs(8), foreground_notepad)
                    .ok_or("Notepad not foreground")?;
                (h, true)
            }
        };
        // Let a freshly launched Notepad finish restoring its tabs first.
        let _ = until(Duration::from_secs(8), || doc(&automation, boot_window));
        thread::sleep(Duration::from_millis(400));
        let window = new_window(boot_window)?;
        let close_created = || {
            let _ = unsafe { PostMessageW(Some(hwnd(window)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            if launched {
                let _ = unsafe {
                    PostMessageW(Some(hwnd(boot_window)), WM_CLOSE, WPARAM(0), LPARAM(0))
                };
            }
        };
        let Some(first) = until(Duration::from_secs(8), || doc(&automation, window)) else {
            let focused = unsafe { automation.GetFocusedElement() }
                .ok()
                .and_then(|e| unsafe { e.CurrentNativeWindowHandle() }.ok())
                .map(class)
                .unwrap_or_default();
            let fg_ok = foreground() == window;
            close_created();
            return Err(format!(
                "no RichEdit in the new window (foreground={fg_ok}, focused class={focused:?})"
            ));
        };
        thread::sleep(Duration::from_millis(250));
        if !read(&first)?.is_empty() {
            close_created();
            return Err("fresh window not empty".into());
        }
        Ok(Self {
            automation,
            boot: launched.then_some(boot_window),
            window,
            first,
            second: Mutex::new(None),
        })
    }

    pub fn focus(&self) -> Result<(), String> {
        if foreground() == self.window {
            return Ok(());
        }
        let _ = unsafe { SetForegroundWindow(hwnd(self.window)) };
        until(Duration::from_secs(2), || {
            (foreground() == self.window).then_some(())
        })
        .ok_or_else(|| "cannot focus the test window".to_string())
    }

    pub fn contents(&self) -> Result<String, String> {
        read(&self.first)
    }

    pub fn open_second(&self) -> Result<(), String> {
        let other = new_window(self.window)?;
        let d = until(Duration::from_secs(3), || doc(&self.automation, other))
            .ok_or("no second RichEdit")?;
        *self.second.lock().unwrap() = Some((other, d));
        Ok(())
    }

    pub fn second_contents(&self) -> Result<String, String> {
        match &*self.second.lock().unwrap() {
            Some((_, d)) => read(d),
            None => Ok(String::new()),
        }
    }

    /// Close every window this fixture created (and the bootstrap one if launched here).
    pub fn close(&self) -> bool {
        let mut ok = true;
        if let Some((w2, d2)) = self.second.lock().unwrap().take() {
            ok &= reset_and_close(&d2, w2);
        }
        ok &= reset_and_close(&self.first, self.window);
        if let Some(boot) = self.boot {
            let _ = unsafe { PostMessageW(Some(hwnd(boot)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        ok
    }
}
