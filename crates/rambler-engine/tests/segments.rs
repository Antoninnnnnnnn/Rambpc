mod common;
use bytes::Bytes;
use common::*;
use futures_util::stream;
use rambler_engine::*;
use rambler_protocol::{
    RECOGNITION_RESPONSE,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn segment(state: Option<u64>, texts: Option<&[&str]>, safety: Option<u64>) -> Bytes {
    let mut recognition = state.map_or_else(Vec::new, |state| v(1, state).unwrap());
    if let Some(texts) = texts {
        let hypotheses: Vec<_> = texts
            .iter()
            .flat_map(|text| {
                let mut h = s(1, text).unwrap();
                if let Some(safety) = safety {
                    h.extend(v(30, safety).unwrap());
                }
                b(3, &h).unwrap()
            })
            .collect();
        recognition.extend(b(3, &hypotheses).unwrap());
    }
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}
fn audio() -> Box<dyn AudioSource> {
    Box::new(StreamAudio::new(stream::iter([Ok(Bytes::from_static(
        b"controlled encoded audio",
    ))])))
}
#[tokio::test]
async fn keeps_all_segment_updates_and_global_marker_with_response_ordinals_no_assembly() {
    let data = vec![
        Bytes::from_static(b"\x08\x00"),
        segment(Some(0), Some(&["A", "alternative"]), None),
        Bytes::from_static(b"\x08\x00"),
        segment(None, Some(&["AB"]), None),
        segment(Some(1), None, None),
        Bytes::from_static(b"\x08\x01"),
    ];
    let sent = data.clone();
    let server = TestServer::new(move |request| {
        let sent = sent.clone();
        async move {
            drain(request.into_inner()).await;
            Ok(replies(stream::iter(sent.into_iter().map(Ok))))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let result = bounded(collect_segments(
        &engine,
        audio(),
        SessionOptions::default(),
        CollectorLimits::default(),
    ))
    .await
    .unwrap();
    assert_eq!(
        result
            .updates
            .iter()
            .map(|u| u.response_ordinal)
            .collect::<Vec<_>>(),
        [2, 4, 5]
    );
    for (update, original) in result.updates.iter().zip([&data[1], &data[3], &data[4]]) {
        assert_eq!(update.response.raw.as_ref(), original.as_ref());
    }
    assert_eq!(result.updates[0].response.hypotheses().len(), 2);
    assert_eq!(
        result.updates[0].response.hypotheses()[0].text.as_deref(),
        Some("A")
    );
    assert_eq!(
        result.updates[1].response.hypotheses()[0].text.as_deref(),
        Some("AB")
    );
    assert_eq!(result.updates[2].response.recognition_state(), Some(1));
    assert!(result.updates[2].response.hypotheses().is_empty());
    assert!(result.metrics.unwrap().audio.eof);
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}
#[tokio::test]
async fn segment_limits_and_late_grpc_failures_never_return_partial_success() {
    for late_error in [false, true] {
        let server = TestServer::new(move |request| async move {
            drain(request.into_inner()).await;
            let mut responses = vec![
                Ok(segment(Some(0), Some(&["A"]), None)),
                Ok(segment(Some(0), Some(&["AB"]), None)),
                Ok(segment(Some(1), Some(&["ABC"]), None)),
                Ok(Bytes::from_static(b"\x08\x01")),
            ];
            if late_error {
                responses.push(Err(tonic::Status::unavailable("PRIVATE")));
            }
            Ok(replies(stream::iter(responses)))
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let result = bounded(collect_segments(
            &engine,
            audio(),
            SessionOptions::default(),
            CollectorLimits {
                max_final_updates: if late_error { 128 } else { 2 },
                ..Default::default()
            },
        ))
        .await;
        if late_error {
            assert!(matches!(
                result,
                Err(EngineError::Transport(
                    rambler_transport::TransportError::Grpc(tonic::Code::Unavailable)
                ))
            ));
        } else {
            assert!(matches!(result, Err(EngineError::ResultLimit)));
        }
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    }
}
#[tokio::test]
async fn safety_in_earlier_segment_is_not_lost_to_a_later_unblocked_global_final() {
    let server = TestServer::new(|request| async move {
        drain(request.into_inner()).await;
        Ok(replies(stream::iter([
            Ok(segment(Some(0), Some(&["blocked"]), Some(1))),
            Ok(segment(Some(1), Some(&["later"]), Some(0))),
            Ok(Bytes::from_static(b"\x08\x01")),
        ])))
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let result = bounded(collect_segments(
        &engine,
        audio(),
        SessionOptions::default(),
        CollectorLimits::default(),
    ))
    .await
    .unwrap();
    assert_eq!(result.updates.len(), 2);
    assert!(matches!(
        result.check_safety(),
        Err(EngineError::SafetyBlocked)
    ));
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}
struct Tracked(Arc<AtomicUsize>);
impl AudioSource for Tracked {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async { Ok(None) })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}
#[tokio::test]
async fn unpolled_segment_collection_and_invalid_limits_keep_immediate_source_ownership() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let closed = Arc::new(AtomicUsize::new(0));
    let future = collect_segments(
        &engine,
        Box::new(Tracked(closed.clone())),
        SessionOptions::default(),
        CollectorLimits::default(),
    );
    drop(future);
    let invalid = collect_segments(
        &engine,
        Box::new(Tracked(closed.clone())),
        SessionOptions::default(),
        CollectorLimits {
            max_final_updates: 0,
            ..Default::default()
        },
    );
    assert!(matches!(invalid.await, Err(EngineError::InvalidConfig)));
    bounded(engine.close()).await.unwrap();
    assert_eq!(closed.load(Ordering::SeqCst), 2);
    transport.close().await.unwrap();
    server.close().await;
}
