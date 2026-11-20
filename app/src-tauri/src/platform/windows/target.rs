use super::office::{self, OfficeKind};
use crate::ports::{
    AppKind, InsertOutcome, NotWrittenReason, TargetChange, TargetInfo, TargetProbe, TargetToken,
    TargetVerdict, UncertainReason, WatchGuard,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
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
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
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

#[derive(Default)]
pub struct UiaTargetProbe {
    worker: Mutex<Option<Arc<Worker>>>,
}

impl UiaTargetProbe {
    pub fn new() -> Self {
        Self::default()
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
                let automation: IUIAutomation = match unsafe {
                    CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                } {
                    Ok(automation) => automation,
                    Err(_) => return,
                };
                if let Ok(settings) = automation.cast::<IUIAutomation2>() {
                    unsafe {
                        let _ = settings.SetConnectionTimeout(250);
                        let _ = settings.SetTransactionTimeout(250);
                    }
                }
                let mut state = State {
                    automation,
                    records: HashMap::new(),
                    last_id: 0,
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

struct Identity {
    worker: Arc<Worker>,
    id: u64,
    window: Window,
    native_focus: Option<usize>,
    native_edit: Option<NativeEdit>,
    office: bool,
    attempted: AtomicBool,
    changed: OnceLock<TargetChange>,
    lifetime: Arc<()>,
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
    last_id: u64,
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
        deadline: Instant,
    ) -> TargetVerdict {
        let snapshot = loop {
            if expected.is_some() && Window::foreground() != expected {
                return Reject::from(NoFocusedField).into();
            }
            match inspect(&self.automation) {
                Ok(snapshot) if expected.is_none_or(|window| window == snapshot.window) => {
                    break snapshot;
                }
                Ok(_) => return Reject::from(NoFocusedField).into(),
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
        self.last_id += 1;
        let identity = Identity {
            worker,
            id: self.last_id,
            window: snapshot.window,
            native_focus: snapshot.native_focus,
            native_edit: snapshot.native_edit,
            office: matches!(snapshot.caret, Caret::Office(_)),
            attempted: AtomicBool::new(false),
            changed: OnceLock::new(),
            lifetime: Arc::new(()),
        };
        let info = snapshot.info;
        self.records
            .insert(identity.id, (Arc::downgrade(&identity.lifetime), snapshot));
        TargetVerdict::Eligible {
            token: TargetToken(Arc::new(identity)),
            info,
        }
    }

    fn revalidate(&mut self, id: u64) -> Result<(), TargetChange> {
        use TargetChange::*;
        let old = &self.records.get(&id).ok_or(Unknown)?.1;
        if old.window.chromium_widget()
            && Window::foreground() == Some(old.window)
            && matches!(old.caret, Caret::Text(_))
        {
            return recheck_chromium(&self.automation, old);
        }
        let now = inspect(&self.automation);
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
            || !unsafe { self.automation.CompareElements(&old.element, &now.element) }
                .map_err(|_| Unknown)?
                .as_bool()
        {
            return Err(FocusMoved);
        }
        if old.native_edit != now.native_edit {
            return Err(SelectionChanged);
        }
        match (&old.caret, &now.caret) {
            (Caret::Office(a), Caret::Office(b)) => {
                if a.same(b) {
                    Ok(())
                } else {
                    Err(SelectionChanged)
                }
            }
            (Caret::Text(a), Caret::Text(b)) => {
                for endpoint in [TextPatternRangeEndpoint_Start, TextPatternRangeEndpoint_End] {
                    if unsafe { a.CompareEndpoints(endpoint, b, endpoint) }.map_err(|_| Unknown)?
                        != 0
                    {
                        return Err(SelectionChanged);
                    }
                }
                Ok(())
            }
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
                if i == j {
                    Ok(())
                } else {
                    Err(SelectionChanged)
                }
            }
            _ => Err(Unknown),
        }
    }
}

fn identity(token: &TargetToken) -> Option<&Identity> {
    token.0.downcast_ref()
}

pub(super) fn revalidate_token(token: &TargetToken) -> Result<(), TargetChange> {
    revalidate(token, false)
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
    let id = target.id;
    let budget = if target.window.chromium_widget() {
        CHROMIUM_RECHECK_BUDGET
    } else {
        REQUEST_BUDGET
    };
    let result = target
        .worker
        .request_until(Instant::now() + budget, move |state| state.revalidate(id))
        .unwrap_or(Err(TargetChange::Unknown));
    // The insertion path owns focus checks after committing to an attempt.
    // An in-flight poll may observe the caret moving from our own first packet.
    if watching && consumed(token) {
        return Ok(());
    }
    if let Err(change) = result {
        let _ = target.changed.set(change);
    }
    target.changed.get().copied().map_or(result, Err)
}

pub(super) fn write_focus(token: &TargetToken) -> Option<HWND> {
    let id = identity(token)?;
    if !on_default_desktop()
        || Window::foreground() != Some(id.window)
        || id.changed.get().is_some()
        || id.worker.expired()
    {
        return None;
    }
    let focus = focus_hwnd(id.window);
    (focus.map(|hwnd| hwnd.0 as usize) == id.native_focus)
        .then(|| focus.unwrap_or_else(|| id.window.handle()))
}

pub(super) fn arm_single_write(token: &TargetToken) -> bool {
    write_focus(token).is_some()
        && identity(token).is_some_and(|id| !id.attempted.swap(true, Ordering::AcqRel))
}

pub(super) fn native_edit(token: &TargetToken) -> Option<NativeEdit> {
    identity(token)?.native_edit
}

pub(super) fn office_body(token: &TargetToken) -> bool {
    identity(token).is_some_and(|id| id.office)
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

fn recheck_chromium(automation: &IUIAutomation, old: &Snapshot) -> Result<(), TargetChange> {
    use TargetChange::*;
    let focus = checked_chromium_focus(automation, old)?;
    let Caret::Text(saved) = &old.caret else {
        return Err(Unknown);
    };
    // Revalidate the range endpoints, not the object identity of the range.
    // A selected single-line range must match at both ends.
    let text: IUIAutomationTextPattern =
        unsafe { focus.GetCurrentPatternAs(UIA_TextPatternId) }.map_err(|_| Unknown)?;
    let selected = unsafe { text.GetSelection() }.map_err(|_| Unknown)?;
    if unsafe { selected.Length() }.map_err(|_| Unknown)? != 1 {
        return Err(SelectionChanged);
    }
    let range = unsafe { selected.GetElement(0) }.map_err(|_| Unknown)?;
    let start = TextPatternRangeEndpoint_Start;
    let end = TextPatternRangeEndpoint_End;
    if (old.info.multiline != Some(false)
        && unsafe { range.CompareEndpoints(start, &range, end) }.map_err(|_| Unknown)? != 0)
        || unsafe { saved.CompareEndpoints(start, &range, start) }.map_err(|_| Unknown)? != 0
        || unsafe { saved.CompareEndpoints(end, &range, end) }.map_err(|_| Unknown)? != 0
    {
        return Err(SelectionChanged);
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
    Ok(())
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
        let deadline = Instant::now()
            + if expected.is_some() {
                CHROMIUM_CAPTURE_BUDGET
            } else {
                REQUEST_BUDGET
            };
        self.worker()
            .and_then(|worker| {
                let owner = worker.clone();
                worker.request_until(deadline, move |state| {
                    state.capture(owner, expected, deadline)
                })
            })
            .unwrap_or_else(|| Reject::from(Unresponsive).into())
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
                while !worker_stop.load(Ordering::Acquire) && !consumed(&worker_token) {
                    if let Err(change) = revalidate(&worker_token, true) {
                        if !worker_stop.swap(true, Ordering::AcqRel) {
                            if let Ok(mut slot) = worker_callback.lock() {
                                if let Some(callback) = slot.take() {
                                    callback(change);
                                }
                            }
                        }
                        return;
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
    window: Window,
    app: AppKind,
) -> Result<bool, Reject> {
    let walk = unsafe { automation.RawViewWalker() }.map_err(|_| Unsupported)?;
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
        let hwnd = unsafe { current.CurrentNativeWindowHandle() }.map_err(|_| NoFocusedField)?;
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
        match unsafe { walk.GetParentElement(&current) } {
            Ok(parent) => current = parent,
            Err(_) => break,
        }
    }
    if matches_window {
        Ok(is_chat)
    } else {
        Err(NoFocusedField.into())
    }
}

fn inspect(automation: &IUIAutomation) -> Result<Snapshot, Reject> {
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
    let is_code_chat = belongs(automation, &element, window, app)?;

    let element_hwnd =
        unsafe { element.CurrentNativeWindowHandle() }.map_err(|_| NoFocusedField)?;
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
    // Never put keyboard packets into the new Outlook or an Outlook header.
    if app == AppKind::Outlook && office_body.is_none() {
        return Err(Unsupported.into());
    }
    if password != Some(false) && !(plain_edit && styles.is_some()) && office_body.is_none() {
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
            if !window.chromium_widget() || control != UIA_EditControlTypeId {
                return None;
            }
            unsafe { element.CurrentAriaProperties() }
                .ok()
                .and_then(|properties| aria_multiline(&properties.to_string()))
        });
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
    fn hung_request_retires_its_apartment_without_waiting() {
        let probe = UiaTargetProbe::new();
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
