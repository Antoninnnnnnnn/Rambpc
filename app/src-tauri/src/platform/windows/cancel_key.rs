//! A take-scoped low-level keyboard hook. No callbacks run on the hook thread.

use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle, Thread},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::VK_ESCAPE, WindowsAndMessaging::*},
};

pub struct CancelKeyHook;

pub struct CancelKeyHookGuard {
    pump: Option<(u32, JoinHandle<()>)>,
    callback: Option<JoinHandle<()>>,
    signal: Arc<Signal>,
}

#[derive(Default)]
struct Signal {
    fired: AtomicBool,
    stopped: AtomicBool,
    swallowed_down: AtomicBool,
}

struct HookState {
    signal: Arc<Signal>,
    callback_thread: Thread,
    escape_down: Cell<bool>,
    allow_injected: bool,
}

thread_local! {
    static HOOK: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Pass,
    Swallow,
    Cancel,
}

fn transition(
    down: &Cell<bool>,
    vk: u32,
    message: u32,
    injected: bool,
    allow_injected: bool,
) -> Action {
    if vk != u32::from(VK_ESCAPE.0) || (injected && !allow_injected) {
        return Action::Pass;
    }
    match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if down.replace(true) {
                Action::Swallow
            } else {
                Action::Cancel
            }
        }
        WM_KEYUP | WM_SYSKEYUP if down.replace(false) => Action::Swallow,
        _ => Action::Pass,
    }
}

unsafe extern "system" fn hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && l.0 != 0 {
        let key = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
        let consumed = HOOK.with_borrow(|state| {
            let Some(state) = state else { return false };
            let stopping = state.signal.stopped.load(Ordering::Acquire);
            if stopping && !state.escape_down.get() {
                return false;
            }
            let action = transition(
                &state.escape_down,
                key.vkCode,
                w.0 as u32,
                key.flags.contains(LLKHF_INJECTED),
                state.allow_injected,
            );
            state
                .signal
                .swallowed_down
                .store(state.escape_down.get(), Ordering::Release);
            match action {
                Action::Pass => false,
                Action::Swallow => true,
                Action::Cancel => {
                    // A single coalesced notification; no allocation, wait, or user code.
                    state.signal.fired.store(true, Ordering::Release);
                    state.callback_thread.unpark();
                    true
                }
            }
        });
        if consumed {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}

impl CancelKeyHook {
    pub fn start(on_cancel: Box<dyn Fn() + Send + Sync>) -> CancelKeyHookGuard {
        Self::start_inner(on_cancel, false)
    }

    fn start_inner(
        on_cancel: Box<dyn Fn() + Send + Sync>,
        allow_injected: bool,
    ) -> CancelKeyHookGuard {
        let signal = Arc::new(Signal::default());
        let mut guard = CancelKeyHookGuard {
            signal: signal.clone(),
            pump: None,
            callback: None,
        };
        let callback_signal = signal.clone();
        let Ok(callback) = thread::Builder::new()
            .name("sambler-cancel-dispatch".into())
            .spawn(move || {
                loop {
                    if callback_signal.fired.swap(false, Ordering::AcqRel) {
                        on_cancel();
                    }
                    if callback_signal.stopped.load(Ordering::Acquire) {
                        break;
                    }
                    thread::park();
                }
            })
        else {
            return guard;
        };
        let callback_thread = callback.thread().clone();
        guard.callback = Some(callback);

        let (ready, started) = mpsc::sync_channel(1);
        let Ok(pump) = thread::Builder::new()
            .name("sambler-cancel-key".into())
            .spawn(move || {
                let mut message = MSG::default();
                let _ = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) };
                let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
                    return;
                };
                let Ok(handle) = (unsafe {
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), Some(module.into()), 0)
                }) else {
                    return;
                };
                HOOK.with_borrow_mut(|slot| {
                    *slot = Some(HookState {
                        signal: signal.clone(),
                        callback_thread,
                        escape_down: Cell::new(false),
                        allow_injected,
                    })
                });
                if ready.send(unsafe { GetCurrentThreadId() }).is_ok() {
                    // The timer also wakes the pump if posting the quit message fails.
                    let fallback = unsafe { SetTimer(None, 0, 250, None) };
                    let mut drain_until = None;
                    loop {
                        if signal.stopped.load(Ordering::Acquire) {
                            // No new cancel can be armed after drop. Pair a
                            // swallowed down with its up (or a bounded timeout).
                            let deadline = drain_until
                                .get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
                            if fallback == 0
                                || !signal.swallowed_down.load(Ordering::Acquire)
                                || Instant::now() >= *deadline
                            {
                                break;
                            }
                        }
                        if unsafe { GetMessageW(&mut message, None, 0, 0) }.0 <= 0 {
                            break;
                        }
                        unsafe { DispatchMessageW(&message) };
                    }
                    if fallback != 0 {
                        let _ = unsafe { KillTimer(None, fallback) };
                    }
                }
                let _ = unsafe { UnhookWindowsHookEx(handle) };
                HOOK.with_borrow_mut(|slot| *slot = None);
            })
        else {
            guard.stop();
            return guard;
        };
        if let Ok(id) = started.recv_timeout(Duration::from_secs(1)) {
            guard.pump = Some((id, pump));
        } else {
            // Do not block on a late system call. Its receiver is gone and
            // the worker will unhook as soon as it returns.
            guard.stop();
        }
        guard
    }
}

impl CancelKeyHookGuard {
    pub fn is_active(&self) -> bool {
        self.pump
            .as_ref()
            .is_some_and(|(_, worker)| !worker.is_finished())
    }

    fn stop(&mut self) {
        self.signal.stopped.store(true, Ordering::Release);
        if let Some((id, pump)) = self.pump.take() {
            if !pump.is_finished() {
                let _ = unsafe { PostThreadMessageW(id, WM_NULL, WPARAM(0), LPARAM(0)) };
            }
            if !self.signal.swallowed_down.load(Ordering::Acquire) {
                let _ = pump.join();
            }
        }
        if let Some(callback) = self.callback.take() {
            // The arbitrary callback may be waiting on the shell thread that
            // is dropping this guard. Never join it from the guard.
            callback.thread().unpark();
        }
    }
}

impl Drop for CancelKeyHookGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancels_on_down_swallows_repeat_and_matching_up() {
        let state = Cell::new(false);
        let escape = u32::from(VK_ESCAPE.0);
        assert_eq!(
            transition(&state, escape, WM_SYSKEYDOWN, false, false),
            Action::Cancel
        );
        assert_eq!(
            transition(&state, escape, WM_KEYDOWN, false, false),
            Action::Swallow
        );
        assert_eq!(
            transition(&state, escape, WM_KEYUP, true, false),
            Action::Pass
        );
        assert_eq!(
            transition(&state, escape, WM_SYSKEYUP, false, false),
            Action::Swallow
        );
        assert_eq!(
            transition(&state, escape, WM_KEYUP, false, false),
            Action::Pass
        );
        assert_eq!(
            transition(&state, escape, WM_KEYDOWN, true, false),
            Action::Pass
        );
        assert_eq!(
            transition(&state, u32::from(b'A'), WM_KEYDOWN, false, false),
            Action::Pass
        );
        assert_eq!(
            transition(&state, escape, WM_KEYDOWN, true, true),
            Action::Cancel
        );
    }

    #[test]
    fn registers_and_unregisters_a_real_take_scoped_hook() {
        let hook = CancelKeyHook::start(Box::new(|| {}));
        assert!(hook.is_active());
        drop(hook);
    }
}
