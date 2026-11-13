mod common;
use bytes::Bytes;
use common::*;
use futures_util::stream;
use rambler_engine::*;
use rambler_protocol::{context_request, end_request, wire::parse_fields};
use rambler_transport::{MetadataFuture, TransportConfig, TransportError};
use std::{
    collections::VecDeque,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;
use tonic::{Code, Status};

struct Audio {
    chunks: VecDeque<Bytes>,
    block: bool,
    closes: Arc<AtomicUsize>,
    closed: Arc<AtomicBool>,
    close_started: Arc<Semaphore>,
    close_release: Option<Arc<Semaphore>>,
    after_first: Option<Arc<Semaphore>>,
    fail: bool,
}
impl Audio {
    fn new(chunks: &[&'static [u8]]) -> Self {
        Self {
            chunks: chunks
                .iter()
                .map(|chunk| Bytes::from_static(chunk))
                .collect(),
            block: false,
            closes: Arc::new(AtomicUsize::new(0)),
            closed: Arc::new(AtomicBool::new(false)),
            close_started: Arc::new(Semaphore::new(0)),
            close_release: None,
            after_first: None,
            fail: false,
        }
    }
}
impl AudioSource for Audio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if let Some(gate) = self.after_first.take() {
                gate.acquire().await.unwrap().forget();
            }
            if let Some(chunk) = self.chunks.pop_front() {
                return Ok(Some(chunk));
            }
            if self.block {
                std::future::pending::<()>().await;
            }
            if self.fail {
                return Err(SourceError::new(std::io::Error::other(
                    "private audio failure",
                )));
            }
            Ok(None)
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.closes.fetch_add(1, Ordering::SeqCst);
            self.close_started.add_permits(1);
            if let Some(gate) = &self.close_release {
                gate.acquire().await.unwrap().forget();
            }
            self.closed.store(true, Ordering::SeqCst);
            Ok(())
        })
    }
}
struct Provider<F>(F);
impl<F, Fut> ContextProvider for Provider<F>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Option<EditingContext>, SourceError>> + Send + 'static,
{
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
        Box::pin((self.0)())
    }
}
fn context(text: &str) -> EditingContext {
    EditingContext {
        before: text.into(),
        cursor_aware: true,
        ..Default::default()
    }
}
async fn collect(mut session: EngineSession) -> Result<Vec<EngineEvent>, EngineError> {
    let mut events = Vec::new();
    while let Some(event) = session.next().await? {
        events.push(event);
    }
    Ok(events)
}

#[tokio::test]
async fn exact_sequence_nonempty_audio_cleanup_before_late_context_and_metrics() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    let server = TestServer::new(move |request| {
        let seen = seen2.clone();
        async move {
            *seen.lock().unwrap() = drain(request.into_inner()).await;
            Ok(replies(stream::iter([
                Ok(Bytes::from_static(b"\x08\x00")),
                Ok(Bytes::from_static(b"\x08\x01")),
            ])))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let audio = Audio::new(&[b"one", b"", b"last"]);
    let closed = audio.closed.clone();
    let closes = audio.closes.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = calls.clone();
    let captured = context("late 😀");
    let expected = captured.clone();
    let provider = Provider(move || {
        assert!(closed.load(Ordering::SeqCst));
        calls2.fetch_add(1, Ordering::SeqCst);
        let value = captured.clone();
        async move { Ok(Some(value)) }
    });
    let events = bounded(collect(
        engine
            .start(
                Box::new(audio),
                SessionOptions {
                    context: ContextInput::Provider(Arc::new(provider)),
                    ..Default::default()
                },
            )
            .unwrap(),
    ))
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.response.is_none())
            .count(),
        1
    );
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 5);
    assert_eq!(seen[3], context_request(&expected).unwrap());
    assert_eq!(seen[4], end_request().as_slice());
    let audio_fields: Vec<_> = seen
        .iter()
        .flat_map(|payload| parse_fields(payload).unwrap())
        .filter(|field| field.number == 293101)
        .collect();
    assert_eq!(audio_fields.len(), 2);
    let metrics = &events.last().unwrap().metrics;
    assert_eq!(metrics.requests_produced, 5);
    assert_eq!(
        metrics.request_protobuf_bytes,
        seen.iter().map(|value| value.len() as u64).sum::<u64>()
    );
    assert_eq!(metrics.responses_received, 2);
    assert_eq!(metrics.response_protobuf_bytes, 4);
    assert!(metrics.first_response_seconds.unwrap() <= metrics.eof_seconds.unwrap());
    assert!(metrics.eof_seconds.unwrap() <= metrics.elapsed_seconds);
    engine.close().await.unwrap();
    // An injected channel remains usable by a different engine.
    let other = Engine::new(transport.clone());
    bounded(collect(
        other
            .start(Box::new(Audio::new(&[])), SessionOptions::default())
            .unwrap(),
    ))
    .await
    .unwrap();
    other.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn late_grpc_failure_wins_over_end_and_s3_error() {
    for response in [
        b"\x08\x01".as_slice(),
        b"\x08\x02\x1a\x07private".as_slice(),
    ] {
        let response = Bytes::copy_from_slice(response);
        let server = TestServer::new(move |request| {
            let response = response.clone();
            async move {
                drain(request.into_inner()).await;
                Ok(replies(stream::iter([
                    Ok(response),
                    Err(Status::permission_denied("private terminal details")),
                ])))
            }
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let result = bounded(collect(
            engine
                .start(Box::new(Audio::new(&[])), SessionOptions::default())
                .unwrap(),
        ))
        .await;
        assert!(matches!(
            result,
            Err(EngineError::Transport(TransportError::Grpc(
                Code::PermissionDenied
            )))
        ));
        let _ = engine.close().await;
        let _ = transport.close().await;
        server.close().await;
    }
}

#[tokio::test]
async fn s3_error_retained_redacted_and_drained_even_after_end() {
    let drained = Arc::new(AtomicBool::new(false));
    let drained2 = drained.clone();
    let server = TestServer::new(move |request| {
        let drained = drained2.clone();
        async move {
            drain(request.into_inner()).await;
            Ok(replies(stream::unfold(0, move |state| {
                let drained = drained.clone();
                async move {
                    match state {
                        0 => Some((Ok(Bytes::from_static(b"\x08\x01")), 1)),
                        1 => Some((Ok(Bytes::from_static(b"\x08\x02\x1a\x07private")), 2)),
                        _ => {
                            drained.store(true, Ordering::SeqCst);
                            None
                        }
                    }
                }
            })))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let error = bounded(collect(
        engine
            .start(Box::new(Audio::new(&[])), SessionOptions::default())
            .unwrap(),
    ))
    .await
    .unwrap_err();
    assert!(drained.load(Ordering::SeqCst));
    assert!(!format!("{error:?} {error}").contains("private"));
    match error {
        EngineError::S3(response) => {
            assert!(response.raw.ends_with(b"private"));
        }
        _ => panic!("expected S3 error"),
    }
    let _ = engine.close().await;
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn nonzero_error_code_is_failure_without_error_status() {
    // error_code is field 2 in the protocol; END alone must not override it.
    let server = TestServer::new(|request| async move {
        drain(request.into_inner()).await;
        Ok(replies(stream::iter([Ok(Bytes::from_static(
            b"\x08\x01\x10\x07",
        ))])))
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    assert!(matches!(
        bounded(collect(
            engine
                .start(Box::new(Audio::new(&[])), SessionOptions::default())
                .unwrap()
        ))
        .await,
        Err(EngineError::S3(_))
    ));
    let _ = engine.close().await;
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn early_end_and_missing_end_never_emit_successful_eof() {
    for early in [true, false] {
        let server = TestServer::new(move |mut request| async move {
            if early {
                request.get_mut().message().await.unwrap();
            } else {
                drain(request.into_inner()).await;
            }
            Ok(replies(stream::iter([Ok(Bytes::from_static(if early {
                b"\x08\x01"
            } else {
                b"\x08\x00"
            }))])))
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let mut audio = Audio::new(&[b"audio"]);
        audio.block = early;
        let closes = audio.closes.clone();
        let mut session = engine
            .start(Box::new(audio), SessionOptions::default())
            .unwrap();
        let mut eof = false;
        let error = loop {
            match bounded(session.next()).await {
                Ok(Some(event)) => eof |= event.response.is_none(),
                Ok(None) => panic!("incomplete session succeeded"),
                Err(error) => break error,
            }
        };
        assert!(matches!(error, EngineError::IncompleteSession(_)));
        assert!(!eof);
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        let _ = engine.close().await;
        let _ = transport.close().await;
        server.close().await;
    }
}

#[tokio::test]
async fn cancel_during_slow_audio_close_keeps_cleanup_stable_and_never_calls_provider() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Arc::new(Engine::new(transport.clone()));
    let mut audio = Audio::new(&[]);
    let release = Arc::new(Semaphore::new(0));
    audio.close_release = Some(release.clone());
    let started = audio.close_started.clone();
    let closes = audio.closes.clone();
    let closed = audio.closed.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = calls.clone();
    let provider = Provider(move || {
        calls2.fetch_add(1, Ordering::SeqCst);
        async { Ok(None) }
    });
    let mut session = engine
        .start(
            Box::new(audio),
            SessionOptions {
                context: ContextInput::Provider(Arc::new(provider)),
                ..Default::default()
            },
        )
        .unwrap();
    bounded(started.acquire()).await.unwrap().forget();
    session.request_cancel();
    let engine2 = engine.clone();
    let closing = tokio::spawn(async move { engine2.close().await });
    tokio::task::yield_now().await;
    assert!(!closing.is_finished());
    assert!(!closed.load(Ordering::SeqCst));
    closing.abort();
    let _ = closing.await; // Cancelling close itself cannot lose the task.
    release.add_permits(1);
    bounded(engine.close()).await.unwrap();
    bounded(session.close()).await.unwrap();
    assert!(closed.load(Ordering::SeqCst));
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn normal_slow_cleanup_finishes_before_provider_and_end() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut audio = Audio::new(&[]);
    let release = Arc::new(Semaphore::new(0));
    audio.close_release = Some(release.clone());
    let started = audio.close_started.clone();
    let closed = audio.closed.clone();
    let closes = audio.closes.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = calls.clone();
    let provider = Provider(move || {
        assert!(closed.load(Ordering::SeqCst));
        calls2.fetch_add(1, Ordering::SeqCst);
        async { Ok(None) }
    });
    let session = engine
        .start(
            Box::new(audio),
            SessionOptions {
                context: ContextInput::Provider(Arc::new(provider)),
                ..Default::default()
            },
        )
        .unwrap();
    bounded(started.acquire()).await.unwrap().forget();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    release.add_permits(1);
    let events = bounded(collect(session)).await.unwrap();
    assert!(events.last().unwrap().response.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn context_errors_and_deadline_never_fall_back_to_none_or_end() {
    for mode in [0, 1] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let server = TestServer::new(move |request| {
            let seen = seen2.clone();
            async move {
                let mut request = request.into_inner();
                while let Ok(Some(payload)) = request.message().await {
                    seen.lock().unwrap().push(payload);
                }
                Ok(replies(stream::empty()))
            }
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let provider = Provider(move || async move {
            if mode == 0 {
                std::future::pending::<()>().await;
            }
            Err(SourceError::new(std::io::Error::other(
                "private provider failure",
            )))
        });
        let options = SessionOptions {
            context: ContextInput::Provider(Arc::new(provider)),
            context_timeout: Duration::from_millis(30),
            ..Default::default()
        };
        let error = bounded(collect(
            engine.start(Box::new(Audio::new(&[])), options).unwrap(),
        ))
        .await
        .unwrap_err();
        if mode == 0 {
            assert!(matches!(error, EngineError::ContextTimeout));
        } else {
            assert!(matches!(error, EngineError::Context(_)));
        }
        assert!(!format!("{error:?}").contains("private"));
        assert!(
            !seen
                .lock()
                .unwrap()
                .iter()
                .any(|value| value.as_ref() == end_request())
        );
        let _ = engine.close().await;
        let _ = transport.close().await;
        server.close().await;
    }
}

struct BlockedCredentials(Arc<AtomicUsize>);
impl MetadataProvider for BlockedCredentials {
    fn metadata(&self) -> MetadataFuture<'_> {
        Box::pin(async {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::future::pending().await
        })
    }
}
#[tokio::test]
async fn cancel_before_first_session_poll_and_blocked_credentials_closes_once() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let audio = Audio::new(&[]);
    let closes = audio.closes.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut session = engine
        .start(
            Box::new(audio),
            SessionOptions {
                credentials: Some(Arc::new(BlockedCredentials(calls))),
                ..Default::default()
            },
        )
        .unwrap();
    session.request_cancel();
    bounded(session.close()).await.unwrap();
    bounded(engine.close()).await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn engine_close_cancels_paused_consumer_without_touching_other_engine() {
    let server = TestServer::new(|mut request| async move {
        request.get_mut().message().await?;
        Ok(replies(stream::repeat_with(|| {
            Ok(Bytes::from_static(b"\x08\x00"))
        })))
    })
    .await;
    let transport = server.transport().await;
    let first = Engine::new(transport.clone());
    let second = Engine::new(transport.clone());
    let mut audio = Audio::new(&[b"audio"]);
    audio.block = true;
    let closes = audio.closes.clone();
    let mut session = first
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    assert!(bounded(session.next()).await.unwrap().is_some());
    bounded(first.close()).await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    let mut other = second
        .start(Box::new(Audio::new(&[])), SessionOptions::default())
        .unwrap();
    assert!(bounded(other.next()).await.unwrap().is_some());
    bounded(other.close()).await.unwrap();
    bounded(second.close()).await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn invalid_static_config_preflight_has_no_rpc_and_still_cleans_audio() {
    let rpc_calls = Arc::new(AtomicUsize::new(0));
    let rpc_calls2 = rpc_calls.clone();
    let server = TestServer::new(move |request| {
        rpc_calls2.fetch_add(1, Ordering::SeqCst);
        complete(request)
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    for mode in 0..4 {
        let audio = Audio::new(&[]);
        let closes = audio.closes.clone();
        let mut options = SessionOptions::default();
        match mode {
            0 => options.config.encoding = 1,
            1 => options.config.platform.clear(),
            2 => options.timeout = Duration::ZERO,
            _ => options.context_timeout = Duration::ZERO,
        }
        assert!(engine.start(Box::new(audio), options).is_err());
        bounded(engine.close()).await.unwrap();
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }
    assert_eq!(rpc_calls.load(Ordering::SeqCst), 0);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn original_audio_failure_is_explicitly_available_and_redacted() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut audio = Audio::new(&[]);
    audio.fail = true;
    let closes = audio.closes.clone();
    let error = bounded(collect(
        engine
            .start(Box::new(audio), SessionOptions::default())
            .unwrap(),
    ))
    .await
    .unwrap_err();
    assert!(!format!("{error:?}").contains("private"));
    match error {
        EngineError::Audio(original) => {
            assert!(original.original().to_string().contains("private"))
        }
        _ => panic!("expected audio error"),
    }
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    let _ = engine.close().await;
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn owned_engine_close_permanently_closes_its_transport_lifecycle() {
    let server = TestServer::new(complete).await;
    let engine = Engine::connect(TransportConfig::loopback(server.address).unwrap())
        .await
        .unwrap();
    bounded(collect(
        engine
            .start(Box::new(Audio::new(&[])), SessionOptions::default())
            .unwrap(),
    ))
    .await
    .unwrap();
    engine.close().await.unwrap();
    assert!(matches!(
        engine.start(Box::new(Audio::new(&[])), SessionOptions::default()),
        Err(EngineError::Closed)
    ));
    engine.close().await.unwrap();
    server.close().await;
}
