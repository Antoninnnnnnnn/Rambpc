//! Offline probe for a fixture owned by the parent harness. Accepts only fixed test text.

#[cfg(windows)]
mod win {
    use sambler_lib::{
        platform::windows::{SendInputInserter, UiaTargetProbe},
        ports::{AppKind, Inserter, TargetProbe, TargetVerdict},
    };
    use std::{
        thread,
        time::{Duration, Instant},
    };
    use windows::Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::{
            Accessibility::*,
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        },
    };

    use windows::core::Interface;

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    fn code_shape(expected_pid: u32, check: Option<&str>) -> bool {
        if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
            return false;
        }
        let _apartment = Apartment;
        let Ok(automation): windows::core::Result<IUIAutomation> =
            (unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) })
        else {
            return false;
        };
        if let Ok(settings) = automation.cast::<IUIAutomation2>() {
            unsafe {
                let _ = settings.SetConnectionTimeout(350);
                let _ = settings.SetTransactionTimeout(350);
            }
        }
        let Ok(element) = (unsafe { automation.GetFocusedElement() }) else {
            return false;
        };
        let control = unsafe { element.CurrentControlType() }.ok().map(|v| v.0);
        let text =
            unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
                .is_ok();
        let pid_matches =
            unsafe { element.CurrentProcessId() }.is_ok_and(|pid| pid as u32 == expected_pid);
        let keyboard_focus =
            unsafe { element.CurrentHasKeyboardFocus() }.is_ok_and(|v| v.as_bool());
        let terminal_marker = [unsafe { element.CurrentClassName() }, unsafe {
            element.CurrentAutomationId()
        }]
        .into_iter()
        .flatten()
        .any(|s| {
            s.to_string().to_ascii_lowercase().contains("terminal")
                || s.to_string().contains("xterm")
        });
        println!(
            "shape: control={control:?} text_pattern={text} app_pid_matches={pid_matches} keyboard_focus={keyboard_focus} terminal_marker={terminal_marker}"
        );
        let Some(expected) = check else {
            return true;
        };
        if !pid_matches
            || !keyboard_focus
            || terminal_marker
            || control != Some(UIA_EditControlTypeId.0)
        {
            return false;
        }
        // Read only the focused editor in the disposable fixture process.
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
            .and_then(|pattern| unsafe { pattern.DocumentRange() })
            .and_then(|range| unsafe { range.GetText(512) })
            .is_ok_and(|value| value.to_string().trim_end_matches(['\r', '\n']) == expected)
    }

    const HEAD: &str = "Éléonore dit : « À jeudi, c’est 14 h. » Café à 5 € 🦀. Let’s meet on Thursday; déjà vu, naïve façade. ";
    pub fn main() -> Result<(), String> {
        let mut args = std::env::args().skip(1);
        let app = match args.next().as_deref() {
            Some("word") => AppKind::Word,
            Some("edge") => AppKind::Edge,
            Some("chrome") => AppKind::Chrome,
            Some("code") => AppKind::VsCode,
            _ => return Err("unknown app fixture".into()),
        };
        let pid: u32 = args
            .next()
            .and_then(|arg| arg.parse().ok())
            .ok_or("missing fixture PID")?;
        let mode = args.next().ok_or("missing fixture mode")?;
        let hwnd: Option<usize> = args.next().and_then(|arg| arg.parse().ok());
        if app == AppKind::Word && hwnd.is_none() {
            return Err("Word fixture must name its own HWND".into());
        }
        let focus = || {
            let mut current = 0;
            let now = unsafe { GetForegroundWindow() };
            unsafe { GetWindowThreadProcessId(now, Some(&mut current)) };
            current == pid && hwnd.is_none_or(|expected| now.0 as usize == expected)
        };
        let until = Instant::now() + Duration::from_secs(3);
        while !focus() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        if !focus() {
            return Err("fixture is not foreground".into());
        }
        let chord = match mode.as_str() {
            "undo" => Some((None, VIRTUAL_KEY(b'Z'.into()))),
            "save" if app == AppKind::VsCode => Some((None, VIRTUAL_KEY(b'S'.into()))),
            "code-editor" if app == AppKind::VsCode => Some((None, VIRTUAL_KEY(b'1'.into()))),
            "code-chat" if app == AppKind::VsCode => {
                Some((Some(VK_MENU), VIRTUAL_KEY(b'I'.into())))
            }
            "code-terminal" if app == AppKind::VsCode => Some((Some(VK_SHIFT), VK_OEM_3)),
            _ => None,
        };
        if let Some((extra, key)) = chord {
            let keys: Vec<_> = [
                Some((VK_CONTROL, false)),
                extra.map(|key| (key, false)),
                Some((key, false)),
                Some((key, true)),
                extra.map(|key| (key, true)),
                Some((VK_CONTROL, true)),
            ]
            .into_iter()
            .flatten()
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
            if !focus()
                || unsafe { SendInput(&keys, std::mem::size_of::<INPUT>() as i32) } as usize
                    != keys.len()
            {
                return Err("could not undo in fixture".into());
            }
            return Ok(());
        }
        if app == AppKind::VsCode
            && matches!(mode.as_str(), "code-shape" | "code-read" | "code-empty")
        {
            let check = match mode.as_str() {
                "code-read" => Some("office-fixture"),
                "code-empty" => Some(""),
                _ => None,
            };
            if !code_shape(pid, check) {
                return Err("fixture UIA readback failed".into());
            }
            if check.is_some() {
                println!("fixture UIA readback exact");
            }
            return Ok(());
        }
        let probe = UiaTargetProbe::new();
        let started = Instant::now();
        let verdict = probe.capture();
        let capture_ms = started.elapsed().as_secs_f64() * 1000.0;
        println!("capture_ms={capture_ms:.1}");
        let TargetVerdict::Eligible { token, info } = verdict else {
            println!("verdict={verdict:?}");
            return Ok(());
        };
        println!(
            "app={:?} verdict=Eligible multiline={:?}",
            info.app, info.multiline
        );
        if info.app != app || !focus() {
            return Err("target is not the expected fixture".into());
        }
        if mode == "probe" {
            return Ok(());
        }
        let _watch = probe.watch(&token, Box::new(|_| {}));
        let text = match mode.as_str() {
            "one" => "office-fixture".to_owned(),
            "two" | "packet-lf" => "office-fixture\nsecond-fixture".to_owned(),
            "u200" | "u200-pair" => format!(
                "{HEAD}{}",
                "àb"
                    .chars()
                    .cycle()
                    .take(200 - HEAD.encode_utf16().count())
                    .collect::<String>()
            ),
            _ => return Err("invalid fixture mode".into()),
        };
        let start = Instant::now();
        let inserter = match mode.as_str() {
            "packet-lf" => SendInputInserter::paced_fixture(false, true),
            "u200-pair" => SendInputInserter::paced_fixture(true, false),
            _ => SendInputInserter::new(),
        };
        let out = inserter.insert_once(&token, &text);
        println!(
            "outcome={out:?} elapsed_ms={:.1}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> Result<(), String> {
    win::main()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows fixture only");
}
