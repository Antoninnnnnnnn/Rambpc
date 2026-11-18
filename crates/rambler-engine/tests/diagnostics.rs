mod common;
use bytes::Bytes;
use common::*;
use futures_util::{StreamExt, stream};
use rambler_engine::*;
use rambler_protocol::{
    RECOGNITION_RESPONSE,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use rambler_transport::TransportError;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::Poll,
    time::Duration,
};
use tokio::sync::Semaphore;
use tonic::{Code, Status};

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
        for forbidden in ["private", "session_id", "hypothesis", "raw", "credential"] {
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

#[tokio::test]
async fn remote_deadline_after_end_separates_silence_from_late_partials() {
    let hypothesis = s(1, "PRIVATE_PARTIAL").unwrap();
    let recognition = [v(1, 0).unwrap(), b(4, &b(1, &hypothesis).unwrap()).unwrap()].concat();
    let partial: Bytes = [
        v(1, 0).unwrap(),
        b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap(),
    ]
    .concat()
    .into();
    let partial_size = partial.len() as u64;
    for late_partial in [false, true] {
        let payload = late_partial.then(|| partial.clone());
        let server = TestServer::new(move |mut request| {
            let payload = payload.clone();
            async move {
                // Send the initial S3 response before EOF. The same response stream
                // then waits for END and verifies that the peer sees the half-close.
                assert!(request.metadata().get("grpc-timeout").is_some());
                assert!(request.get_mut().message().await?.is_some());
                let request = request.into_inner();
                let after_upload = stream::once(async move {
                    let remaining = drain(request).await;
                    assert_eq!(remaining.len(), 3);
                    assert_eq!(
                        remaining.last().unwrap().as_ref(),
                        rambler_protocol::end_request()
                    );
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    payload.map(Ok)
                })
                .filter_map(std::future::ready);
                Ok(replies(
                    stream::iter([Ok(Bytes::from_static(b"\x08\x03"))])
                        .chain(after_upload)
                        .chain(stream::once(async {
                            tokio::time::sleep(Duration::from_millis(25)).await;
                            Err(Status::deadline_exceeded("PRIVATE_REMOTE_STATUS"))
                        })),
                ))
            }
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let context = ContextInput::Static(EditingContext {
            before: "PRIVATE_CONTEXT".into(),
            ..Default::default()
        });
        let (release, audio_gate) = tokio::sync::oneshot::channel();
        let audio = StreamAudio::new(stream::once(async move {
            audio_gate.await.unwrap();
            Ok(Bytes::from_static(b"PRIVATE_AUDIO"))
        }));
        let mut session = engine
            .start(
                Box::new(audio),
                SessionOptions {
                    context,
                    ..Default::default()
                },
            )
            .unwrap();
        let diagnostics = session.diagnostics();
        assert_eq!(
            bounded(session.next())
                .await
                .unwrap()
                .unwrap()
                .response
                .unwrap()
                .status,
            Some(3)
        );
        release.send(()).unwrap();
        let error = bounded(collect_session(session, CollectorLimits::default()))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            EngineError::Transport(TransportError::Grpc(Code::DeadlineExceeded))
        ));
        let diagnostic = diagnostics.final_snapshot().unwrap();
        let metrics = &diagnostic.metrics;
        let timeline = &metrics.timeline;
        let audio = timeline.last_audio_sent_seconds.unwrap();
        let context = timeline.context_sent_seconds.unwrap();
        let end = timeline.end_sent_seconds.unwrap();
        let half_close = timeline.half_close_sent_seconds.unwrap();
        let last = timeline.last_response_seconds.unwrap();
        assert!(audio <= context && context <= end && end <= half_close);
        assert!(half_close < metrics.elapsed_seconds);
        assert!(metrics.first_response_seconds.unwrap() <= last && last < metrics.elapsed_seconds);
        if late_partial {
            assert!(half_close < last);
            assert_eq!(timeline.last_response_s3_status, Some(0));
            assert_eq!(timeline.last_partial_response_seconds, Some(last));
        } else {
            assert_eq!(Some(last), metrics.first_response_seconds);
            assert!(last <= audio);
            assert_eq!(timeline.last_response_s3_status, Some(3));
            assert_eq!(timeline.last_partial_response_seconds, None);
        }
        assert_eq!(metrics.responses_received, 1 + u64::from(late_partial));
        assert_eq!(
            metrics.response_protobuf_bytes,
            2 + u64::from(late_partial) * partial_size
        );
        assert_eq!(timeline.recognition_finals_received, 0);
        assert_eq!(timeline.requests_submitted, 4);
        assert_eq!(
            timeline.request_protobuf_bytes_submitted,
            metrics.request_protobuf_bytes
        );
        assert!(!diagnostic.saw_s3_end);
        assert!(diagnostic.completion.upload_eof_polled);
        assert_eq!(
            diagnostic.completion.terminal_code,
            Some(Code::DeadlineExceeded)
        );
        assert!(!format!("{diagnostic:?}{error:?}").contains("PRIVATE"));
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    }
}

#[tokio::test]
async fn peer_withholds_upload_credit_before_produced_end_is_sent() {
    let server = TestServer::new(|mut request| async move {
        assert!(request.get_mut().message().await?.is_some()); // Init.
        assert!(request.get_mut().message().await?.is_some()); // Small audio.
        Ok(replies(
            stream::iter([Ok(Bytes::from_static(b"\x08\x00"))]).chain(stream::poll_fn(move |_| {
                // Keep the request alive but never poll or release its next DATA.
                let _hold = &request;
                Poll::Pending
            })),
        ))
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut audio = Audio::new(false);
    audio
        .chunks
        .push_back(Bytes::from(vec![0; 2 * 1024 * 1024]));
    let session = engine
        .start(
            Box::new(audio),
            SessionOptions {
                timeout: Duration::from_millis(750),
                ..Default::default()
            },
        )
        .unwrap();
    let diagnostics = session.diagnostics();
    let error = bounded(collect_session(session, CollectorLimits::default()))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        EngineError::Transport(TransportError::Timeout)
    ));
    let diagnostic = diagnostics.final_snapshot().unwrap();
    let metrics = &diagnostic.metrics;
    let timeline = &metrics.timeline;
    assert!(metrics.audio.eof && metrics.audio.cleanup_succeeded);
    assert!(metrics.first_response_seconds.is_some());
    assert_eq!(metrics.responses_received, 1);
    assert_eq!(metrics.response_protobuf_bytes, 2);
    assert_eq!(timeline.last_response_s3_status, Some(0));
    assert_eq!(metrics.requests_produced, 4); // END queued behind blocked audio.
    assert_eq!(timeline.requests_submitted, 2); // Init and first audio only.
    assert!(timeline.request_protobuf_bytes_submitted < metrics.request_protobuf_bytes);
    assert!(timeline.last_audio_sent_seconds.is_some());
    assert_eq!(timeline.end_sent_seconds, None);
    assert_eq!(timeline.half_close_sent_seconds, None);
    assert!(!diagnostic.completion.upload_eof_polled);
    assert_eq!(diagnostic.completion.terminal_code, None);
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
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
