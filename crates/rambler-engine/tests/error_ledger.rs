//! Offline regression tests for independently observable engine failures.
mod common;
use bytes::Bytes;
use common::*;
use rambler_engine::*;
use rambler_transport::{TransportConfig, TransportError};
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, oneshot},
};

#[derive(Debug, thiserror::Error)]
#[error("private source detail {0}")]
struct PrivateFailure(u32);
fn failure(id: u32) -> SourceError {
    SourceError::new(PrivateFailure(id))
}
fn assert_private(error: EngineError, id: u32, cleanup: bool) {
    assert!(!format!("{error} {error:?}").contains("private source detail"));
    assert!(error.source().is_none(), "no automatic private error chain");
    let source = match error {
        EngineError::AudioCleanup(source) if cleanup => source,
        EngineError::Audio(source) if !cleanup => source,
        other => panic!("unexpected public error: {other:?}"),
    };
    assert!(!format!("{source} {source:?}").contains("private source detail"));
    assert!(source.source().is_none());
    assert_eq!(
        source
            .original()
            .downcast_ref::<PrivateFailure>()
            .unwrap()
            .0,
        id
    );
}

struct Audio {
    next_error: Option<SourceError>,
    cleanup_error: Option<SourceError>,
    eof: bool,
    cleanup_started: Arc<Semaphore>,
    release: Option<Arc<Semaphore>>,
    closes: Arc<AtomicUsize>,
}
impl Audio {
    fn new(next_error: Option<SourceError>, cleanup_error: Option<SourceError>) -> Self {
        Self {
            next_error,
            cleanup_error,
            eof: false,
            cleanup_started: Arc::new(Semaphore::new(0)),
            release: None,
            closes: Arc::new(AtomicUsize::new(0)),
        }
    }
}
impl AudioSource for Audio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if let Some(error) = self.next_error.take() {
                Err(error)
            } else if self.eof {
                Ok(None)
            } else {
                std::future::pending().await
            }
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.closes.fetch_add(1, Ordering::SeqCst);
            self.cleanup_started.add_permits(1);
            if let Some(release) = &self.release {
                release.acquire().await.unwrap().forget();
            }
            self.cleanup_error.take().map_or(Ok(()), Err)
        })
    }
}
async fn quiet_server() -> TestServer {
    TestServer::new(|mut request| async move {
        while let Ok(Some(_)) = request.get_mut().message().await {}
        std::future::pending().await
    })
    .await
}

#[tokio::test]
async fn next_failure_keeps_primary_and_reports_independent_cleanup_once() {
    let server = quiet_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let audio = Audio::new(Some(failure(1)), Some(failure(2)));
    let closes = audio.closes.clone();
    let mut session = engine
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    assert_private(bounded(session.next()).await.unwrap_err(), 1, false);
    assert!(session.audio_progress().cleanup_finished);
    assert!(!session.audio_progress().cleanup_succeeded);
    assert_private(bounded(engine.close()).await.unwrap_err(), 2, true);
    bounded(engine.close()).await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn same_source_failure_identity_is_not_reported_as_independent_cleanup() {
    let server = quiet_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let shared = failure(4);
    let audio = Audio::new(Some(shared.clone()), Some(shared));
    let closes = audio.closes.clone();
    let mut session = engine
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    assert_private(bounded(session.next()).await.unwrap_err(), 4, false);
    engine.close().await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn identical_source_error_details_do_not_deduplicate_independent_objects() {
    let server = quiet_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let primary = failure(8);
    let secondary = failure(8);
    let mut session = engine
        .start(
            Box::new(Audio::new(Some(primary.clone()), Some(secondary.clone()))),
            SessionOptions::default(),
        )
        .unwrap();
    match bounded(session.next()).await.unwrap_err() {
        EngineError::Audio(source) => assert!(std::ptr::eq(source.original(), primary.original())),
        other => panic!("unexpected primary error: {other:?}"),
    }
    match engine.close().await.unwrap_err() {
        EngineError::AudioCleanup(source) => {
            assert!(std::ptr::eq(source.original(), secondary.original()))
        }
        other => panic!("unexpected cleanup error: {other:?}"),
    }
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn memoized_cleanup_already_primary_is_not_reported_twice() {
    let server = quiet_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut audio = Audio::new(None, Some(failure(5)));
    audio.eof = true;
    let closes = audio.closes.clone();
    let mut session = engine
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    assert_private(bounded(session.next()).await.unwrap_err(), 5, true);
    assert_private(session.close().await.unwrap_err(), 5, true);
    assert_private(session.close().await.unwrap_err(), 5, true);
    engine.close().await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn interrupted_next_keeps_both_failures_and_does_not_cancel_cleanup() {
    let server = quiet_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut audio = Audio::new(Some(failure(6)), Some(failure(7)));
    let release = Arc::new(Semaphore::new(0));
    audio.release = Some(release.clone());
    let started = audio.cleanup_started.clone();
    let closes = audio.closes.clone();
    let mut session = engine
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    bounded(started.acquire()).await.unwrap().forget();
    assert!(
        tokio::time::timeout(Duration::from_millis(10), session.next())
            .await
            .is_err()
    );
    assert!(!session.audio_progress().cleanup_finished);
    release.add_permits(1);
    assert_private(bounded(session.next()).await.unwrap_err(), 6, false);
    assert_private(bounded(engine.close()).await.unwrap_err(), 7, true);
    engine.close().await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn interrupted_cancellation_closes_wait_for_cleanup_without_replay() {
    let server = quiet_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut audio = Audio::new(None, Some(failure(9)));
    let release = Arc::new(Semaphore::new(0));
    audio.release = Some(release.clone());
    let started = audio.cleanup_started.clone();
    let closes = audio.closes.clone();
    let mut session = engine
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    session.request_cancel();
    bounded(started.acquire()).await.unwrap().forget();
    assert!(
        tokio::time::timeout(Duration::from_millis(10), session.close())
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(10), engine.close())
            .await
            .is_err()
    );
    assert!(!session.audio_progress().cleanup_finished);
    release.add_permits(1);
    assert_private(bounded(session.close()).await.unwrap_err(), 9, true);
    assert_private(session.close().await.unwrap_err(), 9, true);
    engine.close().await.unwrap();
    assert!(session.audio_progress().cleanup_finished);
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn secondary_survives_observed_primary_reaping_and_concurrent_closes() {
    let server = quiet_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    for id in [10, 20] {
        let mut session = engine
            .start(
                Box::new(Audio::new(Some(failure(id)), Some(failure(id + 1)))),
                SessionOptions::default(),
            )
            .unwrap();
        assert_private(bounded(session.next()).await.unwrap_err(), id, false);
        // Explicit repeated close observes only the unchanged primary error.
        assert_private(session.close().await.unwrap_err(), id, false);
        assert_private(session.close().await.unwrap_err(), id, false);
    }
    // A rejected start still runs the same reaper and must not remove secondaries.
    assert!(
        engine
            .start(
                Box::new(Audio::new(None, None)),
                SessionOptions {
                    timeout: Duration::ZERO,
                    ..Default::default()
                }
            )
            .is_err()
    );
    let results =
        bounded(async { tokio::join!(engine.close(), engine.close(), engine.close()) }).await;
    let mut ids = Vec::new();
    let mut successes = 0;
    for result in [results.0, results.1, results.2] {
        match result {
            Err(EngineError::AudioCleanup(source)) => {
                ids.push(
                    source
                        .original()
                        .downcast_ref::<PrivateFailure>()
                        .unwrap()
                        .0,
                );
            }
            Ok(()) => successes += 1,
            other => panic!("unexpected close result: {other:?}"),
        }
    }
    ids.sort();
    assert_eq!(ids, [11, 21]);
    assert_eq!(successes, 1);
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_start_reaping_during_terminal_publication_keeps_all_secondaries() {
    let server = quiet_server().await;
    let transport = server.transport().await;
    transport.close().await.unwrap();
    let engine = Arc::new(Engine::new(transport.clone()));
    let reaping = engine.clone();
    let reaper = tokio::spawn(async move {
        for _ in 0..256 {
            assert!(
                reaping
                    .start(
                        Box::new(Audio::new(None, None)),
                        SessionOptions {
                            timeout: Duration::ZERO,
                            ..Default::default()
                        }
                    )
                    .is_err()
            );
            tokio::task::yield_now().await;
        }
    });
    let mut observers = Vec::new();
    for id in 100..164 {
        let engine = engine.clone();
        observers.push(tokio::spawn(async move {
            let mut session = engine
                .start(
                    Box::new(Audio::new(None, Some(failure(id + 100)))),
                    // The closed injected transport fails synchronously, isolating
                    // publication/reaping from peer stream/reset limits.
                    SessionOptions::default(),
                )
                .unwrap();
            assert!(matches!(
                bounded(session.next()).await,
                Err(EngineError::Transport(TransportError::Closed))
            ));
        }));
    }
    for observer in observers {
        bounded(observer).await.unwrap();
    }
    bounded(reaper).await.unwrap();
    let mut ids = Vec::new();
    for _ in 0..64 {
        match bounded(engine.close()).await.unwrap_err() {
            EngineError::AudioCleanup(source) => ids.push(
                source
                    .original()
                    .downcast_ref::<PrivateFailure>()
                    .unwrap()
                    .0,
            ),
            other => panic!("unexpected secondary error: {other:?}"),
        }
    }
    ids.sort();
    assert_eq!(ids, (200..264).collect::<Vec<_>>());
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn rejected_start_retains_cleanup_without_rpc_or_audio_poll() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let server = TestServer::new(move |request| {
        observed.fetch_add(1, Ordering::SeqCst);
        complete(request)
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let audio = Audio::new(Some(failure(30)), Some(failure(31)));
    let closes = audio.closes.clone();
    assert!(matches!(
        engine.start(
            Box::new(audio),
            SessionOptions {
                timeout: Duration::ZERO,
                ..Default::default()
            }
        ),
        Err(EngineError::InvalidConfig)
    ));
    assert_private(bounded(engine.close()).await.unwrap_err(), 31, true);
    engine.close().await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    transport.close().await.unwrap();
    server.close().await;
}

async fn frame(tcp: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut header = [0; 9];
    tcp.read_exact(&mut header).await.unwrap();
    let len = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
    let mut data = vec![0; len];
    tcp.read_exact(&mut data).await.unwrap();
    (header[3], data)
}
async fn send_frame(tcp: &mut TcpStream, kind: u8, payload: &[u8]) {
    let len = (payload.len() as u32).to_be_bytes();
    tcp.write_all(&[len[1], len[2], len[3], kind, 0, 0, 0, 0, 0])
        .await
        .unwrap();
    tcp.write_all(payload).await.unwrap();
}
// Raw HTTP/2 avoids adding an h2 dev dependency to the engine crate. A control
// message chooses fatal GOAWAY after the initial upload; EOF proves it was read.
async fn fatal_peer() -> (
    TransportConfig,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = TransportConfig::loopback(listener.local_addr().unwrap()).unwrap();
    let (ready_tx, ready_rx) = oneshot::channel();
    let (fatal_tx, fatal_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut preface = [0; 24];
        tcp.read_exact(&mut preface).await.unwrap();
        assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
        send_frame(&mut tcp, 4, &[]).await;
        while frame(&mut tcp).await.0 != 0 {}
        ready_tx.send(()).unwrap();
        fatal_rx.await.unwrap();
        // last stream 0, PROTOCOL_ERROR 1. No debug payload/private data.
        send_frame(&mut tcp, 7, &[0, 0, 0, 0, 0, 0, 0, 1]).await;
        let mut buffer = [0; 1024];
        while tcp.read(&mut buffer).await.is_ok_and(|n| n != 0) {}
    });
    (config, ready_rx, fatal_tx, task)
}

#[tokio::test]
async fn owned_close_does_not_overwrite_driver_with_dropped_session_cleanup() {
    for mode in 0..3 {
        let (config, uploaded, fatal, peer) = fatal_peer().await;
        let engine = Engine::connect(config).await.unwrap();
        let mut audio = Audio::new(None, Some(failure(3)));
        let release = Arc::new(Semaphore::new(0));
        audio.release = Some(release.clone());
        let started = audio.cleanup_started.clone();
        let closes = audio.closes.clone();
        let session = engine
            .start(Box::new(audio), SessionOptions::default())
            .unwrap();
        bounded(uploaded).await.unwrap();
        drop(session);
        bounded(started.acquire()).await.unwrap().forget();
        fatal.send(()).unwrap();
        bounded(peer).await.unwrap();
        if mode == 1 {
            assert!(
                tokio::time::timeout(Duration::from_millis(10), engine.close())
                    .await
                    .is_err()
            );
            assert_eq!(closes.load(Ordering::SeqCst), 1);
        }
        release.add_permits(1);
        let results = if mode == 2 {
            let (a, b, c) =
                bounded(async { tokio::join!(engine.close(), engine.close(), engine.close()) })
                    .await;
            vec![a, b, c]
        } else {
            vec![
                bounded(engine.close()).await,
                bounded(engine.close()).await,
                bounded(engine.close()).await,
            ]
        };
        let mut driver = 0;
        let mut cleanup = 0;
        let mut successes = 0;
        for result in results {
            match result {
                Err(EngineError::Transport(TransportError::Connect)) => driver += 1,
                Err(error @ EngineError::AudioCleanup(_)) => {
                    assert_private(error, 3, true);
                    cleanup += 1;
                }
                Ok(()) => successes += 1,
                other => panic!("unexpected failure: {other:?}"),
            }
        }
        assert_eq!((driver, cleanup, successes), (1, 1, 1), "mode {mode}");
        bounded(engine.close()).await.unwrap();
        assert_eq!(closes.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn observed_goaway_stream_failure_does_not_acknowledge_driver_failure() {
    let (config, uploaded, fatal, peer) = fatal_peer().await;
    let engine = Engine::connect(config).await.unwrap();
    let audio = Audio::new(None, None);
    let closes = audio.closes.clone();
    let mut session = engine
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    bounded(uploaded).await.unwrap();
    fatal.send(()).unwrap();
    bounded(peer).await.unwrap();
    assert!(matches!(
        bounded(session.next()).await,
        Err(EngineError::Transport(TransportError::Grpc(_)))
    ));
    assert!(matches!(
        bounded(engine.close()).await,
        Err(EngineError::Transport(TransportError::Connect))
    ));
    engine.close().await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_transport_start_keeps_its_primary_and_audio_cleanup() {
    let server = quiet_server().await;
    let transport = server.transport().await;
    transport.close().await.unwrap();
    let engine = Engine::new(transport);
    let mut session = engine
        .start(
            Box::new(Audio::new(None, Some(failure(40)))),
            SessionOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        bounded(session.next()).await,
        Err(EngineError::Transport(TransportError::Closed))
    ));
    assert_private(engine.close().await.unwrap_err(), 40, true);
    engine.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn deadline_and_interrupted_close_do_not_cut_off_cleanup_or_lose_details() {
    let server = quiet_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut audio = Audio::new(None, Some(failure(50)));
    let release = Arc::new(Semaphore::new(0));
    audio.release = Some(release.clone());
    let started = audio.cleanup_started.clone();
    let closes = audio.closes.clone();
    let mut session = engine
        .start(
            Box::new(audio),
            SessionOptions {
                timeout: Duration::from_millis(20),
                ..Default::default()
            },
        )
        .unwrap();
    // This is signalled only when the deadline has stopped the pending source.
    bounded(started.acquire()).await.unwrap().forget();
    assert!(
        tokio::time::timeout(Duration::from_millis(10), session.close())
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(10), engine.close())
            .await
            .is_err()
    );
    assert!(!session.audio_progress().cleanup_finished);
    release.add_permits(1);
    assert!(matches!(
        bounded(session.close()).await,
        Err(EngineError::Transport(TransportError::Timeout))
    ));
    assert!(session.audio_progress().cleanup_finished);
    assert!(!session.audio_progress().cleanup_succeeded);
    assert_private(engine.close().await.unwrap_err(), 50, true);
    engine.close().await.unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    transport.close().await.unwrap();
    server.close().await;
}
