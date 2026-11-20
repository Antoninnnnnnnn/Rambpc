use super::target::{self, NativeEdit};
use crate::ports::{InsertOutcome, Inserter, NotWrittenReason, TargetToken};
use std::{
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_WINDOW_HANDLE, ERROR_SUCCESS, GetLastError, HWND,
        LPARAM, SetLastError, WPARAM,
    },
    UI::{
        Controls::EM_REPLACESEL,
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
            VK_SHIFT,
        },
        WindowsAndMessaging::{
            SMTO_ABORTIFHUNG, SMTO_BLOCK, SMTO_ERRORONEXIT, SendMessageTimeoutW, WM_NULL,
        },
    },
};
use zeroize::Zeroizing;

const MAX_UNITS: usize = 4096;
const PACKET_PAUSE: Duration = Duration::from_millis(5);
const PACE_BUDGET: Duration = Duration::from_secs(32);
const MESSAGE_MS: u32 = 200;

#[derive(Default)]
pub struct SendInputInserter {
    force_paced: bool,
    fixture_pair: bool,
    fixture_lf: bool,
}
impl SendInputInserter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Exercises the keyboard path against an otherwise supported native edit.
    #[doc(hidden)]
    pub fn paced_for_diagnostics() -> Self {
        Self {
            force_paced: true,
            ..Self::default()
        }
    }

    /// Test-only knobs for an isolated fixture, not used by the application.
    #[doc(hidden)]
    pub fn paced_fixture(pair: bool, literal_lf: bool) -> Self {
        Self {
            force_paced: true,
            fixture_pair: pair,
            fixture_lf: literal_lf,
        }
    }
}

fn modifiers_held() -> bool {
    [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|key| unsafe { GetAsyncKeyState(i32::from(key.0)) } < 0)
}

#[derive(Clone, Copy)]
enum LineEnds {
    Single,
    Edit,
    Rich,
    Lf,
}

fn utf16(text: &str, lines: LineEnds) -> Option<Zeroizing<Vec<u16>>> {
    if text.is_empty() || text.len() > MAX_UNITS * 3 {
        return None;
    }
    let mut units = Zeroizing::new(Vec::with_capacity(text.len().min(MAX_UNITS) + 1));
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' | '\n' => {
                if matches!(lines, LineEnds::Single) {
                    return None;
                }
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                units.push(
                    if matches!(lines, LineEnds::Lf) {
                        b'\n'
                    } else {
                        b'\r'
                    }
                    .into(),
                );
                if matches!(lines, LineEnds::Edit) {
                    units.push(b'\n'.into());
                }
            }
            ch if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') => return None,
            ch => units.extend_from_slice(ch.encode_utf16(&mut [0; 2])),
        }
        if units.len() > MAX_UNITS {
            return None;
        }
    }
    Some(units)
}

fn packet(unit: u16) -> [INPUT; 2] {
    [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP].map(|flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    })
}

fn not_written(reason: NotWrittenReason) -> InsertOutcome {
    InsertOutcome::NotWritten { reason }
}

fn insert_native(token: &TargetToken, edit: NativeEdit, units: &mut Vec<u16>) -> InsertOutcome {
    units.push(0);
    let hwnd = edit.hwnd();
    if target::write_focus(token) != Some(hwnd)
        || !edit.writable_now(hwnd)
        || !edit.caret_unchanged()
        || target::write_focus(token) != Some(hwnd)
    {
        return not_written(NotWrittenReason::TargetChanged);
    }
    if !target::write_permitted(token) {
        return not_written(NotWrittenReason::TargetChanged);
    }
    unsafe {
        SetLastError(ERROR_SUCCESS);
        // EM_REPLACESEL is below WM_USER; USER32 marshals the UTF-16 string
        // into the target. Target this control, not the keyboard input queue.
        let sent = SendMessageTimeoutW(
            hwnd,
            EM_REPLACESEL,
            WPARAM(1),
            LPARAM(units.as_ptr() as isize),
            SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
            MESSAGE_MS,
            None,
        );
        if sent.0 != 0 {
            InsertOutcome::Delivered
        } else if matches!(
            GetLastError(),
            ERROR_ACCESS_DENIED | ERROR_INVALID_WINDOW_HANDLE
        ) {
            not_written(NotWrittenReason::Refused)
        } else {
            // A timeout can occur after the target begins handling the message.
            // Do not claim it wrote nothing or fall back to keyboard input.
            InsertOutcome::Partial
        }
    }
}

fn responsive(hwnd: HWND) -> bool {
    unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_NULL,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
            MESSAGE_MS,
            None,
        )
    }
    .0 != 0
}

fn insert_paced(
    token: &TargetToken,
    edit: Option<NativeEdit>,
    units: &[u16],
    pair: bool,
) -> InsertOutcome {
    let until = Instant::now() + PACE_BUDGET;
    let chunk_size = if pair && edit.is_none() { 2 } else { 1 };
    for (index, chunk) in units.chunks(chunk_size).enumerate() {
        let unit = chunk[0];
        let stop = |reason| {
            if index == 0 {
                not_written(reason)
            } else {
                InsertOutcome::Partial
            }
        };
        let Some(hwnd) = target::write_focus(token) else {
            return stop(NotWrittenReason::TargetChanged);
        };
        if index > 0 {
            // A sent WM_NULL checks responsiveness; it is not a queue-drain
            // acknowledgement. Give GetMessage/TranslateMessage a turn too.
            if !responsive(hwnd) {
                return InsertOutcome::Partial;
            }
            thread::sleep(PACKET_PAUSE);
        }
        let same_field = match edit {
            Some(edit) => edit.writable_now(hwnd) && (index != 0 || edit.caret_unchanged()),
            // Chromium/Office can move between distinct fields on one HWND.
            None => target::same_uia_focus(token),
        };
        if !same_field || target::write_focus(token) != Some(hwnd) {
            return stop(NotWrittenReason::TargetChanged);
        }
        if modifiers_held() {
            return stop(NotWrittenReason::ModifiersHeld);
        }
        if Instant::now() >= until {
            return stop(NotWrittenReason::Refused);
        }
        // Even adjacent surrogate halves are paced individually: two VK_PACKET
        // values queued before translation can both resolve to the later unit.
        let first = packet(unit);
        let batch = if let [_, second] = chunk {
            let second = packet(*second);
            [first[0], first[1], second[0], second[1]]
        } else {
            [first[0], first[1], INPUT::default(), INPUT::default()]
        };
        let events = &batch[..2 * chunk.len()];
        if !target::write_permitted(token) {
            return stop(NotWrittenReason::TargetChanged);
        }
        match unsafe { SendInput(events, std::mem::size_of::<INPUT>() as i32) } as usize {
            n if n == events.len() => {}
            0 => return stop(NotWrittenReason::Refused),
            _ => return InsertOutcome::Partial,
        }
        // In diagnostics the fallback can additionally observe the trusted
        // native caret. Await a complete scalar, not a lone high surrogate.
        // This reads only the selection index, never the inserted text.
        if let Some(edit) = edit.filter(|_| !(0xd800..=0xdbff).contains(&unit)) {
            let ack_deadline = until.min(Instant::now() + Duration::from_millis(200));
            loop {
                if target::write_focus(token) != Some(hwnd) || Instant::now() >= ack_deadline {
                    return InsertOutcome::Partial;
                }
                if edit.caret_advanced((index + 1) as u32) {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
    }
    InsertOutcome::Delivered
}

impl Inserter for SendInputInserter {
    fn insert_once(&self, token: &TargetToken, text: &str) -> InsertOutcome {
        let edit = target::native_edit(token);
        let direct = edit.filter(|_| !self.force_paced);
        let office = !self.force_paced && target::office_body(token);
        let lines = match direct {
            _ if office => LineEnds::Rich,
            _ if self.fixture_lf => LineEnds::Lf,
            Some(edit) if edit.multiline && edit.rich => LineEnds::Rich,
            Some(edit) if edit.multiline => LineEnds::Edit,
            _ => LineEnds::Single,
        };
        let Some(mut units) = utf16(text, lines) else {
            return not_written(NotWrittenReason::Unsupported);
        };
        let deadline = Instant::now() + Duration::from_millis(1500);
        while modifiers_held() {
            if !target::write_permitted(token) {
                return not_written(NotWrittenReason::TargetChanged);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return not_written(NotWrittenReason::ModifiersHeld);
            }
            thread::sleep(remaining.min(Duration::from_millis(10)));
        }
        if target::revalidate_token(token).is_err() {
            return not_written(NotWrittenReason::TargetChanged);
        }
        if modifiers_held() {
            return not_written(NotWrittenReason::ModifiersHeld);
        }
        if !target::arm_single_write(token) {
            return not_written(NotWrittenReason::TargetChanged);
        }
        // One logical attempt, with no fallback or retry after any write call.
        if office {
            return target::insert_office(token, units);
        }
        match direct {
            Some(edit) => insert_native(token, edit, &mut units),
            None => insert_paced(token, edit, &units, self.fixture_pair),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_unicode_unit_is_a_separate_down_up_packet() {
        let units = utf16("é🦀", LineEnds::Single).unwrap();
        assert_eq!(&units[..], &[0x00e9, 0xd83e, 0xdd80]);
        for &expected in units.iter() {
            for (entry, flags) in packet(expected)
                .iter()
                .zip([KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP])
            {
                assert_eq!(entry.r#type, INPUT_KEYBOARD);
                let key = unsafe { entry.Anonymous.ki };
                assert_eq!(key.wVk, VIRTUAL_KEY(0));
                assert_eq!(key.wScan, expected);
                assert_eq!(key.dwFlags, flags);
            }
        }
    }

    #[test]
    fn text_admissibility_and_line_endings_are_path_specific() {
        for rejected in [
            "",
            "a\t",
            "a\u{1b}",
            "a\0",
            "a\u{85}",
            "a\u{2028}",
            "a\u{2029}",
        ] {
            for lines in [LineEnds::Single, LineEnds::Edit, LineEnds::Rich] {
                assert!(utf16(rejected, lines).is_none());
            }
        }
        assert!(utf16("a\nb", LineEnds::Single).is_none());
        for (lines, exact) in [
            (LineEnds::Edit, "a\r\nb\r\nc\r\nd"),
            (LineEnds::Rich, "a\rb\rc\rd"),
        ] {
            let got = utf16("a\nb\r\nc\rd", lines).unwrap();
            assert_eq!(String::from_utf16(&got).unwrap(), exact);
        }
        assert!(utf16(&"é".repeat(MAX_UNITS), LineEnds::Single).is_some());
        assert!(utf16(&"é".repeat(MAX_UNITS + 1), LineEnds::Single).is_none());
        assert!(utf16(&"🦀".repeat(MAX_UNITS / 2), LineEnds::Single).is_some());
        assert!(utf16(&"🦀".repeat(MAX_UNITS / 2 + 1), LineEnds::Single).is_none());
        assert!(utf16(&"\n".repeat(MAX_UNITS / 2 + 1), LineEnds::Edit).is_none());
    }
}
