//! Acceptance harness for the RUNNING Sambler app (NOT part of the shipped
//! app): the real global shortcut, the real microphone, the real Rambler
//! service and a fresh Windows 11 Notepad window. The harness presses the
//! shortcut through SendInput like a keyboard, plays the WAV on the speakers,
//! releases, then reads the Notepad window it created.
//!
//!   cargo run --example recette_app -- <wav> <hold|toggle|toggle-escape|silence|switch>
//!
//! Sambler must already run with consent and Rambler access, with the gesture
//! matching the scenario (hold or toggle) and the default Ctrl+Shift+F2.

#[cfg(windows)]
mod recette {
    use std::thread;
    use std::time::{Duration, Instant};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
        VK_CONTROL, VK_ESCAPE, VK_F2, VK_SHIFT,
    };

    #[path = "../recette_support/notepad.rs"]
    mod notepad;

    fn keys(events: &[(VIRTUAL_KEY, bool)]) {
        let batch: Vec<INPUT> = events
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
        unsafe { SendInput(&batch, std::mem::size_of::<INPUT>() as i32) };
    }

    fn press_shortcut() {
        keys(&[(VK_CONTROL, false), (VK_SHIFT, false), (VK_F2, false)]);
    }
    fn release_shortcut() {
        keys(&[(VK_F2, true), (VK_SHIFT, true), (VK_CONTROL, true)]);
    }
    fn tap_shortcut() {
        press_shortcut();
        thread::sleep(Duration::from_millis(80));
        release_shortcut();
    }

    fn play(path: &str) {
        let script = format!("(New-Object Media.SoundPlayer '{path}').PlaySync()");
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .status();
    }

    fn wait_text(pad: &notepad::Fixture, timeout: Duration) -> Option<(String, u128)> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if let Ok(text) = pad.contents() {
                if !text.is_empty() {
                    // Let the editor settle, then read again.
                    thread::sleep(Duration::from_millis(300));
                    return Some((pad.contents().unwrap_or(text), start.elapsed().as_millis()));
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        None
    }

    pub fn main(args: Vec<String>) -> Result<(), String> {
        let wav = args.get(1).ok_or("usage: <wav> <scenario>")?.clone();
        let scenario = args.get(2).cloned().unwrap_or_else(|| "hold".into());
        let pad = notepad::Fixture::open()?;
        pad.focus()?;
        thread::sleep(Duration::from_millis(400));
        let result = (|| -> Result<String, String> {
            match scenario.as_str() {
                "hold" => {
                    press_shortcut();
                    thread::sleep(Duration::from_millis(700)); // microphone opening
                    play(&wav);
                    thread::sleep(Duration::from_millis(400));
                    release_shortcut();
                }
                "toggle" => {
                    tap_shortcut();
                    thread::sleep(Duration::from_millis(700));
                    play(&wav);
                    thread::sleep(Duration::from_millis(400));
                    tap_shortcut();
                }
                "toggle-escape" => {
                    tap_shortcut();
                    thread::sleep(Duration::from_millis(700));
                    play(&wav);
                    keys(&[(VK_ESCAPE, false), (VK_ESCAPE, true)]);
                    thread::sleep(Duration::from_secs(5));
                    let text = pad.contents()?;
                    return if text.is_empty() {
                        Ok("cancelled with Escape: field untouched after 5 s".into())
                    } else {
                        Err(format!("text written after Escape: {text:?}"))
                    };
                }
                "switch" => {
                    press_shortcut();
                    thread::sleep(Duration::from_millis(700));
                    play(&wav);
                    thread::sleep(Duration::from_millis(400));
                    release_shortcut();
                    // Change window while Rambler finalizes.
                    pad.open_second()?;
                    thread::sleep(Duration::from_secs(5));
                    let (first, second) = (pad.contents()?, pad.second_contents()?);
                    return if first.is_empty() && second.is_empty() {
                        Ok("both windows untouched; text expected in Sambler".into())
                    } else {
                        Err(format!("written after switch: first={first:?} second={second:?}"))
                    };
                }
                "silence" => {
                    press_shortcut();
                    thread::sleep(Duration::from_millis(2500));
                    release_shortcut();
                    thread::sleep(Duration::from_secs(4));
                    let text = pad.contents()?;
                    return if text.is_empty() {
                        Ok("silence: nothing inserted".into())
                    } else {
                        Err(format!("text inserted from silence: {text:?}"))
                    };
                }
                other => return Err(format!("unknown scenario {other}")),
            }
            match wait_text(&pad, Duration::from_secs(10)) {
                Some((text, ms)) => Ok(format!("field={text:?}; visible {ms} ms after the stop")),
                None => Err("no text in the field within 10 s".into()),
            }
        })();
        let cleanup = pad.close();
        match result {
            Ok(report) => {
                println!("[OK]   {scenario}: {report}; cleanup={cleanup}");
                Ok(())
            }
            Err(error) => {
                println!("[FAIL] {scenario}: {error}; cleanup={cleanup}");
                Err(error)
            }
        }
    }
}

#[cfg(windows)]
fn main() {
    if recette::main(std::env::args().collect()).is_err() {
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This harness requires Windows.");
}
