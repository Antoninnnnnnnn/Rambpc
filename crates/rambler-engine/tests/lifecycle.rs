mod common;
use bytes::Bytes;
use common::*;
use futures_util::stream;
use rambler_engine::*;
use rambler_protocol::MAX_MESSAGE_BYTES;
use rambler_transport::{Metadata, MetadataFuture, TransportError};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;

struct TrackedAudio {
    nexts: Arc<AtomicUsize>,
    closes: Arc<AtomicUsize>,
    started: Arc<Semaphore>,
    release: Option<Arc<Semaphore>>,
    block: bool,
    fail_close: bool,
}
impl TrackedAudio {
    fn new() -> Self {
        Self {
            nexts: Arc::new(AtomicUsize::new(0)),
            closes: Arc::new(AtomicUsize::new(0)),
            started: Arc::new(Semaphore::new(0)),
            release: None,
            block: false,
            fail_close: false,
        }
    }
}
impl AudioSource for TrackedAudio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            self.nexts.fetch_add(1, Ordering::SeqCst);
            if self.block {
                std::future::pending::<()>().await;
            }
            Ok(None)
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.closes.fetch_add(1, Ordering::SeqCst);
            self.started.add_permits(1);
            if let Some(release) = &self.release {
                release.acquire().await.unwrap().forget();
            }
            if self.fail_close {
                return Err(SourceError::new(std::io::Error::other(
                    "private close failure",
                )));
            }
            Ok(())
        })
    }
}
struct Credentials {
    calls: Arc<AtomicUsize>,
    entered: Arc<Semaphore>,
    fail: bool,
}
impl MetadataProvider for Credentials {
    fn metadata(&self) -> MetadataFuture<'_> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.add_permits(1);
            if self.fail {
                Err(TransportError::Credentials)
            } else {
                std::future::pending().await
            }
        })
    }
}

#[tokio::test]
async fn every_preflight_rejection_consumes_audio_but_closes_once_without_rpc() {
    let rpc_calls = Arc::new(AtomicUsize::new(0));
    let rpc_calls2 = rpc_calls.clone();
    let server = TestServer::new(move |request| {
        rpc_calls2.fetch_add(1, Ordering::SeqCst);
        complete(request)
    })
    .await;
    let transport = server.transport().await;
    for mode in 0..7 {
        let engine = Engine::new(transport.clone());
        let source = TrackedAudio::new();
        let closes = source.closes.clone();
        let nexts = source.nexts.clone();
        let mut options = SessionOptions::default();
        match mode {
            0 => options.timeout = Duration::ZERO,
            1 => options.context_timeout = Duration::ZERO,
            2 => options.config.sample_rate = 48000,
            3 => options.config.channels = 2,
            4 => options.config.language.clear(),
            5 => {
                options.context = ContextInput::Static(EditingContext {
                    before: "x".repeat(MAX_MESSAGE_BYTES + 1),
                    ..Default::default()
                })
            }
            _ => engine.close().await.unwrap(),
        }
        assert!(engine.start(Box::new(source), options).is_err());
        bounded(engine.close()).await.unwrap();
        assert_eq!(closes.load(Ordering::SeqCst), 1, "mode {mode}");
        assert_eq!(nexts.load(Ordering::SeqCst), 0, "mode {mode}");
    }
    assert_eq!(rpc_calls.load(Ordering::SeqCst), 0);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn unpolled_collector_drop_and_invalid_limits_still_cleanup_audio() {
    let rpc_calls = Arc::new(AtomicUsize::new(0));
    let rpc_calls2 = rpc_calls.clone();
    let server = TestServer::new(move |request| {
        rpc_calls2.fetch_add(1, Ordering::SeqCst);
        complete(request)
    })
    .await;
    let transport = server.transport().await;
    for valid in [true, false] {
        let engine = Engine::new(transport.clone());
        let source = TrackedAudio::new();
        let closes = source.closes.clone();
        let limits = if valid {
            CollectorLimits::default()
        } else {
            CollectorLimits {
                max_final_updates: 0,
                ..Default::default()
            }
        };
        let future =
            collect_dictation(&engine, Box::new(source), SessionOptions::default(), limits);
        drop(future); // absolutely no poll of the returned future
        bounded(engine.close()).await.unwrap();
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }
    assert_eq!(rpc_calls.load(Ordering::SeqCst), 0);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn accepted_credential_failure_or_block_cleans_source_exactly_once() {
    let rpc_calls = Arc::new(AtomicUsize::new(0));
    let rpc_calls2 = rpc_calls.clone();
    let server = TestServer::new(move |request| {
        rpc_calls2.fetch_add(1, Ordering::SeqCst);
        complete(request)
    })
    .await;
    let transport = server.transport().await;
    for fail in [false, true] {
        let engine = Engine::new(transport.clone());
        let mut source = TrackedAudio::new();
        source.block = true;
        let closes = source.closes.clone();
        let entered = Arc::new(Semaphore::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let options = SessionOptions {
            credentials: Some(Arc::new(Credentials {
                calls: calls.clone(),
                entered: entered.clone(),
                fail,
            })),
            ..Default::default()
        };
        let mut session = engine.start(Box::new(source), options).unwrap();
        bounded(entered.acquire()).await.unwrap().forget();
        if fail {
            assert!(matches!(
                bounded(session.next()).await,
                Err(EngineError::Transport(TransportError::Credentials))
            ));
        } else {
            bounded(engine.close()).await.unwrap();
        }
        let _ = session.close().await;
        let _ = engine.close().await;
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    assert_eq!(rpc_calls.load(Ordering::SeqCst), 0);
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn transport_start_rejection_does_not_drop_audio_without_async_cleanup() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    transport.close().await.unwrap();
    let engine = Engine::new(transport.clone());
    let source = TrackedAudio::new();
    let closes = source.closes.clone();
    let nexts = source.nexts.clone();
    let mut session = engine
        .start(Box::new(source), SessionOptions::default())
        .unwrap();
    assert!(matches!(
        bounded(session.next()).await,
        Err(EngineError::Transport(TransportError::Closed))
    ));
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(nexts.load(Ordering::SeqCst), 0);
    let _ = engine.close().await;
    server.close().await;
}

#[tokio::test]
async fn dropped_response_next_future_does_not_cancel_shared_cleanup_task() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut source = TrackedAudio::new();
    let release = Arc::new(Semaphore::new(0));
    source.release = Some(release.clone());
    let started = source.started.clone();
    let closes = source.closes.clone();
    let mut session = engine
        .start(Box::new(source), SessionOptions::default())
        .unwrap();
    bounded(started.acquire()).await.unwrap().forget();
    assert!(
        tokio::time::timeout(Duration::from_millis(10), session.next())
            .await
            .is_err()
    );
    release.add_permits(1);
    let mut eof = false;
    while let Some(event) = bounded(session.next()).await.unwrap() {
        eof |= event.response.is_none();
    }
    assert!(eof);
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn drop_signals_only_and_explicit_engine_close_awaits_slow_cleanup() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Arc::new(Engine::new(transport.clone()));
    let mut source = TrackedAudio::new();
    source.block = true;
    let release = Arc::new(Semaphore::new(0));
    source.release = Some(release.clone());
    let started = source.started.clone();
    let closes = source.closes.clone();
    let session = engine
        .start(Box::new(source), SessionOptions::default())
        .unwrap();
    drop(session);
    bounded(started.acquire()).await.unwrap().forget();
    let engine2 = engine.clone();
    let task = tokio::spawn(async move { engine2.close().await });
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    release.add_permits(1);
    bounded(task).await.unwrap().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn source_close_failure_is_not_treated_as_success_or_omitted_context() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut source = TrackedAudio::new();
    source.fail_close = true;
    let closes = source.closes.clone();
    let error = bounded(collect_dictation(
        &engine,
        Box::new(source),
        SessionOptions::default(),
        CollectorLimits::default(),
    ))
    .await
    .unwrap_err();
    assert!(matches!(error, EngineError::AudioCleanup(_)));
    assert!(!format!("{error:?}").contains("private"));
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    let _ = engine.close().await;
    let _ = transport.close().await;
    server.close().await;
}

struct SnapshotProvider {
    value: String,
    calls: Arc<AtomicUsize>,
    gate: Option<Arc<Semaphore>>,
}
impl ContextProvider for SnapshotProvider {
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.gate {
                gate.acquire().await.unwrap().forget();
            }
            Ok(Some(EditingContext {
                before: self.value.clone(),
                ..Default::default()
            }))
        })
    }
}
#[tokio::test]
async fn concurrent_and_reused_engine_sessions_have_isolated_context_ids_and_metrics() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let slow_gate = Arc::new(Semaphore::new(0));
    let calls1 = Arc::new(AtomicUsize::new(0));
    let calls2 = Arc::new(AtomicUsize::new(0));
    let options1 = SessionOptions {
        context: ContextInput::Provider(Arc::new(SnapshotProvider {
            value: "first".into(),
            calls: calls1.clone(),
            gate: Some(slow_gate.clone()),
        })),
        ..Default::default()
    };
    let options2 = SessionOptions {
        context: ContextInput::Provider(Arc::new(SnapshotProvider {
            value: "second".into(),
            calls: calls2.clone(),
            gate: None,
        })),
        ..Default::default()
    };
    let first = collect_dictation(
        &engine,
        Box::new(TrackedAudio::new()),
        options1,
        CollectorLimits::default(),
    );
    let second = bounded(collect_dictation(
        &engine,
        Box::new(TrackedAudio::new()),
        options2.clone(),
        CollectorLimits::default(),
    ))
    .await
    .unwrap();
    assert_eq!(second.context.unwrap().before, "second");
    slow_gate.add_permits(1);
    let first = bounded(first).await.unwrap();
    assert_eq!(first.context.unwrap().before, "first");
    let third = bounded(collect_dictation(
        &engine,
        Box::new(TrackedAudio::new()),
        options2,
        CollectorLimits::default(),
    ))
    .await
    .unwrap();
    assert_eq!(calls1.load(Ordering::SeqCst), 1);
    assert_eq!(calls2.load(Ordering::SeqCst), 2);
    let ids = [
        first.metrics.unwrap().session_id,
        second.metrics.unwrap().session_id,
        third.metrics.unwrap().session_id,
    ];
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn malformed_response_closes_a_blocked_source_once() {
    let server = TestServer::new(|mut request| async move {
        request.get_mut().message().await?;
        Ok(replies(stream::iter([Ok(Bytes::from_static(b"\x08"))])))
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut source = TrackedAudio::new();
    source.block = true;
    let closes = source.closes.clone();
    let error = bounded(collect_dictation(
        &engine,
        Box::new(source),
        SessionOptions::default(),
        CollectorLimits::default(),
    ))
    .await
    .unwrap_err();
    assert!(matches!(error, EngineError::Protocol(_)));
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    let _ = engine.close().await;
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn provider_deadline_uses_remaining_global_budget_not_its_own_long_timeout() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let options = SessionOptions {
        context: ContextInput::Provider(Arc::new(SnapshotProvider {
            value: "never".into(),
            calls: calls.clone(),
            gate: Some(Arc::new(Semaphore::new(0))),
        })),
        timeout: Duration::from_millis(30),
        context_timeout: Duration::from_secs(20),
        ..Default::default()
    };
    let error = bounded(collect_dictation(
        &engine,
        Box::new(TrackedAudio::new()),
        options,
        CollectorLimits::default(),
    ))
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        EngineError::ContextTimeout | EngineError::Transport(TransportError::Timeout)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let _ = engine.close().await;
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn cancellation_does_not_silently_hide_an_audio_cleanup_failure() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut source = TrackedAudio::new();
    source.block = true;
    source.fail_close = true;
    let closes = source.closes.clone();
    let mut session = engine
        .start(Box::new(source), SessionOptions::default())
        .unwrap();
    session.request_cancel();
    assert!(matches!(
        bounded(session.close()).await,
        Err(EngineError::AudioCleanup(_))
    ));
    // Session close delivered the cleanup error; engine shutdown must not replay it.
    bounded(engine.close()).await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn dropping_engine_signals_sessions_but_does_not_close_injected_channel() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut source = TrackedAudio::new();
    source.block = true;
    let release = Arc::new(Semaphore::new(0));
    source.release = Some(release.clone());
    let started = source.started.clone();
    let closes = source.closes.clone();
    let mut session = engine
        .start(Box::new(source), SessionOptions::default())
        .unwrap();
    drop(engine);
    bounded(started.acquire()).await.unwrap().forget();
    release.add_permits(1);
    bounded(session.close()).await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    let other = Engine::new(transport.clone());
    bounded(collect_dictation(
        &other,
        Box::new(TrackedAudio::new()),
        SessionOptions::default(),
        CollectorLimits::default(),
    ))
    .await
    .unwrap();
    other.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn global_deadline_keeps_running_while_event_consumer_is_paused() {
    let server = TestServer::new(|mut request| async move {
        request.get_mut().message().await?;
        Ok(replies(stream::repeat_with(|| {
            Ok(Bytes::from_static(b"\x08\x00"))
        })))
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut source = TrackedAudio::new();
    source.block = true;
    let closed = source.started.clone();
    let closes = source.closes.clone();
    let options = SessionOptions {
        timeout: Duration::from_millis(100),
        ..Default::default()
    };
    let mut session = engine.start(Box::new(source), options).unwrap();
    assert!(bounded(session.next()).await.unwrap().is_some());
    // No further event polls. Independent supervision still enforces the budget.
    bounded(closed.acquire()).await.unwrap().forget();
    assert!(matches!(
        bounded(engine.close()).await,
        Err(EngineError::Transport(TransportError::Timeout))
    ));
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    let _ = session.close().await;
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn empty_metadata_provider_is_not_special_cased_as_environment_access() {
    // Only explicitly injected providers are consulted. This never reads .env or
    // process credentials; the transport uses this validated empty snapshot.
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let options = SessionOptions {
        credentials: Some(Arc::new(Metadata::default())),
        ..Default::default()
    };
    bounded(collect_dictation(
        &engine,
        Box::new(TrackedAudio::new()),
        options,
        CollectorLimits::default(),
    ))
    .await
    .unwrap();
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}
