use super::{
    insertion_method,
    office::{self, OfficeKind},
};
use crate::{
    ports::{
        AppKind, InsertOutcome, NotWrittenReason, TargetChange, TargetInfo, TargetProbe,
        TargetToken, TargetVerdict, UncertainReason, WatchGuard,
    },
    settings::InsertionMethod,
};
use std::{
    cell::Cell,
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{
        CloseHandle, ERROR_SUCCESS, GetLastError, HANDLE, HWND, LPARAM, SetLastError, WPARAM,
    },
    Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
        TOKEN_QUERY, TokenIntegrityLevel,
    },
    System::{
        Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        StationsAndDesktops::{
            CloseDesktop, DESKTOP_READOBJECTS, GetUserObjectInformationW, OpenInputDesktop,
            UOI_NAME,
        },
        Threading::{
            GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenProcessToken,
            PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
        Variant::{VARIANT, VT_BOOL, VariantClear},
    },
    UI::{
        Accessibility::*,
        Controls::{EM_GETPASSWORDCHAR, EM_GETSEL},
        Input::KeyboardAndMouse::IsWindowEnabled,
        WindowsAndMessaging::*,
    },
};
use windows::core::{Interface, PWSTR};
use zeroize::Zeroizing;

const REQUEST_BUDGET: Duration = Duration::from_millis(300);
const CHROMIUM_CAPTURE_BUDGET: Duration = Duration::from_millis(1450);
const CHROMIUM_RECHECK_BUDGET: Duration = Duration::from_millis(500);
const CHROMIUM_RETRY: Duration = Duration::from_millis(30);
const WATCH_PERIOD: Duration = Duration::from_millis(75);

pub struct UiaTargetProbe {
    worker: Mutex<Option<Arc<Worker>>>,
    _warmup: Option<ForegroundWarmup>,
}

impl Default for UiaTargetProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl UiaTargetProbe {
    pub fn new() -> Self {
        Self::with_warmup(ForegroundWarmup::start(None))
    }

    /// Isolate desktop qualification from all windows except the fixture.
    #[doc(hidden)]
    pub fn for_fixture(pid: u32, warmup: bool) -> Self {
        Self::with_warmup(warmup.then(|| ForegroundWarmup::start(Some(pid))).flatten())
    }

    fn with_warmup(warmup: Option<ForegroundWarmup>) -> Self {
        Self {
            worker: Mutex::new(None),
            _warmup: warmup,
        }
    }

    fn worker(&self) -> Option<Arc<Worker>> {
        // No Windows calls or waits while holding this lock.
        let mut slot = self.worker.lock().ok()?;
        if let Some(worker) = slot.as_ref().filter(|worker| !worker.expired()) {
            return Some(worker.clone());
        }
        let worker = Worker::spawn()?;
        *slot = Some(worker.clone());
        Some(worker)
    }
}

type Job = Box<dyn FnOnce(&mut State) + Send>;

struct Worker {
    jobs: mpsc::SyncSender<Job>,
    expired: Arc<AtomicBool>,
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn automation_client() -> Option<IUIAutomation> {
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }.ok()?;
    if let Ok(settings) = automation.cast::<IUIAutomation2>() {
        unsafe {
            let _ = settings.SetConnectionTimeout(250);
            let _ = settings.SetTransactionTimeout(250);
        }
    }
    Some(automation)
}

// Keep provider traffic for speculative work off the capture/recheck queue.
// This MTA pumps only foreground events; it never captures a token or reads text.
struct ForegroundWarmup(Arc<AtomicU32>);
const STOP_WARMUP: u32 = u32::MAX;
const WM_WARM_FOREGROUND: u32 = WM_APP + 1;

impl ForegroundWarmup {
    fn start(only_pid: Option<u32>) -> Option<Self> {
        let id = Arc::new(AtomicU32::new(0));
        let state = id.clone();
        thread::Builder::new()
            .name("sambler-uia-foreground".into())
            .spawn(move || {
                if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                    return;
                }
                let _apartment = Apartment;
                let Some(automation) = automation_client() else {
                    return;
                };
                let mut msg = MSG::default();
                let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
                let thread_id = unsafe { GetCurrentThreadId() };
                if state
                    .compare_exchange(0, thread_id, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    return;
                }
                unsafe extern "system" fn foreground_event(
                    _: HWINEVENTHOOK,
                    _: u32,
                    hwnd: HWND,
                    _: i32,
                    _: i32,
                    _: u32,
                    _: u32,
                ) {
                    // Never enter COM from the WinEvent callback; COM can pump messages.
                    let _ = unsafe {
                        PostThreadMessageW(
                            GetCurrentThreadId(),
                            WM_WARM_FOREGROUND,
                            WPARAM(hwnd.0 as usize),
                            LPARAM(0),
                        )
                    };
                }
                let hook = unsafe {
                    SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_SYSTEM_FOREGROUND,
                        None,
                        Some(foreground_event),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    )
                };
                if !hook.0.is_null() {
                    let mut last = None;
                    let mut warm = |hwnd| {
                        let window = Window::new(hwnd);
                        if window == last {
                            return;
                        }
                        last = window;
                        if let Some(window) = window
                            && window.pid != std::process::id()
                            && only_pid.is_none_or(|pid| window.pid == pid)
                            && window.chromium_widget()
                            && Window::foreground() == Some(window)
                            && on_default_desktop()
                        {
                            let _ = unsafe { automation.GetFocusedElement() };
                        }
                    };
                    warm(unsafe { GetForegroundWindow() });
                    while state.load(Ordering::Acquire) != STOP_WARMUP
                        && unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0
                    {
                        if state.load(Ordering::Acquire) == STOP_WARMUP {
                            break;
                        }
                        if msg.message == WM_WARM_FOREGROUND {
                            warm(HWND(msg.wParam.0 as _));
                        } else {
                            unsafe { DispatchMessageW(&msg) };
                        }
                    }
                    let _ = unsafe { UnhookWinEvent(hook) };
                }
                let _ = state.compare_exchange(thread_id, 0, Ordering::AcqRel, Ordering::Acquire);
            })
            .ok()?;
        Some(Self(id))
    }
}

impl Drop for ForegroundWarmup {
    fn drop(&mut self) {
        let id = self.0.swap(STOP_WARMUP, Ordering::AcqRel);
        if id != 0 {
            let _ = unsafe { PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        }
    }
}

impl Worker {
    fn spawn() -> Option<Arc<Self>> {
        let (jobs, requests) = mpsc::sync_channel::<Job>(4);
        let expired = Arc::new(AtomicBool::new(false));
        let worker_expired = expired.clone();
        thread::Builder::new()
            .name("sambler-uia-mta".into())
            .spawn(move || {
                if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                    return;
                }
                let _apartment = Apartment;
                let Some(automation) = automation_client() else {
                    return;
                };
                let mut state = State {
                    automation,
                    records: HashMap::new(),
                };
                while !worker_expired.load(Ordering::Acquire) {
                    let Ok(job) = requests.recv() else {
                        break;
                    };
                    if worker_expired.load(Ordering::Acquire) {
                        break;
                    }
                    job(&mut state);
                    state
                        .records
                        .retain(|_, (lifetime, _)| lifetime.strong_count() != 0);
                }
            })
            .ok()?;
        Some(Arc::new(Self { jobs, expired }))
    }

    fn expired(&self) -> bool {
        self.expired.load(Ordering::Acquire)
    }

    fn request<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut State) -> T + Send + 'static,
    ) -> Option<T> {
        self.request_until(Instant::now() + REQUEST_BUDGET, work)
    }

    fn request_until<T: Send + 'static>(
        &self,
        deadline: Instant,
        work: impl FnOnce(&mut State) -> T + Send + 'static,
    ) -> Option<T> {
        if self.expired() || Instant::now() >= deadline {
            return None;
        }
        let (reply, answer) = mpsc::sync_channel(1);
        let expired = self.expired.clone();
        let job = Box::new(move |state: &mut State| {
            if Instant::now() < deadline && !expired.load(Ordering::Acquire) {
                let _ = reply.send(work(state));
            }
        });
        if self.jobs.try_send(job).is_ok() {
            if let Ok(answer) =
                answer.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                if !self.expired() {
                    return Some(answer);
                }
            }
        }
        // A stuck COM call cannot be killed safely. Retire this generation and
        // all its tokens immediately. Its apartment exits if the provider returns.
        self.expired.store(true, Ordering::Release);
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Window {
    hwnd: usize,
    thread: u32,
    pid: u32,
}
impl Window {
    fn new(hwnd: HWND) -> Option<Self> {
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        (thread != 0 && pid != 0).then_some(Self {
            hwnd: hwnd.0 as usize,
            thread,
            pid,
        })
    }
    fn foreground() -> Option<Self> {
        Self::new(unsafe { GetForegroundWindow() })
    }
    fn handle(self) -> HWND {
        HWND(self.hwnd as _)
    }

    fn chromium_widget(self) -> bool {
        class_name(self.handle()).eq_ignore_ascii_case("Chrome_WidgetWin_1")
    }
}

/// Cheap HWND snapshot captured at the shortcut edge, before any queue or COM wait.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct TriggerFocus {
    window: Option<Window>,
    focused: Option<usize>,
}

impl TriggerFocus {
    pub(crate) fn capture() -> Self {
        let window = Window::foreground();
        Self {
            window,
            focused: window.and_then(focus_hwnd).map(|hwnd| hwnd.0 as usize),
        }
    }

    pub(crate) fn still_focused(self) -> bool {
        self.window.is_some() && self == Self::capture()
    }

    pub(crate) fn identifies(self, token: &TargetToken) -> bool {
        identity(token).is_some_and(|id| {
            self.window == Some(id.window) && (id.late.is_some() || self.focused == id.native_focus)
        })
    }
}

#[derive(Clone, Copy)]
struct LateWindow {
    root: Window,
    integrity: u32,
    app: AppKind,
}

#[derive(Clone, Copy)]
struct LateField {
    native_focus: Option<usize>,
    multiline: Option<bool>,
}

impl LateWindow {
    fn capture(root: Window) -> Option<Self> {
        if root.pid == std::process::id() || !on_default_desktop() {
            return None;
        }
        // Late Paste intentionally remembers only the foreground root: Chromium
        // can give its root HWND focus for both the page and the omnibox. A field
        // change inside this root cannot disarm an as-yet unidentified field.
        // End-of-take UIA must fully qualify and freeze the actual editor first.
        let process = Handle(
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, root.pid) }.ok()?,
        );
        let app = app_kind(process.0);
        // Do not relax targeting for terminals embedded in Chromium shells.
        if app == AppKind::VsCode {
            return None;
        }
        let integrity = integrity(process.0).filter(|&level| {
            integrity(unsafe { GetCurrentProcess() }).is_some_and(|ours| level <= ours)
        })?;
        let late = Self {
            root,
            integrity,
            app,
        };
        late.check().ok()?;
        Some(late)
    }

    fn check(self) -> Result<(), TargetChange> {
        if !on_default_desktop() {
            return Err(TargetChange::Protected);
        }
        if Window::new(self.root.handle()) != Some(self.root) {
            return Err(TargetChange::WindowGone);
        }
        if Window::foreground() != Some(self.root) {
            return Err(TargetChange::FocusMoved);
        }
        Ok(())
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct Identity {
    worker: Arc<Worker>,
    id: u64,
    window: Window,
    native_focus: Option<usize>,
    native_edit: Mutex<Option<NativeEdit>>,
    office: bool,
    info: TargetInfo,
    late: Option<LateWindow>,
    qualified_late: OnceLock<LateField>,
    attempted: AtomicBool,
    write_permission: OnceLock<Arc<AtomicBool>>,
    protection: OnceLock<Protection>,
    changed: OnceLock<TargetChange>,
    lifetime: Arc<()>,
}

impl Identity {
    fn new(
        worker: Arc<Worker>,
        window: Window,
        native_focus: Option<usize>,
        info: TargetInfo,
    ) -> Self {
        Self {
            worker,
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            window,
            native_focus,
            info,
            native_edit: Mutex::new(None),
            office: false,
            late: None,
            qualified_late: OnceLock::new(),
            attempted: AtomicBool::new(false),
            write_permission: OnceLock::new(),
            protection: OnceLock::new(),
            changed: OnceLock::new(),
            lifetime: Arc::new(()),
        }
    }

    fn latch_protection(&self) {
        let _ = self.changed.set(TargetChange::Protected);
        if let Some(protection) = self.protection.get() {
            protection.live.store(false, Ordering::Release);
            protection.detected.store(true, Ordering::Release);
        }
    }

    fn verdict(self) -> TargetVerdict {
        let info = self.info;
        TargetVerdict::Eligible {
            token: TargetToken(Arc::new(self)),
            info,
        }
    }
}

impl Drop for Identity {
    fn drop(&mut self) {
        // Release COM references on their owning apartment, without waiting.
        let id = self.id;
        let _ = self.worker.jobs.try_send(Box::new(move |state| {
            state.records.remove(&id);
        }));
    }
}

struct State {
    automation: IUIAutomation,
    records: HashMap<u64, (Weak<()>, Snapshot)>,
}

struct Snapshot {
    window: Window,
    native_focus: Option<usize>,
    native_edit: Option<NativeEdit>,
    element: IUIAutomationElement,
    caret: Caret,
    info: TargetInfo,
}

enum Caret {
    Text(IUIAutomationTextRange),
    Edit { hwnd: HWND, selection: [u32; 2] },
    Office(office::BodyCaret),
}

#[derive(Clone, Copy)]
enum Reject {
    Protected,
    OwnProcess,
    Uncertain(UncertainReason),
}
impl From<UncertainReason> for Reject {
    fn from(reason: UncertainReason) -> Self {
        Self::Uncertain(reason)
    }
}
impl From<Reject> for TargetVerdict {
    fn from(value: Reject) -> Self {
        match value {
            Reject::Protected => Self::Protected,
            Reject::OwnProcess => Self::Sambler,
            Reject::Uncertain(reason) => Self::Uncertain { reason },
        }
    }
}

use UncertainReason::{
    Elevated, NoFocusedField, ReadOnly, RemoteDesktop, Selection, Terminal, Unresponsive,
    Unsupported,
};

impl State {
    fn capture(
        &mut self,
        worker: Arc<Worker>,
        expected: Option<Window>,
        try_late: bool,
        deadline: Instant,
    ) -> TargetVerdict {
        let snapshot = loop {
            if expected.is_some() && Window::foreground() != expected {
                return Reject::from(NoFocusedField).into();
            }
            match inspect(&self.automation, false) {
                Ok(snapshot) if expected.is_none_or(|window| window == snapshot.window) => {
                    break snapshot;
                }
                Ok(_) => return Reject::from(NoFocusedField).into(),
                Err(Reject::Uncertain(NoFocusedField | Unresponsive)) if try_late => {
                    return Reject::from(NoFocusedField).into();
                }
                Err(Reject::Uncertain(NoFocusedField | Unresponsive | Unsupported))
                    if expected.is_some() && Instant::now() + CHROMIUM_RETRY < deadline =>
                {
                    // Chromium may not have attached its accessibility tree
                    // yet. Requery only in the original foreground window.
                    thread::sleep(CHROMIUM_RETRY);
                }
                Err(reject) => return reject.into(),
            }
        };
        let mut identity = Identity::new(
            worker,
            snapshot.window,
            snapshot.native_focus,
            snapshot.info,
        );
        identity.native_edit = Mutex::new(snapshot.native_edit);
        identity.office = matches!(snapshot.caret, Caret::Office(_));
        self.records
            .insert(identity.id, (Arc::downgrade(&identity.lifetime), snapshot));
        identity.verdict()
    }

    fn qualify_late(
        &mut self,
        id: &Identity,
        late: LateWindow,
        deadline: Instant,
        pin_caret: bool,
    ) -> Result<LateField, TargetChange> {
        late.check()?;
        let process = Handle(
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, late.root.pid) }
                .map_err(|_| TargetChange::WindowGone)?,
        );
        if integrity(process.0) != Some(late.integrity) {
            return Err(TargetChange::Unknown);
        }
        if let Some((_, known)) = self.records.get_mut(&id.id) {
            let range = recheck_chromium(&self.automation, known, pin_caret)?;
            if !pin_caret {
                known.caret = Caret::Text(range);
            }
            return Ok(LateField {
                native_focus: known.native_focus,
                multiline: known.info.multiline,
            });
        }
        loop {
            late.check()?;
            match inspect(&self.automation, true) {
                Ok(snapshot) => {
                    if snapshot.window != late.root
                        || !matches!(snapshot.caret, Caret::Text(_))
                        || snapshot.native_edit.is_some()
                    {
                        return Err(TargetChange::FocusMoved);
                    }
                    late.check()?;
                    if Instant::now() >= deadline {
                        return Err(TargetChange::Unknown);
                    }
                    let field = LateField {
                        native_focus: snapshot.native_focus,
                        multiline: snapshot.info.multiline,
                    };
                    self.records
                        .insert(id.id, (Arc::downgrade(&id.lifetime), snapshot));
                    return Ok(field);
                }
                Err(Reject::Protected) => return Err(TargetChange::Protected),
                Err(Reject::Uncertain(NoFocusedField | Unresponsive))
                    if Instant::now() + CHROMIUM_RETRY < deadline =>
                {
                    thread::sleep(CHROMIUM_RETRY)
                }
                Err(Reject::Uncertain(Selection)) => return Err(TargetChange::SelectionChanged),
                Err(_) => return Err(TargetChange::Unknown),
            }
        }
    }

    fn revalidate(&mut self, id: u64, pin_caret: bool) -> Result<Option<NativeEdit>, TargetChange> {
        use TargetChange::*;
        let old = &mut self.records.get_mut(&id).ok_or(Unknown)?.1;
        if old.window.chromium_widget()
            && Window::foreground() == Some(old.window)
            && matches!(old.caret, Caret::Text(_))
        {
            let range = recheck_chromium(&self.automation, old, pin_caret)?;
            if !pin_caret {
                old.caret = Caret::Text(range);
                if let Some(edit) = &mut old.native_edit {
                    edit.selection = edit_selection(edit.hwnd()).map_err(|_| SelectionChanged)?;
                }
            }
            return Ok(old.native_edit);
        }
        let now = inspect(&self.automation, false);
        // Protection takes precedence even when the password field is in a different window.
        if matches!(now, Err(Reject::Protected)) {
            return Err(Protected);
        }
        if Window::new(old.window.handle()) != Some(old.window) {
            return Err(WindowGone);
        }
        if Window::foreground() != Some(old.window) {
            return Err(FocusMoved);
        }
        let now = now.map_err(|reject| match reject {
            Reject::Uncertain(Selection) => SelectionChanged,
            Reject::OwnProcess | Reject::Uncertain(NoFocusedField) => FocusMoved,
            _ => Unknown,
        })?;
        if now.window != old.window
            || now.native_focus != old.native_focus
            || !unsafe { self.automation.CompareElements(&old.element, &now.element) }
                .map_err(|_| Unknown)?
                .as_bool()
        {
            return Err(FocusMoved);
        }
        if old.native_edit.map(NativeEdit::shape) != now.native_edit.map(NativeEdit::shape)
            || old.info.multiline != now.info.multiline
        {
            return Err(Unknown);
        }
        match (&old.caret, &now.caret) {
            (Caret::Office(a), Caret::Office(b)) if !a.same_document(b) => return Err(FocusMoved),
            (Caret::Office(a), Caret::Office(b)) if pin_caret && !a.same(b) => {
                return Err(SelectionChanged);
            }
            (Caret::Office(_), Caret::Office(_)) => {}
            (Caret::Text(a), Caret::Text(b)) if pin_caret => compare_carets(a, b)?,
            (Caret::Text(_), Caret::Text(_)) => {}
            (
                Caret::Edit {
                    hwnd: a,
                    selection: i,
                },
                Caret::Edit {
                    hwnd: b,
                    selection: j,
                },
            ) if a == b => {
                if pin_caret && i != j {
                    return Err(SelectionChanged);
                }
            }
            _ => return Err(Unknown),
        }
        if !pin_caret {
            old.caret = now.caret;
            old.native_edit = now.native_edit;
        }
        Ok(old.native_edit)
    }
}

fn identity(token: &TargetToken) -> Option<&Identity> {
    token.0.downcast_ref()
}

pub(super) fn revalidate_token(token: &TargetToken) -> Result<(), TargetChange> {
    revalidate(token, false)
}

pub(crate) fn revalidate_after_install(
    probe: &dyn TargetProbe,
    token: &TargetToken,
) -> Result<(), TargetChange> {
    if is_late(token) {
        revalidate(token, true)
    } else {
        probe.revalidate(token)
    }
}

#[doc(hidden)]
pub fn is_late(token: &TargetToken) -> bool {
    identity(token).is_some_and(|target| target.late.is_some())
}

fn consumed(token: &TargetToken) -> bool {
    identity(token).is_some_and(|id| id.attempted.load(Ordering::Acquire))
}

fn revalidate(token: &TargetToken, watching: bool) -> Result<(), TargetChange> {
    let target = identity(token).ok_or(TargetChange::Unknown)?;
    if watching && consumed(token) {
        return Ok(());
    }
    if let Some(&change) = target.changed.get() {
        return Err(change);
    }
    if let Some(late) = target.late {
        let result = if watching {
            late.check()
        } else if insertion_method() != InsertionMethod::Paste {
            Err(TargetChange::Unknown)
        } else {
            let budget = if target.qualified_late.get().is_some() {
                CHROMIUM_RECHECK_BUDGET
            } else {
                CHROMIUM_CAPTURE_BUDGET
            };
            let deadline = Instant::now() + budget;
            let pin_caret = consumed(token);
            let token = token.clone();
            target
                .worker
                .request_until(deadline, move |state| {
                    let id = identity(&token).ok_or(TargetChange::Unknown)?;
                    state.qualify_late(id, late, deadline, pin_caret)
                })
                .unwrap_or(Err(TargetChange::Unknown))
                .map(|field| {
                    let _ = target.qualified_late.set(field);
                })
        };
        if watching && consumed(token) {
            return Ok(());
        }
        if result == Err(TargetChange::Protected) {
            target.latch_protection();
        }
        return target.changed.get().copied().map_or(result, Err);
    }
    let id = target.id;
    let budget = if target.window.chromium_widget() {
        CHROMIUM_RECHECK_BUDGET
    } else {
        REQUEST_BUDGET
    };
    let pin_caret = watching || consumed(token);
    let result = target
        .worker
        .request_until(Instant::now() + budget, move |state| {
            state.revalidate(id, pin_caret)
        })
        .unwrap_or(Err(TargetChange::Unknown))
        .and_then(|edit| {
            if !pin_caret {
                *target
                    .native_edit
                    .lock()
                    .map_err(|_| TargetChange::Unknown)? = edit;
            }
            Ok(())
        });
    // The insertion path owns focus checks after committing to an attempt.
    // An in-flight poll may observe the caret moving from our own first packet.
    if watching && consumed(token) {
        return Ok(());
    }
    if result == Err(TargetChange::Protected) {
        target.latch_protection();
    }
    target.changed.get().copied().map_or(result, Err)
}

struct Protection {
    live: Arc<AtomicBool>,
    detected: Arc<AtomicBool>,
}

pub(crate) fn bind_protection(
    token: &TargetToken,
    live: &Arc<AtomicBool>,
    detected: &Arc<AtomicBool>,
) {
    if let Some(id) = identity(token) {
        let _ = id.protection.set(Protection {
            live: live.clone(),
            detected: detected.clone(),
        });
    }
}

pub(crate) fn bind_write_permission(token: &TargetToken, allowed: &Arc<AtomicBool>) {
    if let Some(target) = identity(token) {
        let _ = target.write_permission.set(allowed.clone());
    }
}

pub(super) fn write_permitted(token: &TargetToken) -> bool {
    identity(token).is_some_and(|id| {
        id.changed.get().is_none()
            && id
                .write_permission
                .get()
                .is_none_or(|allowed| allowed.load(Ordering::Acquire))
    })
}

pub(super) fn write_focus(token: &TargetToken) -> Option<HWND> {
    let id = identity(token)?;
    if !write_permitted(token)
        || !on_default_desktop()
        || Window::foreground() != Some(id.window)
        || id.worker.expired()
    {
        return None;
    }
    let expected_focus = if id.late.is_some() {
        id.qualified_late.get()?.native_focus
    } else {
        id.native_focus
    };
    let focus = focus_hwnd(id.window);
    (focus.map(|hwnd| hwnd.0 as usize) == expected_focus)
        .then(|| focus.unwrap_or_else(|| id.window.handle()))
}

pub(super) fn arm_single_write(token: &TargetToken) -> bool {
    write_focus(token).is_some()
        && identity(token).is_some_and(|id| !id.attempted.swap(true, Ordering::AcqRel))
}

pub(super) fn native_edit(token: &TargetToken) -> Option<NativeEdit> {
    *identity(token)?.native_edit.lock().ok()?
}

pub(super) fn office_body(token: &TargetToken) -> bool {
    identity(token).is_some_and(|id| id.office)
}

pub(super) fn known_multiline(token: &TargetToken) -> bool {
    identity(token).is_some_and(|id| {
        id.qualified_late
            .get()
            .map_or(id.info.multiline, |field| field.multiline)
            == Some(true)
    })
}

pub(super) fn outlook_paste_only(token: &TargetToken) -> bool {
    identity(token).is_some_and(|id| id.info.app == AppKind::Outlook && !id.office)
}

pub(super) fn insert_office(token: &TargetToken, text: Zeroizing<Vec<u16>>) -> InsertOutcome {
    let not_started = InsertOutcome::NotWritten {
        reason: NotWrittenReason::TargetChanged,
    };
    let Some(id) = identity(token) else {
        return not_started;
    };
    let key = id.id;
    let root = id.window.hwnd;
    let Some(body) = id.native_focus else {
        return not_started;
    };
    let token = token.clone();
    let phase = Arc::new(std::sync::atomic::AtomicU8::new(0)); // ready, started, or cancelled
    let worker_phase = phase.clone();
    let deadline = Instant::now() + REQUEST_BUDGET;
    let answer = id.worker.request(move |state| {
        let Some((_, saved)) = state.records.get(&key) else {
            return office::Write::NotStarted;
        };
        let Caret::Office(caret) = &saved.caret else {
            return office::Write::NotStarted;
        };
        let still_focused =
            || Instant::now() < deadline && write_focus(&token) == Some(HWND(body as _));
        office::write(
            caret,
            HWND(root as _),
            HWND(body as _),
            &text,
            || {
                still_focused()
                    && write_permitted(&token)
                    && worker_phase
                        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
            },
            still_focused,
        )
    });
    match answer {
        Some(office::Write::Delivered) => InsertOutcome::Delivered,
        Some(office::Write::NotStarted) => not_started,
        Some(office::Write::Uncertain) => InsertOutcome::Partial,
        None if phase.swap(2, Ordering::AcqRel) == 1 => InsertOutcome::Partial,
        None => not_started,
    }
}

fn checked_chromium_focus(
    automation: &IUIAutomation,
    old: &Snapshot,
) -> Result<IUIAutomationElement, TargetChange> {
    use TargetChange::*;
    if !on_default_desktop() {
        return Err(Protected);
    }
    let focus = unsafe { automation.GetFocusedElement() }.map_err(|_| Unknown)?;
    let password = unsafe { focus.GetCurrentPropertyValueEx(UIA_IsPasswordPropertyId, true) }
        .map_err(|_| Unknown)
        .map(exact_bool)?;
    if password == Some(true) {
        return Err(Protected);
    }
    if Window::foreground() != Some(old.window)
        || focus_hwnd(old.window).map(|hwnd| hwnd.0 as usize) != old.native_focus
        || !unsafe { automation.CompareElements(&old.element, &focus) }
            .map_err(|_| Unknown)?
            .as_bool()
    {
        return Err(FocusMoved);
    }
    if password != Some(false)
        || !unsafe { focus.CurrentIsEnabled() }
            .map_err(|_| Unknown)?
            .as_bool()
    {
        return Err(Unknown);
    }
    Ok(focus)
}

fn compare_carets(
    saved: &IUIAutomationTextRange,
    current: &IUIAutomationTextRange,
) -> Result<(), TargetChange> {
    for endpoint in [TextPatternRangeEndpoint_Start, TextPatternRangeEndpoint_End] {
        if unsafe { saved.CompareEndpoints(endpoint, current, endpoint) }
            .map_err(|_| TargetChange::Unknown)?
            != 0
        {
            return Err(TargetChange::SelectionChanged);
        }
    }
    Ok(())
}

fn recheck_chromium(
    automation: &IUIAutomation,
    old: &Snapshot,
    pin_caret: bool,
) -> Result<IUIAutomationTextRange, TargetChange> {
    use TargetChange::*;
    let focus = checked_chromium_focus(automation, old)?;
    let Caret::Text(saved) = &old.caret else {
        return Err(Unknown);
    };
    // The current range is authoritative until arming the actual write.
    let text: IUIAutomationTextPattern =
        unsafe { focus.GetCurrentPatternAs(UIA_TextPatternId) }.map_err(|_| Unknown)?;
    let selected = unsafe { text.GetSelection() }.map_err(|_| Unknown)?;
    if unsafe { selected.Length() }.map_err(|_| Unknown)? != 1 {
        return Err(SelectionChanged);
    }
    let range = unsafe { selected.GetElement(0) }.map_err(|_| Unknown)?;
    let start = TextPatternRangeEndpoint_Start;
    let end = TextPatternRangeEndpoint_End;
    if old.info.multiline != Some(false)
        && unsafe { range.CompareEndpoints(start, &range, end) }.map_err(|_| Unknown)? != 0
    {
        return Err(SelectionChanged);
    }
    if pin_caret {
        compare_carets(saved, &range)?;
    }
    if unsafe { range.GetAttributeValue(UIA_IsReadOnlyAttributeId) }
        .ok()
        .and_then(exact_bool)
        == Some(true)
    {
        return Err(Unknown);
    }
    if let Ok(value) =
        unsafe { focus.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
    {
        if !unsafe { value.CurrentIsReadOnly() }.is_ok_and(|readonly| !readonly.as_bool()) {
            return Err(Unknown);
        }
    }
    Ok(range)
}

pub(super) fn same_uia_focus(token: &TargetToken) -> bool {
    let Some(id) = identity(token) else {
        return false;
    };
    let key = id.id;
    id.worker
        .request(move |state| {
            let Some((_, old)) = state.records.get(&key) else {
                return false;
            };
            if old.window.chromium_widget() {
                return checked_chromium_focus(&state.automation, old).is_ok();
            }
            // No caret comparison here: successful earlier packets move it.
            let Ok(focus) = (unsafe { state.automation.GetFocusedElement() }) else {
                return false;
            };
            unsafe { state.automation.CompareElements(&old.element, &focus) }
                .is_ok_and(|same| same.as_bool())
                && unsafe { focus.CurrentIsEnabled() }.is_ok_and(|enabled| enabled.as_bool())
                && unsafe { focus.GetCurrentPropertyValueEx(UIA_IsPasswordPropertyId, true) }
                    .is_ok_and(|value| exact_bool(value) == Some(false))
        })
        .unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct NativeEdit {
    hwnd: usize,
    selection: [u32; 2],
    pub(super) rich: bool,
    pub(super) multiline: bool,
}

impl NativeEdit {
    fn shape(self) -> (usize, bool, bool) {
        (self.hwnd, self.rich, self.multiline)
    }

    pub(super) fn hwnd(self) -> HWND {
        HWND(self.hwnd as _)
    }

    pub(super) fn writable_now(self, hwnd: HWND) -> bool {
        hwnd == self.hwnd()
            && unsafe { IsWindowEnabled(hwnd) }.as_bool()
            && literal_class(&class_name(hwnd)) == Some(self.rich)
            && window_style(hwnd).is_ok_and(|style| {
                style & (ES_PASSWORD | ES_READONLY) as u32 == 0
                    && (style & ES_MULTILINE as u32 != 0) == self.multiline
            })
            && edit_message(hwnd, EM_GETPASSWORDCHAR, WPARAM(0), LPARAM(0))
                .is_ok_and(|mask| mask == 0)
    }

    pub(super) fn caret_unchanged(self) -> bool {
        edit_selection(self.hwnd()).is_ok_and(|selection| selection == self.selection)
    }

    pub(super) fn caret_advanced(self, units: u32) -> bool {
        self.selection[0]
            .checked_add(units)
            .is_some_and(|expected| {
                edit_selection(self.hwnd()).is_ok_and(|selection| selection == [expected, expected])
            })
    }
}

fn literal_class(class: &str) -> Option<bool> {
    if class.eq_ignore_ascii_case("Edit") {
        return Some(false);
    }
    ["RichEdit20W", "RICHEDIT50W", "RichEditD2DPT"]
        .iter()
        .any(|name| class.eq_ignore_ascii_case(name))
        .then_some(true)
}

fn edit_selection(hwnd: HWND) -> Result<[u32; 2], Reject> {
    // Unlike EM_EXGETSEL (>= WM_USER), EM_GETSEL marshals DWORD pointers
    // across processes. Both Unicode Edit and RichEdit implement it.
    let mut start = u32::MAX;
    let mut end = u32::MAX;
    edit_message(
        hwnd,
        EM_GETSEL,
        WPARAM((&mut start as *mut u32) as usize),
        LPARAM((&mut end as *mut u32) as isize),
    )?;
    if start > end || end == u32::MAX {
        return Err(Selection.into());
    }
    Ok([start, end])
}

thread_local! {
    static LATE_FOREGROUND: Cell<Option<(Window, bool)>> = const { Cell::new(None) };
}

// Foreground events preserve a switch away and back between cheap HWND polls.
// Use a per-watch hook because a late take must also work if prewarm failed.
struct LateWatchHook(HWINEVENTHOOK);
impl LateWatchHook {
    fn new(root: Window) -> Option<Self> {
        unsafe extern "system" fn moved(
            _: HWINEVENTHOOK,
            _: u32,
            hwnd: HWND,
            _: i32,
            _: i32,
            _: u32,
            _: u32,
        ) {
            let _ = LATE_FOREGROUND.try_with(|state| {
                if let Some((root, false)) = state.get()
                    && hwnd != root.handle()
                {
                    state.set(Some((root, true)));
                }
            });
        }
        LATE_FOREGROUND.set(Some((root, false)));
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(moved),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        (!hook.0.is_null()).then_some(Self(hook))
    }

    fn displaced(&self) -> bool {
        let mut message = MSG::default();
        unsafe {
            let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
        }
        LATE_FOREGROUND.get().is_some_and(|(_, moved)| moved)
    }
}
impl Drop for LateWatchHook {
    fn drop(&mut self) {
        let _ = unsafe { UnhookWinEvent(self.0) };
        LATE_FOREGROUND.set(None);
    }
}

struct Watch {
    stopped: Arc<AtomicBool>,
    thread: thread::Thread,
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

impl TargetProbe for UiaTargetProbe {
    fn capture(&self) -> TargetVerdict {
        let expected = Window::foreground().filter(|window| window.chromium_widget());
        let late = if insertion_method() == InsertionMethod::Paste {
            expected.and_then(LateWindow::capture)
        } else {
            None
        };
        let try_late = late.is_some();
        let deadline = Instant::now()
            + if expected.is_some() && !try_late {
                CHROMIUM_CAPTURE_BUDGET
            } else {
                REQUEST_BUDGET
            };
        let verdict = self
            .worker()
            .and_then(|worker| {
                let owner = worker.clone();
                worker.request_until(deadline, move |state| {
                    state.capture(owner, expected, try_late, deadline)
                })
            })
            .unwrap_or_else(|| Reject::from(Unresponsive).into());
        if matches!(
            verdict,
            TargetVerdict::Uncertain {
                reason: NoFocusedField | Unresponsive
            }
        ) && let Some(late) = late
        {
            if let Err(change) = late.check() {
                return if change == TargetChange::Protected {
                    TargetVerdict::Protected
                } else {
                    verdict
                };
            }
            // A hung press-time apartment may have been retired; give the
            // late token a usable generation for end-of-take qualification.
            if let Some(worker) = self.worker() {
                let mut id = Identity::new(
                    worker,
                    late.root,
                    None,
                    TargetInfo {
                        app: late.app,
                        multiline: None,
                    },
                );
                id.late = Some(late);
                return id.verdict();
            }
        }
        verdict
    }

    fn watch(
        &self,
        token: &TargetToken,
        on_change: Box<dyn FnOnce(TargetChange) + Send>,
    ) -> WatchGuard {
        // Providers can omit UIA events, including for native Edit. Use the
        // same proof as final validation every 75 ms. No callback runs on the
        // COM thread; even a hung provider is detected by the request deadline.
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = stopped.clone();
        let worker_token = token.clone();
        let callback = Arc::new(Mutex::new(Some(on_change)));
        let worker_callback = callback.clone();
        let worker = thread::Builder::new()
            .name("sambler-target-watch".into())
            .spawn(move || {
                let late_hook = identity(&worker_token)
                    .and_then(|id| id.late)
                    .and_then(|late| LateWatchHook::new(late.root));
                let mut displaced = false;
                while !worker_stop.load(Ordering::Acquire) && !consumed(&worker_token) {
                    if displaced {
                        // Keep checking for protected focus after the one-shot
                        // displacement callback has already been delivered.
                        let Some(id) = identity(&worker_token) else {
                            break;
                        };
                        let Some(protection) = id.protection.get() else {
                            break;
                        };
                        if !protection.live.load(Ordering::Acquire) {
                            break;
                        }
                        if protected_focus(&id.worker) {
                            id.latch_protection();
                            break;
                        }
                    } else if let Err(change) = revalidate(&worker_token, true).and_then(|()| {
                        if late_hook.as_ref().is_some_and(LateWatchHook::displaced) {
                            Err(TargetChange::FocusMoved)
                        } else {
                            Ok(())
                        }
                    }) {
                        let callback = worker_callback.lock().ok().and_then(|mut slot| slot.take());
                        if worker_stop.load(Ordering::Acquire) {
                            break;
                        }
                        if let Some(callback) = callback {
                            callback(change);
                        }
                        if change == TargetChange::Protected {
                            break;
                        }
                        displaced = true;
                        continue;
                    }
                    thread::park_timeout(WATCH_PERIOD);
                }
            });
        match worker {
            Ok(worker) => Box::new(Watch {
                stopped,
                thread: worker.thread().clone(),
            }),
            Err(_) => {
                if let Some(target) = identity(token) {
                    let _ = target.changed.set(TargetChange::Unknown);
                }
                if let Ok(mut slot) = callback.lock() {
                    if let Some(callback) = slot.take() {
                        callback(TargetChange::Unknown);
                    }
                }
                Box::new(())
            }
        }
    }

    fn revalidate(&self, token: &TargetToken) -> Result<(), TargetChange> {
        revalidate_token(token)
    }
}

fn protected_focus(worker: &Worker) -> bool {
    if !on_default_desktop() {
        return true;
    }
    if let Some(hwnd) = Window::foreground().and_then(focus_hwnd)
        && literal_class(&class_name(hwnd)).is_some()
        && (window_style(hwnd).is_ok_and(|style| style & ES_PASSWORD as u32 != 0)
            || edit_message(hwnd, EM_GETPASSWORDCHAR, WPARAM(0), LPARAM(0))
                .is_ok_and(|mask| mask != 0))
    {
        return true;
    }
    worker
        .request(|state| {
            unsafe { state.automation.GetFocusedElement() }
                .ok()
                .is_some_and(|focus| {
                    unsafe { focus.GetCurrentPropertyValueEx(UIA_IsPasswordPropertyId, true) }
                        .is_ok_and(|value| exact_bool(value) == Some(true))
                })
        })
        .unwrap_or(false)
}

fn on_default_desktop() -> bool {
    let Ok(desktop) = (unsafe { OpenInputDesktop(Default::default(), false, DESKTOP_READOBJECTS) })
    else {
        return false;
    };
    let mut name = [0u16; 64];
    let read = unsafe {
        let result = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            std::mem::size_of_val(&name) as u32,
            None,
        );
        let _ = CloseDesktop(desktop);
        result
    };
    // Fail closed on Winlogon and any other non-default desktop.
    read.is_ok()
        && name.iter().position(|&unit| unit == 0).is_some_and(|end| {
            String::from_utf16_lossy(&name[..end]).eq_ignore_ascii_case("Default")
        })
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn integrity(process: HANDLE) -> Option<u32> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
    let token = Handle(token);
    // A mandatory label consists of a TOKEN_MANDATORY_LABEL followed by a SID.
    // Aligned storage exceeds both the label and SECURITY_MAX_SID_SIZE.
    let mut buffer = [0usize; 32];
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            Some(buffer.as_mut_ptr().cast()),
            std::mem::size_of_val(&buffer) as u32,
            &mut size,
        )
        .ok()?;
        let label = &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        Some(*GetSidSubAuthority(
            label.Label.Sid,
            u32::from(count.checked_sub(1)?),
        ))
    }
}

fn app_kind(process: HANDLE) -> AppKind {
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    }
    .is_err()
    {
        return AppKind::Other;
    }
    let name = buffer[..len as usize]
        .rsplit(|&unit| unit == b'\\' as u16)
        .next()
        .unwrap_or_default();
    match String::from_utf16_lossy(name).to_ascii_lowercase().as_str() {
        "notepad.exe" => AppKind::Notepad,
        "winword.exe" => AppKind::Word,
        "outlook.exe" | "olk.exe" => AppKind::Outlook,
        "msedge.exe" => AppKind::Edge,
        "chrome.exe" => AppKind::Chrome,
        "code.exe" | "code - insiders.exe" => AppKind::VsCode,
        _ => AppKind::Other,
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut name = [0u16; 256];
    let size = unsafe { GetClassNameW(hwnd, &mut name) };
    String::from_utf16_lossy(&name[..size.max(0) as usize])
}

fn focus_hwnd(window: Window) -> Option<HWND> {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(window.thread, &mut info) }.ok()?;
    let actual = Window::new(info.hwndFocus)?;
    (actual.pid == window.pid && unsafe { GetAncestor(info.hwndFocus, GA_ROOT) } == window.handle())
        .then_some(info.hwndFocus)
}

fn edit_message(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Result<usize, Reject> {
    let mut result = 0;
    if unsafe {
        SendMessageTimeoutW(
            hwnd,
            message,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
            50,
            Some(&mut result),
        )
    }
    .0 == 0
    {
        return Err(Unresponsive.into());
    }
    Ok(result)
}

fn window_style(hwnd: HWND) -> Result<u32, Reject> {
    unsafe {
        SetLastError(ERROR_SUCCESS);
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        if style == 0 && GetLastError() != ERROR_SUCCESS {
            return Err(NoFocusedField.into());
        }
        Ok(style as u32)
    }
}

fn exact_bool(mut value: VARIANT) -> Option<bool> {
    unsafe {
        let scalar = &value.Anonymous.Anonymous;
        let answer = (scalar.vt == VT_BOOL).then(|| scalar.Anonymous.boolVal.0 != 0);
        // Missing-property sentinels can contain an owned IUnknown.
        let _ = VariantClear(&mut value);
        answer
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodeArea {
    Terminal,
    Chat,
    Unspecified,
}

fn code_area(metadata: &str) -> CodeArea {
    let metadata = metadata.to_ascii_lowercase();
    if [
        "xterm",
        "integrated-terminal",
        "terminal-input",
        "terminal-instance",
        "terminal-editor",
    ]
    .iter()
    .any(|id| metadata.contains(id))
    {
        CodeArea::Terminal
    } else if ["chat-input", "chatinput", "interactive-input-part"]
        .iter()
        .any(|id| metadata.contains(id))
    {
        CodeArea::Chat
    } else {
        CodeArea::Unspecified
    }
}

fn aria_multiline(properties: &str) -> Option<bool> {
    properties.split(';').find_map(|property| {
        let (key, value) = property.trim().split_once('=')?;
        if key != "multiline" {
            return None;
        }
        match value {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    })
}

fn belongs(
    automation: &IUIAutomation,
    element: &IUIAutomationElement,
    mut hwnd: HWND,
    window: Window,
    app: AppKind,
) -> Result<bool, Reject> {
    let mut walk = None;
    let mut current = element.clone();
    let mut matches_window = false;
    let mut is_chat = false;
    for _ in 0..32 {
        if app == AppKind::VsCode {
            for metadata in [unsafe { current.CurrentAutomationId() }, unsafe {
                current.CurrentClassName()
            }]
            .into_iter()
            .flatten()
            {
                match code_area(&metadata.to_string()) {
                    CodeArea::Terminal => return Err(Terminal.into()),
                    CodeArea::Chat => is_chat = true,
                    CodeArea::Unspecified => {}
                }
            }
        }
        if let Some(native) = Window::new(hwnd) {
            if native.pid != window.pid || unsafe { GetAncestor(hwnd, GA_ROOT) } != window.handle()
            {
                return if matches_window {
                    Ok(is_chat)
                } else {
                    Err(NoFocusedField.into())
                };
            }
            matches_window = true;
            if hwnd == window.handle() || app != AppKind::VsCode {
                return Ok(is_chat);
            }
        }
        let walker = match &walk {
            Some(walker) => walker,
            None => walk.insert(unsafe { automation.RawViewWalker() }.map_err(|_| Unsupported)?),
        };
        match unsafe { walker.GetParentElement(&current) } {
            Ok(parent) => current = parent,
            Err(_) => break,
        }
        hwnd = unsafe { current.CurrentNativeWindowHandle() }.map_err(|_| NoFocusedField)?;
    }
    if matches_window {
        Ok(is_chat)
    } else {
        Err(NoFocusedField.into())
    }
}

fn inspect(automation: &IUIAutomation, require_explicit_password_false: bool) -> Result<Snapshot, Reject> {
    if !on_default_desktop() {
        return Err(Reject::Protected);
    }
    let window = Window::foreground().ok_or(NoFocusedField)?;
    if window.pid == std::process::id() {
        return Err(Reject::OwnProcess);
    }
    match class_name(window.handle()).to_ascii_lowercase().as_str() {
        "consolewindowclass" | "cascadia_hosting_window_class" => return Err(Terminal.into()),
        "tscshellcontainerclass" => return Err(RemoteDesktop.into()),
        _ => {}
    }

    let native_focus = focus_hwnd(window);
    let native_class = native_focus.map(class_name);
    let plain_edit = native_class
        .as_deref()
        .is_some_and(|class| class.eq_ignore_ascii_case("Edit"));
    let rich_edit = native_class
        .as_deref()
        .is_some_and(|class| class.to_ascii_lowercase().starts_with("richedit"));
    let native_editor = native_focus.filter(|_| plain_edit || rich_edit);
    let native_style = native_editor.map(window_style).transpose()?;
    if native_style.is_some_and(|style| style & ES_PASSWORD as u32 != 0) {
        return Err(Reject::Protected);
    }
    if plain_edit
        && edit_message(
            native_editor.ok_or(NoFocusedField)?,
            EM_GETPASSWORDCHAR,
            WPARAM(0),
            LPARAM(0),
        )? != 0
    {
        return Err(Reject::Protected);
    }

    let element = unsafe { automation.GetFocusedElement() }.map_err(|_| NoFocusedField)?;
    let password = unsafe { element.GetCurrentPropertyValueEx(UIA_IsPasswordPropertyId, true) }
        .map(exact_bool)
        .map_err(|_| Unsupported)?;
    if password == Some(true) {
        return Err(Reject::Protected);
    }

    let process = Handle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, window.pid) }
            .map_err(|_| Elevated)?,
    );
    let our_integrity = integrity(unsafe { GetCurrentProcess() }).ok_or(Elevated)?;
    if integrity(process.0).ok_or(Elevated)? > our_integrity {
        return Err(Elevated.into());
    }
    if unsafe { element.CurrentProcessId() }.map_err(|_| NoFocusedField)? as u32 != window.pid {
        return Err(NoFocusedField.into());
    }
    let app = app_kind(process.0);
    let element_hwnd =
        unsafe { element.CurrentNativeWindowHandle() }.map_err(|_| NoFocusedField)?;
    let is_code_chat = belongs(automation, &element, element_hwnd, window, app)?;
    // Only trust Win32 styles or messages for the exact native element with focus.
    let styles = native_style.filter(|_| native_editor == Some(element_hwnd));
    let office_body = match (app, native_class.as_deref()) {
        (AppKind::Word, Some(class)) if class.eq_ignore_ascii_case("_WwG") => {
            Some(OfficeKind::Word)
        }
        (AppKind::Outlook, Some(class))
            if class.eq_ignore_ascii_case("_WwG")
                && class_name(window.handle()).eq_ignore_ascii_case("rctrl_renwnd32") =>
        {
            Some(OfficeKind::Outlook)
        }
        _ => None,
    };
    if password != Some(false)
        && (require_explicit_password_false
            || (!(plain_edit && styles.is_some()) && office_body.is_none()))
    {
        return Err(Unsupported.into());
    }
    if !unsafe { element.CurrentIsEnabled() }
        .map_err(|_| NoFocusedField)?
        .as_bool()
        || !unsafe { element.CurrentHasKeyboardFocus() }
            .map_err(|_| NoFocusedField)?
            .as_bool()
        || !unsafe { element.CurrentIsKeyboardFocusable() }
            .map_err(|_| NoFocusedField)?
            .as_bool()
    {
        return Err(NoFocusedField.into());
    }
    let control = unsafe { element.CurrentControlType() }.map_err(|_| Unsupported)?;
    if control != UIA_EditControlTypeId && control != UIA_DocumentControlTypeId {
        return Err(NoFocusedField.into());
    }
    if let Some(kind) = office_body {
        let body = native_focus.ok_or(NoFocusedField)?;
        let (caret, _, _) = office::caret(kind, window.handle(), body).ok_or(Unsupported)?;
        if Window::foreground() != Some(window)
            || focus_hwnd(window) != Some(body)
            || !on_default_desktop()
        {
            return Err(NoFocusedField.into());
        }
        return Ok(Snapshot {
            window,
            native_focus: Some(body.0 as usize),
            native_edit: None,
            element,
            caret: Caret::Office(caret),
            info: TargetInfo {
                app,
                multiline: Some(true),
            },
        });
    }
    let native_readonly = styles.map(|style| style & ES_READONLY as u32 != 0);
    let value =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
            .ok();
    let value_readonly = value
        .map(|value| unsafe { value.CurrentIsReadOnly() }.map(|value| value.as_bool()))
        .transpose()
        .map_err(|_| Unsupported)?;
    if native_readonly == Some(true) || value_readonly == Some(true) {
        return Err(ReadOnly.into());
    }

    let multiline = styles
        .map(|style| style & ES_MULTILINE as u32 != 0)
        .or(
            (app == AppKind::Notepad && control == UIA_DocumentControlTypeId || is_code_chat)
                .then_some(true),
        )
        .or_else(|| {
            if !window.chromium_widget() && app != AppKind::Outlook {
                return None;
            }
            unsafe { element.CurrentAriaProperties() }
                .ok()
                .and_then(|properties| aria_multiline(&properties.to_string()))
        });
    // Without WordEditor, restrict Outlook paste to verified multiline
    // editors; recipient widgets and other header controls stay excluded.
    if app == AppKind::Outlook
        && (multiline != Some(true) || insertion_method() != InsertionMethod::Paste)
    {
        return Err(Unsupported.into());
    }
    let allow_selection = multiline == Some(false);
    let native_selection = |hwnd| -> Result<[u32; 2], Reject> {
        let [start, end] = edit_selection(hwnd)?;
        if start != end && !allow_selection {
            return Err(Selection.into());
        }
        Ok([start, end])
    };
    let caret =
        match unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
        {
            Ok(text) => {
                let selection = unsafe { text.GetSelection() }.map_err(|_| Selection)?;
                if unsafe { selection.Length() }.map_err(|_| Selection)? != 1 {
                    return Err(Selection.into());
                }
                let range = unsafe { selection.GetElement(0) }.map_err(|_| Selection)?;
                if !allow_selection
                    && unsafe {
                        range.CompareEndpoints(
                            TextPatternRangeEndpoint_Start,
                            &range,
                            TextPatternRangeEndpoint_End,
                        )
                    }
                    .map_err(|_| Selection)?
                        != 0
                {
                    return Err(Selection.into());
                }
                let text_readonly = unsafe { range.GetAttributeValue(UIA_IsReadOnlyAttributeId) }
                    .ok()
                    .and_then(exact_bool);
                if text_readonly == Some(true) {
                    return Err(ReadOnly.into());
                }
                if [native_readonly, value_readonly, text_readonly]
                    .iter()
                    .all(Option::is_none)
                {
                    return Err(Unsupported.into());
                }
                Caret::Text(unsafe { range.Clone() }.map_err(|_| Selection)?)
            }
            Err(_) if plain_edit && native_readonly == Some(false) => Caret::Edit {
                hwnd: element_hwnd,
                selection: native_selection(element_hwnd)?,
            },
            Err(_) => return Err(Unsupported.into()),
        };
    let native_edit = match (styles, native_class.as_deref().and_then(literal_class)) {
        (Some(style), Some(rich)) if unsafe { IsWindowUnicode(element_hwnd) }.as_bool() => {
            Some(NativeEdit {
                hwnd: element_hwnd.0 as usize,
                selection: native_selection(element_hwnd)?,
                rich,
                multiline: style & ES_MULTILINE as u32 != 0,
            })
        }
        _ => None,
    };
    if Window::foreground() != Some(window) || focus_hwnd(window) != native_focus {
        return Err(NoFocusedField.into());
    }
    if !on_default_desktop() {
        return Err(Reject::Protected);
    }
    Ok(Snapshot {
        window,
        native_focus: native_focus.map(|hwnd| hwnd.0 as usize),
        native_edit,
        element,
        caret,
        info: TargetInfo { app, multiline },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_separates_terminal_and_chat_and_does_not_guess_multiline() {
        assert_eq!(code_area("xterm-helper-textarea"), CodeArea::Terminal);
        assert_eq!(code_area("chat-input-container"), CodeArea::Chat);
        assert_eq!(
            code_area("chat-input integrated-terminal"),
            CodeArea::Terminal
        );
        assert_eq!(code_area("monaco-editor"), CodeArea::Unspecified);
        assert_eq!(aria_multiline("required=false;multiline=true"), Some(true));
        assert_eq!(aria_multiline("multiline=false"), Some(false));
        assert_eq!(aria_multiline("readonly=false"), None);
    }

    #[test]
    fn native_write_gate_stays_revocable_after_attempt_is_consumed() {
        let (jobs, _unused) = mpsc::sync_channel(1);
        let token = TargetToken(Arc::new(Identity {
            worker: Arc::new(Worker {
                jobs,
                expired: Arc::new(AtomicBool::new(false)),
            }),
            id: 1,
            window: Window {
                hwnd: 1,
                thread: 1,
                pid: 1,
            },
            native_focus: None,
            native_edit: Mutex::new(None),
            office: false,
            info: TargetInfo {
                app: AppKind::Other,
                multiline: None,
            },
            late: None,
            qualified_late: OnceLock::new(),
            attempted: AtomicBool::new(true),
            write_permission: OnceLock::new(),
            protection: OnceLock::new(),
            changed: OnceLock::new(),
            lifetime: Arc::new(()),
        }));
        let allowed = Arc::new(AtomicBool::new(true));
        bind_write_permission(&token, &allowed);
        assert!(write_permitted(&token));
        allowed.store(false, Ordering::Release);
        assert!(!write_permitted(&token));
    }

    #[test]
    fn hung_request_retires_its_apartment_without_waiting() {
        let probe = UiaTargetProbe::for_fixture(0, false);
        let first = probe.worker().unwrap();
        assert_eq!(first.request(|_| 7), Some(7));
        let (resume, unblock) = mpsc::channel::<()>();
        let started = Instant::now();
        assert_eq!(first.request(move |_| unblock.recv().ok()), None);
        assert!(first.expired());
        assert!(started.elapsed() < Duration::from_secs(1));

        // The first apartment is still blocked when the second one answers.
        let second = probe.worker().unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(second.request(|_| 8), Some(8));
        let _ = resume.send(());
    }
}
