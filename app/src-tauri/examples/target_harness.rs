//! Runs against a separate process containing only sacrificial Win32 Edit controls.

#[cfg(windows)]
#[path = "insertion_support/stress.rs"]
mod stress;

#[cfg(windows)]
mod fixture {
    use sambler_lib::{
        platform::windows::{SendInputInserter, UiaTargetProbe},
        ports::{
            InsertOutcome, Inserter, NotWrittenReason, TargetChange, TargetProbe, TargetToken,
            TargetVerdict, UncertainReason,
        },
    };
    use std::{
        process::{Child, Command, Stdio},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::{GetModuleHandleW, LoadLibraryW},
        UI::{
            Controls::{EM_GETSEL, EM_SETSEL},
            Input::KeyboardAndMouse::SetFocus,
            WindowsAndMessaging::*,
        },
    };
    use windows::core::{PCWSTR, w};

    const CLASS: PCWSTR = w!("SamblerPlatformEditFixtureV1");
    const SELECT: u32 = WM_APP + 1;
    const ARM_FOCUS_SWITCH: u32 = WM_APP + 2;
    const SINGLE: i32 = 100;
    const MULTI: i32 = 101;
    const PASSWORD: i32 = 102;
    const RICH: i32 = 103;
    const PHRASE: &str = "Rendez-vous jeudi à 14 h";
    const UNICODE: &str = " é–🦀";

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == ARM_FOCUS_SWITCH {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 1) };
            return LRESULT(1);
        }
        if msg == WM_COMMAND
            && w.0 & 0xffff == MULTI as usize
            && w.0 >> 16 == EN_CHANGE as usize
            && unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 1
        {
            // Switch while the first character is being handled, before the
            // paced sender can observe the caret acknowledgement and continue.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            if let Ok(other) = unsafe { GetDlgItem(Some(hwnd), RICH) } {
                let _ = unsafe { SetFocus(Some(other)) };
            }
        }
        if msg == SELECT {
            let (id, selection) = match w.0 {
                0 => (SINGLE, (0, 0)),
                1 => (MULTI, (0, 0)),
                2 => (PASSWORD, (0, 0)),
                3 => (SINGLE, (0, 2)),
                4 => (SINGLE, (2, 2)),
                5 => (RICH, (0, 0)),
                6 => (MULTI, (0, 2)),
                _ => return LRESULT(0),
            };
            if let Ok(edit) = unsafe { GetDlgItem(Some(hwnd), id) } {
                unsafe {
                    let _ = SetFocus(Some(edit));
                    SendMessageW(
                        edit,
                        EM_SETSEL,
                        Some(WPARAM(selection.0)),
                        Some(LPARAM(selection.1)),
                    );
                }
                return LRESULT(1);
            }
            return LRESULT(0);
        }
        if msg == WM_DESTROY {
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, w, l) }
    }

    fn window() {
        unsafe {
            // Keep the control implementation loaded for the fixture process.
            LoadLibraryW(w!("Msftedit.dll")).unwrap();
            let instance = GetModuleHandleW(None).unwrap().into();
            assert_ne!(
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(wndproc),
                    hInstance: instance,
                    lpszClassName: CLASS,
                    ..Default::default()
                }),
                0
            );
            let hwnd = CreateWindowExW(
                Default::default(),
                CLASS,
                w!("Sambler test fixture"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                500,
                360,
                None,
                None,
                Some(instance),
                None,
            )
            .unwrap();
            for (id, y, style, height) in [
                (SINGLE, 12, ES_AUTOHSCROLL, 28),
                (MULTI, 50, ES_MULTILINE, 87),
                (PASSWORD, 148, ES_PASSWORD, 28),
                (RICH, 188, ES_MULTILINE, 90),
            ] {
                CreateWindowExW(
                    WS_EX_CLIENTEDGE,
                    if id == RICH {
                        w!("RICHEDIT50W")
                    } else {
                        w!("Edit")
                    },
                    PCWSTR::null(),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(style as u32),
                    12,
                    y,
                    450,
                    height,
                    Some(hwnd),
                    Some(HMENU(id as usize as _)),
                    Some(instance),
                    None,
                )
                .unwrap();
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            SendMessageW(hwnd, SELECT, Some(WPARAM(0)), None);
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn message(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> usize {
        let mut result = 0;
        assert_ne!(
            unsafe {
                SendMessageTimeoutW(
                    hwnd,
                    msg,
                    w,
                    l,
                    SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                    300,
                    Some(&mut result),
                )
            }
            .0,
            0,
            "fixture message timed out"
        );
        result
    }

    fn wait_for(mut predicate: impl FnMut() -> bool, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        while Instant::now() < until {
            if predicate() {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn edit_text(hwnd: HWND) -> String {
        let mut text = [0u16; 1024];
        let len = message(
            hwnd,
            WM_GETTEXT,
            WPARAM(text.len()),
            LPARAM(text.as_mut_ptr() as isize),
        );
        String::from_utf16(&text[..len]).unwrap()
    }

    fn capture(target: &UiaTargetProbe, multiline: bool) -> TargetToken {
        match target.capture() {
            TargetVerdict::Eligible { token, info } => {
                assert_eq!(info.multiline, Some(multiline));
                token
            }
            verdict => panic!("expected Eligible, got {verdict:?}"),
        }
    }

    pub fn main() {
        if std::env::args().nth(1).as_deref() == Some("--fixture") {
            return window();
        }
        let child = ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .arg("--fixture")
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let _ = unsafe { AllowSetForegroundWindow(child.0.id()) };
        let mut hwnd = HWND::default();
        assert!(
            wait_for(
                || {
                    let foreground = unsafe { GetForegroundWindow() };
                    let mut pid = 0;
                    if foreground.0.is_null() {
                        return false;
                    }
                    unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid)) };
                    if pid != child.0.id() {
                        return false;
                    }
                    hwnd = foreground;
                    true
                },
                Duration::from_secs(5)
            ),
            "fixture did not get foreground focus"
        );
        let single = unsafe { GetDlgItem(Some(hwnd), SINGLE) }.unwrap();
        let target = UiaTargetProbe::new();
        let inserter = SendInputInserter::new();
        let select = |case| {
            assert_eq!(message(hwnd, SELECT, WPARAM(case), LPARAM(0)), 1);
            thread::sleep(Duration::from_millis(50));
        };

        select(2);
        assert!(matches!(target.capture(), TargetVerdict::Protected));
        println!("password: Protected");
        select(1);
        let _ = capture(&target, true);
        println!("multiline caret: Eligible");
        select(0);
        let token = capture(&target, false);
        println!("single-line caret: Eligible");
        assert_eq!(
            inserter.insert_once(&token, PHRASE),
            InsertOutcome::Delivered
        );
        assert!(
            wait_for(|| edit_text(single) == PHRASE, Duration::from_secs(1)),
            "phrase readback mismatch"
        );
        thread::sleep(Duration::from_millis(100));
        assert!(
            edit_text(single) == PHRASE,
            "phrase appeared more than once"
        );
        println!("phrase: Delivered, exact single readback");

        let unicode_token = capture(&target, false);
        assert_eq!(
            inserter.insert_once(&unicode_token, UNICODE),
            InsertOutcome::Delivered
        );
        let expected = format!("{PHRASE}{UNICODE}");
        assert!(
            wait_for(|| edit_text(single) == expected, Duration::from_secs(1)),
            "Unicode readback mismatch"
        );
        println!("accents and emoji: exact readback");

        select(3);
        let selected = capture(&target, false);
        assert!(target.revalidate(&selected).is_ok());
        select(4);
        assert_eq!(
            target.revalidate(&selected),
            Err(TargetChange::SelectionChanged)
        );
        println!("single-line selection: Eligible; changed endpoints refused");

        select(1);
        let multi = unsafe { GetDlgItem(Some(hwnd), MULTI) }.unwrap();
        super::stress::set_contents(multi, "ab").unwrap();
        select(6);
        let verdict = target.capture();
        assert!(
            matches!(
                verdict,
                TargetVerdict::Uncertain {
                    reason: UncertainReason::Selection
                }
            ),
            "selection verdict: {verdict:?}"
        );
        println!("nonempty selection: Uncertain(Selection)");

        select(0);
        let token = capture(&target, false);
        let (tx, rx) = mpsc::channel();
        let _watch = target.watch(
            &token,
            Box::new(move |change| {
                let _ = tx.send(change);
            }),
        );
        select(4);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            TargetChange::SelectionChanged
        );
        assert!(target.revalidate(&token).is_err());
        println!("caret-only change: SelectionChanged; revalidation refused");

        let token = capture(&target, false);
        let (tx, rx) = mpsc::channel();
        let _watch = target.watch(
            &token,
            Box::new(move |change| {
                let _ = tx.send(change);
            }),
        );
        select(1);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            TargetChange::FocusMoved
        );
        select(4);
        assert!(target.revalidate(&token).is_err());
        assert_eq!(
            inserter.insert_once(&token, PHRASE),
            InsertOutcome::NotWritten {
                reason: NotWrittenReason::TargetChanged
            }
        );
        assert!(
            edit_text(single) == expected,
            "stale token wrote to the fixture"
        );
        println!("focus change and return: FocusMoved; stale insertion refused");

        select(3);
        let selected = capture(&target, false);
        assert_eq!(
            inserter.insert_once(&selected, "OK"),
            InsertOutcome::Delivered
        );
        assert!(edit_text(single) == format!("OK{}", &expected[2..]));
        let caret = message(single, EM_GETSEL, WPARAM(0), LPARAM(0));
        assert_eq!(caret, 2 | (2 << 16));
        println!("native EM_REPLACESEL: replaced selection, caret at insertion end");

        for (case, id, label) in [(1, MULTI, "Win32 Edit"), (5, RICH, "Win32 RICHEDIT50W")] {
            select(case);
            let editor = unsafe { GetDlgItem(Some(hwnd), id) }.unwrap();
            super::stress::run(
                label,
                editor,
                &target,
                || Ok(capture(&target, true)),
                || Some(edit_text(editor)),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        }

        select(1);
        let multi = unsafe { GetDlgItem(Some(hwnd), MULTI) }.unwrap();
        let rich = unsafe { GetDlgItem(Some(hwnd), RICH) }.unwrap();
        let token = capture(&target, true);
        let _watch = target.watch(&token, Box::new(|_| {}));
        assert_eq!(message(hwnd, ARM_FOCUS_SWITCH, WPARAM(0), LPARAM(0)), 1);
        assert_eq!(
            SendInputInserter::paced_for_diagnostics().insert_once(&token, &"p".repeat(200)),
            InsertOutcome::Partial
        );
        assert!(edit_text(multi) == "p" && edit_text(rich).is_empty());
        println!("paced focus interruption: Partial; no write to the new field");
    }
}

#[cfg(windows)]
fn main() {
    fixture::main();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This harness requires Windows.");
}
