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
    MAX_LONG_SEGMENT_BUDGET, Pcm16Opus, Pcm48kTo16k, collect_long_pcm_dictation,
    collect_long_pcm_dictation_with_observer, pcm_channel,
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

struct Peer {
    server: TestServer,
    calls: Arc<AtomicUsize>,
    uploads: Arc<Mutex<Vec<Vec<u8>>>>,
}
impl Peer {
    async fn new(fail_at: Option<usize>, hold_status: Option<(usize, Arc<Notify>)>) -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let uploads = Arc::new(Mutex::new(Vec::new()));
        let server = TestServer::new({
            let calls = Arc::clone(&calls);
            let uploads = Arc::clone(&uploads);
            move |request| {
                let index = calls.fetch_add(1, Ordering::SeqCst);
                let uploads = Arc::clone(&uploads);
                let hold = hold_status
                    .as_ref()
                    .filter(|(held_index, _)| *held_index == index)
                    .map(|(_, gate)| Arc::clone(gate));
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
