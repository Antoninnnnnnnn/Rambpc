#![cfg(feature = "audio")]

#[path = "../../rambler-audio/tests/common/mod.rs"]
mod audio_common;
#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;

use audio_common::{Source, assert_duration, fragment, packets, pages, pcm};
use bytes::Bytes;
use common::{TestServer, bounded, drain, replies};
use futures_util::{StreamExt, stream};
use rambler_audio::{
    AudioError, EncoderConfig, LongDictationObservation, LongDictationOptions,
    MAX_LONG_SEGMENT_BUDGET, Pcm16Opus, Pcm48kTo16k, PcmInputError, collect_long_pcm_dictation,
    collect_long_pcm_dictation_with_observer, pcm_channel, stream_long_pcm_dictation_with_observer,
};
use rambler_engine::{
    CollectorLimits, ContextInput, EditingContext, Engine, EngineError, SessionOptions,
    collect_dictation,
};
use rambler_protocol::{
    AUDIO_DATA, AUDIO_INFO, INITIAL_PARAMETER, RECOGNITION_RESPONSE, end_request,
    wire::{Value, bytes_field, parse_fields, string_field},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
use tonic::Status;

const END: Bytes = Bytes::from_static(b"\x08\x01");
const LIMITS: CollectorLimits = CollectorLimits {
    max_final_updates: 1,
    max_result_bytes: 4096,
};
fn session() -> SessionOptions {
    SessionOptions {
        timeout: Duration::from_secs(4),
        finalize_timeout: Some(Duration::from_secs(2)),
        ..SessionOptions::default()
    }
}
fn budget(ms: u64, search_ms: u64) -> LongDictationOptions {
    LongDictationOptions {
        segment_budget: Duration::from_millis(ms),
        pause_search: Duration::from_millis(search_ms),
    }
}
fn final_text(index: usize) -> Bytes {
    let text = string_field(1, &format!("segment {index}")).unwrap();
    let hypothesis = bytes_field(3, &text).unwrap();
    let recognition = [&b"\x08\x01"[..], &bytes_field(3, &hypothesis).unwrap()].concat();
    let extension = bytes_field(1, &recognition).unwrap();
    Bytes::from(bytes_field(RECOGNITION_RESPONSE, &extension).unwrap())
}
fn nested_audio(message: &[u8]) -> Vec<u8> {
    let fields = parse_fields(message).unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].number, AUDIO_INFO);
    assert_eq!(fields[1].number, AUDIO_DATA);
    let Value::Bytes(data) = &fields[1].value else {
        panic!("expected audio envelope")
    };
    let fields = parse_fields(data).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].number, 1);
    let Value::Bytes(data) = &fields[0].value else {
        panic!("expected audio bytes")
    };
    data.to_vec()
}

struct NotifyOnDrop(Arc<Notify>);
impl Drop for NotifyOnDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

struct Peer {
    server: TestServer,
    calls: Arc<AtomicUsize>,
    uploads: Arc<Mutex<Vec<Vec<u8>>>>,
    held_response_dropped: Arc<Notify>,
}
impl Peer {
    async fn new(fail_at: Option<usize>, hold_status: Option<(usize, Arc<Notify>)>) -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let uploads = Arc::new(Mutex::new(Vec::new()));
        let held_response_dropped = Arc::new(Notify::new());
        let server = TestServer::new({
            let calls = Arc::clone(&calls);
            let uploads = Arc::clone(&uploads);
            let held_response_dropped = Arc::clone(&held_response_dropped);
            move |request| {
                let index = calls.fetch_add(1, Ordering::SeqCst);
                let uploads = Arc::clone(&uploads);
                let hold = hold_status
                    .as_ref()
                    .filter(|(held_index, _)| *held_index == index)
                    .map(|(_, gate)| Arc::clone(gate));
                let drop_guard = hold
                    .as_ref()
                    .map(|_| NotifyOnDrop(Arc::clone(&held_response_dropped)));
                async move {
                    let messages = drain(request.into_inner()).await;
                    let (init, messages) = messages.split_first().unwrap();
                    assert!(
                        parse_fields(init)
                            .unwrap()
                            .iter()
                            .any(|f| f.number == INITIAL_PARAMETER)
                    );
                    let (end, audio) = messages.split_last().unwrap();
                    assert_eq!(end.as_ref(), end_request());
                    let audio = audio.iter().flat_map(|chunk| nested_audio(chunk)).collect();
                    let mut record = uploads.lock().unwrap();
                    assert_eq!(record.len(), index);
                    record.push(audio);
                    let terminal = stream::once(async move {
                        let _drop_guard = drop_guard;
                        if let Some(hold) = hold {
                            hold.notified().await;
                        }
                        (fail_at == Some(index)).then(|| Err(Status::internal("synthetic status")))
                    })
                    .filter_map(std::future::ready);
                    Ok(replies(
                        stream::iter([Ok(final_text(index)), Ok(END)]).chain(terminal),
                    ))
                }
            }
        })
        .await;
        Self {
            server,
            calls,
            uploads,
            held_response_dropped,
        }
    }

    async fn assert_exact_pcm(&self, all: &[u8], lengths: &[usize]) {
        let uploads = self.uploads.lock().unwrap().clone();
        assert_eq!(uploads.len(), lengths.len());
        let actual_total: usize = lengths.iter().sum();
        assert_eq!(actual_total * 2, all.len());
        let mut cursor = 0;
        let mut serials = std::collections::HashSet::new();
        for (ogg, &samples) in uploads.iter().zip(lengths) {
            assert_duration(ogg, samples);
            assert!(serials.insert(pages(ogg)[0].serial));
            let end = cursor + samples * 2;
            let expected = audio_common::collect(&mut Pcm16Opus::new(
                Source::new([all[cursor..end].to_vec()]),
                EncoderConfig::default(),
            ))
            .await
            .unwrap();
            // Same Opus packets as encoding precisely the expected PCM slice;
            // Ogg serials/CRCs will differ. Duration alone cannot detect reorder.
            assert_eq!(packets(ogg), packets(&expected));
            cursor = end;
        }
    }
}

#[tokio::test]
async fn rolls_at_silence_then_sample_cap_only_after_clean_eof_with_exact_pcm() {
    bounded(async {
        let release_first = Arc::new(Notify::new());
        let peer = Peer::new(None, Some((0, Arc::clone(&release_first)))).await;
        let transport = peer.server.transport().await;
        let engine = Engine::new(transport.clone());
        let mut audio = pcm(32_777);
        audio[12_800 * 2..14_500 * 2].fill(0);
        let (mut sender, input) = pcm_channel();
        let feeding = async {
            for chunk in fragment(&audio, &[237, 1, 3982, 641, 911]) {
                sender.reserve().await.unwrap().send(chunk.into()).unwrap();
            }
            sender.finish().unwrap();
        };
        let first_end = Notify::new();
        let mut completed = 0;
        let mut observe = |event: LongDictationObservation<'_>| match event {
            LongDictationObservation::Response {
                segment_index,
                observation,
            } => {
                assert_eq!(segment_index, completed);
                assert!(observation.metrics.eof_seconds.is_none());
                if segment_index == 0 && observation.metrics.responses_received == 2 {
                    first_end.notify_one();
                }
            }
            LongDictationObservation::SegmentCompleted {
                segment_index,
                sample_range,
            } => {
                assert_eq!(segment_index, completed);
                assert_eq!(
                    sample_range,
                    [0..14_400, 14_400..30_400, 30_400..32_777][segment_index]
                );
                completed += 1;
            }
            _ => unreachable!(),
        };
        let gates = async {
            first_end.notified().await;
            assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
            release_first.notify_one();
        };
        let collecting = collect_long_pcm_dictation_with_observer(
            &engine,
            input,
            session(),
            LIMITS,
            budget(1000, 300),
            &mut observe,
        );
        let ((), result, ()) = tokio::join!(feeding, collecting, gates);
        let result = result.unwrap();
        assert_eq!(completed, 3);
        assert_eq!(result.segments.len(), 3);
        assert_eq!(result.text().unwrap(), "segment 0 segment 1 segment 2");
        assert!(
            result
                .segments
                .iter()
                .all(|segment| segment.context.is_none())
        );
        peer.assert_exact_pcm(&audio, &[14_400, 16_000, 2377]).await;
        assert_eq!(peer.server.accepted_connections(), 1);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        peer.server.close().await;
    })
    .await;
}

#[tokio::test]
async fn exact_cap_eof_completes_once_even_without_projectable_text() {
    bounded(async {
        let calls = Arc::new(AtomicUsize::new(0));
        let peer = TestServer::new({
            let calls = Arc::clone(&calls);
            move |request| {
                calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    let messages = drain(request.into_inner()).await;
                    assert_eq!(messages.last().unwrap().as_ref(), end_request());
                    Ok(replies(stream::iter([Ok(END)])))
                }
            }
        })
        .await;
        let transport = peer.transport().await;
        let engine = Engine::new(transport.clone());
        let mut completed = None;
        let mut observe = |event: LongDictationObservation<'_>| {
            if let LongDictationObservation::SegmentCompleted {
                segment_index,
                sample_range,
            } = event
            {
                assert!(completed.replace((segment_index, sample_range)).is_none());
            }
        };
        let result = collect_long_pcm_dictation_with_observer(
            &engine,
            Source::new([pcm(16_000)]),
            session(),
            LIMITS,
            budget(1000, 0),
            &mut observe,
        )
        .await
        .unwrap();
        assert_eq!(completed, Some((0, 0..16_000)));
        assert_eq!(result.segments.len(), 1);
        assert!(matches!(result.text(), Err(EngineError::AmbiguousResult)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        peer.close().await;
    })
    .await;
}

#[tokio::test]
async fn late_failure_returns_preceding_segments_and_never_replays() {
    bounded(async {
        let peer = Peer::new(Some(1), None).await;
        let transport = peer.server.transport().await;
        let engine = Engine::new(transport.clone());
        let audio = pcm(56_000);
        let source = Source::new([audio.clone()]);
        let stats = Arc::clone(&source.stats);
        let error = collect_long_pcm_dictation(&engine, source, session(), LIMITS, budget(1000, 0))
            .await
            .unwrap_err();
        let EngineError::Transport(error_kind) = &error.cause else {
            panic!("expected transport error: {error}")
        };
        assert!(matches!(
            error_kind,
            rambler_transport::TransportError::Grpc(tonic::Code::Internal)
        ));
        assert_eq!(error.completed.segments.len(), 1);
        assert_eq!(error.completed.text().unwrap(), "segment 0");
        assert!(!format!("{error:?}").contains("segment 0"));
        assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
        assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
        peer.assert_exact_pcm(&audio[..64_000], &[16_000, 16_000])
            .await;
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        peer.server.close().await;
    })
    .await;
}

#[tokio::test]
async fn stalled_final_status_fails_without_pulling_or_opening_the_next_segment() {
    bounded(async {
        let hold = Arc::new(Notify::new());
        let peer = Peer::new(None, Some((1, hold))).await;
        let transport = peer.server.transport().await;
        let engine = Engine::new(transport.clone());
        let audio = pcm(32_002);
        let source = Source::new([
            audio[..32_000].to_vec(),
            audio[32_000..64_000].to_vec(),
            audio[64_000..].to_vec(),
        ]);
        let stats = Arc::clone(&source.stats);
        let options = SessionOptions {
            finalize_timeout: Some(Duration::from_millis(300)),
            ..session()
        };
        let mut ranges = Vec::new();
        let mut saw_second_end = false;
        let mut observe = |event: LongDictationObservation<'_>| match event {
            LongDictationObservation::SegmentCompleted { sample_range, .. } => {
                ranges.push(sample_range)
            }
            LongDictationObservation::Response {
                segment_index: 1,
                observation,
            } => {
                saw_second_end |= observation.metrics.responses_received == 2;
            }
            _ => (),
        };
        let error = collect_long_pcm_dictation_with_observer(
            &engine,
            source,
            options,
            LIMITS,
            budget(1000, 0),
            &mut observe,
        )
        .await
        .unwrap_err();
        assert!(matches!(error.cause, EngineError::FinalizeTimeout));
        assert!(saw_second_end);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 0..16_000);
        assert_eq!(error.completed.text().unwrap(), "segment 0");
        assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
        assert_eq!(stats.reads.load(Ordering::SeqCst), 2);
        assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
        peer.assert_exact_pcm(&audio[..64_000], &[16_000, 16_000])
            .await;
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        peer.server.close().await;
    })
    .await;
}

#[tokio::test]
async fn malformed_tail_and_source_failures_are_not_healthy_eof() {
    bounded(async {
        let ends = Arc::new(AtomicUsize::new(0));
        let peer = TestServer::new({
            let ends = Arc::clone(&ends);
            move |request| {
                let ends = Arc::clone(&ends);
                async move {
                    let mut request = request.into_inner();
                    while let Some(payload) = request.message().await? {
                        if payload.as_ref() == end_request() {
                            ends.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    Ok(replies(stream::iter([Ok(final_text(0)), Ok(END)])))
                }
            }
        })
        .await;
        let transport = peer.transport().await;
        let engine = Engine::new(transport.clone());
        for i in 0..3 {
            let mut source = Source::new([pcm(16_000), vec![0; if i == 0 { 3 } else { 2 }]]);
            source.fail_after = (i == 1).then_some(2);
            source.close_fails = i == 2;
            let stats = Arc::clone(&source.stats);
            let error =
                collect_long_pcm_dictation(&engine, source, session(), LIMITS, budget(1000, 0))
                    .await
                    .unwrap_err();
            match (i, &error.cause) {
                (0, EngineError::Audio(e)) => assert_eq!(
                    e.original().downcast_ref::<AudioError>(),
                    Some(&AudioError::TruncatedPcm)
                ),
                (1, EngineError::Audio(e)) | (2, EngineError::AudioCleanup(e)) => {
                    assert!(e.original().is::<std::io::Error>())
                }
                _ => panic!("unexpected failure: {error:?}"),
            }
            assert_eq!(error.completed.text().unwrap(), "segment 0");
            assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
            assert_eq!(ends.load(Ordering::SeqCst), i + 1);
        }
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        peer.close().await;
    })
    .await;
}

#[tokio::test]
async fn cancellation_closes_idle_source_and_opens_no_session() {
    bounded(async {
        let peer = Peer::new(None, None).await;
        let transport = peer.server.transport().await;
        let engine = Engine::new(transport.clone());
        let mut source = Source::new([pcm(1)]);
        source.block_after = Some(0);
        let read_entered = Arc::clone(&source.read_entered);
        let close_entered = Arc::clone(&source.close_entered);
        let stats = Arc::clone(&source.stats);
        let mut collecting = Box::pin(collect_long_pcm_dictation(
            &engine,
            source,
            session(),
            LIMITS,
            budget(1000, 0),
        ));
        tokio::select! {
            _ = read_entered.notified() => (),
            _ = &mut collecting => panic!("stalled source unexpectedly completed"),
        }
        drop(collecting);
        close_entered.notified().await;
        assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
        assert_eq!(peer.calls.load(Ordering::SeqCst), 0);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        peer.server.close().await;
    })
    .await;
}

fn tracked_pcm(samples: usize) -> (Bytes, std::sync::Weak<()>) {
    struct Owner {
        pcm: Vec<u8>,
        _live: Arc<()>,
    }
    impl AsRef<[u8]> for Owner {
        fn as_ref(&self) -> &[u8] {
            &self.pcm
        }
    }
    let live = Arc::new(());
    let weak = Arc::downgrade(&live);
    (
        Bytes::from_owner(Owner {
            pcm: pcm(samples),
            _live: live,
        }),
        weak,
    )
}

#[tokio::test]
async fn owned_delivery_distinguishes_finish_abort_and_late_failure() {
    use tokio::sync::mpsc;

    #[derive(PartialEq, Eq)]
    enum Stop {
        Finish,
        Abort,
        Fail,
    }
    for stop in [Stop::Finish, Stop::Abort, Stop::Fail] {
        bounded(async {
            let release_second = Arc::new(Notify::new());
            let peer = Peer::new(
                (stop == Stop::Fail).then_some(1),
                Some((1, Arc::clone(&release_second))),
            )
            .await;
            let transport = peer.server.transport().await;
            let engine = Engine::new(transport.clone());
            let (mut producer, input) = pcm_channel();
            let (initial, initial_owner) = tracked_pcm(32_000);
            producer.reserve().await.unwrap().send(initial).unwrap();

            let (deliver, mut receive) = mpsc::unbounded_channel();
            let calls = &peer.calls;
            let mut sink = |segment: rambler_audio::CompletedDictationSegment| {
                assert_eq!(calls.load(Ordering::SeqCst), segment.segment_index + 1);
                deliver.send(segment).unwrap();
            };
            let second_end = Notify::new();
            let mut completed = 0;
            let mut observe = |event: LongDictationObservation<'_>| match event {
                LongDictationObservation::Response {
                    segment_index: 1,
                    observation,
                } if observation.metrics.responses_received == 2 => {
                    second_end.notify_one();
                }
                LongDictationObservation::SegmentCompleted {
                    segment_index,
                    sample_range,
                } => {
                    assert_eq!(segment_index, completed);
                    assert_eq!(
                        sample_range,
                        [0..16_000, 16_000..32_000, 32_000..32_005][segment_index]
                    );
                    completed += 1;
                }
                _ => (),
            };
            let mut collecting = Box::pin(stream_long_pcm_dictation_with_observer(
                &engine,
                input,
                SessionOptions {
                    timeout: Duration::from_secs(60),
                    ..SessionOptions::default()
                },
                LIMITS,
                budget(1000, 0),
                &mut sink,
                &mut observe,
            ));
            let first = tokio::select! {
                first = receive.recv() => first.unwrap(),
                result = &mut collecting => panic!("collection ended before delivery: {result:?}"),
            };
            assert_eq!(first.segment_index, 0);
            assert_eq!(first.sample_range, 0..16_000);
            assert!(first.result.metrics.as_ref().unwrap().eof_seconds.is_some());
            assert!(!format!("{first:?}").contains("segment 0"));

            tokio::select! {
                _ = second_end.notified() => (),
                result = &mut collecting => panic!("collection crossed held status: {result:?}"),
            }
            assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
            assert!(matches!(
                receive.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            assert!(initial_owner.upgrade().is_none());
            // Keep the last 5 samples queued behind the held RPC. Finish drains
            // them; an abort or a late RPC failure instead discards them.
            let (tail, tail_owner) = tracked_pcm(5);
            producer.reserve().await.unwrap().send(tail).unwrap();
            let mut expected_pcm = pcm(32_000);
            match stop {
                Stop::Abort => {
                    drop(collecting);
                    assert!(matches!(
                        producer.reserve().await,
                        Err(PcmInputError::Closed)
                    ));
                    // Drop alone aborts the RPC, without waiting for its deadline
                    // or closing the engine or transport first.
                    peer.held_response_dropped.notified().await;
                    assert_eq!(completed, 1);
                }
                Stop::Fail => {
                    release_second.notify_one();
                    assert!(matches!(
                        collecting.await,
                        Err(EngineError::Transport(
                            rambler_transport::TransportError::Grpc(tonic::Code::Internal)
                        ))
                    ));
                    assert!(matches!(
                        producer.reserve().await,
                        Err(PcmInputError::Closed)
                    ));
                    assert_eq!(completed, 1);
                }
                Stop::Finish => {
                    producer.finish().unwrap();
                    release_second.notify_one();
                    collecting.await.unwrap();
                    assert_eq!(completed, 3);
                    for (index, range, text) in [
                        (1, 16_000..32_000, "segment 1"),
                        (2, 32_000..32_005, "segment 2"),
                    ] {
                        let segment = receive.try_recv().unwrap();
                        assert_eq!(segment.segment_index, index);
                        assert_eq!(segment.sample_range, range);
                        assert_eq!(segment.result.text().unwrap(), text);
                    }
                    expected_pcm.extend(pcm(5));
                }
            }
            assert_eq!(first.result.text().unwrap(), "segment 0");
            assert!(matches!(
                receive.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            assert!(tail_owner.upgrade().is_none());
            assert_eq!(
                peer.calls.load(Ordering::SeqCst),
                if stop == Stop::Finish { 3 } else { 2 }
            );
            let lengths = if stop == Stop::Finish {
                &[16_000, 16_000, 5][..]
            } else {
                &[16_000, 16_000]
            };
            peer.assert_exact_pcm(&expected_pcm, lengths).await;
            engine.close().await.unwrap();
            transport.close().await.unwrap();
            peer.server.close().await;
        })
        .await;
    }
}

#[tokio::test]
async fn short_48k_capture_matches_ordinary_collector_and_upload() {
    bounded(async {
        let pcm48 = pcm(4819);
        let source = || Pcm48kTo16k::new(Source::new(fragment(&pcm48, &[1, 6, 279, 3046, 11])));
        for long in [false, true] {
            let peer = Peer::new(None, None).await;
            let transport = peer.server.transport().await;
            let engine = Engine::new(transport.clone());
            let (result, joined) = if long {
                let mut result = collect_long_pcm_dictation(
                    &engine,
                    source(),
                    session(),
                    LIMITS,
                    LongDictationOptions::default(),
                )
                .await
                .unwrap();
                assert_eq!(result.segments.len(), 1);
                let joined = result.text().unwrap();
                (result.segments.pop().unwrap(), Some(joined))
            } else {
                (
                    collect_dictation(
                        &engine,
                        Box::new(Pcm16Opus::new(source(), EncoderConfig::default())),
                        session(),
                        LIMITS,
                    )
                    .await
                    .unwrap(),
                    None,
                )
            };
            assert_eq!(result.text().unwrap(), "segment 0");
            assert_eq!(result.final_updates.len(), 1);
            assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
            if let Some(joined) = joined {
                assert_eq!(joined, result.text().unwrap());
            }
            let pcm16 = audio_common::collect(&mut source()).await.unwrap();
            peer.assert_exact_pcm(&pcm16, &[4819_usize.div_ceil(3)])
                .await;
            engine.close().await.unwrap();
            transport.close().await.unwrap();
            peer.server.close().await;
        }
    })
    .await;
}

#[tokio::test]
async fn stalled_capture_rolls_on_wall_budget_and_keeps_orphan_byte() {
    bounded(async {
        let peer = Peer::new(None, None).await;
        let transport = peer.server.transport().await;
        let engine = Engine::new(transport.clone());
        let (mut sender, input) = pcm_channel();
        let audio = pcm(3000);
        let cut = 711 * 2;
        let completed = Notify::new();
        let feed = async {
            sender
                .reserve()
                .await
                .unwrap()
                .send(Bytes::copy_from_slice(&audio[..=cut]))
                .unwrap();
            // A sample-count-only cutoff would deadlock waiting for this producer.
            completed.notified().await;
            sender
                .reserve()
                .await
                .unwrap()
                .send(Bytes::copy_from_slice(&audio[cut + 1..]))
                .unwrap();
            sender.finish().unwrap();
        };
        let mut observe = |event: LongDictationObservation<'_>| {
            if let LongDictationObservation::SegmentCompleted {
                segment_index,
                sample_range,
            } = event
            {
                match segment_index {
                    0 => {
                        assert_eq!(sample_range, 0..711);
                        completed.notify_one();
                    }
                    1 => assert_eq!(sample_range, 711..3000),
                    _ => panic!("unexpected additional segment"),
                }
            }
        };
        let collecting = collect_long_pcm_dictation_with_observer(
            &engine,
            input,
            session(),
            LIMITS,
            budget(250, 0),
            &mut observe,
        );
        let ((), result) = tokio::join!(feed, collecting);
        assert_eq!(result.unwrap().segments.len(), 2);
        peer.assert_exact_pcm(&audio, &[711, 2289]).await;
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        peer.server.close().await;
    })
    .await;
}

#[tokio::test]
async fn rejects_invalid_policy_and_context_and_does_not_open_empty_sessions() {
    bounded(async {
        let peer = Peer::new(None, None).await;
        let transport = peer.server.transport().await;
        let engine = Engine::new(transport.clone());
        let over = LongDictationOptions {
            segment_budget: MAX_LONG_SEGMENT_BUDGET + Duration::from_millis(1),
            pause_search: Duration::ZERO,
        };
        for (options, session) in [
            (over, session()),
            (budget(0, 0), session()),
            (budget(1000, 1000), session()),
            (budget(1000, 10), SessionOptions { context: ContextInput::Static(EditingContext::default()), ..session() }),
        ] {
            let source = Source::new([vec![0, 0]]);
            let stats = Arc::clone(&source.stats);
            let error = collect_long_pcm_dictation(&engine, source, session, LIMITS, options).await.unwrap_err();
            assert!(matches!(error.cause, EngineError::InvalidConfig));
            assert!(error.completed.segments.is_empty());
            assert_eq!(stats.reads.load(Ordering::SeqCst), 0);
            assert_eq!(stats.closes.load(Ordering::SeqCst), 1);
        }
        let empty = collect_long_pcm_dictation(&engine, Source::new([]), session(), LIMITS, budget(1000, 0)).await.unwrap();
        assert!(empty.segments.is_empty());
        assert_eq!(empty.text().unwrap(), "");
        let tail = Source::new([vec![123]]);
        let error = collect_long_pcm_dictation(&engine, tail, session(), LIMITS, budget(1000, 0)).await.unwrap_err();
        assert!(matches!(error.cause, EngineError::Audio(error) if error.original().downcast_ref::<AudioError>() == Some(&AudioError::TruncatedPcm)));
        assert_eq!(peer.calls.load(Ordering::SeqCst), 0);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        peer.server.close().await;
    }).await;
}

fn interactive_fixture(samples: usize, tail_samples: usize) -> Vec<u8> {
    let tone: Vec<_> = (0..160)
        .map(|i| (f64::sin(std::f64::consts::TAU * f64::from(i) * 3.0 / 160.0) * 1200.0) as i16)
        .collect();
    (0..samples)
        .flat_map(|i| {
            let sample = if i >= samples - tail_samples {
                10 * tone[i % 160]
            } else if (10 * 16_000..19 * 16_000).contains(&i) {
                0
            } else {
                tone[i % 160]
            };
            sample.to_le_bytes()
        })
        .collect()
}

fn decoded_pcm(ogg: &[u8]) -> Vec<i16> {
    let packets = packets(ogg);
    let skip = u16::from_le_bytes(packets[0][10..12].try_into().unwrap()) as usize;
    let end = pages(ogg).last().unwrap().granule as usize;
    let mut decoder = opus::Decoder::new(48000, opus::Channels::Mono).unwrap();
    let mut decoded = Vec::new();
    let mut frame = [0_i16; 960];
    for packet in &packets[2..] {
        let count = decoder.decode(packet, &mut frame, false).unwrap();
        decoded.extend_from_slice(&frame[..count]);
    }
    decoded[skip..end].to_vec()
}

fn mean_square(pcm: &[i16]) -> f64 {
    pcm.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() / pcm.len() as f64
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accelerated_interactive_cadence_preserves_pause_tail_and_boundaries() {
    use rambler_audio::InteractivePcmProfile;
    use rambler_engine::StreamAudio;
    use tokio::time::{Instant, sleep_until, timeout};

    timeout(Duration::from_secs(90), async {
        let profile = InteractivePcmProfile::default();
        assert_eq!(
            profile.segmentation.segment_budget,
            Duration::from_secs(120)
        );
        assert_eq!(profile.segmentation.pause_search, Duration::from_secs(5));
        assert_eq!(profile.session.timeout, Duration::from_secs(135));
        assert_eq!(
            profile.session.finalize_timeout,
            Some(Duration::from_secs(5))
        );
        assert_eq!(profile.session.context_timeout, Duration::from_secs(2));
        const RATE: usize = 16_000;
        const BATCH: usize = RATE * 2;
        for (samples, expected_lengths) in [
            (29 * RATE, &[29 * RATE][..]),
            (30 * RATE, &[30 * RATE]),
            (31 * RATE, &[31 * RATE]),
            (120 * RATE - 1, &[120 * RATE - 1]),
            (120 * RATE, &[120 * RATE]),
            (120 * RATE + 1, &[120 * RATE, 1]),
            (300 * RATE, &[120 * RATE, 120 * RATE, 60 * RATE]),
        ] {
            let peer = Peer::new(None, None).await;
            let engine = Engine::connect_ready(
                rambler_transport::TransportConfig::loopback(peer.server.address).unwrap(),
            )
            .await
            .unwrap();
            let audio = Bytes::from(interactive_fixture(samples, 3200));
            let first_release = Instant::now();
            // One second of real-sized PCM per 6 ms, plus a distinct no-data
            // pause at the same position as the encoded mid-capture silence.
            let release_offset = |position: usize| {
                Duration::from_millis(
                    ((position / BATCH) * 6 + usize::from(position >= 10 * BATCH) * 120) as u64,
                )
            };
            let source = StreamAudio::new(stream::unfold(
                (audio, 0),
                move |(audio, offset)| async move {
                    if offset == audio.len() {
                        return None;
                    }
                    sleep_until(first_release + release_offset(offset)).await;
                    let end = (offset + BATCH).min(audio.len());
                    Some((Ok(audio.slice(offset..end)), (audio, end)))
                },
            ));
            let mut ranges = Vec::new();
            let mut observer = |event: LongDictationObservation<'_>| {
                if let LongDictationObservation::SegmentCompleted { sample_range, .. } = event {
                    ranges.push(sample_range);
                }
            };
            let result = collect_long_pcm_dictation_with_observer(
                &engine,
                source,
                profile.session.clone(),
                LIMITS,
                profile.segmentation,
                &mut observer,
            )
            .await
            .unwrap();
            assert!(first_release.elapsed() >= release_offset(samples * 2 - 1));
            let mut expected_start = 0;
            for (range, &length) in ranges.iter().zip(expected_lengths) {
                let end = expected_start + length as u64;
                assert_eq!(*range, expected_start..end);
                expected_start = end;
            }
            assert_eq!(ranges.len(), expected_lengths.len());
            assert_eq!(result.segments.len(), expected_lengths.len());
            assert_eq!(peer.calls.load(Ordering::SeqCst), expected_lengths.len());
            assert_eq!(peer.server.accepted_connections(), 1);
            let uploads = peer.uploads.lock().unwrap().clone();
            for (ogg, &length) in uploads.iter().zip(expected_lengths) {
                assert_duration(ogg, length);
            }
            let first = decoded_pcm(&uploads[0]);
            assert!(mean_square(&first[14 * 48_000..15 * 48_000]) < 100.0);
            // At cap + one sample the witness mostly belongs to the preceding
            // segment; assert the one-sample tail separately by EOS granule.
            let witness = if uploads.len() == 1 || expected_lengths.last() == Some(&1) {
                &first
            } else {
                &decoded_pcm(uploads.last().unwrap())
            };
            let end = witness.len();
            assert!(mean_square(&witness[end - 7000..end - 1000]) > 4_000.0f64.powi(2));
            assert!(
                result
                    .text()
                    .unwrap()
                    .ends_with(&format!("segment {}", uploads.len() - 1))
            );
            engine.close().await.unwrap();
            peer.server.close().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn interactive_budget_requires_whole_samples_and_checks_arithmetic() {
    use rambler_audio::InteractivePcmProfile;
    let options = LongDictationOptions {
        segment_budget: Duration::from_nanos(62_500),
        pause_search: Duration::ZERO,
    };
    let finalize = Duration::from_millis(125);
    let profile = InteractivePcmProfile::new(options, finalize).unwrap();
    assert_eq!(
        profile.session.timeout,
        Duration::from_secs(10) + options.segment_budget + finalize,
    );
    for (options, finalize) in [
        (
            LongDictationOptions {
                segment_budget: Duration::from_nanos(62_499),
                ..options
            },
            finalize,
        ),
        (options, Duration::ZERO),
        (options, Duration::MAX),
    ] {
        assert!(matches!(
            InteractivePcmProfile::new(options, finalize),
            Err(EngineError::InvalidConfig)
        ));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interactive_cli_retains_completed_wav_segment_on_late_finalize_timeout() {
    use std::process::Stdio;
    use tokio::{io::AsyncReadExt, process::Command};
    tokio::time::timeout(Duration::from_secs(25), async {
        let held = Arc::new(Notify::new());
        let peer = Peer::new(None, Some((2, held))).await;
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("synthetic-interactive.wav");
        let mut expected_total_requests = 0;
        for (samples, healthy) in [(3_307, true), (7 * 16_000 + 7, false)] {
            let pcm = interactive_fixture(samples, 3200);
            std::fs::write(&path, audio_common::wav(&pcm)).unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_rambler"));
            command.env_clear();
            if let Some(root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", &root);
                #[cfg(windows)]
                {
                    let root = std::path::PathBuf::from(root);
                    command.env(
                        "PATH",
                        std::env::join_paths([root.join("System32"), root]).unwrap(),
                    );
                }
            }
            let mut child = command
                .current_dir(fixture.path())
                .args([
                    "dictate-wav",
                    "--interactive",
                    "--segment-seconds",
                    "6",
                    "--timeout",
                    "12",
                    "--finalize-timeout",
                    "1",
                    "--loopback",
                ])
                .arg(peer.server.address.to_string())
                .arg(&path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let mut stdout = Vec::new();
            let mut limited = child.stdout.take().unwrap().take(16 * 1024);
            let (status, _) =
                tokio::try_join!(child.wait(), limited.read_to_end(&mut stdout)).unwrap();
            let json: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
            let completed = json["completed_segments"].as_array().unwrap();
            assert_eq!(completed.len(), 1);
            assert_eq!(completed[0]["type"], "result");
            let range = completed[0]["sample_range"].as_array().unwrap();
            assert_eq!(range[0], 0);
            let first_end = range[1].as_u64().unwrap() as usize;
            if healthy {
                expected_total_requests += 1;
                assert_eq!(status.code(), Some(0));
                assert_eq!(json["type"], "result");
                assert_eq!(json["text"], "segment 0");
                assert_eq!(first_end, samples);
            } else {
                expected_total_requests += 2;
                assert_eq!(status.code(), Some(1));
                assert_eq!(json["type"], "error");
                assert_eq!(json["category"], "finalize_timeout");
                assert!(json.get("text").is_none());
                assert_eq!(completed[0]["text"], "segment 1");
                assert!((5 * 16_000..=6 * 16_000).contains(&first_end));
                let uploads = peer.uploads.lock().unwrap();
                assert_duration(&uploads[1], first_end);
                assert_duration(&uploads[2], samples - first_end);
                assert!(!String::from_utf8_lossy(&stdout).contains("segment 2"));
            }
            assert_eq!(peer.calls.load(Ordering::SeqCst), expected_total_requests);
        }
        peer.server.close().await;
    })
    .await
    .unwrap();
}
