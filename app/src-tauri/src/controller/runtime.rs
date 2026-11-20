//! I/O driver for the reducer. The actor owns resources; workers return tagged
//! observations only. No callback is allowed to transition a take or write UI.

use super::{
    ControllerDeps, ControllerHandle, ControllerInit, CopyError,
    state::{Effect, Input, Now, State},
};
use crate::{
    ipc::PersonalRules,
    ipc::{DraftView, InterruptReason, Phase, UiState, UnavailableReason},
    model::TakeId,
    ports::{
        ClipboardError, InsertOutcome, MicError, NotWrittenReason, TargetChange, TargetVerdict,
        WatchGuard,
    },
    rambler::{self, CaptureHealth, OpenedMic, RamblerAccess, Segment},
    storage::history::{History, Ticket},
};
use rambler_engine::Engine;
use std::{
    collections::VecDeque,
    future::pending,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::{JoinError, JoinHandle, JoinSet},
    time::{Instant, MissedTickBehavior, interval},
};

const COMMAND_CAPACITY: usize = 32;
const SEGMENT_CAPACITY: usize = 16;
const POLL: Duration = Duration::from_millis(50);

type CopyReply = oneshot::Sender<Result<(), CopyError>>;

pub(super) enum Command {
    Input(Input),
    Access(Option<RamblerAccess>),
    Draft(oneshot::Sender<Option<DraftView>>),
    Copy(CopyReply),
}

// The shell methods are synchronous and cannot backpressure their callers.
// Use a bounded mailbox for routine work and keep an ordered critical spill
// only when saturated. This preserves cancellation and shutdown without
// waiting on a GUI callback or spawning detached, reordered senders.
type Overflow = Arc<Mutex<VecDeque<Command>>>;

#[derive(Clone)]
pub(super) struct Inbox {
    tx: mpsc::Sender<Command>,
    overflow: Overflow,
}

impl Inbox {
    pub(super) fn send(&self, command: Command) {
        // The synchronous shell API cannot await capacity. Keep ordered delivery
        // of edges and safety controls even at saturation. Do not spill ambient
        // updates onto the ordered lane or spawn one task per command.
        let mut spill = self.overflow.lock().unwrap();
        if self.tx.is_closed() {
            return;
        }
        let command = if spill.is_empty() {
            match self.tx.try_send(command) {
                Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => return,
                Err(mpsc::error::TrySendError::Full(command)) => command,
            }
        } else {
            command
        };
        match (spill.back_mut(), command) {
            // Preserve the latest preferences, without moving them past a trigger.
            (Some(Command::Input(Input::Settings(old))), Command::Input(Input::Settings(new))) => {
                *old = new
            }
            (Some(Command::Input(Input::Hotkey(old))), Command::Input(Input::Hotkey(new)))
                if *old == new => {}
            (Some(Command::Input(Input::Finish(old))), Command::Input(Input::Finish(new)))
                if *old == new => {}
            (Some(Command::Input(Input::Cancel(old))), Command::Input(Input::Cancel(new)))
                if *old == new => {}
            // Bound backlog from repeatable UI requests. Keep safety controls,
            // key edges and preference/privacy commits ordered and lossless.
            (_, Command::Draft(reply)) => {
                let _ = reply.send(None);
            }
            (_, Command::Copy(reply)) => {
                let _ = reply.send(Err(ClipboardError::Busy.into()));
            }
            (_, Command::Input(Input::StartInternal | Input::Retry)) => (),
            (_, command) => spill.push_back(command),
        }
    }
}

struct Commands {
    rx: mpsc::Receiver<Command>,
    overflow: Overflow,
}

impl Commands {
    async fn next(&mut self) -> Option<Command> {
        {
            // A sender must hold this same lock before using the bounded queue,
            // so the queue is always drained before its FIFO spill is consumed.
            let mut spill = self.overflow.lock().unwrap();
            if let Ok(command) = self.rx.try_recv() {
                return Some(command);
            }
            if let Some(command) = spill.pop_front() {
                return Some(command);
            }
        }
        self.rx.recv().await
    }

    fn close(&mut self) {
        let mut spill = self.overflow.lock().unwrap();
        self.rx.close();
        while self.rx.try_recv().is_ok() {}
        spill.clear();
    }
}

struct TakeIo {
    id: TakeId,
    live: Arc<AtomicBool>,
    can_enter_inserter: Arc<AtomicBool>,
    change: Arc<OnceLock<TargetChange>>,
    change_read: bool,
    guard: Option<WatchGuard>,
    health: Arc<CaptureHealth>,
    history: Option<Ticket>,
    pending_open: Option<OpenedMic>,
    stream: Option<Box<dyn crate::ports::MicStream>>,
}

impl TakeIo {
    fn new(id: TakeId, origin: Instant) -> Self {
        Self {
            id,
            live: Arc::new(AtomicBool::new(true)),
            can_enter_inserter: Arc::new(AtomicBool::new(true)),
            change: Arc::new(OnceLock::new()),
            change_read: false,
            guard: None,
            health: Arc::new(CaptureHealth::new(origin)),
            history: None,
            pending_open: None,
            stream: None,
        }
    }
}

struct Probe {
    id: TakeId,
    watch: bool,
    live: Arc<AtomicBool>,
    can_enter_inserter: Arc<AtomicBool>,
    change: Arc<OnceLock<TargetChange>>,
}

enum Work {
    Target(TakeId, TargetVerdict, Option<WatchGuard>),
    Opened(TakeId, Result<OpenedMic, MicError>),
    MicFree(TakeId),
    Inserted(TakeId, InsertOutcome, Option<TargetChange>),
    Copied(TakeId, Result<(), CopyError>, CopyReply),
    Cleaned,
}

struct Collector {
    id: TakeId,
    task: JoinHandle<Result<(), InterruptReason>>,
    rx: mpsc::Receiver<Segment>,
}

type Warm = (Option<u64>, Result<Option<Arc<Engine>>, UnavailableReason>);

struct Runtime {
    deps: ControllerDeps,
    state: State,
    history: Option<History>,
    state_tx: watch::Sender<UiState>,
    level_tx: watch::Sender<f32>,
    origin: Instant,
    access: Option<RamblerAccess>,
    engine: Option<Arc<Engine>>,
    warm: Option<JoinHandle<Warm>>,
    connect_next: Option<u64>,
    reconfigure: bool,
    io: Option<TakeIo>,
    collector: Option<Collector>,
    jobs: JoinSet<Work>,
    probe_running: bool,
    probe_next: Option<Probe>,
    copy: Option<(TakeId, Arc<AtomicBool>)>,
    #[cfg(test)]
    test_config: Option<TestConnection>,
}

pub(super) fn spawn(
    deps: ControllerDeps,
    init: ControllerInit,
    profile: Option<(Arc<PersonalRules>, History)>,
) -> ControllerHandle {
    make(
        deps,
        init,
        profile,
        #[cfg(test)]
        None,
    )
}

fn make(
    deps: ControllerDeps,
    init: ControllerInit,
    profile: Option<(Arc<PersonalRules>, History)>,
    #[cfg(test)] test_config: Option<TestConnection>,
) -> ControllerHandle {
    let mut state = State::new(init.settings, init.consent, init.access.is_some());
    let history = profile.map(|(rules, history)| {
        state.use_profile(rules);
        history
    });
    let (state_tx, state_rx) = watch::channel(state.ui(Now { mono: 0, unix: 0 }));
    let (level_tx, level_rx) = watch::channel(0.0);
    let (stopped_tx, stopped) = watch::channel(false);
    let (tx, rx) = mpsc::channel(COMMAND_CAPACITY);
    let overflow = Overflow::default();
    let mut runtime = Runtime {
        deps,
        state,
        history,
        state_tx,
        level_tx,
        origin: Instant::now(),
        access: init.access,
        engine: None,
        warm: None,
        connect_next: None,
        reconfigure: false,
        io: None,
        collector: None,
        jobs: JoinSet::new(),
        probe_running: false,
        probe_next: None,
        copy: None,
        #[cfg(test)]
        test_config,
    };
    let overflow_for_actor = overflow.clone();
    tokio::spawn(async move {
        runtime
            .run(Commands {
                rx,
                overflow: overflow_for_actor,
            })
            .await;
        stopped_tx.send_replace(true);
    });
    ControllerHandle {
        inbox: Inbox { tx, overflow },
        state_rx,
        level_rx,
        stopped,
    }
}

impl Runtime {
    fn now(&self) -> Now {
        Now {
            mono: self.origin.elapsed().as_millis() as u64,
            unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }

    async fn run(&mut self, mut commands: Commands) {
        self.apply(Input::Retry);
        let mut timer = interval(POLL);
        timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                command = commands.next() => match command {
                    Some(Command::Input(Input::Shutdown)) | None => break,
                    Some(Command::Input(input)) => {
                        // Stop must check frame arrival before treating release as EOF.
                        if matches!(input, Input::Hotkey(_) | Input::Finish(_)) { self.inspect(); }
                        self.apply(input);
                    }
                    Some(Command::Access(access)) => {
                        self.access = access;
                        self.apply(Input::Access(self.access.is_some()));
                    }
                    Some(Command::Draft(reply)) => {
                        self.inspect();
                        let _ = reply.send(self.state.draft());
                    }
                    Some(Command::Copy(reply)) => {
                        self.inspect();
                        self.copy(reply);
                    }
                },
                _ = timer.tick() => self.inspect(),
                result = join(&mut self.warm) => {
                    self.warm = None;
                    self.inspect();
                    self.warmed(result);
                },
                Some(result) = self.jobs.join_next(), if !self.jobs.is_empty() => {
                    self.inspect();
                    if let Ok(work) = result { self.work(work); }
                },
                event = next_segment(&mut self.collector) => match event {
                    CollectEvent::Segment(id, segment) => {
                        self.inspect();
                        self.apply(Input::Delivered(id, segment));
                    }
                    CollectEvent::Finished(id, result) => self.collected(id, result),
                },
            }
            self.inspect();
            self.publish();
        }
        commands.close();
        self.apply(Input::Shutdown);
        self.publish();
        // Join all blocking workers, including driver opens that could not be
        // cancelled. A stale returned stream is explicitly aborted and joined.
        while let Some(result) = self.jobs.join_next().await {
            if let Ok(work) = result {
                self.work(work);
            }
        }
        if let Some(warm) = self.warm.take()
            && let Ok((_, Ok(Some(engine)))) = warm.await
        {
            let _ = engine.close().await;
        }
        if let Some(engine) = self.engine.take() {
            let _ = engine.close().await;
        }
        if let Some(history) = self.history.take() {
            // A FIFO status request is also a barrier for completed takes.
            let _ = tokio::task::spawn_blocking(move || history.status()).await;
        }
    }

    fn publish(&self) {
        let ui = self.state.ui(self.now());
        let level = if ui.phase == Phase::Listening {
            self.io.as_ref().map_or(0.0, |io| io.health.level.get())
        } else {
            0.0
        };
        self.state_tx.send_if_modified(|current| {
            if *current == ui {
                false
            } else {
                *current = ui;
                true
            }
        });
        self.level_tx.send_if_modified(|current| {
            if *current == level {
                false
            } else {
                *current = level;
                true
            }
        });
    }

    fn inspect(&mut self) {
        // A focus displacement is latched synchronously by its callback, not
        // queued behind completed text where it could lose the insertion race.
        let change = self.io.as_mut().and_then(|io| {
            if io.change_read {
                return None;
            }
            let change = *io.change.get()?;
            io.change_read = true;
            Some((io.id, change))
        });
        if let Some((id, change)) = change {
            self.apply(Input::TargetChanged(id, change));
        }
        if let Some(io) = &self.io
            && let Some(reason) = io.health.failure()
        {
            self.apply(Input::MicFault(io.id, reason));
        }
        self.apply(Input::Tick {
            last_frame: self.io.as_ref().and_then(|io| io.health.last_frame()),
        });
    }

    fn apply(&mut self, input: Input) {
        let effects = self.state.step(input, self.now());
        if let Some((id, live)) = &self.copy
            && self.state.draft_id() != Some(*id)
        {
            live.store(false, Ordering::Release);
        }
        for effect in effects {
            self.effect(effect);
        }
    }

    fn take_io(&mut self, id: TakeId) -> &mut TakeIo {
        let origin = self.origin;
        self.io.get_or_insert_with(|| TakeIo::new(id, origin))
    }

    fn effect(&mut self, effect: Effect) {
        match effect {
            Effect::Reconfigure(epoch) => {
                self.connect_next = epoch;
                self.reconfigure = true;
                self.warm_if_idle();
            }
            Effect::Probe { take, watch } => {
                let io = self.take_io(take);
                self.probe_next = Some(Probe {
                    id: take,
                    watch,
                    live: io.live.clone(),
                    can_enter_inserter: io.can_enter_inserter.clone(),
                    change: io.change.clone(),
                });
                self.probe_if_idle();
            }
            Effect::Open { take, mic } => {
                let ticket = self.history.as_ref().and_then(History::ticket);
                let io = self.take_io(take);
                io.history = ticket;
                let (health, live) = (io.health.clone(), io.live.clone());
                let microphone = self.deps.microphone.clone();
                self.jobs.spawn_blocking(move || {
                    Work::Opened(take, rambler::open(&*microphone, &mic, health, live))
                });
            }
            Effect::Collect { take, language } => {
                let OpenedMic {
                    stream,
                    rate,
                    input,
                } = self.io.as_mut().unwrap().pending_open.take().unwrap();
                self.io.as_mut().unwrap().stream = Some(stream);
                let (engine, access) = (
                    self.engine.as_ref().unwrap().clone(),
                    self.access.as_ref().unwrap().clone(),
                );
                let (tx, rx) = mpsc::channel(SEGMENT_CAPACITY);
                self.collector = Some(Collector {
                    id: take,
                    task: tokio::spawn(rambler::collect(
                        engine,
                        access,
                        language,
                        rate,
                        input,
                        tx,
                        #[cfg(test)]
                        self.test_config
                            .as_ref()
                            .and_then(|test| test.segment_budget),
                    )),
                    rx,
                });
            }
            Effect::Stop(id) => {
                if let Some(stream) = self.io.as_mut().and_then(|io| io.stream.take()) {
                    self.jobs.spawn_blocking(move || {
                        stream.stop();
                        Work::MicFree(id)
                    });
                }
            }
            Effect::Retire(id) => self.retire(id),
            Effect::Archive(take) => {
                if let Some(history) = &self.history
                    && let Some(io) = &mut self.io
                    && io.id == take.id
                    && let Some(ticket) = io.history.take()
                {
                    history.archive(ticket, take);
                }
            }
            Effect::Insert(id, token, text) => {
                let io = self.io.as_ref().unwrap();
                let can_enter = io.can_enter_inserter.clone();
                let signal = io.change.clone();
                let target = self.deps.target.clone();
                let inserter = self.deps.inserter.clone();
                self.jobs.spawn_blocking(move || {
                    let change = signal.get().copied().or_else(|| {
                        if can_enter.load(Ordering::Acquire) {
                            target.revalidate(&token).err()
                        } else {
                            None
                        }
                    });
                    // The exchange orders the handoff against watch callbacks
                    // and cancellation, even if focus already returned by now.
                    if change.is_some() || !can_enter.swap(false, Ordering::AcqRel) {
                        return Work::Inserted(
                            id,
                            InsertOutcome::NotWritten {
                                reason: NotWrittenReason::TargetChanged,
                            },
                            change.or_else(|| signal.get().copied()),
                        );
                    }
                    // The port performs its own last check after waiting for
                    // modifiers. Once inside the port, Windows cannot retract it.
                    Work::Inserted(id, inserter.insert_once(&token, &text), None)
                });
            }
        }
    }

    fn retire(&mut self, id: TakeId) {
        if self.probe_next.as_ref().is_some_and(|probe| probe.id == id) {
            self.probe_next = None;
        }
        if let Some(io) = self.io.take() {
            debug_assert_eq!(io.id, id);
            io.can_enter_inserter.store(false, Ordering::Release);
            io.live.store(false, Ordering::Release);
            self.drop_watch(io.guard);
            if let Some(opened) = io.pending_open {
                self.abort(id, opened.stream);
            }
            if let Some(stream) = io.stream {
                self.abort(id, stream);
            }
        }
        if let Some(collector) = self.collector.take() {
            debug_assert_eq!(collector.id, id);
            collector.task.abort();
            self.jobs.spawn(async move {
                let _ = collector.task.await;
                Work::Cleaned
            });
        }
    }

    fn abort(&mut self, id: TakeId, stream: Box<dyn crate::ports::MicStream>) {
        self.jobs.spawn_blocking(move || {
            stream.abort();
            Work::MicFree(id)
        });
    }

    fn drop_watch(&mut self, guard: Option<WatchGuard>) {
        if let Some(guard) = guard {
            self.jobs.spawn_blocking(move || {
                drop(guard);
                Work::Cleaned
            });
        }
    }

    fn probe_if_idle(&mut self) {
        if self.probe_running {
            return;
        }
        let Some(probe) = self.probe_next.take() else {
            return;
        };
        self.probe_running = true;
        let target = self.deps.target.clone();
        self.jobs.spawn_blocking(move || {
            let verdict = target.capture();
            let guard = if probe.watch
                && probe.live.load(Ordering::Acquire)
                && let TargetVerdict::Eligible { token, .. } = &verdict
            {
                let (live, signal) = (probe.live.clone(), probe.change.clone());
                let can_enter = probe.can_enter_inserter.clone();
                let guard = target.watch(
                    token,
                    Box::new(move |change| {
                        can_enter.store(false, Ordering::Release);
                        if change == TargetChange::Protected {
                            live.store(false, Ordering::Release);
                        }
                        let _ = signal.set(change);
                    }),
                );
                // Detect displacement between capture and installing the watch.
                if let Err(change) = target.revalidate(token) {
                    probe.can_enter_inserter.store(false, Ordering::Release);
                    if change == TargetChange::Protected {
                        probe.live.store(false, Ordering::Release);
                    }
                    let _ = probe.change.set(change);
                }
                Some(guard)
            } else {
                None
            };
            Work::Target(probe.id, verdict, guard)
        });
    }

    fn work(&mut self, work: Work) {
        match work {
            Work::Target(id, verdict, guard) => {
                self.probe_running = false;
                if let Some(io) = &mut self.io
                    && id == io.id
                {
                    io.guard = guard;
                    self.inspect();
                    self.apply(Input::Target(id, verdict));
                } else {
                    self.drop_watch(guard);
                }
                self.probe_if_idle();
            }
            Work::Opened(id, result) => match result {
                Ok(opened) => {
                    if let Some(io) = &mut self.io
                        && id == io.id
                    {
                        io.pending_open = Some(opened);
                        self.apply(Input::MicOpened(id));
                    } else {
                        self.abort(id, opened.stream);
                    }
                }
                Err(error) => {
                    self.apply(Input::MicOpenFailed(id, error));
                    self.apply(Input::MicFree(id));
                }
            },
            Work::MicFree(id) => self.apply(Input::MicFree(id)),
            Work::Inserted(id, outcome, change) => {
                if let Some(change) = change {
                    self.apply(Input::TargetChanged(id, change));
                }
                self.apply(Input::Insertion(id, outcome));
            }
            Work::Copied(id, result, reply) => {
                self.copy = None;
                if result.is_ok() {
                    self.apply(Input::Copied(id));
                }
                let _ = reply.send(result);
            }
            Work::Cleaned => (),
        }
    }

    fn copy(&mut self, reply: CopyReply) {
        if self.copy.is_some() {
            let _ = reply.send(Err(ClipboardError::Busy.into()));
        } else if let Some((id, text)) = self.state.copy_text() {
            let clipboard = self.deps.clipboard.clone();
            let live = Arc::new(AtomicBool::new(true));
            self.copy = Some((id, live.clone()));
            self.jobs.spawn_blocking(move || {
                let result = if live.load(Ordering::Acquire) {
                    clipboard.copy_private(&text).map_err(CopyError::from)
                } else {
                    Err(CopyError::NoDraft)
                };
                Work::Copied(id, result, reply)
            });
        } else {
            let _ = reply.send(Err(CopyError::NoDraft));
        }
    }

    fn collected(&mut self, id: TakeId, result: Result<Result<(), InterruptReason>, JoinError>) {
        if self
            .collector
            .as_ref()
            .is_some_and(|collector| collector.id == id)
        {
            let mut collector = self.collector.take().unwrap();
            self.inspect();
            while let Ok(segment) = collector.rx.try_recv() {
                self.apply(Input::Delivered(id, segment));
            }
            self.apply(Input::Collection(
                id,
                result.unwrap_or(Err(InterruptReason::Service)),
            ));
        }
    }

    fn warm_if_idle(&mut self) {
        if self.warm.is_some() || !self.reconfigure {
            return;
        }
        self.reconfigure = false;
        let old = self.engine.take();
        let epoch = self.connect_next.take();
        let config = epoch.and_then(|_| self.access.as_ref().map(RamblerAccess::transport_config));
        #[cfg(test)]
        let config = config.map(|config| {
            self.test_config
                .as_ref()
                .map_or(config, |test| test.config.clone())
        });
        if old.is_none() && config.is_none() {
            return;
        }
        self.warm = Some(tokio::spawn(async move {
            if let Some(old) = old {
                let _ = old.close().await;
            }
            let result = if let Some(config) = config {
                Engine::connect_ready(config)
                    .await
                    .map(|engine| Some(Arc::new(engine)))
                    .map_err(|error| rambler::connection_failure(&error))
            } else {
                Ok(None)
            };
            (epoch, result)
        }));
    }

    fn warmed(&mut self, result: Result<Warm, JoinError>) {
        if let Ok((epoch, result)) = result {
            let expected = epoch.filter(|&epoch| self.state.expecting_connection(epoch));
            match result {
                Ok(Some(engine)) => {
                    self.engine = Some(engine);
                    if let Some(epoch) = expected {
                        self.apply(Input::Connected(epoch, Ok(())));
                    } else {
                        self.reconfigure = true;
                    }
                }
                Err(reason) => {
                    if let Some(epoch) = expected {
                        self.apply(Input::Connected(epoch, Err(reason)));
                    }
                }
                Ok(None) => (),
            }
        }
        self.warm_if_idle();
    }
}

async fn join<T>(task: &mut Option<JoinHandle<T>>) -> Result<T, JoinError> {
    match task {
        Some(task) => task.await,
        None => pending().await,
    }
}

enum CollectEvent {
    Segment(TakeId, Segment),
    Finished(TakeId, Result<Result<(), InterruptReason>, JoinError>),
}

async fn next_segment(collector: &mut Option<Collector>) -> CollectEvent {
    let Some(Collector { id, task, rx }) = collector else {
        return pending().await;
    };
    tokio::select! {
        result = &mut *task => CollectEvent::Finished(*id, result),
        Some(segment) = rx.recv() => CollectEvent::Segment(*id, segment),
    }
}

#[cfg(test)]
#[tokio::test]
async fn saturation_keeps_finish_cancel_and_shutdown_ordered() {
    let (tx, rx) = mpsc::channel(COMMAND_CAPACITY);
    let overflow = Overflow::default();
    let inbox = Inbox {
        tx,
        overflow: overflow.clone(),
    };
    let mut commands = Commands { rx, overflow };
    for _ in 0..100 * COMMAND_CAPACITY {
        inbox.send(Command::Input(Input::Retry));
    }
    let take = TakeId(3);
    inbox.send(Command::Input(Input::Finish(take)));
    inbox.send(Command::Input(Input::Finish(take)));
    inbox.send(Command::Input(Input::Cancel(Some(take))));
    inbox.send(Command::Input(Input::Shutdown));
    drop(inbox);
    for _ in 0..COMMAND_CAPACITY {
        assert!(matches!(
            commands.next().await,
            Some(Command::Input(Input::Retry))
        ));
    }
    assert!(matches!(commands.next().await, Some(Command::Input(Input::Finish(id))) if id == take));
    assert!(
        matches!(commands.next().await, Some(Command::Input(Input::Cancel(Some(id)))) if id == take)
    );
    assert!(matches!(
        commands.next().await,
        Some(Command::Input(Input::Shutdown))
    ));
    assert!(commands.next().await.is_none());
}

#[cfg(test)]
struct TestConnection {
    config: rambler_transport::TransportConfig,
    segment_budget: Option<Duration>,
}

#[cfg(test)]
pub(super) fn spawn_loopback(
    deps: ControllerDeps,
    init: ControllerInit,
    address: std::net::SocketAddr,
    segment_budget: Option<Duration>,
) -> ControllerHandle {
    let config = rambler_transport::TransportConfig::loopback(address).unwrap();
    make(
        deps,
        init,
        None,
        Some(TestConnection {
            config,
            segment_budget,
        }),
    )
}
