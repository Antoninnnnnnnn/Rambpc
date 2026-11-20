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
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    };

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
        if mode == "undo" || (mode == "save" && app == AppKind::VsCode) {
            let z = VIRTUAL_KEY(if mode == "save" { b'S' } else { b'Z' }.into());
            let keys = [
                (VK_CONTROL, false),
                (z, false),
                (z, true),
                (VK_CONTROL, true),
            ]
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
            });
            if !focus()
                || unsafe { SendInput(&keys, std::mem::size_of::<INPUT>() as i32) } as usize
                    != keys.len()
            {
                return Err("could not undo in fixture".into());
            }
            return Ok(());
        }
        let probe = UiaTargetProbe::new();
        let verdict = probe.capture();
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
