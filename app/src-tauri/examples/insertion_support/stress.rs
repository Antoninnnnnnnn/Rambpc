//! Offline insertion stress cases for explicitly owned, sacrificial controls.

use sambler_lib::{
    platform::windows::SendInputInserter,
    ports::{InsertOutcome, Inserter, TargetProbe, TargetToken},
};
use std::{
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{
        Controls::EM_SETSEL,
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
        },
        WindowsAndMessaging::*,
    },
};

const LEFT: &str = "[before]";
const RIGHT: &str = "[after]";
const SEED: &str = "[before][after]";
const REPETITIONS: usize = 20;
const UNITS: usize = 200;

fn focused(editor: HWND) -> bool {
    let root = unsafe { GetAncestor(editor, GA_ROOT) };
    let owner = unsafe { GetWindowThreadProcessId(editor, None) };
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    root == unsafe { GetForegroundWindow() }
        && owner != 0
        && unsafe { GetGUIThreadInfo(owner, &mut info) }.is_ok()
        && info.hwndFocus == editor
}

fn send(editor: HWND, msg: u32, w: WPARAM, l: LPARAM) -> bool {
    unsafe {
        SendMessageTimeoutW(
            editor,
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

pub fn set_contents(editor: HWND, text: &str) -> Result<(), String> {
    if !focused(editor) {
        return Err("fixture editor lost focus".into());
    }
    let units: Vec<_> = text.encode_utf16().chain([0]).collect();
    if !send(
        editor,
        WM_SETTEXT,
        WPARAM(0),
        LPARAM(units.as_ptr() as isize),
    ) {
        return Err("could not set fixture contents".into());
    }
    Ok(())
}

fn undo(editor: HWND) -> bool {
    if !focused(editor)
        || [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|key| unsafe { GetAsyncKeyState(key.0.into()) } < 0)
    {
        return false;
    }
    let z = VIRTUAL_KEY(b'Z'.into());
    let batch = [
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
    (unsafe { SendInput(&batch, std::mem::size_of::<INPUT>() as i32) }) as usize == batch.len()
}

fn wait_for_text(read: &mut impl FnMut() -> Option<String>, expected: &str) -> Result<(), String> {
    let until = Instant::now() + Duration::from_millis(650);
    loop {
        let actual = read().map(|s| s.replace("\r\n", "\n").replace('\r', "\n"));
        if actual.as_deref() == Some(expected) {
            return Ok(());
        }
        if Instant::now() >= until {
            let Some(actual) = actual else {
                return Err("readback unavailable".into());
            };
            let prefix = actual
                .encode_utf16()
                .zip(expected.encode_utf16())
                .take_while(|(a, b)| a == b)
                .count();
            return Err(format!(
                "readback mismatch: {} UTF-16 units, {prefix} matching prefix units",
                actual.encode_utf16().count()
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn text(multiline: bool) -> String {
    let break_or_space = if multiline { '\n' } else { ' ' };
    let initial = format!(
        "Éléonore dit : « À jeudi, c’est 14 h. » Café à 5 € 🦀.{break_or_space}Let’s meet on Thursday; the exact amount is five euros. Déjà vu, naïve façade, coöperate."
    );
    let text = format!(
        "{initial} {}!",
        "àb"
            .chars()
            .cycle()
            .take(UNITS - initial.encode_utf16().count() - 2)
            .collect::<String>()
    );
    assert_eq!(text.encode_utf16().count(), UNITS);
    text
}

pub fn run(
    label: &str,
    editor: HWND,
    probe: &impl TargetProbe,
    mut capture: impl FnMut() -> Result<TargetToken, String>,
    mut read: impl FnMut() -> Option<String>,
) -> Result<(), String> {
    let native = SendInputInserter::new();
    let paced = SendInputInserter::paced_for_diagnostics();
    for (name, inserter, multiline) in [
        ("literal single-line", &native, false),
        ("literal two-line", &native, true),
        ("forced paced", &paced, false),
    ] {
        let text = text(multiline);
        let expected = format!("{LEFT}{text}{RIGHT}");
        let mut timings = Vec::with_capacity(REPETITIONS);
        for attempt in 1..=REPETITIONS {
            set_contents(editor, SEED)?;
            if !send(
                editor,
                EM_SETSEL,
                WPARAM(LEFT.len()),
                LPARAM(LEFT.len() as isize),
            ) {
                return Err("could not place fixture caret".into());
            }
            let token = capture()?;
            // Match the real controller: the pre-write watch guard remains live.
            let _watch = probe.watch(&token, Box::new(|_| {}));
            let started = Instant::now();
            let outcome = inserter.insert_once(&token, &text);
            let elapsed = started.elapsed();
            if outcome != InsertOutcome::Delivered {
                return Err(format!("{label} {name} run {attempt}: {outcome:?}"));
            }
            wait_for_text(&mut read, &expected)
                .map_err(|err| format!("{label} {name} run {attempt}: {err}"))?;
            if name.starts_with("literal") {
                if !undo(editor) {
                    return Err("native Ctrl+Z not accepted".into());
                }
                wait_for_text(&mut read, SEED)
                    .map_err(|err| format!("{label} {name} undo run {attempt}: {err}"))?;
            }
            timings.push(elapsed.as_secs_f64() * 1000.0 * 100.0 / UNITS as f64);
        }
        if name == "forced paced" {
            timings.sort_by(f64::total_cmp);
            let median = (timings[REPETITIONS / 2 - 1] + timings[REPETITIONS / 2]) / 2.0;
            println!(
                "{label} {name}: {REPETITIONS}/{REPETITIONS} exact; {median:.1} ms/100 UTF-16 units median ({:.1}–{:.1})",
                timings[0],
                timings[REPETITIONS - 1]
            );
        } else {
            println!(
                "{label} {name}: {REPETITIONS}/{REPETITIONS} exact; native Ctrl+Z {REPETITIONS}/{REPETITIONS}"
            );
        }
    }
    set_contents(editor, "")
}
