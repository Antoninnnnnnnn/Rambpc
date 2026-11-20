//! Interactive Windows 11 Notepad qualification. Only fresh test windows are written.

#[cfg(windows)]
#[path = "insertion_support/stress.rs"]
mod stress;

#[cfg(windows)]
mod fixture {
    use sambler_lib::{
        platform::windows::{SendInputInserter, UiaTargetProbe},
        ports::{
            AppKind, InsertOutcome, Inserter, TargetChange, TargetProbe, TargetToken, TargetVerdict,
        },
    };
    use std::{
        process::Command,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::{
            Accessibility::{
                CUIAutomation8, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
            },
            Controls::EM_SETMODIFY,
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
                SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
            },
            WindowsAndMessaging::*,
        },
    };
    use windows::core::{BOOL, w};

    const PHRASE: &str = "Rendez-vous jeudi à 14 h";

    fn class(hwnd: HWND) -> String {
        let mut class = [0u16; 128];
        let len = unsafe { GetClassNameW(hwnd, &mut class) }.max(0) as usize;
        String::from_utf16_lossy(&class[..len])
    }

    fn notepads() -> Vec<HWND> {
        unsafe extern "system" fn each(hwnd: HWND, list: LPARAM) -> BOOL {
            if class(hwnd).eq_ignore_ascii_case("Notepad")
                && unsafe { IsWindowVisible(hwnd) }.as_bool()
            {
                unsafe { &mut *(list.0 as *mut Vec<HWND>) }.push(hwnd);
            }
            BOOL(1)
        }
        let mut list = Vec::new();
        unsafe { EnumWindows(Some(each), LPARAM(&mut list as *mut Vec<HWND> as isize)) }.unwrap();
        list
    }

    fn until<T>(timeout: Duration, mut f: impl FnMut() -> Option<T>) -> Option<T> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(value) = f() {
                return Some(value);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn foreground() -> HWND {
        unsafe { GetForegroundWindow() }
    }
    fn plain_notepad() -> Option<HWND> {
        let hwnd = foreground();
        class(hwnd).eq_ignore_ascii_case("Notepad").then_some(hwnd)
    }

    fn new_window(from: HWND) -> Result<HWND, &'static str> {
        let previous = notepads();
        if foreground() != from || !previous.contains(&from) {
            return Err("Notepad is not foreground; nothing was sent");
        }
        let keys = [
            (VK_CONTROL, false),
            (VK_SHIFT, false),
            (VIRTUAL_KEY(b'N'.into()), false),
            (VIRTUAL_KEY(b'N'.into()), true),
            (VK_SHIFT, true),
            (VK_CONTROL, true),
        ];
        let batch = keys.map(|(key, up)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            },
        });
        if [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|key| unsafe { GetAsyncKeyState(key.0.into()) } < 0)
            || foreground() != from
        {
            return Err("focus changed or modifiers are held; nothing was sent");
        }
        if unsafe { SendInput(&batch, std::mem::size_of::<INPUT>() as i32) } as usize != batch.len()
        {
            return Err("Notepad new-window shortcut was not accepted");
        }
        until(Duration::from_secs(5), || {
            plain_notepad().filter(|hwnd| !previous.contains(hwnd))
        })
        .ok_or("Notepad did not create a fresh foreground window")
    }

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    struct Document {
        editor: HWND,
        text: IUIAutomationTextPattern,
    }

    impl Document {
        fn from_focus(automation: &IUIAutomation, window: HWND) -> Option<Self> {
            if foreground() != window {
                return None;
            }
            let element = unsafe { automation.GetFocusedElement() }.ok()?;
            let editor = unsafe { element.CurrentNativeWindowHandle() }.ok()?;
            if !class(editor).to_ascii_lowercase().starts_with("richedit")
                || unsafe { GetAncestor(editor, GA_ROOT) } != window
            {
                return None;
            }
            let text = unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) }.ok()?;
            Some(Document { editor, text })
        }

        fn contents(&self) -> Option<String> {
            // This harness alone reads document text, from its verified test window only.
            let range = unsafe { self.text.DocumentRange() }.ok()?;
            let text = unsafe { range.GetText(256) }.ok()?.to_string();
            Some(text.trim_end_matches(['\r', '\n']).to_owned())
        }

        fn reset_fixture(&self) -> bool {
            // Do not clear text if anything other than this exact fixture was written.
            let Some(text) = self.contents() else {
                return false;
            };
            if !text.is_empty()
                && (text != PHRASE
                    || !send(
                        self.editor,
                        WM_SETTEXT,
                        WPARAM(0),
                        LPARAM(w!("").as_ptr() as isize),
                    ))
            {
                return false;
            }
            self.contents().as_deref() == Some("")
                && send(self.editor, EM_SETMODIFY, WPARAM(0), LPARAM(0))
        }
    }

    fn send(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> bool {
        unsafe {
            SendMessageTimeoutW(
                hwnd,
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

    fn close_blank(window: HWND) -> bool {
        if !unsafe { IsWindow(Some(window)) }.as_bool() {
            return true;
        }
        let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        until(Duration::from_secs(2), || {
            (!unsafe { IsWindow(Some(window)) }.as_bool()).then_some(())
        })
        .is_some()
    }

    fn token(probe: &UiaTargetProbe) -> Result<TargetToken, String> {
        match probe.capture() {
            TargetVerdict::Eligible { token, info } if info.app == AppKind::Notepad => Ok(token),
            other => Err(format!("Notepad target verdict: {other:?}")),
        }
    }

    pub fn main() -> Result<(), Box<dyn std::error::Error>> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
        let _apartment = Apartment;
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }?;
        let before = notepads();
        let boot = if let Some(hwnd) = plain_notepad() {
            hwnd
        } else {
            // Windows may restore existing tabs. The bootstrap window is never
            // inspected or written; the test uses a separate new window.
            Command::new("notepad.exe")
                .spawn()
                .map_err(|_| "could not launch Notepad")?;
            until(Duration::from_secs(8), plain_notepad)
                .ok_or("Notepad did not receive foreground focus")?
        };
        let window = new_window(boot)?;
        let probe = UiaTargetProbe::new();
        let inserter = SendInputInserter::new();
        let first = until(Duration::from_secs(3), || {
            Document::from_focus(&automation, window)
        });
        let mut second = None;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let first = first
                .as_ref()
                .ok_or("fresh Notepad window has no focused RichEdit TextPattern")?;
            if first.contents().as_deref() != Some("") {
                return Err("fresh Notepad editor was not empty".into());
            }
            // The native window can be foreground before App SDK and RichEdit
            // finish bringing up the new tab. Let the fixture settle, then
            // require the same editor and an empty document before writing.
            thread::sleep(Duration::from_millis(200));
            if !Document::from_focus(&automation, window).is_some_and(|doc| {
                doc.editor == first.editor && doc.contents().as_deref() == Some("")
            }) {
                return Err("fresh Notepad editor did not remain ready".into());
            }
            let stress = super::stress::run(
                "Notepad RichEditD2DPT",
                first.editor,
                &probe,
                || token(&probe),
                || first.contents(),
            );
            if stress.is_err()
                && first.contents().is_some_and(|text| {
                    text.starts_with("[before]")
                        && text.ends_with("[after]")
                        && text.encode_utf16().count() <= 216
                })
            {
                // The stress markers tag the contents left by this run, even
                // if a test did not deliver the intended middle text.
                let _ = super::stress::set_contents(first.editor, "");
            }
            stress?;
            let initial = token(&probe)?;
            println!("Notepad RichEdit caret: Eligible");
            let outcome = inserter.insert_once(&initial, PHRASE);
            if outcome != InsertOutcome::Delivered {
                return Err(format!("Notepad insertion: {outcome:?}").into());
            }
            if until(Duration::from_secs(2), || {
                (first.contents().as_deref() == Some(PHRASE)).then_some(())
            })
            .is_none()
            {
                let fresh =
                    Document::from_focus(&automation, window).and_then(|doc| doc.contents());
                for value in [first.contents(), fresh] {
                    if let Some(value) = value {
                        let matching_prefix = value
                            .chars()
                            .zip(PHRASE.chars())
                            .take_while(|(a, b)| a == b)
                            .count();
                        eprintln!(
                            "readback diagnostic: units={}, matching prefix={}, occurrences={}, NUL count={}",
                            value.encode_utf16().count(),
                            matching_prefix,
                            value.matches(PHRASE).count(),
                            value.chars().filter(|&c| c == '\0').count()
                        );
                    } else {
                        eprintln!("readback diagnostic: UIA read failed");
                    }
                }
                return Err("Notepad readback did not equal the single fixture phrase".into());
            }
            thread::sleep(Duration::from_millis(125));
            if first.contents().as_deref() != Some(PHRASE) {
                return Err("Notepad readback changed after delivery".into());
            }
            println!("phrase: Delivered, exact single UIA readback");

            let at_end = token(&probe)?;
            let (tx, rx) = mpsc::channel();
            let _watch = probe.watch(
                &at_end,
                Box::new(move |change| {
                    let _ = tx.send(change);
                }),
            );
            let other = new_window(window)?;
            let blank = until(Duration::from_secs(2), || {
                Document::from_focus(&automation, other)
            })
            .is_some_and(|doc| doc.contents().as_deref() == Some(""));
            second = Some((other, blank));
            if !blank {
                return Err("second test window was not a verified blank RichEdit".into());
            }
            let change = rx.recv_timeout(Duration::from_secs(1))?;
            if change != TargetChange::FocusMoved || probe.revalidate(&at_end).is_ok() {
                return Err(format!(
                    "Notepad focus change did not invalidate the original target: {change:?}"
                )
                .into());
            }
            println!("window change: FocusMoved; revalidation refused");
            Ok(())
        })();

        let closed_second = second.is_none_or(|(other, blank)| blank && close_blank(other));
        let closed_first = first.is_some_and(|doc| doc.reset_fixture() && close_blank(window));
        println!(
            "cleanup: first test window closed={closed_first}; second test window closed or not created={closed_second}; bootstrap window was {} and left untouched",
            if before.contains(&boot) {
                "already open"
            } else {
                "launched for setup"
            }
        );
        result?;
        if !closed_first || !closed_second {
            return Err("Notepad test window cleanup needs manual review".into());
        }
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    fixture::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This harness requires Windows.");
}
