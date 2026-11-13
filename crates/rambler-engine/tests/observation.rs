mod common;
use bytes::Bytes;
use common::*;
use futures_util::{StreamExt, stream};
use rambler_engine::*;
use rambler_transport::TransportConfig;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;
use tonic::Status;

struct Audio {
    failure: bool,
    block: bool,
    started: Arc<Semaphore>,
    release: Option<Arc<Semaphore>>,
}
impl Audio {
    fn new(failure: bool) -> Self {
        Self {
            failure,
            block: false,
            started: Arc::new(Semaphore::new(0)),
            release: None,
        }
    }
}
impl AudioSource for Audio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if self.block {
                std::future::pending::<()>().await;
            }
            Ok(None)
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.started.add_permits(1);
            if let Some(release) = &self.release {
                release.acquire().await.unwrap().forget();
            }
            if self.failure {
                Err(SourceError::new(std::io::Error::other(
                    "private cleanup failure",
                )))
            } else {
                Ok(())
            }
        })
    }
}

struct NamedCleanupFailure {
    label: &'static str,
    closes: Arc<AtomicUsize>,
    started: Arc<Semaphore>,
}
impl AudioSource for NamedCleanupFailure {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async { Ok(None) })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.closes.fetch_add(1, Ordering::SeqCst);
            self.started.add_permits(1);
            Err(SourceError::new(std::io::Error::other(self.label)))
        })
    }
}

#[tokio::test]
async fn shutdown_delivers_each_unobserved_cleanup_failure_once_after_reaping() {
    for cancel_shutdown in [false, true] {
        let server = TestServer::new(complete).await;
        let transport = server.transport().await;
        let engine = Arc::new(Engine::new(transport.clone()));
        let mut counts = Vec::new();
        for label in ["cleanup A", "cleanup B"] {
            let closes = Arc::new(AtomicUsize::new(0));
            let started = Arc::new(Semaphore::new(0));
            let source = NamedCleanupFailure {
                label,
                closes: closes.clone(),
                started: started.clone(),
            };
            let mut invalid = SessionOptions::default();
            invalid.config.channels = 2;
            assert!(engine.start(Box::new(source), invalid).is_err());
            bounded(started.acquire()).await.unwrap().forget();
            counts.push(closes);
        }
        // Successful reuse forces reaping after both rejected sources have closed.
        for _ in 0..2 {
            bounded(collect_dictation(
                &engine,
                Box::new(Audio::new(false)),
                SessionOptions::default(),
                CollectorLimits::default(),
            ))
            .await
            .unwrap();
        }
        if cancel_shutdown {
            let mut slow = Audio::new(false);
            let release = Arc::new(Semaphore::new(0));
            slow.release = Some(release.clone());
            let started = slow.started.clone();
            let _session = engine
                .start(Box::new(slow), SessionOptions::default())
                .unwrap();
            bounded(started.acquire()).await.unwrap().forget();
            let other = engine.clone();
            let closing = tokio::spawn(async move { other.close().await });
            tokio::task::yield_now().await;
            assert!(!closing.is_finished());
            closing.abort();
            let _ = closing.await;
            release.add_permits(1);
        }
        for label in ["cleanup A", "cleanup B"] {
            match bounded(engine.close()).await.unwrap_err() {
                EngineError::AudioCleanup(error) => assert_eq!(error.original().to_string(), label),
                error => panic!("expected identified cleanup failure, got {error:?}"),
            }
            assert!(counts.iter().all(|count| count.load(Ordering::SeqCst) == 1));
        }
        bounded(engine.close()).await.unwrap();
        assert!(counts.iter().all(|count| count.load(Ordering::SeqCst) == 1));
        transport.close().await.unwrap();
        server.close().await;
    }
}

#[tokio::test]
async fn collector_limit_preserves_a_cleanup_error_it_does_not_deliver() {
    let server = TestServer::new(|mut request| async move {
        request.get_mut().message().await?;
        let final_state = Bytes::from_static(b"\xca\x8f\xe4\x04\x04\x0a\x02\x08\x01");
        Ok(replies(
            stream::once(async move { Ok(final_state) }).chain(stream::pending()),
        ))
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut source = Audio::new(true);
    source.block = true;
    let result = bounded(collect_dictation(
        &engine,
        Box::new(source),
        SessionOptions::default(),
        CollectorLimits {
            max_result_bytes: 1,
            ..Default::default()
        },
    ))
    .await;
    assert!(matches!(result, Err(EngineError::ResultLimit)));
    assert!(matches!(
        bounded(engine.close()).await,
        Err(EngineError::AudioCleanup(_))
    ));
    bounded(engine.close()).await.unwrap();
    let _ = transport.close().await;
    server.close().await;
}

#[tokio::test]
async fn observed_s3_and_grpc_failures_do_not_poison_shutdown_or_later_success() {
    for grpc in [false, true] {
        for later_success in [false, true] {
            for owned in [false, true] {
                let calls = Arc::new(AtomicUsize::new(0));
                let server = TestServer::new(move |request| {
                    let call = calls.fetch_add(1, Ordering::SeqCst);
                    async move {
                        drain(request.into_inner()).await;
                        let values = if call == 0 {
                            if grpc {
                                vec![
                                    Ok(Bytes::from_static(b"\x08\x01")),
                                    Err(Status::internal("private failure")),
                                ]
                            } else {
                                vec![
                                    Ok(Bytes::from_static(b"\x08\x02")),
                                    Ok(Bytes::from_static(b"\x08\x01")),
                                ]
                            }
                        } else {
                            vec![Ok(Bytes::from_static(b"\x08\x01"))]
                        };
                        Ok(replies(stream::iter(values)))
                    }
                })
                .await;
                let transport = server.transport().await;
                let engine = if owned {
                    Engine::connect(TransportConfig::loopback(server.address).unwrap())
                        .await
                        .unwrap()
                } else {
                    Engine::new(transport.clone())
                };
                let result = bounded(collect_dictation(
                    &engine,
                    Box::new(Audio::new(false)),
                    SessionOptions::default(),
                    CollectorLimits::default(),
                ))
                .await;
                assert!(result.is_err());
                if later_success {
                    bounded(collect_dictation(
                        &engine,
                        Box::new(Audio::new(false)),
                        SessionOptions::default(),
                        CollectorLimits::default(),
                    ))
                    .await
                    .unwrap();
                }
                bounded(engine.close()).await.unwrap();
                transport.close().await.unwrap();
                server.close().await;
            }
        }
    }
}

#[tokio::test]
async fn unobserved_background_cleanup_failure_survives_subsequent_session_reaping() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let bad = Audio::new(true);
    let started = bad.started.clone();
    let mut invalid = SessionOptions::default();
    invalid.config.channels = 2;
    assert!(engine.start(Box::new(bad), invalid).is_err());
    bounded(started.acquire()).await.unwrap().forget();
    // At least two starts exercise reaping after the background cleanup ended.
    for _ in 0..2 {
        bounded(collect_dictation(
            &engine,
            Box::new(Audio::new(false)),
            SessionOptions::default(),
            CollectorLimits::default(),
        ))
        .await
        .unwrap();
    }
    assert!(matches!(
        bounded(engine.close()).await,
        Err(EngineError::AudioCleanup(_))
    ));
    bounded(engine.close()).await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn cancelling_shutdown_does_not_acknowledge_undelivered_background_errors() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Arc::new(Engine::new(transport.clone()));
    let bad = Audio::new(true);
    let started = bad.started.clone();
    let mut invalid = SessionOptions::default();
    invalid.config.channels = 2;
    assert!(engine.start(Box::new(bad), invalid).is_err());
    bounded(started.acquire()).await.unwrap().forget();
    let mut slow = Audio::new(false);
    let release = Arc::new(Semaphore::new(0));
    slow.release = Some(release.clone());
    let slow_started = slow.started.clone();
    let _session = engine
        .start(Box::new(slow), SessionOptions::default())
        .unwrap();
    bounded(slow_started.acquire()).await.unwrap().forget();
    let other = engine.clone();
    let closing = tokio::spawn(async move { other.close().await });
    tokio::task::yield_now().await;
    assert!(!closing.is_finished());
    closing.abort();
    let _ = closing.await;
    release.add_permits(1);
    assert!(matches!(
        bounded(engine.close()).await,
        Err(EngineError::AudioCleanup(_))
    ));
    bounded(engine.close()).await.unwrap();
    let _ = transport.close().await;
    server.close().await;
}
