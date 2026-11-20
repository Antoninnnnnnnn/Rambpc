use crate::{
    model::SystemEvent,
    ports::{Subscription, SystemEvents},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        RemoteDesktop::{
            NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
            WTSUnRegisterSessionNotification,
        },
        Threading::GetCurrentThreadId,
    },
    UI::WindowsAndMessaging::*,
};
use windows::core::{PCWSTR, w};

#[derive(Default)]
pub struct WtsSystemEvents;
impl WtsSystemEvents {
    pub fn new() -> Self {
        Self
    }
}

type Callback = Box<dyn Fn(SystemEvent) + Send + Sync>;
const CLASS: PCWSTR = w!("SamblerPlatformSystemEventsV1");
const STOP_TIMER: usize = 1;

fn notification(message: u32, code: u32) -> Option<SystemEvent> {
    match (message, code) {
        (
            WM_WTSSESSION_CHANGE,
            WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT,
        ) => Some(SystemEvent::SessionLocked),
        (WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK) => Some(SystemEvent::SessionUnlocked),
        (WM_POWERBROADCAST, PBT_APMSUSPEND) => Some(SystemEvent::Suspending),
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC) => Some(SystemEvent::Resumed),
        _ => None,
    }
}

fn notify(callback: &Callback, event: SystemEvent) {
    // Keep an unwinding callback from crossing the Win32 ABI.
    let _ = catch_unwind(AssertUnwindSafe(|| callback(event)));
}

unsafe extern "system" fn wndproc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(event) = notification(message, w.0 as u32) {
        let callback = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Callback;
        if let Some(callback) = unsafe { callback.as_ref() } {
            notify(callback, event);
        }
        return LRESULT(1);
    }
    if message == WM_CLOSE {
        // The owner thread must unregister WTS before destroying the window.
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, w, l) }
}

fn run(callback: &Callback, stop: &AtomicBool, ready: mpsc::Sender<u32>) -> Option<()> {
    let instance = unsafe { GetModuleHandleW(None) }.ok()?.into();
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    if !REGISTERED.get_or_init(|| unsafe {
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: CLASS,
            ..Default::default()
        }) != 0
    }) {
        return None;
    }
    // A hidden, ordinary top-level window receives power broadcasts; an
    // HWND_MESSAGE window does not.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE,
            CLASS,
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .ok()?;
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, callback as *const Callback as isize) };
    // The timer is a fallback for shutdown if posting WM_QUIT fails.
    let armed = unsafe { SetTimer(Some(hwnd), STOP_TIMER, 250, None) } != 0;
    let registered =
        armed && unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) }.is_ok();
    if armed && ready.send(unsafe { GetCurrentThreadId() }).is_ok() {
        if !registered {
            // The port has no error return; signal that monitoring cannot arm capture safely.
            notify(callback, SystemEvent::SessionLocked);
        }
        let mut message = MSG::default();
        while !stop.load(Ordering::Acquire)
            && unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0
        {
            if !stop.load(Ordering::Acquire) {
                unsafe { DispatchMessageW(&message) };
            }
        }
    }
    unsafe {
        if registered {
            let _ = WTSUnRegisterSessionNotification(hwnd);
        }
        if armed {
            let _ = KillTimer(Some(hwnd), STOP_TIMER);
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd);
    }
    armed.then_some(())
}

struct Active {
    id: u32,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            if !worker.is_finished() {
                let _ = unsafe { PostThreadMessageW(self.id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            }
            // A callback may release the subscription from the window thread itself.
            if worker.thread().id() != thread::current().id() {
                let _ = worker.join();
            }
        }
    }
}

impl SystemEvents for WtsSystemEvents {
    fn subscribe(&self, callback: Callback) -> Subscription {
        let (ready, started) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        // Keep the callback available for a fail-closed notification if spawn fails.
        let callback = Arc::new(callback);
        let worker_callback = callback.clone();
        let Ok(thread) = thread::Builder::new()
            .name("sambler-windows-events".into())
            .spawn(move || {
                if run(&worker_callback, &worker_stop, ready).is_none() {
                    notify(&worker_callback, SystemEvent::SessionLocked);
                }
            })
        else {
            notify(&callback, SystemEvent::SessionLocked);
            return Box::new(());
        };
        match started.recv() {
            Ok(id) => Box::new(Active {
                id,
                stop,
                thread: Some(thread),
            }),
            Err(_) => {
                let _ = thread.join();
                Box::new(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_top_level_window_dispatches_and_is_destroyed_on_drop() {
        use std::time::Duration;

        unsafe extern "system" fn find_own(hwnd: HWND, out: LPARAM) -> windows::core::BOOL {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            let mut class = [0u16; 64];
            let len = unsafe { GetClassNameW(hwnd, &mut class) }.max(0) as usize;
            if pid == std::process::id()
                && String::from_utf16_lossy(&class[..len]) == "SamblerPlatformSystemEventsV1"
            {
                unsafe { *(out.0 as *mut HWND) = hwnd };
            }
            windows::core::BOOL(1)
        }
        let (tx, rx) = mpsc::channel();
        let subscription = WtsSystemEvents::new().subscribe(Box::new(move |e| {
            let _ = tx.send(e);
        }));
        let mut window = HWND::default();
        unsafe { EnumWindows(Some(find_own), LPARAM(&mut window as *mut HWND as isize)) }.unwrap();
        assert!(!window.0.is_null());
        assert!(!unsafe { IsWindowVisible(window) }.as_bool());
        assert_eq!(unsafe { GetAncestor(window, GA_ROOT) }, window);
        // Target only our test window; no actual suspend, lock or broadcast.
        for (msg, w, event) in [
            (
                WM_WTSSESSION_CHANGE,
                WTS_SESSION_LOCK,
                SystemEvent::SessionLocked,
            ),
            (
                WM_WTSSESSION_CHANGE,
                WTS_SESSION_UNLOCK,
                SystemEvent::SessionUnlocked,
            ),
            (WM_POWERBROADCAST, PBT_APMSUSPEND, SystemEvent::Suspending),
            (
                WM_POWERBROADCAST,
                PBT_APMRESUMEAUTOMATIC,
                SystemEvent::Resumed,
            ),
        ] {
            unsafe { PostMessageW(Some(window), msg, WPARAM(w as usize), LPARAM(0)) }.unwrap();
            assert_eq!(rx.recv_timeout(Duration::from_secs(1)), Ok(event));
        }
        drop(subscription);
        assert!(!unsafe { IsWindow(Some(window)) }.as_bool());
        assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected));
    }

    #[test]
    fn maps_session_and_power_notifications() {
        for code in [
            WTS_SESSION_LOCK,
            WTS_CONSOLE_DISCONNECT,
            WTS_REMOTE_DISCONNECT,
        ] {
            assert_eq!(
                notification(WM_WTSSESSION_CHANGE, code),
                Some(SystemEvent::SessionLocked)
            );
        }
        assert_eq!(
            notification(WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK),
            Some(SystemEvent::SessionUnlocked)
        );
        assert_eq!(
            notification(WM_POWERBROADCAST, PBT_APMSUSPEND),
            Some(SystemEvent::Suspending)
        );
        assert_eq!(
            notification(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC),
            Some(SystemEvent::Resumed)
        );
        assert_eq!(notification(WM_WTSSESSION_CHANGE, WTS_REMOTE_CONNECT), None);
    }
}
