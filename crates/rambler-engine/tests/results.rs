mod common;
use bytes::Bytes;
use common::*;
use futures_util::stream;
use rambler_engine::*;
use rambler_protocol::{
    RECOGNITION_RESPONSE, VOICE_EDITING_RESULT, decode_response,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use rambler_transport::TransportError;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;
use tonic::{Code, Status};

fn final_payload(text: &str, count: usize) -> Bytes {
    let edit = [v(4, 2).unwrap(), s(3, text).unwrap()].concat();
    let hypothesis = [
        s(1, "hypothesis").unwrap(),
        b(VOICE_EDITING_RESULT, &edit).unwrap(),
    ]
    .concat();
    let hypotheses = (0..count)
        .flat_map(|_| b(3, &hypothesis).unwrap())
        .collect::<Vec<_>>();
    let recognition = [v(1, 1).unwrap(), b(3, &hypotheses).unwrap()].concat();
    [
        b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap(),
        v(99, 12).unwrap(),
    ]
    .concat()
    .into()
}
fn audio() -> Box<dyn AudioSource> {
    Box::new(StreamAudio::new(stream::iter([Ok(Bytes::from_static(
        b"audio",
    ))])))
}

#[tokio::test]
async fn collector_retains_all_final_updates_in_order_without_dedup_or_merge() {
    let first = final_payload("first", 1);
    let second = final_payload("second", 1);
    let payloads = vec![
        first.clone(),
        first.clone(),
        second.clone(),
        Bytes::from_static(b"\x08\x01"),
    ];
    let server = TestServer::new(move |request| {
        let payloads = payloads.clone();
        async move {
            drain(request.into_inner()).await;
            Ok(replies(stream::iter(payloads.into_iter().map(Ok))))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let result = bounded(collect_dictation(
        &engine,
        audio(),
        SessionOptions::default(),
        CollectorLimits::default(),
    ))
    .await
    .unwrap();
    assert_eq!(result.final_updates.len(), 3);
    assert_eq!(result.final_updates[0].raw, first);
    assert_eq!(result.final_updates[1].raw, first);
    assert_eq!(result.final_updates[2].raw, second);
    assert!(!result.final_updates[0].unknown_fields.is_empty());
    assert!(matches!(result.text(), Err(EngineError::AmbiguousResult)));
    assert!(result.metrics.unwrap().eof_seconds.is_some());
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn collector_limits_count_original_protobuf_bytes_and_close_only_the_session() {
    let payload = final_payload("edited", 1);
    let length = payload.len();
    let server = TestServer::new(move |request| {
        let payload = payload.clone();
        async move {
            drain(request.into_inner()).await;
            Ok(replies(stream::iter([
                Ok(payload.clone()),
                Ok(payload),
                Ok(Bytes::from_static(b"\x08\x01")),
            ])))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    for limits in [
        CollectorLimits {
            max_final_updates: 1,
            max_result_bytes: usize::MAX,
        },
        CollectorLimits {
            max_final_updates: 128,
            max_result_bytes: length * 2 - 1,
        },
    ] {
        assert!(matches!(
            bounded(collect_dictation(
                &engine,
                audio(),
                SessionOptions::default(),
                limits
            ))
            .await,
            Err(EngineError::ResultLimit)
        ));
    }
    let result = bounded(collect_dictation(
        &engine,
        audio(),
        SessionOptions::default(),
        CollectorLimits {
            max_final_updates: 2,
            max_result_bytes: length * 2,
        },
    ))
    .await
    .unwrap();
    assert_eq!(result.final_updates.len(), 2);
    let _ = engine.close().await;
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn late_s3_or_grpc_or_missing_end_never_return_a_result() {
    for mode in 0..3 {
        let server = TestServer::new(move |request| async move {
            drain(request.into_inner()).await;
            let mut payloads = vec![Ok(final_payload("edited", 1))];
            match mode {
                0 => {
                    payloads.push(Ok(Bytes::from_static(b"\x08\x01")));
                    payloads.push(Err(Status::internal("private failure")));
                }
                1 => {
                    payloads.push(Ok(Bytes::from_static(b"\x08\x02")));
                    payloads.push(Ok(Bytes::from_static(b"\x08\x01")));
                }
                _ => {}
            }
            Ok(replies(stream::iter(payloads)))
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let error = bounded(collect_dictation(
            &engine,
            audio(),
            SessionOptions::default(),
            CollectorLimits::default(),
        ))
        .await
        .unwrap_err();
        match mode {
            0 => assert!(matches!(
                error,
                EngineError::Transport(TransportError::Grpc(Code::Internal))
            )),
            1 => assert!(matches!(error, EngineError::S3(_))),
            _ => assert!(matches!(error, EngineError::IncompleteSession(_))),
        }
        let _ = engine.close().await;
        let _ = transport.close().await;
        server.close().await;
    }
}

#[tokio::test]
async fn no_result_before_end_and_terminal_eof_even_after_final() {
    let sent_final = Arc::new(Semaphore::new(0));
    let release_end = Arc::new(Semaphore::new(0));
    let sent_final2 = sent_final.clone();
    let release_end2 = release_end.clone();
    let server = TestServer::new(move |request| {
        let sent = sent_final2.clone();
        let release = release_end2.clone();
        async move {
            drain(request.into_inner()).await;
            Ok(replies(stream::unfold(0, move |state| {
                let sent = sent.clone();
                let release = release.clone();
                async move {
                    match state {
                        0 => {
                            sent.add_permits(1);
                            Some((Ok(final_payload("edited", 1)), 1))
                        }
                        1 => {
                            release.acquire().await.unwrap().forget();
                            Some((Ok(Bytes::from_static(b"\x08\x01")), 2))
                        }
                        _ => None,
                    }
                }
            })))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Arc::new(Engine::new(transport.clone()));
    let engine2 = engine.clone();
    let task = tokio::spawn(async move {
        collect_dictation(
            &engine2,
            audio(),
            SessionOptions::default(),
            CollectorLimits::default(),
        )
        .await
    });
    bounded(sent_final.acquire()).await.unwrap().forget();
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    release_end.add_permits(1);
    assert_eq!(
        bounded(task).await.unwrap().unwrap().text().unwrap(),
        "edited"
    );
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

struct LateContext {
    calls: Arc<AtomicUsize>,
}
impl ContextProvider for LateContext {
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
        Box::pin(async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(EditingContext {
                before: "actually captured late".into(),
                ..Default::default()
            }))
        })
    }
}
#[tokio::test]
async fn collector_uses_the_actual_late_context_for_proposals() {
    let server = TestServer::new(|request| async move {
        drain(request.into_inner()).await;
        Ok(replies(stream::iter([
            Ok(final_payload("edited", 1)),
            Ok(Bytes::from_static(b"\x08\x01")),
        ])))
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let options = SessionOptions {
        context: ContextInput::Provider(Arc::new(LateContext {
            calls: calls.clone(),
        })),
        ..Default::default()
    };
    let result = bounded(collect_dictation(
        &engine,
        audio(),
        options,
        CollectorLimits::default(),
    ))
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        result.context.as_ref().unwrap().before,
        "actually captured late"
    );
    let proposal = replacement_proposal(&result, Some(Revision::from(7i64))).unwrap();
    assert_eq!(proposal.snapshot.before, "actually captured late");
    assert_eq!(proposal.replacement_text, "edited");
    assert_eq!(proposal.selection_utf16_range(), None);
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[test]
fn multiple_hypotheses_are_ambiguous_even_in_one_final_update() {
    let result = DictationResult {
        final_updates: vec![decode_response(&final_payload("edited", 2)).unwrap()],
        ..Default::default()
    };
    assert!(matches!(result.text(), Err(EngineError::AmbiguousResult)));
    assert!(matches!(
        replacement_proposal(&result, None),
        Err(EngineError::AmbiguousResult)
    ));
}

#[tokio::test]
async fn zero_final_updates_is_valid_completion_but_not_unique_text() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let result = bounded(collect_dictation(
        &engine,
        audio(),
        SessionOptions::default(),
        CollectorLimits::default(),
    ))
    .await
    .unwrap();
    assert!(result.final_updates.is_empty());
    assert!(matches!(result.text(), Err(EngineError::AmbiguousResult)));
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn tiny_global_deadline_is_not_success() {
    let server = TestServer::new(|request| async move {
        drain(request.into_inner()).await;
        Ok(replies(stream::pending()))
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let options = SessionOptions {
        timeout: Duration::from_millis(20),
        ..Default::default()
    };
    assert!(matches!(
        bounded(collect_dictation(
            &engine,
            audio(),
            options,
            CollectorLimits::default()
        ))
        .await,
        Err(EngineError::Transport(TransportError::Timeout))
    ));
    let _ = engine.close().await;
    let _ = transport.close().await;
    server.close().await;
}
