//! Reducer races and end-to-end controller/SDK tests. No remote endpoint.

#[path = "../../../../crates/rambler-engine/tests/common/mod.rs"]
mod fixture;

use super::{
    runtime::spawn_loopback,
    state::{Effect, Input, Now, State},
    *,
};
use crate::{
    ipc::{
        Availability, InterruptReason, KeptReason, PartStatus, Phase, RefusalReason, TakeOutcome,
        UnavailableReason,
    },
    model::{Gesture, MicChoice, RenderProfile},
    ports::{
        AppKind, FaultCallback, InsertOutcome, MicError, MicInfo, MicStream, PcmRate, PcmSink,
        SinkFactory, TargetChange, TargetInfo, TargetToken, TargetVerdict, UncertainReason,
        WatchGuard,
    },
    rambler::Segment,
};
use bytes::Bytes;
use fixture::{TestServer, bounded, replies};
use futures_util::stream;
use rambler_protocol::{
    RECOGNITION_RESPONSE, end_request,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc as std_mpsc,
    },
    time::Duration,
};
use tokio::{sync::Notify, time::sleep};
use tonic::Code;
use zeroize::Zeroizing;

const FIRST: &str = "Premier passage.";
const SECOND: &str = "Deuxième passage.";

fn eligible() -> TargetVerdict {
    TargetVerdict::Eligible {
        token: TargetToken(Arc::new(())),
        info: TargetInfo {
            app: AppKind::Notepad,
            multiline: Some(true),
        },
    }
}

#[derive(Default)]
struct FakeMic {
    current: Mutex<Option<Arc<FakeStream>>>,
    opened: AtomicUsize,
    stop_count: Arc<AtomicUsize>,
    abort_count: Arc<AtomicUsize>,
    delayed: Mutex<Option<std_mpsc::Receiver<()>>>,
    opening: Notify,
}

struct FakeStream(Mutex<Option<Box<dyn PcmSink>>>);
struct Driver {
    stream: Arc<FakeStream>,
    stopped: Arc<AtomicUsize>,
    aborted: Arc<AtomicUsize>,
}
impl MicStream for Driver {
    fn stop(self: Box<Self>) {
        self.stream.0.lock().unwrap().take().unwrap().finish();
        self.stopped.fetch_add(1, Ordering::SeqCst);
    }
    fn abort(self: Box<Self>) {
        self.stream.0.lock().unwrap().take().unwrap().abort();
        self.aborted.fetch_add(1, Ordering::SeqCst);
    }
}
impl Microphone for FakeMic {
    fn inputs(&self) -> Result<Vec<MicInfo>, MicError> {
        Ok(vec![])
    }
    fn open(
        &self,
        _: &MicChoice,
        factory: SinkFactory,
        _: FaultCallback,
    ) -> Result<Box<dyn MicStream>, MicError> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        self.opening.notify_one();
        if let Some(wait) = self.delayed.lock().unwrap().take() {
            wait.recv_timeout(Duration::from_secs(3))
                .map_err(|_| MicError::OpenTimeout)?;
        }
        let mut sink = factory(PcmRate::Hz48000);
        // Start with an irregular short callback, less than a 20 ms block.
        sink.push(&[0; 71]);
        let stream = Arc::new(FakeStream(Mutex::new(Some(sink))));
        *self.current.lock().unwrap() = Some(stream.clone());
        Ok(Box::new(Driver {
            stream,
            stopped: self.stop_count.clone(),
            aborted: self.abort_count.clone(),
        }))
    }
}
impl FakeMic {
    fn feed(&self, samples: &[i16]) {
        let stream = self.current.lock().unwrap().clone().unwrap();
        stream.0.lock().unwrap().as_mut().unwrap().push(samples);
    }
}

type OnChange = Box<dyn FnOnce(TargetChange) + Send>;
struct Target {
    verdict: Mutex<TargetVerdict>,
    change: Mutex<Option<Arc<Mutex<Option<OnChange>>>>>,
}
struct Watch(Arc<Mutex<Option<OnChange>>>);
impl Drop for Watch {
    fn drop(&mut self) {
        self.0.lock().unwrap().take();
    }
}
impl TargetProbe for Target {
    fn capture(&self) -> TargetVerdict {
        self.verdict.lock().unwrap().clone()
    }
    fn watch(&self, _: &TargetToken, callback: OnChange) -> WatchGuard {
        let callback = Arc::new(Mutex::new(Some(callback)));
        *self.change.lock().unwrap() = Some(callback.clone());
        Box::new(Watch(callback))
    }
    fn revalidate(&self, _: &TargetToken) -> Result<(), TargetChange> {
        Ok(())
    }
}
impl Target {
    fn change(&self, change: TargetChange) {
        let watch = self.change.lock().unwrap().clone().unwrap();
        let callback = watch.lock().unwrap().take().unwrap();
        callback(change);
    }
}

#[derive(Default)]
struct TextRecorder(Mutex<Vec<String>>);
impl Inserter for TextRecorder {
    fn insert_once(&self, _: &TargetToken, text: &str) -> InsertOutcome {
        self.0.lock().unwrap().push(text.to_owned());
        InsertOutcome::Delivered
    }
}
impl Clipboard for TextRecorder {
    fn copy_private(&self, text: &str) -> Result<(), ClipboardError> {
        self.0.lock().unwrap().push(text.to_owned());
        Ok(())
    }
}

struct Rig {
    ctl: ControllerHandle,
    mic: Arc<FakeMic>,
    target: Arc<Target>,
    inserted: Arc<TextRecorder>,
    copied: Arc<TextRecorder>,
    peer: TestServer,
    requests: Arc<AtomicUsize>,
    second: Arc<Notify>,
}
impl Rig {
    async fn new(gesture: Gesture, segmented: bool, fail_second: Option<Code>) -> Self {
        Self::with_hold(gesture, segmented, fail_second, None).await
    }

    async fn with_hold(
        gesture: Gesture,
        segmented: bool,
        fail_second: Option<Code>,
        hold: Option<usize>,
    ) -> Self {
        let requests = Arc::new(AtomicUsize::new(0));
        let second = Arc::new(Notify::new());
        let peer = TestServer::new({
            let requests = requests.clone();
            let second = second.clone();
            move |request| {
                let index = requests.fetch_add(1, Ordering::SeqCst);
                if index == 1 {
                    second.notify_one();
                }
                async move {
                    assert!(request.metadata().contains_key("x-goog-api-key"));
                    let mut body = request.into_inner();
                    let mut upload = Vec::new();
                    while let Some(frame) = body.message().await? {
                        upload.push(frame);
                    }
                    assert!(upload.len() >= 4, "PCM went through the Opus encoder");
                    assert_eq!(upload.last().unwrap().as_ref(), end_request());
                    let text = if index == 0 { FIRST } else { SECOND };
                    let hypothesis = b(3, &s(1, text).unwrap()).unwrap();
                    let recognition = [v(1, 1).unwrap(), b(3, &hypothesis).unwrap()].concat();
                    let mut result = vec![
                        Ok(Bytes::from(
                            b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap(),
                        )),
                        Ok(Bytes::from_static(b"\x08\x01")),
                    ];
                    // Fail only after the final-looking text and application END.
                    if index == 1
                        && let Some(code) = fail_second
                    {
                        result.push(Err(tonic::Status::new(code, "test trailer")));
                    }
                    if hold == Some(index) {
                        use futures_util::StreamExt;
                        Ok(replies(stream::iter(result).chain(stream::pending())))
                    } else {
                        Ok(replies(stream::iter(result)))
                    }
                }
            }
        })
        .await;
        let mic = Arc::new(FakeMic::default());
        let target = Arc::new(Target {
            verdict: Mutex::new(eligible()),
            change: Default::default(),
        });
        let inserted = Arc::new(TextRecorder::default());
        let copied = Arc::new(TextRecorder::default());
        let deps = ControllerDeps {
            microphone: mic.clone(),
            target: target.clone(),
            inserter: inserted.clone(),
            clipboard: copied.clone(),
        };
        let init = ControllerInit {
            settings: Settings {
                gesture,
                profile: RenderProfile::Faithful,
                ..Settings::default()
            },
            consent: true,
            access: Some(
                RamblerAccess::new(
                    &peer.address.to_string(),
                    Zeroizing::new("test-key".into()),
                    Zeroizing::new("example.test".into()),
                    Zeroizing::new("test-cert".into()),
                )
                .unwrap(),
            ),
        };
        let ctl = spawn_loopback(
            deps,
            init,
            peer.address,
            segmented.then_some(Duration::from_millis(80)),
        );
        wait(&ctl, |s| s.availability == Availability::Ready).await;
        Self {
            ctl,
            mic,
            target,
            inserted,
            copied,
            peer,
            requests,
            second,
        }
    }
    async fn listening(&self) -> TakeId {
        self.ctl.hotkey(KeyEdge::Pressed);
        wait(&self.ctl, |s| s.phase == Phase::Listening)
            .await
            .take
            .unwrap()
    }
    async fn idle(&self) -> UiState {
        wait(&self.ctl, |s| s.phase == Phase::Idle).await
    }
    async fn close(self) {
        bounded(self.ctl.shutdown()).await;
        bounded(self.peer.close()).await;
    }
}

async fn wait(ctl: &ControllerHandle, check: impl Fn(&UiState) -> bool) -> UiState {
    bounded(ctl.state().wait_for(check)).await.unwrap().clone()
}

fn interrupted(reason: InterruptReason, recovered: bool) -> Option<TakeOutcome> {
    Some(TakeOutcome::Interrupted { reason, recovered })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sdk_48k_multisegment_inserts_only_the_joined_result_once() {
    let rig = Rig::new(Gesture::Hold, true, None).await;
    let take = rig.listening().await;
    rig.mic.feed(&[0; 4800]);
    bounded(rig.second.notified()).await;
    rig.ctl.hotkey(KeyEdge::Released);
    rig.ctl.finish(take);
    assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Inserted));
    assert_eq!(rig.requests.load(Ordering::SeqCst), 2);
    assert_eq!(rig.mic.stop_count.load(Ordering::SeqCst), 1);
    let full = format!("{FIRST} {SECOND}");
    assert_eq!(
        rig.inserted.0.lock().unwrap().as_slice(),
        std::slice::from_ref(&full)
    );
    let draft = rig.ctl.draft().await.unwrap();
    assert_eq!(draft.parts.len(), 2);
    assert_eq!(draft.full_text, Some(full));
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sdk_finalization_error_recovers_only_the_committed_prefix() {
    let rig = Rig::new(Gesture::Hold, true, Some(Code::DeadlineExceeded)).await;
    rig.listening().await;
    rig.mic.feed(&[0; 4800]);
    bounded(rig.second.notified()).await;
    rig.ctl.hotkey(KeyEdge::Released);
    assert_eq!(
        rig.idle().await.last_outcome,
        interrupted(InterruptReason::Timeout, true)
    );
    let draft = rig.ctl.draft().await.unwrap();
    assert!(draft.full_text.is_none());
    assert_eq!(draft.parts[0].text.as_deref(), Some(FIRST));
    assert_eq!(draft.parts[1].status, PartStatus::Missing);
    rig.ctl.copy_draft().await.unwrap();
    assert_eq!(*rig.copied.0.lock().unwrap(), [FIRST]);
    assert!(rig.inserted.0.lock().unwrap().is_empty());
    assert_eq!(rig.requests.load(Ordering::SeqCst), 2);
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stall_then_release_aborts_instead_of_inserting_the_prefix() {
    let rig = Rig::with_hold(Gesture::Hold, true, None, Some(1)).await;
    rig.listening().await;
    rig.mic.feed(&[0; 4800]);
    bounded(rig.second.notified()).await;
    assert_eq!(
        rig.idle().await.last_outcome,
        interrupted(InterruptReason::MicrophoneStalled, true)
    );
    rig.ctl.hotkey(KeyEdge::Released);
    rig.ctl.copy_draft().await.unwrap();
    assert_eq!(*rig.copied.0.lock().unwrap(), [FIRST]);
    assert!(rig.inserted.0.lock().unwrap().is_empty());
    bounded(rig.ctl.shutdown()).await;
    assert_eq!(rig.mic.stop_count.load(Ordering::SeqCst), 0);
    assert_eq!(rig.mic.abort_count.load(Ordering::SeqCst), 1);
    // The deliberately aborted RPC may not have a clean request EOF; closing
    // the peer still joins its HTTP/2 connection.
    bounded(rig.peer.close()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silent_callbacks_keep_a_take_alive_past_the_watchdog() {
    let rig = Rig::new(Gesture::Hold, true, None).await;
    rig.listening().await;
    rig.mic.feed(&[0; 4800]);
    bounded(rig.second.notified()).await; // First segment already confirmed.
    for _ in 0..6 {
        rig.mic.feed(&[0; 960]);
        sleep(Duration::from_millis(210)).await;
        assert_eq!(rig.ctl.state().borrow().phase, Phase::Listening);
        assert_eq!(*rig.ctl.level().borrow(), 0.0);
    }
    rig.ctl.hotkey(KeyEdge::Released);
    assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Inserted));
    {
        let inserted = rig.inserted.0.lock().unwrap();
        assert_eq!(inserted.len(), 1);
        assert!(inserted[0].starts_with(&format!("{FIRST} {SECOND}")));
    }
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn release_while_opening_invalidates_and_joins_the_late_microphone() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    let (open, delay) = std_mpsc::channel();
    *rig.mic.delayed.lock().unwrap() = Some(delay);
    rig.ctl.hotkey(KeyEdge::Pressed);
    bounded(rig.mic.opening.notified()).await;
    assert_eq!(rig.ctl.state().borrow().phase, Phase::Preparing);
    rig.ctl.hotkey(KeyEdge::Released);
    assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Cancelled));
    open.send(()).unwrap();
    bounded(rig.ctl.shutdown()).await;
    assert_eq!(rig.mic.abort_count.load(Ordering::SeqCst), 1);
    assert_eq!(rig.requests.load(Ordering::SeqCst), 0);
    rig.close().await; // Idempotent even after dropping the command receiver.
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn target_change_stays_disarmed_and_pending_draft_needs_two_triggers() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    rig.listening().await;
    rig.mic.feed(&[0; 975]);
    rig.target.change(TargetChange::FocusMoved);
    rig.ctl.hotkey(KeyEdge::Released);
    assert_eq!(
        rig.idle().await.last_outcome,
        Some(TakeOutcome::KeptInSambler {
            reason: KeptReason::TargetChanged {
                change: TargetChange::FocusMoved
            }
        })
    );
    assert_eq!(
        rig.ctl.draft().await.unwrap().full_text.as_deref(),
        Some(FIRST)
    );
    assert!(rig.inserted.0.lock().unwrap().is_empty());
    rig.ctl.hotkey(KeyEdge::Pressed);
    wait(&rig.ctl, |s| {
        s.last_outcome
            == Some(TakeOutcome::Refused {
                reason: RefusalReason::PendingDraft,
            })
    })
    .await;
    assert_eq!(rig.mic.opened.load(Ordering::SeqCst), 1);
    rig.ctl.hotkey(KeyEdge::Released);
    rig.listening().await;
    assert!(rig.ctl.draft().await.is_none());
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn toggle_ignores_repeat_then_finishes_on_next_physical_press() {
    let rig = Rig::new(Gesture::Toggle, false, None).await;
    rig.listening().await;
    rig.ctl.hotkey(KeyEdge::Pressed);
    rig.ctl.hotkey(KeyEdge::Released);
    // Ordered read as a barrier after both edges.
    assert!(rig.ctl.draft().await.is_none());
    assert_eq!(rig.ctl.state().borrow().phase, Phase::Listening);
    rig.mic.feed(&[0; 1000]);
    rig.ctl.hotkey(KeyEdge::Pressed);
    assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Inserted));
    assert_eq!(rig.mic.stop_count.load(Ordering::SeqCst), 1);
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consent_lock_and_protected_focus_interrupt_active_takes() {
    for (action, reason) in [
        (0, InterruptReason::ConsentWithdrawn),
        (1, InterruptReason::SystemInterrupted),
        (2, InterruptReason::SystemInterrupted),
    ] {
        let rig = Rig::new(Gesture::Hold, false, None).await;
        rig.listening().await;
        rig.mic.feed(&[0; 975]);
        rig.ctl.hotkey(KeyEdge::Released);
        assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Inserted));
        rig.listening().await;
        match action {
            0 => rig.ctl.set_consent(false),
            1 => rig.ctl.system(SystemEvent::SessionLocked),
            _ => rig.target.change(TargetChange::Protected),
        }
        assert_eq!(rig.idle().await.last_outcome, interrupted(reason, false));
        if action == 0 {
            assert_eq!(
                rig.ctl.state().borrow().availability,
                Availability::NeedsConsent
            );
        }
        if action == 1 {
            assert!(rig.ctl.draft().await.is_none());
        }
        bounded(rig.ctl.shutdown()).await;
        assert_eq!(rig.mic.abort_count.load(Ordering::SeqCst), 1);
        assert_eq!(rig.requests.load(Ordering::SeqCst), 1);
        rig.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protected_and_uncertain_refuse_without_opening_and_explicit_draft_is_allowed() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    for (verdict, reason) in [
        (TargetVerdict::Protected, RefusalReason::ProtectedField),
        (
            TargetVerdict::Uncertain {
                reason: UncertainReason::Selection,
            },
            RefusalReason::UncertainTarget {
                reason: UncertainReason::Selection,
            },
        ),
    ] {
        *rig.target.verdict.lock().unwrap() = verdict;
        rig.ctl.hotkey(KeyEdge::Pressed);
        wait(&rig.ctl, |s| {
            s.last_outcome
                == Some(TakeOutcome::Refused {
                    reason: reason.clone(),
                })
        })
        .await;
        rig.ctl.hotkey(KeyEdge::Released);
        assert_eq!(rig.mic.opened.load(Ordering::SeqCst), 0);
    }
    rig.ctl.dictate_in_sambler();
    let id = wait(&rig.ctl, |s| s.phase == Phase::Listening)
        .await
        .take
        .unwrap();
    rig.mic.feed(&[0; 975]);
    rig.ctl.finish(id);
    assert_eq!(
        rig.idle().await.last_outcome,
        Some(TakeOutcome::KeptInSambler {
            reason: KeptReason::SamblerDestination
        })
    );
    assert_eq!(
        rig.ctl.draft().await.unwrap().full_text.as_deref(),
        Some(FIRST)
    );
    assert!(rig.inserted.0.lock().unwrap().is_empty());
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sdk_access_refusal_is_not_automatically_retried() {
    let rig = Rig::new(Gesture::Hold, true, Some(Code::PermissionDenied)).await;
    rig.listening().await;
    rig.mic.feed(&[0; 4800]);
    bounded(rig.second.notified()).await;
    rig.ctl.hotkey(KeyEdge::Released);
    assert_eq!(
        rig.idle().await.last_outcome,
        interrupted(InterruptReason::AccessRefused, true)
    );
    assert_eq!(
        rig.ctl.state().borrow().availability,
        Availability::Unavailable {
            reason: UnavailableReason::AccessRefused
        }
    );
    rig.ctl.hotkey(KeyEdge::Pressed);
    wait(&rig.ctl, |s| {
        s.last_outcome
            == Some(TakeOutcome::Refused {
                reason: RefusalReason::Unavailable {
                    reason: UnavailableReason::AccessRefused,
                },
            })
    })
    .await;
    assert_eq!(rig.mic.opened.load(Ordering::SeqCst), 1);
    rig.ctl.retry_connection();
    wait(&rig.ctl, |s| s.availability == Availability::Ready).await;
    assert_eq!(rig.peer.accepted_connections(), 2);
    assert_eq!(rig.requests.load(Ordering::SeqCst), 2);
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_a_cannot_insert_or_cancel_b_and_duplicate_stop_inserts_b_once() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    let a = rig.listening().await;
    let stale_callback = rig.target.change.lock().unwrap().clone().unwrap();
    let stale_callback = stale_callback.lock().unwrap().take().unwrap();
    rig.ctl.cancel(Some(a));
    rig.ctl.hotkey(KeyEdge::Released);
    assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Cancelled));
    let b = rig.listening().await;
    stale_callback(TargetChange::Protected);
    rig.ctl.inbox.send(Command::Input(Input::Delivered(
        a,
        segment(0, 0..160, "Too late"),
    )));
    rig.ctl
        .inbox
        .send(Command::Input(Input::Collection(a, Ok(()))));
    rig.ctl.finish(a);
    rig.ctl.cancel(Some(a));
    rig.mic.feed(&[0; 975]);
    rig.ctl.finish(b);
    rig.ctl.finish(b);
    assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Inserted));
    assert_eq!(*rig.inserted.0.lock().unwrap(), [FIRST]);
    assert_eq!(
        rig.ctl.draft().await.unwrap().full_text.as_deref(),
        Some(FIRST)
    );
    bounded(rig.ctl.shutdown()).await;
    assert_eq!(rig.mic.stop_count.load(Ordering::SeqCst), 1);
    assert_eq!(rig.mic.abort_count.load(Ordering::SeqCst), 1);
    assert_eq!(rig.requests.load(Ordering::SeqCst), 1);
    rig.close().await;
}

#[test]
fn frozen_rules_render_the_draft_insertion_and_archive_identically() {
    use crate::{ipc::PersonalRules, text::LexiconEntry};
    let rules = |written: &str| {
        Arc::new(PersonalRules {
            lexicon: vec![LexiconEntry {
                id: "test".into(),
                heard: "texte".into(),
                written: written.into(),
                enabled: true,
                apply_in_faithful: true,
            }],
            expressions: vec![],
            unreadable: false,
        })
    };
    let mut state = ready();
    state.use_profile(rules("Ancienne règle"));
    let id = start(&mut state, 0);
    state.step(Input::PersonalRules(rules("Nouvelle règle")), at(1));
    state.step(Input::Delivered(id, segment(0, 0..100, "texte")), at(1));
    state.step(Input::Finish(id), at(2));
    state.step(Input::MicFree(id), at(3));
    let effects = state.step(Input::Collection(id, Ok(())), at(4));
    let [Effect::Insert(take, _, text)] = effects.as_slice() else {
        panic!("expected insertion")
    };
    assert_eq!(*take, id);
    assert_eq!(&**text, "Ancienne règle");
    assert_eq!(
        state.draft().unwrap().full_text.as_deref(),
        Some("Ancienne règle")
    );
    assert!(matches!(
        state.step(Input::Insertion(id, InsertOutcome::Delivered), at(5)).as_slice(),
        [Effect::Archive(value), Effect::Retire(_)]
        if value.id == id && value.created == at(4).unix && Arc::ptr_eq(&value.text, text)
    ));
    state.step(Input::Hotkey(KeyEdge::Released), at(5));
    let next = start(&mut state, 6);
    state.step(Input::Delivered(next, segment(0, 0..100, "texte")), at(7));
    state.step(Input::Finish(next), at(7));
    state.step(Input::MicFree(next), at(8));
    let effects = state.step(Input::Collection(next, Ok(())), at(9));
    assert!(
        matches!(effects.as_slice(), [Effect::Insert(_, _, text)] if &**text == "Nouvelle règle")
    );
    assert!(matches!(
        state.step(Input::Insertion(next, InsertOutcome::Delivered), at(10)).as_slice(),
        [Effect::Archive(value), Effect::Retire(_)] if &*value.text == "Nouvelle règle"
    ));
}

fn at(mono: u64) -> Now {
    Now {
        mono,
        unix: 1_700_000_000_000 + mono,
    }
}

fn ready() -> State {
    let mut state = State::new(
        Settings {
            profile: RenderProfile::Faithful,
            ..Settings::default()
        },
        true,
        true,
    );
    state.step(Input::Retry, at(0));
    state.step(Input::Connected(1, Ok(())), at(0));
    state
}

fn start(state: &mut State, time: u64) -> TakeId {
    state.step(Input::Hotkey(KeyEdge::Pressed), at(time));
    let id = state.take_id().unwrap();
    state.step(Input::Target(id, eligible()), at(time));
    state.step(Input::MicOpened(id), at(time));
    state.step(
        Input::Tick {
            last_frame: Some(time),
        },
        at(time),
    );
    id
}

fn segment(index: usize, range: std::ops::Range<u64>, text: &str) -> Segment {
    Segment {
        index,
        range,
        part: crate::ipc::DraftPart {
            status: PartStatus::Admitted,
            text: Some(text.into()),
        },
    }
}

#[test]
fn cancel_a_start_b_late_a_and_double_stop_b_produce_one_insertion_effect() {
    let mut state = ready();
    let a = start(&mut state, 0);
    state.step(Input::Delivered(a, segment(0, 0..100, "A")), at(1));
    state.step(Input::Cancel(Some(a)), at(1));
    state.step(Input::Hotkey(KeyEdge::Released), at(1));
    state.step(Input::MicFree(a), at(2));
    let b = start(&mut state, 3);
    for event in [
        Input::Delivered(a, segment(1, 100..200, "Too late")),
        Input::Collection(a, Ok(())),
        Input::TargetChanged(a, TargetChange::Protected),
        Input::MicFault(a, InterruptReason::MicrophoneStalled),
        Input::Cancel(Some(a)),
        Input::Finish(a),
    ] {
        assert!(state.step(event, at(4)).is_empty());
    }
    assert!(
        matches!(state.step(Input::Finish(b), at(4)).as_slice(), [Effect::Stop(id)] if *id == b)
    );
    assert!(state.step(Input::Finish(b), at(5)).is_empty());
    state.step(Input::Delivered(b, segment(0, 0..100, "Only B")), at(5));
    state.step(Input::Delivered(b, segment(0, 0..100, "Duplicate")), at(5));
    assert!(state.step(Input::Collection(b, Ok(())), at(5)).is_empty());
    assert!(
        matches!(state.step(Input::MicFree(b), at(6)).as_slice(), [Effect::Insert(id, _, text)] if *id == b && &**text == "Only B")
    );
    assert!(state.step(Input::Collection(b, Ok(())), at(7)).is_empty());
    state.step(Input::Insertion(b, InsertOutcome::Delivered), at(7));
    assert_eq!(state.ui(at(7)).last_outcome, Some(TakeOutcome::Inserted));
    assert_eq!(state.draft().unwrap().full_text.as_deref(), Some("Only B"));
    state.step(Input::Tick { last_frame: None }, at(600_006));
    assert!(state.draft().is_none());
}

#[test]
fn cancelling_during_insertion_does_not_archive_or_keep_the_draft() {
    let mut state = ready();
    state.use_profile(Arc::new(crate::ipc::PersonalRules {
        lexicon: vec![],
        expressions: vec![],
        unreadable: false,
    }));
    let id = start(&mut state, 0);
    state.step(Input::Delivered(id, segment(0, 0..100, "Private")), at(1));
    state.step(Input::Finish(id), at(2));
    state.step(Input::MicFree(id), at(3));
    let effects = state.step(Input::Collection(id, Ok(())), at(4));
    assert!(matches!(effects.as_slice(), [Effect::Insert(take, ..)] if *take == id));
    state.step(Input::Cancel(Some(id)), at(5));
    assert!(state.draft().is_none());
    assert!(
        state
            .step(Input::Insertion(id, InsertOutcome::Delivered), at(6))
            .is_empty()
    );
}

#[test]
fn always_keep_hotkey_still_watches_for_protected_focus() {
    let mut state = ready();
    state.step(
        Input::Settings(Settings {
            always_keep_in_sambler: true,
            ..Settings::default()
        }),
        at(0),
    );
    let effects = state.step(Input::Hotkey(KeyEdge::Pressed), at(1));
    assert!(matches!(
        effects.as_slice(),
        [Effect::Probe { watch: true, .. }]
    ));
}

#[test]
fn gaps_and_ambiguous_number_boundaries_never_insert() {
    for (index, begin) in [(2, 150), (1, 100)] {
        let mut state = ready();
        let id = start(&mut state, 0);
        state.step(Input::Delivered(id, segment(0, 0..100, "3.")), at(1));
        state.step(
            Input::Delivered(id, segment(index, begin..200, "14")),
            at(2),
        );
        state.step(Input::Finish(id), at(3));
        state.step(Input::MicFree(id), at(4));
        assert!(
            matches!(state.step(Input::Collection(id, Ok(())), at(5)).as_slice(), [Effect::Retire(take)] if *take == id)
        );
        assert_eq!(
            state.ui(at(5)).last_outcome,
            Some(TakeOutcome::KeptInSambler {
                reason: KeptReason::Incomplete
            })
        );
        assert_eq!(state.draft().unwrap().full_text, None);
    }
}

#[test]
fn duration_limit_finishes_and_never_mistakes_a_dead_mic_for_eof() {
    let mut state = ready();
    let id = start(&mut state, 0);
    assert!(
        matches!(state.step(Input::Tick { last_frame: Some(299_980) }, at(300_000)).as_slice(), [Effect::Stop(take)] if *take == id)
    );
    assert_eq!(
        state.ui(at(300_000)).last_outcome,
        interrupted(InterruptReason::DurationLimit, false)
    );
    let mut stalled = ready();
    let id = start(&mut stalled, 0);
    assert!(
        matches!(stalled.step(Input::Tick { last_frame: Some(299_000) }, at(300_000)).as_slice(), [Effect::Retire(take)] if *take == id)
    );
    assert_eq!(
        stalled.ui(at(300_000)).last_outcome,
        interrupted(InterruptReason::MicrophoneStalled, false)
    );
}
