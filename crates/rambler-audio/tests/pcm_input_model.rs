//! Sequential public-API model, not a scheduler/memory-order or engine-delivery proof.
//!
//! The finite graph abstracts token identity to an occupied/empty slot. Each replay
//! separately tracks ordinary FIFO/token ownership and checks the actual Bytes.
//! Every graph edge and composable edge pair gets a shortest-state-prefix witness;
//! deterministic walks add longer histories. Pending `next` calls are immediately
//! cancelled. Reservations and permits live in lexical helpers, never duplicated.
//!
//! Excluded: simultaneous threads, persistent pending `next`, multiple producers,
//! oversized chunks/encoder boundaries (covered elsewhere), and arbitrary-length
//! histories. Graph coverage is exhaustive only for this stated abstraction.
use bytes::Bytes;
use rambler_audio::{AudioError, PcmInput, PcmInputError, PcmPermit, PcmSender, pcm_channel};
use rambler_engine::AudioSource;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

const MAX_STATES: usize = 128;
const MAX_WITNESSES: usize = 4_096;
const MAX_WITNESS_ACTIONS: usize = 24;
const WALK_SEEDS: u64 = 64;
const WALK_ACTIONS: usize = 48;
const MAX_CLEANUP: usize = 8;
const TOKENS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Producer {
    Live,
    Finished,
    Abandoned,
    FailedFinish,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Active,
    Closed,
    Dropped,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Reservation {
    Idle,
    WaitingFull,
    GrantAvailable,
    Held,
    Rejected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Model {
    producer: Producer,
    source: Source,
    queued: bool,
    reservation: Reservation,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            producer: Producer::Live,
            source: Source::Active,
            queued: false,
            reservation: Reservation::Idle,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Action {
    Reserve,
    Repoll,
    Cancel,
    PermitDrop,
    Send,
    Next,
    Close,
    UnpolledClose,
    SourceDrop,
    Finish,
    Abandon,
}
// Stable order is part of the seeded corpus definition, not implementation order.
const ACTIONS: [Action; 11] = [
    Action::Reserve,
    Action::Repoll,
    Action::Cancel,
    Action::PermitDrop,
    Action::Send,
    Action::Next,
    Action::Close,
    Action::UnpolledClose,
    Action::SourceDrop,
    Action::Finish,
    Action::Abandon,
];

impl Model {
    fn transition(self, action: Action) -> Option<Self> {
        use Action::*;
        use Reservation::*;
        let mut next = self;
        match action {
            Reserve if self.producer == Producer::Live && self.reservation == Idle => {
                next.reservation = if self.source != Source::Active {
                    Idle // A rejected reserve never owns a slot.
                } else if self.queued {
                    WaitingFull
                } else {
                    Held
                };
            }
            Repoll if matches!(self.reservation, WaitingFull | GrantAvailable | Rejected) => {
                next.reservation = match self.reservation {
                    GrantAvailable => Held,
                    Rejected => Idle,
                    _ => WaitingFull,
                };
            }
            Cancel if matches!(self.reservation, WaitingFull | GrantAvailable | Rejected) => {
                next.reservation = Idle;
            }
            PermitDrop | Send if self.reservation == Held => {
                next.reservation = Idle;
                if action == Send && self.source == Source::Active {
                    next.queued = true;
                }
            }
            Next if self.source != Source::Dropped => {
                next.queued = false;
                if self.queued && self.reservation == WaitingFull {
                    next.reservation = GrantAvailable;
                }
            }
            Close | UnpolledClose | SourceDrop if self.source != Source::Dropped => {
                next.source = if action == SourceDrop {
                    Source::Dropped
                } else {
                    Source::Closed
                };
                next.queued = false;
                if matches!(self.reservation, WaitingFull | GrantAvailable) {
                    next.reservation = Rejected;
                }
            }
            Finish | Abandon if self.producer == Producer::Live && self.reservation == Idle => {
                next.producer = if action == Abandon {
                    Producer::Abandoned
                } else if self.source == Source::Active {
                    Producer::Finished
                } else {
                    Producer::FailedFinish
                };
            }
            _ => return None,
        }
        Some(next)
    }

    fn legal(self) -> Vec<Action> {
        ACTIONS
            .into_iter()
            .filter(|&action| self.transition(action).is_some())
            .collect()
    }
}

type Edge = (Model, Action, Model);
#[derive(Default)]
struct Coverage {
    states: BTreeSet<Model>,
    edges: BTreeSet<Edge>,
    pairs: BTreeSet<(Edge, Edge)>,
}

fn graph() -> (BTreeMap<Model, Vec<Action>>, Coverage) {
    let initial = Model::default();
    let mut paths = BTreeMap::from([(initial, Vec::new())]);
    let mut pending = VecDeque::from([initial]);
    let mut coverage = Coverage::default();
    while let Some(state) = pending.pop_front() {
        coverage.states.insert(state);
        for action in state.legal() {
            let next = state.transition(action).unwrap();
            let edge = (state, action, next);
            coverage.edges.insert(edge);
            for following in next.legal() {
                coverage
                    .pairs
                    .insert((edge, (next, following, next.transition(following).unwrap())));
            }
            if !paths.contains_key(&next) {
                let mut path = paths[&state].clone();
                path.push(action);
                assert!(
                    path.len() < MAX_WITNESS_ACTIONS,
                    "graph path bound reached: {path:?}"
                );
                paths.insert(next, path);
                assert!(
                    paths.len() < MAX_STATES,
                    "graph state bound reached: {next:?}"
                );
                pending.push_back(next);
            }
        }
    }
    (paths, coverage)
}

fn cleanup(mut state: Model) -> Vec<Action> {
    fn add(state: &mut Model, actions: &mut Vec<Action>, action: Action) {
        *state = state.transition(action).expect("illegal model cleanup");
        actions.push(action);
    }
    let mut actions = Vec::new();
    match state.reservation {
        Reservation::WaitingFull | Reservation::GrantAvailable | Reservation::Rejected => {
            add(&mut state, &mut actions, Action::Cancel);
        }
        Reservation::Held => add(&mut state, &mut actions, Action::PermitDrop),
        Reservation::Idle => {}
    }
    if state.producer == Producer::Live {
        add(&mut state, &mut actions, Action::Finish);
    }
    if state.source != Source::Dropped {
        if state.queued {
            add(&mut state, &mut actions, Action::Next);
        }
        for action in [
            Action::Next,
            Action::Next,
            Action::Close,
            Action::Next,
            Action::Next,
        ] {
            add(&mut state, &mut actions, action);
        }
    }
    assert!(
        actions.len() <= MAX_CLEANUP,
        "cleanup action bound exceeded: {actions:?}"
    );
    actions
}

#[derive(Default)]
struct WakeFlag(AtomicBool);
impl Wake for WakeFlag {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Caller,
    Queued,
    Received,
    Discarded,
}
struct Token {
    caller: Option<Bytes>,
    witness: Bytes,
    expected: Vec<u8>,
    owner: Owner,
}
impl Token {
    fn new(variant: usize) -> Self {
        let expected = match variant % 4 {
            0 => vec![],
            1 => vec![0x51],
            2 => vec![0x13, 0x82, 0xf0],
            _ => vec![0x27, 0x94, 0x01, 0xc8, 0x62],
        };
        let bytes = if variant % 4 == 3 {
            // A nonempty slice retaining a distinct padded backing allocation.
            let mut backing = vec![0xa5; expected.len() + 9];
            for (index, &byte) in expected.iter().enumerate() {
                backing[index + 4] = byte;
            }
            Bytes::from(backing).slice(4..4 + expected.len())
        } else {
            Bytes::from(expected.clone())
        };
        Self {
            witness: bytes.clone(),
            caller: Some(bytes),
            expected,
            owner: Owner::Caller,
        }
    }
    fn intact(&self, bytes: &Bytes) -> bool {
        bytes.as_ref() == self.expected.as_slice()
            && bytes.len() == self.witness.len()
            && (bytes.is_empty() || bytes.as_ptr() == self.witness.as_ptr())
    }
}

struct Replay<'a> {
    id: String,
    actions: Vec<Action>,
    index: usize,
    model: Model,
    tokens: Vec<Token>,
    queue: VecDeque<usize>,
    accepted: Vec<usize>,
    received: Vec<usize>,
    wake: Arc<WakeFlag>,
    waker: Waker,
    previous: Option<Edge>,
    coverage: &'a mut Coverage,
}
impl Replay<'_> {
    fn check(&self, condition: bool, message: &str) {
        assert!(
            condition,
            "{message}; {} action={} model={:?} prefix={:?}",
            self.id,
            self.index,
            self.model,
            &self.actions[..(self.index + 1).min(self.actions.len())]
        );
    }
    fn action(&self) -> Action {
        self.check(
            self.index < self.actions.len(),
            "unterminated reservation helper",
        );
        self.actions[self.index]
    }
    fn advance(&mut self, action: Action) {
        self.check(self.action() == action, "replay action mismatch");
        let next = self.model.transition(action);
        self.check(next.is_some(), "illegal replay transition");
        let next = next.unwrap();
        let edge = (self.model, action, next);
        self.coverage.states.extend([self.model, next]);
        self.coverage.edges.insert(edge);
        if let Some(previous) = self.previous {
            self.coverage.pairs.insert((previous, edge));
        }
        self.previous = Some(edge);
        self.model = next;
        self.invariants();
        self.index += 1;
    }
    fn invariants(&self) {
        self.check(self.queue.len() <= 1, "capacity exceeded");
        self.check(
            self.model.queued == !self.queue.is_empty(),
            "model/ledger queue mismatch",
        );
        self.check(
            self.model.reservation != Reservation::Held || self.queue.is_empty(),
            "permit and queued token both own capacity",
        );
        let mut accepted = BTreeSet::new();
        for &id in &self.accepted {
            self.check(accepted.insert(id), "duplicate acceptance");
            self.check(
                self.tokens[id].owner != Owner::Caller,
                "accepted token returned to caller",
            );
        }
        self.check(
            self.accepted.starts_with(&self.received),
            "lost/duplicated/non-FIFO receipt",
        );
        for (id, token) in self.tokens.iter().enumerate() {
            self.check(
                token.caller.is_some() == (token.owner == Owner::Caller),
                "caller ownership mismatch",
            );
            if let Some(bytes) = &token.caller {
                self.check(token.intact(bytes), "caller pointer/content changed");
            }
            self.check(
                self.queue.contains(&id) == (token.owner == Owner::Queued),
                "queue ownership mismatch",
            );
            self.check(
                accepted.contains(&id) == (token.owner != Owner::Caller),
                "lost accepted token",
            );
            self.check(
                self.received.contains(&id) == (token.owner == Owner::Received),
                "receipt ownership mismatch",
            );
            self.check(
                token.owner != Owner::Discarded || self.model.source != Source::Active,
                "discard without intentional source closure",
            );
        }
    }
    fn check_reserve(&self, result: &Poll<Result<PcmPermit<'_>, PcmInputError>>, action: Action) {
        let expected = self.model.transition(action).unwrap().reservation;
        self.check(
            match expected {
                Reservation::Held => matches!(result, Poll::Ready(Ok(_))),
                Reservation::WaitingFull => result.is_pending(),
                Reservation::Idle => matches!(result, Poll::Ready(Err(PcmInputError::Closed))),
                _ => false,
            },
            "reservation poll disagrees with model",
        );
    }
}

fn source_action(action: Action, source: &mut Option<PcmInput>, run: &mut Replay<'_>) {
    let before = run.model;
    run.wake.0.store(false, Ordering::SeqCst);
    let waker = run.waker.clone();
    let mut context = Context::from_waker(&waker);
    match action {
        Action::Next => {
            let result = {
                let mut next = source.as_mut().unwrap().next();
                next.as_mut().poll(&mut context)
            }; // Pending next is cancelled before any other receiver operation.
            if let Some(id) = run.queue.pop_front() {
                match result {
                    Poll::Ready(Ok(Some(bytes))) => {
                        run.check(
                            run.tokens[id].intact(&bytes),
                            "receipt pointer/content changed",
                        );
                        run.tokens[id].owner = Owner::Received;
                        run.received.push(id);
                    }
                    _ => run.check(false, "queued token not immediately received"),
                }
            } else if before.source == Source::Active && before.producer == Producer::Live {
                run.check(result.is_pending(), "live empty source did not pend");
            } else if before.source == Source::Active && before.producer == Producer::Abandoned {
                run.check(
                    matches!(result, Poll::Ready(Err(ref error)) if matches!(
                        error.original().downcast_ref::<AudioError>(),
                        Some(AudioError::InputAbandoned)
                    )),
                    "abandonment was not the stable typed error",
                );
            } else {
                run.check(
                    matches!(result, Poll::Ready(Ok(None))),
                    "terminal source did not return EOF",
                );
            }
        }
        Action::Close | Action::UnpolledClose | Action::SourceDrop => {
            match action {
                Action::Close => {
                    let mut close = source.as_mut().unwrap().close();
                    run.check(
                        matches!(close.as_mut().poll(&mut context), Poll::Ready(Ok(()))),
                        "close was not immediately successful",
                    );
                }
                Action::UnpolledClose => drop(source.as_mut().unwrap().close()),
                Action::SourceDrop => drop(source.take().unwrap()),
                _ => unreachable!(),
            }
            for id in run.queue.drain(..) {
                run.tokens[id].owner = Owner::Discarded;
            }
        }
        _ => run.check(false, "non-source action inside source helper"),
    }
    if before.reservation == Reservation::WaitingFull && (action != Action::Next || before.queued) {
        run.check(
            run.wake.0.load(Ordering::SeqCst),
            "registered reservation was not woken",
        );
    }
    run.advance(action);
}

fn held(permit: PcmPermit<'_>, source: &mut Option<PcmInput>, run: &mut Replay<'_>) {
    let mut permit = Some(permit);
    loop {
        let action = run.action();
        match action {
            Action::PermitDrop => {
                drop(permit.take().unwrap());
                run.advance(action);
                return;
            }
            Action::Send => {
                let id = run
                    .tokens
                    .iter()
                    .position(|token| token.owner == Owner::Caller);
                run.check(id.is_some(), "synthetic token bound reached");
                let id = id.unwrap();
                let bytes = run.tokens[id].caller.take().unwrap();
                let result = permit.take().unwrap().send(bytes);
                if run.model.source == Source::Active {
                    run.check(result.is_ok(), "live permit rejected token");
                    run.tokens[id].owner = Owner::Queued;
                    run.queue.push_back(id);
                    run.accepted.push(id);
                } else {
                    run.check(result.is_err(), "closed permit accepted token");
                    let error = result.unwrap_err();
                    run.check(
                        error.kind() == PcmInputError::Closed,
                        "wrong permit rejection kind",
                    );
                    let bytes = error.into_chunk();
                    run.check(
                        run.tokens[id].intact(&bytes),
                        "rejected pointer/content changed",
                    );
                    run.tokens[id].caller = Some(bytes);
                }
                run.advance(action);
                return;
            }
            _ => source_action(action, source, run),
        }
    }
}

fn reservation(sender: &mut PcmSender, source: &mut Option<PcmInput>, run: &mut Replay<'_>) {
    let mut future = Box::pin(sender.reserve());
    let waker = run.waker.clone();
    let mut context = Context::from_waker(&waker);
    let mut result = future.as_mut().poll(&mut context);
    run.check_reserve(&result, Action::Reserve);
    run.advance(Action::Reserve);
    loop {
        match result {
            Poll::Ready(Ok(permit)) => {
                drop(future);
                held(permit, source, run);
                return;
            }
            Poll::Ready(Err(_)) => return,
            Poll::Pending => match run.action() {
                Action::Cancel => {
                    drop(future);
                    run.advance(Action::Cancel);
                    return;
                }
                Action::Repoll => {
                    result = future.as_mut().poll(&mut context);
                    run.check_reserve(&result, Action::Repoll);
                    run.advance(Action::Repoll);
                }
                action => {
                    source_action(action, source, run);
                    result = Poll::Pending;
                }
            },
        }
    }
}

fn replay(id: String, mut actions: Vec<Action>, variant: usize, coverage: &mut Coverage) {
    let end = actions.iter().fold(Model::default(), |state, &action| {
        state
            .transition(action)
            .expect("illegal generated core trace")
    });
    actions.extend(cleanup(end));
    let wake = Arc::new(WakeFlag::default());
    let waker = Waker::from(Arc::clone(&wake));
    let mut run = Replay {
        id,
        actions,
        index: 0,
        model: Model::default(),
        tokens: (0..TOKENS).map(|id| Token::new(variant + id)).collect(),
        queue: VecDeque::new(),
        accepted: Vec::new(),
        received: Vec::new(),
        wake,
        waker,
        previous: None,
        coverage,
    };
    let (sender, source) = pcm_channel();
    let mut sender = Some(sender);
    let mut source = Some(source);
    run.coverage.states.insert(run.model);
    while run.index < run.actions.len() {
        let action = run.action();
        match action {
            Action::Reserve => reservation(sender.as_mut().unwrap(), &mut source, &mut run),
            Action::Finish => {
                let result = sender.take().unwrap().finish();
                run.check(
                    if run.model.source == Source::Active {
                        result.is_ok()
                    } else {
                        result == Err(PcmInputError::Closed)
                    },
                    "finish result disagrees with source state",
                );
                run.advance(action);
            }
            Action::Abandon => {
                drop(sender.take().unwrap());
                run.advance(action);
            }
            _ => source_action(action, &mut source, &mut run),
        }
    }
    run.check(
        run.queue.is_empty(),
        "accepted token left unclassified at trace end",
    );
    run.check(
        run.model.producer != Producer::Live,
        "cleanup left producer live",
    );
    run.check(
        run.model.reservation == Reservation::Idle,
        "cleanup left reservation live",
    );
}

#[test]
fn bounded_public_pcm_reference_model() {
    let (paths, expected) = graph();
    let mut witnesses = BTreeSet::new();
    for &(state, action, next) in &expected.edges {
        let mut trace = paths[&state].clone();
        trace.push(action);
        witnesses.insert(trace.clone());
        for following in next.legal() {
            let mut pair_trace = trace.clone();
            pair_trace.push(following);
            witnesses.insert(pair_trace);
        }
    }
    assert!(
        witnesses.len() < MAX_WITNESSES,
        "witness trace bound reached"
    );
    assert!(
        witnesses
            .iter()
            .all(|trace| trace.len() < MAX_WITNESS_ACTIONS),
        "witness action bound reached"
    );
    // Explicit obligations make reductions of the abstract graph visible.
    let contains = |pattern: &[Action]| {
        witnesses
            .iter()
            .any(|trace| trace.windows(pattern.len()).any(|w| w == pattern))
    };
    assert!(
        contains(&[
            Action::Reserve,
            Action::Send,
            Action::Reserve,
            Action::Next,
            Action::Cancel,
            Action::Reserve
        ]),
        "missing wake/cancel-before-repoll/reserve-again witness"
    );
    for close in [Action::Close, Action::UnpolledClose, Action::SourceDrop] {
        for release in [Action::Send, Action::PermitDrop] {
            assert!(
                contains(&[Action::Reserve, close, release]),
                "missing held/{close:?}/{release:?} witness"
            );
        }
    }
    for release in [Action::Cancel, Action::PermitDrop] {
        for terminal in [Action::Finish, Action::Abandon] {
            assert!(
                contains(&[release, terminal]),
                "missing {release:?}/{terminal:?} history"
            );
        }
    }
    let witness_count = witnesses.len();
    let max_witness = witnesses.iter().map(Vec::len).max().unwrap();
    let mut coverage = Coverage::default();
    let mut action_count = 0;
    for (index, trace) in witnesses.into_iter().enumerate() {
        action_count += trace.len()
            + cleanup(
                trace
                    .iter()
                    .fold(Model::default(), |s, &a| s.transition(a).unwrap()),
            )
            .len();
        replay(format!("witness-{index}"), trace, index, &mut coverage);
    }
    let mut walk_count = 0;
    for seed in 0..WALK_SEEDS {
        // Fixed u64 LCG; both arithmetic and action selection are specified here.
        let mut rng = seed;
        let mut state = Model::default();
        let mut trace = Vec::new();
        for _ in 0..WALK_ACTIONS {
            let legal = state.legal();
            if legal.is_empty() {
                break; // A model terminal, not an exploration-bound truncation.
            }
            rng = rng
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let action = legal[((rng >> 32) % legal.len() as u64) as usize];
            trace.push(action);
            state = state.transition(action).unwrap();
        }
        assert!(
            trace.len() < WALK_ACTIONS || state.legal().is_empty(),
            "seed-{seed} hit the walk bound before a model terminal: state={state:?} prefix={trace:?}"
        );
        action_count += trace.len() + cleanup(state).len();
        replay(format!("seed-{seed}"), trace, seed as usize, &mut coverage);
        walk_count += 1;
    }
    assert_eq!(
        coverage.states, expected.states,
        "abstract state coverage mismatch"
    );
    assert_eq!(
        coverage.edges, expected.edges,
        "abstract edge coverage mismatch"
    );
    assert_eq!(
        coverage.pairs, expected.pairs,
        "adjacent-edge-pair coverage mismatch"
    );
    eprintln!(
        "pcm model: states={} edges={} adjacent_pairs={} witnesses={} walks={} traces={} actions={} max_witness_core={} seeds=0..63 walk_prefix_bound={} cleanup_bound={}",
        coverage.states.len(),
        coverage.edges.len(),
        coverage.pairs.len(),
        witness_count,
        walk_count,
        witness_count + walk_count,
        action_count,
        max_witness,
        WALK_ACTIONS,
        MAX_CLEANUP
    );
}
