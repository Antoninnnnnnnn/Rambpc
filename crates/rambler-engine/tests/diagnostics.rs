mod common;
use bytes::Bytes;
use common::*;
use futures_util::stream;
use rambler_engine::*;
use rambler_protocol::{
    RECOGNITION_RESPONSE,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;
use tonic::Code;

struct Audio {
    chunks: VecDeque<Bytes>,
    block_after_chunks: bool,
    closes: Arc<AtomicUsize>,
    closed: Arc<AtomicBool>,
    close_started: Arc<Semaphore>,
    close_release: Option<Arc<Semaphore>>,
}
impl Audio {
    fn new(block: bool) -> Self {
        Self {
            chunks: [Bytes::from_static(b"private audio chunk")].into(),
            block_after_chunks: block,
            closes: Arc::new(AtomicUsize::new(0)),
            closed: Arc::new(AtomicBool::new(false)),
            close_started: Arc::new(Semaphore::new(0)),
            close_release: None,
        }
    }
}
impl AudioSource for Audio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if let Some(chunk) = self.chunks.pop_front() {
                return Ok(Some(chunk));
            }
            if self.block_after_chunks {
                std::future::pending::<()>().await;
            }
            Ok(None)
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.closes.fetch_add(1, Ordering::SeqCst);
            self.close_started.add_permits(1);
            if let Some(release) = &self.close_release {
                release.acquire().await.unwrap().forget();
            }
            self.closed.store(true, Ordering::SeqCst);
            Ok(())
        })
    }
}
async fn incomplete(session: &mut EngineSession) -> Arc<IncompleteSessionDiagnostic> {
    loop {
        match bounded(session.next()).await {
            Ok(Some(event)) => assert!(
                event.response.is_some(),
                "incomplete session emitted success EOF"
            ),
            Err(EngineError::IncompleteSession(diagnostic)) => return diagnostic,
            Err(error) => panic!("unexpected error {error:?}"),
            Ok(None) => panic!("incomplete session succeeded"),
        }
    }
}
fn check_safe(diagnostic: &Arc<IncompleteSessionDiagnostic>) {
    let error = EngineError::IncompleteSession(diagnostic.clone());
    for formatted in [
        format!("{error}"),
        format!("{error:?}"),
        format!("{diagnostic:?}"),
    ] {
        for forbidden in [
            "private",
            "session_id",
            "hypothesis",
            "raw",
            "context",
            "credential",
        ] {
            assert!(
                !formatted.contains(forbidden),
                "diagnostic contained forbidden data category"
            );
        }
    }
    assert!(diagnostic.metrics.elapsed_seconds.is_finite());
    assert!(
        diagnostic.metrics.first_response_seconds.unwrap() <= diagnostic.metrics.elapsed_seconds
    );
    assert_eq!(diagnostic.metrics.eof_seconds, None);
    assert!(diagnostic.completion.source_closed);
    assert_eq!(diagnostic.metrics.audio.chunks, 1);
    assert_eq!(
        diagnostic.metrics.audio.bytes,
        b"private audio chunk".len() as u64
    );
    assert!(diagnostic.metrics.audio.cleanup_finished);
    assert!(diagnostic.metrics.audio.cleanup_succeeded);
    assert_eq!(diagnostic.completion.terminal_code, Some(Code::Ok));
}

#[tokio::test]
async fn missing_s3_end_reports_complete_upload_and_safe_final_counters() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured2 = captured.clone();
    let hypothesis = s(1, "private hypothesis text").unwrap();
    let recognition = [v(1, 0).unwrap(), b(3, &b(3, &hypothesis).unwrap()).unwrap()].concat();
    let payload = [
        v(1, 0).unwrap(),
        s(3, "private server details").unwrap(),
        b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap(),
        b(99, b"private opaque bytes").unwrap(),
    ]
    .concat();
    let response_size = payload.len() as u64;
    let server = TestServer::new(move |request| {
        let captured = captured2.clone();
        let payload = payload.clone();
        async move {
            *captured.lock().unwrap() = drain(request.into_inner()).await;
            Ok(replies(stream::iter([Ok(Bytes::from(payload))])))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let audio = Audio::new(false);
    let closes = audio.closes.clone();
    let closed = audio.closed.clone();
    let options = SessionOptions {
        context: ContextInput::Static(EditingContext {
            before: "private editing snapshot".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut session = engine.start(Box::new(audio), options).unwrap();
    let diagnostic = incomplete(&mut session).await;
    assert!(!diagnostic.saw_s3_end);
    assert!(diagnostic.metrics.audio.eof);
    assert!(diagnostic.completion.source_exhausted);
    assert!(diagnostic.completion.upload_eof_polled);
    assert_eq!(diagnostic.completion.requests_produced, 4);
    assert_eq!(diagnostic.completion.requests_polled, 4);
    assert_eq!(diagnostic.completion.responses_received, 1);
    assert_eq!(diagnostic.metrics.requests_produced, 4);
    assert_eq!(
        diagnostic.metrics.request_protobuf_bytes,
        captured
            .lock()
            .unwrap()
            .iter()
            .map(|payload| payload.len() as u64)
            .sum::<u64>()
    );
    assert_eq!(diagnostic.metrics.responses_received, 1);
    assert_eq!(diagnostic.metrics.response_protobuf_bytes, response_size);
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert!(closed.load(Ordering::SeqCst));
    check_safe(&diagnostic);
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

async fn early_server() -> TestServer {
    TestServer::new(|mut request| async move {
        // Observe init AND one audio chunk, but never wait for audio-source EOF.
        assert!(request.get_mut().message().await?.is_some());
        assert!(request.get_mut().message().await?.is_some());
        Ok(replies(stream::iter([Ok(Bytes::from_static(b"\x08\x01"))])))
    })
    .await
}

#[tokio::test]
async fn early_s3_end_grpc_ok_reports_unexhausted_audio_and_remains_an_error() {
    let server = early_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let audio = Audio::new(true);
    let closed = audio.closed.clone();
    let closes = audio.closes.clone();
    let mut session = engine
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    let diagnostic = incomplete(&mut session).await;
    assert!(diagnostic.saw_s3_end);
    assert!(!diagnostic.metrics.audio.eof);
    assert!(!diagnostic.completion.source_exhausted);
    assert_eq!(diagnostic.completion.requests_produced, 2);
    assert_eq!(diagnostic.completion.requests_polled, 2);
    assert_eq!(diagnostic.metrics.requests_produced, 2);
    assert_eq!(diagnostic.metrics.responses_received, 1);
    assert_eq!(diagnostic.metrics.response_protobuf_bytes, 2);
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert!(closed.load(Ordering::SeqCst));
    check_safe(&diagnostic);
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn incomplete_diagnostic_is_final_only_after_slow_cleanup_despite_dropped_next() {
    let server = early_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut audio = Audio::new(true);
    let release = Arc::new(Semaphore::new(0));
    audio.close_release = Some(release.clone());
    let started = audio.close_started.clone();
    let closed = audio.closed.clone();
    let closes = audio.closes.clone();
    let mut session = engine
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    bounded(started.acquire()).await.unwrap().forget();
    assert_eq!(
        bounded(session.next())
            .await
            .unwrap()
            .unwrap()
            .response
            .unwrap()
            .status,
        Some(1)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(25), session.next())
            .await
            .is_err()
    );
    assert!(!closed.load(Ordering::SeqCst));
    assert!(!session.audio_progress().eof);
    assert!(!session.audio_progress().cleanup_finished);
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    release.add_permits(1);
    let diagnostic = incomplete(&mut session).await;
    assert!(closed.load(Ordering::SeqCst));
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert!(diagnostic.metrics.elapsed_seconds >= 0.025);
    assert!(diagnostic.saw_s3_end);
    assert!(!diagnostic.metrics.audio.eof);
    assert!(!diagnostic.completion.source_exhausted);
    check_safe(&diagnostic);
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn explicit_cancel_during_slow_cleanup_is_not_relabelled_as_incomplete_or_success() {
    let server = early_server().await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut audio = Audio::new(true);
    let release = Arc::new(Semaphore::new(0));
    audio.close_release = Some(release.clone());
    let started = audio.close_started.clone();
    let closed = audio.closed.clone();
    let closes = audio.closes.clone();
    let mut session = engine
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    bounded(started.acquire()).await.unwrap().forget();
    assert!(
        bounded(session.next())
            .await
            .unwrap()
            .unwrap()
            .response
            .is_some()
    );
    session.request_cancel();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), engine.close())
            .await
            .is_err()
    );
    assert!(!closed.load(Ordering::SeqCst));
    assert!(!session.audio_progress().eof);
    assert!(!session.audio_progress().cleanup_finished);
    release.add_permits(1);
    bounded(engine.close()).await.unwrap();
    assert!(matches!(
        bounded(session.next()).await,
        Err(EngineError::Cancelled)
    ));
    assert!(closed.load(Ordering::SeqCst));
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(
        session.audio_progress(),
        EncodedAudioProgress {
            chunks: 1,
            bytes: 19,
            eof: false,
            cleanup_finished: true,
            cleanup_succeeded: true,
        }
    );
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn completed_collector_preserves_both_incomplete_diagnostic_kinds() {
    for early in [false, true] {
        let server = if early {
            early_server().await
        } else {
            TestServer::new(|request| async move {
                drain(request.into_inner()).await;
                Ok(replies(stream::iter([Ok(Bytes::from_static(b"\x08\x00"))])))
            })
            .await
        };
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let result = bounded(collect_dictation(
            &engine,
            Box::new(Audio::new(early)),
            SessionOptions::default(),
            CollectorLimits::default(),
        ))
        .await;
        let EngineError::IncompleteSession(diagnostic) = result.unwrap_err() else {
            panic!("collector lost the incomplete diagnostic");
        };
        assert_eq!(diagnostic.saw_s3_end, early);
        assert_eq!(diagnostic.metrics.audio.eof, !early);
        assert_eq!(diagnostic.completion.source_exhausted, !early);
        check_safe(&diagnostic);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    }
}

#[test]
fn safe_metrics_conversion_does_not_retain_the_session_identifier() {
    let metrics = SessionMetrics {
        session_id: u64::MAX,
        audio: EncodedAudioProgress::default(),
        requests_produced: 4,
        request_protobuf_bytes: 100,
        responses_received: 1,
        response_protobuf_bytes: 2,
        elapsed_seconds: 0.5,
        first_response_seconds: Some(0.1),
        eof_seconds: None,
    };
    let safe = DiagnosticMetrics::from(&metrics);
    assert_eq!(safe.requests_produced, 4);
    assert_eq!(safe.elapsed_seconds, 0.5);
    let formatted = format!("{safe:?}");
    assert!(!formatted.contains("session_id"));
    assert!(!formatted.contains(&u64::MAX.to_string()));
}
