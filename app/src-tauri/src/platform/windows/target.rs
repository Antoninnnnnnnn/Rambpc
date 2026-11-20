use crate::ports::{
    AppKind, TargetChange, TargetInfo, TargetProbe, TargetToken, TargetVerdict, UncertainReason,
    WatchGuard,
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
        WindowsAndMessaging::*,
    },
};
use windows::core::{Interface, PWSTR};

const REQUEST_BUDGET: Duration = Duration::from_millis(300);
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
        if self.expired() {
            return None;
        }
        let deadline = Instant::now() + REQUEST_BUDGET;
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
}

struct Identity {
    worker: Arc<Worker>,
    id: u64,
    window: Window,
    native_focus: Option<usize>,
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
    element: IUIAutomationElement,
    caret: Caret,
    info: TargetInfo,
}

enum Caret {
    Text(IUIAutomationTextRange),
    Edit { hwnd: HWND, index: u32 },
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
    fn capture(&mut self, worker: Arc<Worker>) -> TargetVerdict {
        let snapshot = match inspect(&self.automation) {
            Ok(snapshot) => snapshot,
            Err(reject) => return reject.into(),
        };
        self.last_id += 1;
        let identity = Identity {
            worker,
            id: self.last_id,
            window: snapshot.window,
            native_focus: snapshot.native_focus,
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
        match (&old.caret, &now.caret) {
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
            (Caret::Edit { hwnd: a, index: i }, Caret::Edit { hwnd: b, index: j }) if a == b => {
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
    let target = identity(token).ok_or(TargetChange::Unknown)?;
    if let Some(&change) = target.changed.get() {
        return Err(change);
    }
    let id = target.id;
    let result = target
        .worker
        .request(move |state| state.revalidate(id))
        .unwrap_or(Err(TargetChange::Unknown));
    if let Err(change) = result {
        let _ = target.changed.set(change);
    }
    target.changed.get().copied().map_or(result, Err)
}

pub(super) fn arm_single_write(token: &TargetToken) -> bool {
    identity(token).is_some_and(|id| {
        on_default_desktop()
            && Window::foreground() == Some(id.window)
            && focus_hwnd(id.window).map(|hwnd| hwnd.0 as usize) == id.native_focus
            && id.changed.get().is_none()
            && !id.worker.expired()
            && !id.attempted.swap(true, Ordering::AcqRel)
    })
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
        self.worker()
            .and_then(|worker| {
                let owner = worker.clone();
                worker.request(move |state| state.capture(owner))
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
                while !worker_stop.load(Ordering::Acquire) {
                    if let Err(change) = revalidate_token(&worker_token) {
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

fn belongs(
    automation: &IUIAutomation,
    element: &IUIAutomationElement,
    window: Window,
    app: AppKind,
) -> Result<(), Reject> {
    let walk = unsafe { automation.RawViewWalker() }.map_err(|_| Unsupported)?;
    let mut current = element.clone();
    let mut matches_window = false;
    for _ in 0..32 {
        if app == AppKind::VsCode {
            let is_terminal = |id: windows::core::BSTR| {
                let id = id.to_string().to_ascii_lowercase();
                id.contains("xterm")
                    || id.contains("integrated-terminal")
                    || id.contains("terminal-input")
                    || id.contains("terminal-instance")
            };
            if unsafe { current.CurrentAutomationId() }.is_ok_and(is_terminal)
                || unsafe { current.CurrentClassName() }.is_ok_and(is_terminal)
            {
                return Err(Terminal.into());
            }
        }
        let hwnd = unsafe { current.CurrentNativeWindowHandle() }.map_err(|_| NoFocusedField)?;
        if let Some(native) = Window::new(hwnd) {
            if native.pid != window.pid || unsafe { GetAncestor(hwnd, GA_ROOT) } != window.handle()
            {
                return if matches_window {
                    Ok(())
                } else {
                    Err(NoFocusedField.into())
                };
            }
            matches_window = true;
            if hwnd == window.handle() || app != AppKind::VsCode {
                return Ok(());
            }
        }
        match unsafe { walk.GetParentElement(&current) } {
            Ok(parent) => current = parent,
            Err(_) => break,
        }
    }
    if matches_window {
        Ok(())
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
    belongs(automation, &element, window, app)?;

    let element_hwnd =
        unsafe { element.CurrentNativeWindowHandle() }.map_err(|_| NoFocusedField)?;
    // Only trust Win32 styles or messages for the exact native element with focus.
    let styles = native_style.filter(|_| native_editor == Some(element_hwnd));
    if password != Some(false) && !(plain_edit && styles.is_some()) {
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

    let caret =
        match unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
        {
            Ok(text) => {
                let selection = unsafe { text.GetSelection() }.map_err(|_| Selection)?;
                if unsafe { selection.Length() }.map_err(|_| Selection)? != 1 {
                    return Err(Selection.into());
                }
                let range = unsafe { selection.GetElement(0) }.map_err(|_| Selection)?;
                if unsafe {
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
            Err(_) if plain_edit && native_readonly == Some(false) => {
                // EM_GETSEL is a system message and marshals its optional pointers
                // across processes. The Win32 Edit provider lacks TextPattern on
                // some releases. No content is read in this fallback.
                let mut start = u32::MAX;
                let mut end = u32::MAX;
                edit_message(
                    element_hwnd,
                    EM_GETSEL,
                    WPARAM((&mut start as *mut u32) as usize),
                    LPARAM((&mut end as *mut u32) as isize),
                )?;
                if start == u32::MAX || start != end {
                    return Err(Selection.into());
                }
                Caret::Edit {
                    hwnd: element_hwnd,
                    index: start,
                }
            }
            Err(_) => return Err(Unsupported.into()),
        };
    if Window::foreground() != Some(window) || focus_hwnd(window) != native_focus {
        return Err(NoFocusedField.into());
    }
    if !on_default_desktop() {
        return Err(Reject::Protected);
    }
    let multiline = styles
        .map(|style| style & ES_MULTILINE as u32 != 0)
        .or((app == AppKind::Notepad && control == UIA_DocumentControlTypeId).then_some(true));
    Ok(Snapshot {
        window,
        native_focus: native_focus.map(|hwnd| hwnd.0 as usize),
        element,
        caret,
        info: TargetInfo { app, multiline },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
