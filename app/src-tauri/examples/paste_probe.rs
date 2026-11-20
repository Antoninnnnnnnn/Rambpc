//! Offline, synthetic paste probe. Only acts on an explicitly supplied fixture PID.

#[cfg(windows)]
mod win {
    use sambler_lib::{
        platform::windows::{SendInputInserter, UiaTargetProbe},
        ports::{AppKind, Inserter, TargetProbe, TargetVerdict},
    };
    use std::{
        io::{self, BufRead, Write},
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM},
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            UI::{
                Accessibility::*,
                Input::KeyboardAndMouse::*,
                WindowsAndMessaging::{
                    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
                    IsWindowVisible, SetForegroundWindow, SwitchToThisWindow,
                },
            },
        },
        core::Interface,
    };

    const ONE: &str = "sambler fixture é🦀";
    const TWO: &str = "sambler fixture é🦀\nsecond line";

    fn foreground(pid: u32) -> bool {
        let mut current = 0;
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut current)) };
        pid != 0 && current == pid
    }

    fn foreground_fixture(pid: u32) -> bool {
        struct Search {
            pid: u32,
            hwnd: HWND,
        }
        unsafe extern "system" fn find(hwnd: HWND, arg: LPARAM) -> windows::core::BOOL {
            let search = unsafe { &mut *(arg.0 as *mut Search) };
            let mut owner = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
            if owner == search.pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
                let mut class = [0u16; 64];
                let count = unsafe { GetClassNameW(hwnd, &mut class) }.max(0) as usize;
                if class[..count]
                    .iter()
                    .copied()
                    .eq("Chrome_WidgetWin_1".encode_utf16())
                {
                    search.hwnd = hwnd;
                    return false.into();
                }
            }
            true.into()
        }
        if !foreground(pid) && pid != 0 {
            let mut search = Search {
                pid,
                hwnd: HWND::default(),
            };
            let _ =
                unsafe { EnumWindows(Some(find), LPARAM((&mut search as *mut Search) as isize)) };
            if !search.hwnd.0.is_null() {
                let _ = unsafe { SetForegroundWindow(search.hwnd) };
                if !foreground(pid) {
                    // Fixture only: ask the shell to switch to this owned window.
                    // No keys are sent to an unrelated foreground application.
                    unsafe { SwitchToThisWindow(search.hwnd, true) };
                }
            }
        }
        let until = Instant::now() + Duration::from_millis(500);
        while !foreground(pid) && Instant::now() < until {
            thread::sleep(Duration::from_millis(15));
        }
        foreground(pid)
    }

    fn keys(pid: u32, ctrl: bool, key: VIRTUAL_KEY) -> bool {
        let inputs: Vec<_> = [
            (VK_CONTROL, false),
            (key, false),
            (key, true),
            (VK_CONTROL, true),
        ]
        .into_iter()
        .filter(|(key, _)| ctrl || *key != VK_CONTROL)
        .map(|(key, up)| INPUT {
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
        })
        .collect();
        foreground(pid)
            && unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize == inputs.len()
    }

    struct Automation(IUIAutomation);
    impl Automation {
        fn new() -> Option<Self> {
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .ok()?;
            match unsafe {
                CoCreateInstance::<_, IUIAutomation>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
            } {
                Ok(value) => {
                    if let Ok(v2) = value.cast::<IUIAutomation2>() {
                        unsafe {
                            let _ = v2.SetConnectionTimeout(200);
                            let _ = v2.SetTransactionTimeout(200);
                        }
                    }
                    Some(Self(value))
                }
                Err(_) => {
                    unsafe { CoUninitialize() };
                    None
                }
            }
        }
    }
    impl Drop for Automation {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }

    fn read_field(automation: &Automation, pid: u32, chat: bool) -> Option<(String, bool)> {
        if !foreground(pid) {
            return None;
        }
        unsafe {
            let el = automation.0.GetFocusedElement().ok()?;
            if el.CurrentProcessId().ok()? as u32 != pid
                || !el.CurrentHasKeyboardFocus().ok()?.as_bool()
                || el.CurrentIsPassword().ok()?.as_bool()
            {
                return None;
            }
            // The browser driver establishes the omnibox by Ctrl+L and the
            // original fixture URL; Chromium forks need not share its class.
            if !chat && el.CurrentControlType().ok()? != UIA_EditControlTypeId {
                return None;
            }
            let pattern: IUIAutomationTextPattern =
                el.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
            let doc = pattern.DocumentRange().ok()?;
            let mut text = if chat {
                doc.GetText(1024).ok()?.to_string().replace("\r\n", "\n")
            } else {
                let value: IUIAutomationValuePattern =
                    el.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
                value.CurrentValue().ok()?.to_string()
            };
            if chat && text.ends_with('\n') {
                text.pop();
            }
            let ranges = pattern.GetSelection().ok()?;
            let caret = if ranges.Length().ok()? == 1 {
                let range = ranges.GetElement(0).ok()?;
                let start = TextPatternRangeEndpoint_Start;
                let end = TextPatternRangeEndpoint_End;
                range.CompareEndpoints(start, &range, end).ok()? == 0
                    && (range.CompareEndpoints(end, &doc, end).ok()? == 0
                        || (chat && {
                            let prefix = doc.Clone().ok()?;
                            prefix.MoveEndpointByRange(end, &range, end).ok()?;
                            prefix.GetText(1024).ok()?.to_string().replace("\r\n", "\n") == text
                        }))
            } else {
                false
            };
            Some((text, caret))
        }
    }

    fn millis(t: Duration) -> f64 {
        t.as_secs_f64() * 1000.0
    }
    fn epoch_ms() -> f64 {
        millis(SystemTime::now().duration_since(UNIX_EPOCH).unwrap())
    }

    fn run(app: AppKind, pid: u32, cmd: &str, probe: &UiaTargetProbe) -> String {
        let browser = app != AppKind::VsCode;
        if cmd == "foreground" {
            return format!("keys={}", foreground_fixture(pid));
        }
        if !foreground(pid) {
            return "foreground_lost=true".into();
        }
        match cmd {
            "address" if browser => return format!("keys={}", keys(pid, true, VK_L)),
            "undo" => return format!("keys={}", keys(pid, true, VK_Z)),
            "escape" if browser => return format!("keys={}", keys(pid, false, VK_ESCAPE)),
            "address-restored" | "chat-empty" => {
                let valid = Automation::new()
                    .and_then(|a| read_field(&a, pid, !browser))
                    .is_some_and(|(text, _)| {
                        if browser {
                            text.ends_with("/editor.html")
                        } else {
                            text.is_empty()
                        }
                    });
                return format!("restored={valid}");
            }
            _ => {}
        }
        let text = match cmd {
            "one" | "address-one" | "chat-one" => ONE,
            "two" | "address-two" | "chat-two" => TWO,
            _ => return "invalid_command=true".into(),
        };
        let started = Instant::now();
        let verdict = probe.capture();
        let capture_ms = millis(started.elapsed());
        let TargetVerdict::Eligible { token, info } = verdict else {
            return format!("capture_ms={capture_ms:.2} verdict={verdict:?}");
        };
        if info.app != app || !foreground(pid) {
            return "wrong_fixture=true".into();
        }
        let started = Instant::now();
        let revalidated = probe.revalidate(&token);
        let recheck_ms = millis(started.elapsed());
        if revalidated.is_err() {
            return format!(
                "capture_ms={capture_ms:.2} recheck_ms={recheck_ms:.2} recheck={revalidated:?}"
            );
        }
        let observer = (cmd == "address-one" || cmd.starts_with("chat-")).then(|| {
            thread::spawn(move || {
                let a = Automation::new()?;
                let deadline = Instant::now() + Duration::from_millis(650);
                while Instant::now() < deadline {
                    if let Some((visible, at_end)) = read_field(&a, pid, !browser)
                        && !visible.is_empty()
                        && (visible == text || visible.starts_with("sambler fixture"))
                    {
                        return Some((epoch_ms(), visible == text, at_end));
                    }
                    thread::sleep(Duration::from_millis(4));
                }
                None
            })
        });
        let start_epoch = epoch_ms();
        let started = Instant::now();
        let outcome = SendInputInserter::new().insert_once(&token, text);
        let elapsed = millis(started.elapsed());
        let mut result = format!(
            "capture_ms={capture_ms:.2} recheck_ms={recheck_ms:.2} multiline={:?} outcome={outcome:?} epoch={start_epoch:.3} insertion_ms={elapsed:.2}",
            info.multiline
        );
        if let Some(observed) = observer {
            if let Ok(Some((visible_epoch, exact, caret))) = observed.join() {
                result.push_str(&format!(
                    " exact={exact} caret={caret} visible_ms={:.2}",
                    visible_epoch - start_epoch
                ));
            } else {
                result.push_str(" exact=false caret=false");
            }
        }
        result
    }

    pub fn main() -> Result<(), &'static str> {
        let mut args = std::env::args().skip(1);
        let app = match args.next().as_deref() {
            Some("edge") => AppKind::Edge,
            Some("chrome") => AppKind::Chrome,
            Some("brave") => AppKind::Other,
            Some("code") => AppKind::VsCode,
            _ => return Err("invalid fixture app"),
        };
        let pid = args
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or("missing fixture pid")?;
        let probe = UiaTargetProbe::new();
        let mut stdout = io::stdout().lock();
        for line in io::stdin().lock().lines() {
            let line = line.map_err(|_| "fixture command failed")?;
            writeln!(stdout, "{}", run(app, pid, &line, &probe))
                .map_err(|_| "fixture output failed")?;
            stdout.flush().map_err(|_| "fixture output failed")?;
        }
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> Result<(), &'static str> {
    win::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows fixture only");
}
