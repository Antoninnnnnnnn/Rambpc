use super::target;
use crate::ports::{InsertOutcome, Inserter, NotWrittenReason, TargetToken};
use std::{
    thread,
    time::{Duration, Instant},
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

const MAX_UNITS: usize = 4096;

#[derive(Default)]
pub struct SendInputInserter;
impl SendInputInserter {
    pub fn new() -> Self {
        Self
    }
}

fn modifiers_held() -> bool {
    [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|key| unsafe { GetAsyncKeyState(i32::from(key.0)) } < 0)
}

fn inputs(text: &str) -> Option<Vec<INPUT>> {
    // The two Unicode line separators are not classified as control characters.
    if text.is_empty()
        || text.len() > MAX_UNITS * 3
        || text
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}'))
    {
        return None;
    }
    let units = text.encode_utf16().count();
    if units > MAX_UNITS {
        return None;
    }
    let mut batch = Vec::with_capacity(units * 2);
    for unit in text.encode_utf16() {
        for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
            batch.push(INPUT {
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
            });
        }
    }
    Some(batch)
}

impl Inserter for SendInputInserter {
    fn insert_once(&self, token: &TargetToken, text: &str) -> InsertOutcome {
        let not_written = |reason| InsertOutcome::NotWritten { reason };
        let Some(batch) = inputs(text) else {
            return not_written(NotWrittenReason::Unsupported);
        };
        let deadline = Instant::now() + Duration::from_millis(1500);
        while modifiers_held() {
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
        // Windows offers no atomic operation that binds SendInput to this HWND.
        // This is the sole attempt; a partial acceptance is never retried.
        match unsafe { SendInput(&batch, std::mem::size_of::<INPUT>() as i32) } as usize {
            0 => not_written(NotWrittenReason::Refused),
            count if count == batch.len() => InsertOutcome::Delivered,
            _ => InsertOutcome::Partial,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_units_have_adjacent_down_up_pairs() {
        let batch = inputs("é🦀").unwrap();
        assert_eq!(batch.len(), 6);
        for (pair, expected) in batch.chunks_exact(2).zip([0x00e9, 0xd83e, 0xdd80]) {
            for (entry, flags) in pair
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
    fn only_bounded_single_line_text_is_admitted() {
        for rejected in [
            "",
            "a\r",
            "a\n",
            "a\t",
            "a\u{1b}",
            "a\0",
            "a\u{85}",
            "a\u{2028}",
            "a\u{2029}",
        ] {
            assert!(inputs(rejected).is_none());
        }
        assert!(inputs(&"é".repeat(MAX_UNITS)).is_some());
        assert!(inputs(&"é".repeat(MAX_UNITS + 1)).is_none());
        assert!(inputs(&"🦀".repeat(MAX_UNITS / 2)).is_some());
        assert!(inputs(&"🦀".repeat(MAX_UNITS / 2 + 1)).is_none());
    }
}
