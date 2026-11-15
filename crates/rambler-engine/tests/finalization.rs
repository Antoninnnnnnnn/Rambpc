mod common;

use bytes::Bytes;
use common::*;
use futures_util::{FutureExt, StreamExt, stream};
use rambler_engine::*;
use rambler_protocol::end_request;
use rambler_transport::TransportError;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;

const FINAL: Duration = Duration::from_secs(1);
const HARD: Duration = Duration::from_secs(8);

#[derive(Clone)]
struct Gate(Arc<Semaphore>);
impl Gate {
    fn new() -> Self {
        Self(Arc::new(Semaphore::new(0)))
    }
    fn open(&self) {
        self.0.add_permits(1);
    }
    async fn wait(&self) {
        self.0.acquire().await.unwrap().forget();
    }
}

struct Audio {
    next_entered: Gate,
    next_release: Option<Gate>,
    close_entered: Gate,
    close_release: Option<Gate>,
    closes: Arc<AtomicUsize>,
    source_error: Option<SourceError>,
    cleanup_error: Option<SourceError>,
}
impl Audio {
    fn new() -> Self {
        Self {
            next_entered: Gate::new(),
            next_release: None,
            close_entered: Gate::new(),
            close_release: None,
            closes: Arc::new(AtomicUsize::new(0)),
            source_error: None,
            cleanup_error: None,
        }
    }
}
impl AudioSource for Audio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            self.next_entered.open();
            if let Some(gate) = &self.next_release {
                gate.wait().await;
            }
            match &self.source_error {
                Some(error) => Err(error.clone()),
                None => Ok(None),
            }
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.closes.fetch_add(1, Ordering::SeqCst);
            self.close_entered.open();
            if let Some(gate) = &self.close_release {
                gate.wait().await;
            }
            self.cleanup_error.clone().map_or(Ok(()), Err)
        })
    }
}
struct Context {
    entered: Gate,
    calls: Arc<AtomicUsize>,
}
impl ContextProvider for Context {
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.open();
            std::future::pending().await
        })
    }
}
fn pending_context() -> (ContextInput, Gate, Arc<AtomicUsize>) {
    let entered = Gate::new();
    let calls = Arc::new(AtomicUsize::new(0));
    (
        ContextInput::Provider(Arc::new(Context {
            entered: entered.clone(),
            calls: calls.clone(),
        })),
        entered,
        calls,
    )
}
fn options() -> SessionOptions {
    SessionOptions {
        timeout: HARD,
        finalize_timeout: Some(FINAL),
        context_timeout: Duration::from_secs(6),
        ..Default::default()
    }
}
async fn terminal(session: &mut EngineSession) -> Result<(), EngineError> {
    while let Some(event) = session.next().await? {
        assert!(
            event.response.is_some(),
            "timed out session emitted successful EOF"
        );
    }
    Ok(())
}
async fn complete_tolerating_cancel(
    request: tonic::Request<tonic::Streaming<Bytes>>,
) -> Result<tonic::Response<Replies>, tonic::Status> {
    let mut request = request.into_inner();
    while let Ok(Some(_)) = request.message().await {}
    Ok(replies(stream::iter([Ok(Bytes::from_static(b"\x08\x01"))])))
}
async fn scenario(future: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(Duration::from_secs(15), future)
        .await
        .expect("whole finalization scenario deadline");
}

#[tokio::test]
async fn pending_late_context_finalizes_without_request_end() {
    scenario(async {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let upload_done = Gate::new();
        let server = TestServer::new({
            let seen = seen.clone();
            let done = upload_done.clone();
            move |request| {
                let seen = seen.clone();
                let done = done.clone();
                async move {
                    let mut request = request.into_inner();
                    while let Ok(Some(payload)) = request.message().await {
                        seen.lock().unwrap().push(payload);
                    }
                    done.open();
                    Ok(replies(stream::empty()))
                }
            }
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let audio = Audio::new();
        let closes = audio.closes.clone();
        let (context, entered, calls) = pending_context();
        let mut session = engine
            .start(
                Box::new(audio),
                SessionOptions {
                    context,
                    ..options()
                },
            )
            .unwrap();
        entered.wait().await;
        assert!(matches!(
            terminal(&mut session).await,
            Err(EngineError::FinalizeTimeout)
        ));
        upload_done.wait().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert!(
            !seen
                .lock()
                .unwrap()
                .iter()
                .any(|payload| payload.as_ref() == end_request())
        );
        assert!(session.audio_progress().cleanup_succeeded);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn s3_end_without_grpc_status_is_finalize_timeout_not_success() {
    scenario(async {
        let server = TestServer::new(|request| async move {
            drain(request.into_inner()).await;
            Ok(replies(
                stream::once(async { Ok(Bytes::from_static(b"\x08\x01")) })
                    .chain(stream::pending()),
            ))
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let mut session = engine.start(Box::new(Audio::new()), options()).unwrap();
        let event = session.next().await.unwrap().unwrap();
        assert_eq!(event.response.unwrap().status, Some(1));
        assert!(matches!(
            terminal(&mut session).await,
            Err(EngineError::FinalizeTimeout)
        ));
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn hard_cap_wins_when_finalization_is_capped_including_huge_budget() {
    scenario(async {
        let server = TestServer::new(complete_tolerating_cancel).await;
        let transport = server.transport().await;
        for budget in [Duration::from_secs(5), Duration::MAX] {
            let engine = Engine::new(transport.clone());
            let (context, entered, _) = pending_context();
            let mut session = engine
                .start(
                    Box::new(Audio::new()),
                    SessionOptions {
                        timeout: FINAL,
                        finalize_timeout: Some(budget),
                        context,
                        ..options()
                    },
                )
                .unwrap();
            entered.wait().await;
            assert!(matches!(
                terminal(&mut session).await,
                Err(EngineError::Transport(TransportError::Timeout))
            ));
            engine.close().await.unwrap();
        }
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn cancellation_before_deadlines_stays_cancelled() {
    scenario(async {
        let server = TestServer::new(complete_tolerating_cancel).await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let (context, entered, _) = pending_context();
        let audio = Audio::new();
        let closes = audio.closes.clone();
        let mut session = engine
            .start(
                Box::new(audio),
                SessionOptions {
                    context,
                    ..options()
                },
            )
            .unwrap();
        entered.wait().await;
        session.request_cancel();
        assert!(matches!(
            terminal(&mut session).await,
            Err(EngineError::Cancelled)
        ));
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn audio_and_async_cleanup_can_each_exceed_finalize_budget() {
    scenario(async {
        let server = TestServer::new(complete_tolerating_cancel).await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let mut audio = Audio::new();
        let next_entered = audio.next_entered.clone();
        let close_entered = audio.close_entered.clone();
        let next_release = Gate::new();
        let close_release = Gate::new();
        audio.next_release = Some(next_release.clone());
        audio.close_release = Some(close_release.clone());
        let closes = audio.closes.clone();
        let (context, entered, calls) = pending_context();
        let mut session = engine
            .start(
                Box::new(audio),
                SessionOptions {
                    context,
                    finalize_timeout: Some(Duration::from_millis(250)),
                    ..options()
                },
            )
            .unwrap();
        next_entered.wait().await;
        // Intentional timer behavior, not synchronization with socket I/O.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(session.next().now_or_never().is_none());
        assert!(!session.audio_progress().eof);
        next_release.open();
        close_entered.wait().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(session.next().now_or_never().is_none());
        assert!(session.audio_progress().eof);
        assert!(!session.audio_progress().cleanup_finished);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        close_release.open();
        entered.wait().await;
        assert!(matches!(
            terminal(&mut session).await,
            Err(EngineError::FinalizeTimeout)
        ));
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn shorter_context_budget_and_none_keep_context_timeout() {
    scenario(async {
        let defaults = SessionOptions::default();
        assert_eq!(defaults.finalize_timeout, None);
        assert_eq!(defaults.timeout, Duration::from_secs(30));
        assert_eq!(defaults.context_timeout, Duration::from_secs(2));
        let server = TestServer::new(complete_tolerating_cancel).await;
        let transport = server.transport().await;
        for finalize_timeout in [Some(Duration::from_secs(4)), None] {
            let engine = Engine::new(transport.clone());
            let (context, entered, _) = pending_context();
            let mut session = engine
                .start(
                    Box::new(Audio::new()),
                    SessionOptions {
                        context,
                        finalize_timeout,
                        context_timeout: Duration::from_millis(250),
                        ..options()
                    },
                )
                .unwrap();
            entered.wait().await;
            assert!(matches!(
                terminal(&mut session).await,
                Err(EngineError::ContextTimeout)
            ));
            engine.close().await.unwrap();
        }
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn none_allows_gated_final_status_and_successful_eof() {
    scenario(async {
        let uploaded = Gate::new();
        let release = Gate::new();
        let server = TestServer::new({
            let uploaded = uploaded.clone();
            let release = release.clone();
            move |request| {
                let uploaded = uploaded.clone();
                let release = release.clone();
                async move {
                    drain(request.into_inner()).await;
                    uploaded.open();
                    release.wait().await;
                    Ok(replies(stream::iter([Ok(Bytes::from_static(b"\x08\x01"))])))
                }
            }
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let mut session = engine
            .start(
                Box::new(Audio::new()),
                SessionOptions {
                    finalize_timeout: None,
                    ..options()
                },
            )
            .unwrap();
        uploaded.wait().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(session.next().now_or_never().is_none());
        release.open();
        assert_eq!(
            session
                .next()
                .await
                .unwrap()
                .unwrap()
                .response
                .unwrap()
                .status,
            Some(1)
        );
        assert!(session.next().await.unwrap().unwrap().response.is_none());
        assert!(session.next().await.unwrap().is_none());
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn cancellation_after_observed_eof_cannot_revoke_success() {
    // Integration coverage, not a deterministic RED of the old publication race:
    // the old supervisor can also finish before this single-thread consumer runs.
    scenario(async {
        let server = TestServer::new(complete_tolerating_cancel).await;
        let transport = server.transport().await;
        for finalize_timeout in [None, Some(FINAL)] {
            let engine = Engine::new(transport.clone());
            let audio = Audio::new();
            let closes = audio.closes.clone();
            let mut session = engine
                .start(
                    Box::new(audio),
                    SessionOptions {
                        finalize_timeout,
                        ..options()
                    },
                )
                .unwrap();
            let mut eof_count = 0;
            while let Some(event) = session.next().await.unwrap() {
                if event.response.is_none() {
                    eof_count += 1;
                    assert!(event.metrics.audio.cleanup_succeeded);
                    assert!(event.metrics.eof_seconds.is_some());
                    session.request_cancel();
                    break;
                }
            }
            assert_eq!(eof_count, 1);
            assert!(session.next().await.unwrap().is_none());
            session.close().await.unwrap();
            assert!(session.next().await.unwrap().is_none());
            assert_eq!(closes.load(Ordering::SeqCst), 1);
            engine.close().await.unwrap();
        }
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn selected_failure_is_not_replaced_when_cleanup_crosses_hard_deadline() {
    scenario(async {
        for cancelled in [true, false] {
            let server = TestServer::new(move |request| async move {
                // Retain upload and withhold status: only the engine's selected
                // cancellation/protobuf failure can initiate cleanup before HARD.
                Ok(replies(stream::unfold(
                    request.into_inner(),
                    move |request| async move {
                        if cancelled {
                            std::future::pending::<()>().await;
                        }
                        Some((Ok(Bytes::from_static(b"\xff")), request))
                    },
                )))
            })
            .await;
            let transport = server.transport().await;
            let engine = Engine::new(transport.clone());
            let mut audio = Audio::new();
            let next_entered = audio.next_entered.clone();
            let close_entered = audio.close_entered.clone();
            let closes = audio.closes.clone();
            let release = Gate::new();
            audio.next_release = Some(Gate::new());
            audio.close_release = Some(release.clone());
            let cleanup = SourceError::new(std::io::Error::other("private cleanup"));
            if !cancelled {
                audio.cleanup_error = Some(cleanup.clone());
            }
            let mut session = engine
                .start(
                    Box::new(audio),
                    SessionOptions {
                        timeout: Duration::from_millis(500),
                        ..options()
                    },
                )
                .unwrap();
            if cancelled {
                next_entered.wait().await;
                session.request_cancel();
            }
            close_entered.wait().await;
            tokio::time::sleep(Duration::from_secs(1)).await;
            assert!(session.next().now_or_never().is_none());
            release.open();
            let primary = terminal(&mut session).await.unwrap_err();
            if cancelled {
                assert!(matches!(primary, EngineError::Cancelled));
                engine.close().await.unwrap();
            } else {
                assert!(matches!(primary, EngineError::Protocol(_)));
                match engine.close().await.unwrap_err() {
                    EngineError::AudioCleanup(actual) => {
                        assert!(std::ptr::eq(actual.original(), cleanup.original()));
                    }
                    other => panic!("secondary cleanup was lost: {other:?}"),
                }
                engine.close().await.unwrap();
            }
            assert_eq!(closes.load(Ordering::SeqCst), 1);
            transport.close().await.unwrap();
            server.close().await;
        }
    })
    .await;
}

#[tokio::test]
async fn hard_timeout_awaits_cleanup_and_preserves_secondary_error_once() {
    scenario(async {
        let server = TestServer::new(complete_tolerating_cancel).await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let mut audio = Audio::new();
        let entered = audio.close_entered.clone();
        let closes = audio.closes.clone();
        let release = Gate::new();
        audio.close_release = Some(release.clone());
        audio.cleanup_error = Some(SourceError::new(std::io::Error::other("private cleanup")));
        let (context, _, calls) = pending_context();
        let mut session = engine
            .start(
                Box::new(audio),
                SessionOptions {
                    context,
                    timeout: Duration::from_millis(500),
                    finalize_timeout: Some(Duration::from_millis(100)),
                    ..options()
                },
            )
            .unwrap();
        entered.wait().await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(
            session.next().now_or_never().is_none(),
            "deadline abandoned asynchronous cleanup"
        );
        release.open();
        assert!(matches!(
            terminal(&mut session).await,
            Err(EngineError::Transport(TransportError::Timeout))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert!(matches!(
            engine.close().await,
            Err(EngineError::AudioCleanup(_))
        ));
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn source_and_cleanup_identity_deduplication_survives_optional_budget() {
    scenario(async {
        let server = TestServer::new(complete_tolerating_cancel).await;
        let transport = server.transport().await;
        for shared in [true, false] {
            let engine = Engine::new(transport.clone());
            let error = SourceError::new(std::io::Error::other("private source"));
            let mut audio = Audio::new();
            let closes = audio.closes.clone();
            audio.source_error = Some(error.clone());
            audio.cleanup_error = Some(if shared {
                error.clone()
            } else {
                SourceError::new(std::io::Error::other("private cleanup"))
            });
            let mut session = engine.start(Box::new(audio), options()).unwrap();
            let primary = terminal(&mut session).await.unwrap_err();
            match primary {
                EngineError::Audio(actual) => {
                    assert!(std::ptr::eq(actual.original(), error.original()))
                }
                _ => panic!("source error was replaced: {primary:?}"),
            }
            assert_eq!(closes.load(Ordering::SeqCst), 1);
            let closing = engine.close().await;
            if shared {
                closing.unwrap();
            } else {
                assert!(matches!(closing, Err(EngineError::AudioCleanup(_))));
            }
            engine.close().await.unwrap();
        }
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn zero_finalize_budget_rejected_and_audio_still_cleaned_once() {
    scenario(async {
        let admissions = Arc::new(AtomicUsize::new(0));
        let server = TestServer::new({
            let admissions = admissions.clone();
            move |request| {
                admissions.fetch_add(1, Ordering::SeqCst);
                complete_tolerating_cancel(request)
            }
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let audio = Audio::new();
        let closes = audio.closes.clone();
        assert!(matches!(
            engine.start(
                Box::new(audio),
                SessionOptions {
                    finalize_timeout: Some(Duration::ZERO),
                    ..options()
                }
            ),
            Err(EngineError::InvalidConfig)
        ));
        engine.close().await.unwrap();
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(admissions.load(Ordering::SeqCst), 0);
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn early_s3_end_does_not_start_finalization_before_audio_eof() {
    scenario(async {
        let server = TestServer::new(|request| async move {
            Ok(replies(stream::unfold(
                (false, request.into_inner()),
                |(sent, mut request)| async move {
                    // Emit S3 END before the producer has finished. Keep final gRPC
                    // status withheld while retaining and draining the upload.
                    if sent {
                        while let Ok(Some(_)) = request.message().await {}
                        std::future::pending::<()>().await;
                    }
                    Some((Ok(Bytes::from_static(b"\x08\x01")), (true, request)))
                },
            )))
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let mut audio = Audio::new();
        let entered = audio.next_entered.clone();
        let release = Gate::new();
        audio.next_release = Some(release.clone());
        let (context, context_entered, _) = pending_context();
        let mut session = engine
            .start(
                Box::new(audio),
                SessionOptions {
                    context,
                    finalize_timeout: Some(Duration::from_millis(250)),
                    ..options()
                },
            )
            .unwrap();
        entered.wait().await;
        assert_eq!(
            session
                .next()
                .await
                .unwrap()
                .unwrap()
                .response
                .unwrap()
                .status,
            Some(1)
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(session.next().now_or_never().is_none());
        release.open();
        context_entered.wait().await;
        assert!(matches!(
            terminal(&mut session).await,
            Err(EngineError::FinalizeTimeout)
        ));
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn unsuccessful_cleanup_never_enters_finalization_or_context() {
    scenario(async {
        let server = TestServer::new(complete_tolerating_cancel).await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let mut audio = Audio::new();
        let entered = audio.close_entered.clone();
        let closes = audio.closes.clone();
        let release = Gate::new();
        audio.close_release = Some(release.clone());
        let error = SourceError::new(std::io::Error::other("private cleanup"));
        audio.cleanup_error = Some(error.clone());
        let (context, _, calls) = pending_context();
        let mut session = engine
            .start(
                Box::new(audio),
                SessionOptions {
                    context,
                    finalize_timeout: Some(Duration::from_millis(250)),
                    ..options()
                },
            )
            .unwrap();
        entered.wait().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(session.next().now_or_never().is_none());
        release.open();
        let primary = terminal(&mut session).await.unwrap_err();
        match primary {
            EngineError::AudioCleanup(actual) => {
                assert!(std::ptr::eq(actual.original(), error.original()))
            }
            _ => panic!("cleanup error was replaced: {primary:?}"),
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert!(session.audio_progress().eof);
        assert!(!session.audio_progress().cleanup_succeeded);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}
