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
    settings::InsertionTarget,
};
use bytes::Bytes;
use fixture::{TestServer, bounded, replies};
use futures_util::stream;
use rambler_protocol::{
    AUDIO_DATA, RECOGNITION_RESPONSE, end_request,
    wire::{Value, bytes_field as b, parse_fields, string_field as s, varint_field as v},
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
    eligible_at(1)
}

fn eligible_at(field: u8) -> TargetVerdict {
    TargetVerdict::Eligible {
        token: TargetToken(Arc::new(field)),
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
    ready: Notify,
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
        self.ready.notify_one();
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
    delayed: Mutex<Option<std_mpsc::Receiver<()>>>,
    capturing_delayed: Notify,
    captures: AtomicUsize,
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
        self.captures.fetch_add(1, Ordering::SeqCst);
        let wait = self.delayed.lock().unwrap().take();
        if let Some(wait) = wait {
            self.capturing_delayed.notify_one();
            let _ = wait.recv_timeout(Duration::from_secs(5));
        }
        self.verdict.lock().unwrap().clone()
    }
    fn watch(&self, _: &TargetToken, callback: OnChange) -> WatchGuard {
        let callback = Arc::new(Mutex::new(Some(callback)));
        *self.change.lock().unwrap() = Some(callback.clone());
        Box::new(Watch(callback))
    }
    fn revalidate(&self, original: &TargetToken) -> Result<(), TargetChange> {
        match &*self.verdict.lock().unwrap() {
            TargetVerdict::Eligible { token, .. }
                if token.0.downcast_ref::<u8>() == original.0.downcast_ref::<u8>() =>
            {
                Ok(())
            }
            TargetVerdict::Protected => Err(TargetChange::Protected),
            _ => Err(TargetChange::FocusMoved),
        }
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
struct TextRecorder(Mutex<Vec<String>>, Mutex<Vec<u8>>);
impl Inserter for TextRecorder {
    fn insert_once(&self, token: &TargetToken, text: &str) -> InsertOutcome {
        self.0.lock().unwrap().push(text.to_owned());
        self.1
            .lock()
            .unwrap()
            .push(*token.0.downcast_ref::<u8>().unwrap());
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
    uploaded_samples: Arc<AtomicUsize>,
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
        let uploaded_samples = Arc::new(AtomicUsize::new(0));
        let second = Arc::new(Notify::new());
        let peer = TestServer::new({
            let requests = requests.clone();
            let uploaded_samples = uploaded_samples.clone();
            let second = second.clone();
            move |request| {
                let index = requests.fetch_add(1, Ordering::SeqCst);
                let uploaded_samples = uploaded_samples.clone();
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
                    uploaded_samples.fetch_add(ogg_samples(&upload), Ordering::SeqCst);
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
            delayed: Default::default(),
            capturing_delayed: Notify::new(),
            captures: AtomicUsize::new(0),
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
            uploaded_samples,
            second,
        }
    }
    fn delay_target(&self, verdict: TargetVerdict) -> std_mpsc::Sender<()> {
        let (release, wait) = std_mpsc::channel();
        *self.target.verdict.lock().unwrap() = verdict;
        *self.target.delayed.lock().unwrap() = Some(wait);
        release
    }
    async fn hotkey_with_mic(&self) -> TakeId {
        self.ctl.hotkey(KeyEdge::Pressed);
        let id = wait(&self.ctl, |s| s.phase == Phase::Listening)
            .await
            .take
            .unwrap();
        bounded(self.mic.ready.notified()).await;
        id
    }
    async fn listening(&self) -> TakeId {
        let id = self.hotkey_with_mic().await;
        wait(&self.ctl, |s| s.destination.is_some()).await;
        id
    }
    async fn idle(&self) -> UiState {
        wait(&self.ctl, |s| s.phase == Phase::Idle).await
    }
    async fn close(self) {
        bounded(self.ctl.shutdown()).await;
        bounded(self.peer.close()).await;
    }
}

fn ogg_samples(upload: &[Bytes]) -> usize {
    let mut ogg = Vec::new();
    for message in upload {
        for field in parse_fields(message).unwrap() {
            if let (AUDIO_DATA, Value::Bytes(data)) = (field.number, field.value) {
                for item in parse_fields(&data).unwrap() {
                    if let (1, Value::Bytes(audio)) = (item.number, item.value) {
                        ogg.extend_from_slice(&audio);
                    }
                }
            }
        }
    }
    let mut offset = 0;
    let mut preskip = 0;
    let mut end = None;
    while offset < ogg.len() {
        let page = &ogg[offset..];
        assert_eq!(&page[..4], b"OggS");
        let lacing_end = 27 + usize::from(page[26]);
        if offset == 0 {
            assert_eq!(&page[lacing_end..][..8], b"OpusHead");
            preskip = usize::from(u16::from_le_bytes(
                page[lacing_end + 10..][..2].try_into().unwrap(),
            ));
        }
        if page[5] & 4 != 0 {
            end = Some(u64::from_le_bytes(page[6..14].try_into().unwrap()) as usize);
        }
        offset += lacing_end
            + page[27..lacing_end]
                .iter()
                .map(|&n| usize::from(n))
                .sum::<usize>();
    }
    assert_eq!(offset, ogg.len());
    end.expect("encoder committed the Ogg EOS page") - preskip
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
    rig.ctl.copy_draft(None).await.unwrap();
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
    rig.ctl.copy_draft(None).await.unwrap();
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
async fn release_while_opening_ignores_callbacks_that_start_after_release() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    let (open, delay) = std_mpsc::channel();
    *rig.mic.delayed.lock().unwrap() = Some(delay);
    rig.ctl.hotkey(KeyEdge::Pressed);
    bounded(rig.mic.opening.notified()).await;
    wait(&rig.ctl, |s| s.phase == Phase::Listening).await;
    rig.ctl.hotkey(KeyEdge::Released);
    wait(&rig.ctl, |s| s.phase == Phase::Finalizing).await;
    open.send(()).unwrap();
    assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::NoText));
    assert_eq!(rig.mic.stop_count.load(Ordering::SeqCst), 1);
    assert_eq!(rig.requests.load(Ordering::SeqCst), 0);
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_mode_keeps_text_when_away_and_the_kept_result_does_not_block_next_take() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    let first = rig.listening().await;
    // The take must still use OriginalField even if the preference changes now.
    rig.ctl.apply_settings(Settings {
        insertion_target: InsertionTarget::CursorAtEnd,
        ..Settings::default()
    });
    rig.mic.feed(&[0; 975]);
    *rig.target.verdict.lock().unwrap() = eligible_at(2);
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
    let second = rig.listening().await;
    assert_eq!(rig.mic.opened.load(Ordering::SeqCst), 2);
    assert_eq!(rig.ctl.draft().await.unwrap().meta.take, first);
    rig.mic.feed(&[0; 975]);
    rig.ctl.hotkey(KeyEdge::Released);
    let ui = rig.idle().await;
    assert_eq!(ui.last_outcome, Some(TakeOutcome::Inserted));
    assert_eq!(ui.recent_count, 2);
    assert_eq!(ui.draft.unwrap().take, second);
    let recent = rig.ctl.recent_drafts().await;
    assert_eq!(
        recent.iter().map(|d| d.meta.take).collect::<Vec<_>>(),
        [second, first]
    );
    rig.ctl.copy_draft(Some(first)).await.unwrap();
    rig.ctl.copy_draft(None).await.unwrap();
    assert_eq!(*rig.copied.0.lock().unwrap(), [FIRST, SECOND]);
    assert_eq!(rig.ctl.clear_draft(Some(first)).await.recent_count, 1);
    assert_eq!(
        rig.ctl.copy_draft(Some(first)).await,
        Err(CopyError::NoDraft)
    );
    assert_eq!(rig.ctl.clear_draft(None).await.recent_count, 0);
    let stats = rig.ctl.stats().await.unwrap();
    assert_eq!(stats.takes_total, 2);
    assert_eq!(stats.words_total, 4);
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leaving_and_returning_inserts_once_into_the_original_field_in_either_mode() {
    for mode in [InsertionTarget::OriginalField, InsertionTarget::CursorAtEnd] {
        let rig = Rig::new(Gesture::Hold, false, None).await;
        rig.ctl.apply_settings(Settings {
            insertion_target: mode,
            ..Settings::default()
        });
        let take = rig.listening().await;
        *rig.target.verdict.lock().unwrap() = eligible_at(2);
        rig.target.change(TargetChange::FocusMoved);
        rig.mic.feed(&[0; 975]);
        *rig.target.verdict.lock().unwrap() = eligible();
        rig.ctl.hotkey(KeyEdge::Released);
        rig.ctl.finish(take);
        assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Inserted));
        assert_eq!(*rig.inserted.1.lock().unwrap(), [1]);
        assert_eq!(rig.target.captures.load(Ordering::SeqCst), 1);
        rig.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cursor_at_end_inserts_once_after_an_eligible_or_uncertain_start() {
    for uncertain_start in [false, true] {
        let rig = Rig::new(Gesture::Hold, false, None).await;
        rig.ctl.apply_settings(Settings {
            insertion_target: InsertionTarget::CursorAtEnd,
            ..Settings::default()
        });
        if uncertain_start {
            *rig.target.verdict.lock().unwrap() = TargetVerdict::Uncertain {
                reason: UncertainReason::Selection,
            };
        }
        let take = rig.listening().await;
        *rig.target.verdict.lock().unwrap() = eligible_at(2);
        if !uncertain_start {
            rig.target.change(TargetChange::FocusMoved);
        }
        rig.mic.feed(&[0; 975]);
        rig.ctl.hotkey(KeyEdge::Released);
        rig.ctl.finish(take);
        assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Inserted));
        assert_eq!(*rig.inserted.1.lock().unwrap(), [2]);
        assert_eq!(rig.target.captures.load(Ordering::SeqCst), 2);
        rig.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cursor_at_end_never_retargets_an_explicit_sambler_destination() {
    for in_app in [false, true] {
        let rig = Rig::new(Gesture::Hold, false, None).await;
        rig.ctl.apply_settings(Settings {
            always_keep_in_sambler: !in_app,
            insertion_target: InsertionTarget::CursorAtEnd,
            ..Settings::default()
        });
        *rig.target.verdict.lock().unwrap() = TargetVerdict::Uncertain {
            reason: UncertainReason::Selection,
        };
        let take = if in_app {
            rig.ctl.dictate_in_sambler();
            let ui = wait(&rig.ctl, |s| {
                s.phase == Phase::Listening && s.destination.is_some()
            })
            .await;
            bounded(rig.mic.ready.notified()).await;
            ui.take.unwrap()
        } else {
            rig.listening().await
        };
        *rig.target.verdict.lock().unwrap() = eligible_at(2);
        rig.mic.feed(&[0; 975]);
        rig.ctl.finish(take);
        assert_eq!(
            rig.idle().await.last_outcome,
            Some(TakeOutcome::KeptInSambler {
                reason: KeptReason::UncertainTarget {
                    reason: UncertainReason::Selection
                },
            })
        );
        assert!(rig.inserted.1.lock().unwrap().is_empty());
        assert_eq!(rig.target.captures.load(Ordering::SeqCst), 1);
        rig.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cursor_at_end_keeps_text_when_end_capture_has_no_eligible_field() {
    for (verdict, reason) in [
        (TargetVerdict::Sambler, KeptReason::SamblerDestination),
        (
            TargetVerdict::Uncertain {
                reason: UncertainReason::ReadOnly,
            },
            KeptReason::UncertainTarget {
                reason: UncertainReason::ReadOnly,
            },
        ),
    ] {
        let rig = Rig::new(Gesture::Hold, false, None).await;
        rig.ctl.apply_settings(Settings {
            insertion_target: InsertionTarget::CursorAtEnd,
            ..Settings::default()
        });
        rig.listening().await;
        *rig.target.verdict.lock().unwrap() = verdict;
        rig.target.change(TargetChange::FocusMoved);
        rig.mic.feed(&[0; 975]);
        rig.ctl.hotkey(KeyEdge::Released);
        assert_eq!(
            rig.idle().await.last_outcome,
            Some(TakeOutcome::KeptInSambler { reason })
        );
        assert_eq!(
            rig.ctl.draft().await.unwrap().full_text.as_deref(),
            Some(FIRST)
        );
        assert!(rig.inserted.1.lock().unwrap().is_empty());
        assert_eq!(rig.target.captures.load(Ordering::SeqCst), 2);
        rig.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn end_capture_respects_protected_focus_and_cancellation() {
    for cancel in [false, true] {
        let rig = Rig::new(Gesture::Hold, false, None).await;
        rig.ctl.apply_settings(Settings {
            insertion_target: InsertionTarget::CursorAtEnd,
            ..Settings::default()
        });
        let take = rig.listening().await;
        let release = rig.delay_target(eligible_at(2));
        rig.target.change(TargetChange::FocusMoved);
        rig.mic.feed(&[0; 975]);
        rig.ctl.hotkey(KeyEdge::Released);
        bounded(rig.target.capturing_delayed.notified()).await;
        if cancel {
            rig.ctl.cancel(Some(take));
            assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Cancelled));
            release.send(()).unwrap(); // An eligible result arrives for the stale take.
        } else {
            *rig.target.verdict.lock().unwrap() = TargetVerdict::Protected;
            release.send(()).unwrap();
            assert_eq!(
                rig.idle().await.last_outcome,
                Some(TakeOutcome::KeptInSambler {
                    reason: KeptReason::TargetChanged {
                        change: TargetChange::Protected
                    },
                })
            );
        }
        assert!(rig.ctl.recent_drafts().await.is_empty());
        assert_eq!(rig.ctl.stats().await.unwrap(), UsageStats::default());
        bounded(rig.ctl.shutdown()).await;
        assert!(rig.inserted.1.lock().unwrap().is_empty());
        assert_eq!(rig.target.captures.load(Ordering::SeqCst), 2);
        rig.close().await;
    }
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
async fn protected_verdict_aborts_buffered_mic_without_a_dictation_request() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    let verdict = rig.delay_target(TargetVerdict::Protected);
    rig.hotkey_with_mic().await;
    rig.mic.feed(&[42; 4_800]);
    assert!(rig.ctl.state().borrow().destination.is_none());
    assert_eq!(rig.requests.load(Ordering::SeqCst), 0);
    verdict.send(()).unwrap();
    assert_eq!(
        rig.idle().await.last_outcome,
        Some(TakeOutcome::Refused {
            reason: RefusalReason::ProtectedField,
        })
    );
    assert!(rig.ctl.recent_drafts().await.is_empty());
    assert_eq!(rig.ctl.stats().await.unwrap(), UsageStats::default());
    bounded(rig.ctl.shutdown()).await;
    assert_eq!(rig.mic.abort_count.load(Ordering::SeqCst), 1);
    assert_eq!(rig.requests.load(Ordering::SeqCst), 0);
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_verdict_and_explicit_start_keep_results_in_sambler() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    *rig.target.verdict.lock().unwrap() = TargetVerdict::Uncertain {
        reason: UncertainReason::Selection,
    };
    rig.listening().await;
    rig.mic.feed(&[0; 975]);
    rig.ctl.hotkey(KeyEdge::Released);
    assert_eq!(
        rig.idle().await.last_outcome,
        Some(TakeOutcome::KeptInSambler {
            reason: KeptReason::UncertainTarget {
                reason: UncertainReason::Selection
            },
        })
    );
    assert_eq!(
        rig.ctl.draft().await.unwrap().full_text.as_deref(),
        Some(FIRST)
    );

    let verdict = rig.delay_target(TargetVerdict::Sambler);
    rig.ctl.dictate_in_sambler();
    let id = wait(&rig.ctl, |s| {
        s.phase == Phase::Listening && s.destination.is_none()
    })
    .await
    .take
    .unwrap();
    bounded(rig.mic.ready.notified()).await;
    rig.mic.feed(&[0; 975]);
    verdict.send(()).unwrap();
    rig.ctl.finish(id);
    assert_eq!(
        rig.idle().await.last_outcome,
        Some(TakeOutcome::KeptInSambler {
            reason: KeptReason::SamblerDestination,
        })
    );
    assert_eq!(
        rig.ctl.draft().await.unwrap().full_text.as_deref(),
        Some(SECOND)
    );
    assert!(rig.inserted.0.lock().unwrap().is_empty());
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn release_stops_the_mic_without_waiting_for_the_target_and_preserves_its_tail() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    let verdict = rig.delay_target(eligible());
    rig.hotkey_with_mic().await;
    rig.mic.feed(&[42; 1_033]); // 1,104 total: a full chunk and a partial tail.
    rig.ctl.hotkey(KeyEdge::Released);
    wait(&rig.ctl, |s| s.phase == Phase::Finalizing).await;
    bounded(async {
        while rig.mic.stop_count.load(Ordering::SeqCst) == 0 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    assert_eq!(rig.requests.load(Ordering::SeqCst), 0);
    verdict.send(()).unwrap();
    assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Inserted));
    assert_eq!(rig.uploaded_samples.load(Ordering::SeqCst), 1_104);
    assert_eq!(*rig.inserted.0.lock().unwrap(), [FIRST]);
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_before_the_verdict_drops_buffered_audio_and_ignores_the_late_result() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    let verdict = rig.delay_target(eligible());
    let take = rig.hotkey_with_mic().await;
    rig.mic.feed(&[42; 4_800]);
    rig.ctl.cancel(Some(take));
    assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Cancelled));
    verdict.send(()).unwrap();
    bounded(rig.ctl.shutdown()).await;
    assert_eq!(rig.mic.abort_count.load(Ordering::SeqCst), 1);
    assert_eq!(rig.requests.load(Ordering::SeqCst), 0);
    assert!(rig.inserted.0.lock().unwrap().is_empty());
    rig.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_verdict_does_not_lose_the_first_frame_or_any_of_1500_ms_of_pcm() {
    let rig = Rig::new(Gesture::Hold, false, None).await;
    let verdict = rig.delay_target(eligible());
    rig.hotkey_with_mic().await;
    for _ in 0..10 {
        rig.mic.feed(&[42; 7_200]);
        sleep(Duration::from_millis(150)).await;
        let ui = rig.ctl.state();
        assert_eq!(ui.borrow().phase, Phase::Listening);
        assert!(ui.borrow().destination.is_none());
    }
    rig.mic.feed(&[42]); // 72,072 samples including the fake mic's first callback.
    assert_eq!(rig.requests.load(Ordering::SeqCst), 0);
    verdict.send(()).unwrap();
    rig.ctl.hotkey(KeyEdge::Released);
    assert_eq!(rig.idle().await.last_outcome, Some(TakeOutcome::Inserted));
    assert_eq!(rig.requests.load(Ordering::SeqCst), 1);
    assert_eq!(rig.uploaded_samples.load(Ordering::SeqCst), 72_072);
    let stats = rig.ctl.stats().await.unwrap();
    assert_eq!(stats.words_total, 2);
    assert_eq!(stats.takes_total, 1);
    assert!(stats.average_wpm.is_some_and(|wpm| (1..=80).contains(&wpm)));
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
    assert!(state.draft().is_none()); // Not committed until the one attempt settles.
    let completed = state.step(Input::Insertion(id, Ok(InsertOutcome::Delivered)), at(5));
    let [Effect::Completed(completion), Effect::Retire(_)] = completed.as_slice() else {
        panic!("expected completion only after insertion settled");
    };
    assert_eq!(completion.words, 2);
    assert_eq!(completion.listening_ms, 2);
    let value = completion.archive.as_ref().unwrap();
    assert_eq!(value.id, id);
    assert_eq!(value.created, at(4).unix);
    assert!(Arc::ptr_eq(&value.text, text));
    assert_eq!(
        state.draft().unwrap().full_text.as_deref(),
        Some("Ancienne règle")
    );
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
        state.step(Input::Insertion(next, Ok(InsertOutcome::Delivered)), at(10)).as_slice(),
        [Effect::Completed(completion), Effect::Retire(_)]
        if completion.archive.as_ref().is_some_and(|value| &*value.text == "Nouvelle règle")
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
    state.step(Input::Target(id, Ok(eligible())), at(time));
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
    state.step(Input::Insertion(b, Ok(InsertOutcome::Delivered)), at(7));
    assert_eq!(state.ui(at(7)).last_outcome, Some(TakeOutcome::Inserted));
    assert_eq!(state.draft().unwrap().full_text.as_deref(), Some("Only B"));
    state.step(Input::Tick { last_frame: None }, at(600_006));
    assert!(state.draft().is_some());
    state.step(Input::Tick { last_frame: None }, at(600_007));
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
            .step(Input::Insertion(id, Ok(InsertOutcome::Delivered)), at(6))
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
    let id = state.take_id().unwrap();
    assert_eq!(state.ui(at(1)).phase, Phase::Listening);
    assert!(
        matches!(effects.as_slice(), [Effect::Probe(take), Effect::Open { .. }] if *take == id)
    );
    state.step(Input::Target(id, Ok(eligible())), at(2));
    state.step(Input::MicOpened(id), at(3));
    state.step(Input::TargetChanged(id, TargetChange::Protected), at(4));
    assert_eq!(
        state.ui(at(4)).last_outcome,
        interrupted(InterruptReason::SystemInterrupted, false)
    );
}

#[test]
fn recents_cap_at_five_expire_individually_and_clear_on_privacy_events() {
    use std::mem::discriminant;
    for clear in [
        Input::System(SystemEvent::SessionLocked),
        Input::System(SystemEvent::Suspending),
        Input::Shutdown,
    ] {
        let mut state = ready();
        state.step(
            Input::Settings(Settings {
                draft_minutes: 1,
                ..Settings::default()
            }),
            at(0),
        );
        let mut ids = Vec::new();
        for time in (0..6).map(|i| 1_000 * i) {
            state.step(Input::StartInternal, at(time));
            let id = state.take_id().unwrap();
            ids.push(id);
            state.step(Input::MicOpened(id), at(time));
            state.step(Input::Target(id, Ok(TargetVerdict::Sambler)), at(time));
            state.step(
                Input::Delivered(id, segment(0, 0..100, "L’été, 中文!")),
                at(time + 10),
            );
            state.step(Input::Finish(id), at(time + 20));
            state.step(Input::MicFree(id), at(time + 30));
            let effects = state.step(Input::Collection(id, Ok(())), at(time + 40));
            assert!(
                matches!(effects.as_slice(), [Effect::Completed(c), Effect::Retire(_)] if c.words == 3 && c.listening_ms == 20)
            );
        }
        let ids_in_ram = |state: &State| {
            state
                .recent_drafts()
                .into_iter()
                .map(|d| d.meta.take)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids_in_ram(&state), [ids[5], ids[4], ids[3], ids[2], ids[1]]);

        state.step(Input::StartInternal, at(6_000));
        state.step(Input::Cancel(None), at(6_010));
        assert_eq!(state.ui(at(6_010)).recent_count, 5);
        state.step(Input::Cancel(Some(ids[3])), at(6_020));
        assert_eq!(ids_in_ram(&state), [ids[5], ids[4], ids[2], ids[1]]);
        state.step(Input::Tick { last_frame: None }, at(61_040));
        assert_eq!(ids_in_ram(&state), [ids[5], ids[4], ids[2]]);
        assert_eq!(state.ui(at(61_040)).draft.unwrap().take, ids[5]);
        let kind = discriminant(&clear);
        state.step(clear, at(61_041));
        assert!(ids_in_ram(&state).is_empty(), "privacy event {kind:?}");
    }
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
