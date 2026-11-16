mod common;
use bytes::Bytes;
use common::*;
use futures_util::stream;
use rambler_engine::*;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::Semaphore;

struct Audio {
    chunks: VecDeque<Bytes>,
    read_error: bool,
    close_error: bool,
}
impl Audio {
    fn new(read_error: bool, close_error: bool) -> Self {
        Self {
            chunks: [
                Bytes::new(),
                Bytes::from_static(b"abc"),
                Bytes::new(),
                Bytes::from_static(b"de"),
            ]
            .into(),
            read_error,
            close_error,
        }
    }
}
fn private_error() -> SourceError {
    SourceError::new(std::io::Error::other("PRIVATE SOURCE DETAIL"))
}
impl AudioSource for Audio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            if let Some(chunk) = self.chunks.pop_front() {
                return Ok(Some(chunk));
            }
            if self.read_error {
                Err(private_error())
            } else {
                Ok(None)
            }
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            if self.close_error {
                Err(private_error())
            } else {
                Ok(())
            }
        })
    }
}
fn expected(eof: bool, cleanup_succeeded: bool) -> EncodedAudioProgress {
    EncodedAudioProgress {
        chunks: 2,
        bytes: 5,
        eof,
        cleanup_finished: true,
        cleanup_succeeded,
    }
}

#[tokio::test]
async fn successful_metrics_count_only_nonempty_encoded_chunks_and_match_live_snapshot() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut session = engine
        .start(
            Box::new(Audio::new(false, false)),
            SessionOptions::default(),
        )
        .unwrap();
    let mut final_metrics = None;
    while let Some(event) = bounded(session.next()).await.unwrap() {
        if event.response.is_none() {
            final_metrics = Some(event.metrics);
        }
    }
    assert_eq!(final_metrics.unwrap().audio, expected(true, true));
    assert_eq!(session.audio_progress(), expected(true, true));
    bounded(engine.close()).await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}

#[tokio::test]
async fn read_and_cleanup_failures_do_not_fabricate_eof_or_successful_cleanup() {
    for (read_error, close_error) in [(true, false), (false, true), (true, true)] {
        let server = TestServer::new(|mut request| async move {
            while let Ok(Some(_)) = request.get_mut().message().await {}
            Ok(replies(stream::empty()))
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let mut session = engine
            .start(
                Box::new(Audio::new(read_error, close_error)),
                SessionOptions::default(),
            )
            .unwrap();
        let error = loop {
            match bounded(session.next()).await {
                Ok(Some(_)) => {}
                Err(error) => break error,
                Ok(None) => panic!("failed source succeeded"),
            }
        };
        if read_error {
            assert!(matches!(error, EngineError::Audio(_)));
        } else {
            assert!(matches!(error, EngineError::AudioCleanup(_)));
        }
        let progress = session.audio_progress();
        assert_eq!(progress, expected(!read_error, !close_error));
        assert!(!format!("{progress:?}").contains("PRIVATE"));
        if read_error && close_error {
            // Each call creates a distinct original error. Observing Audio did
            // not deliver or acknowledge the independent cleanup failure.
            assert!(matches!(
                bounded(engine.close()).await,
                Err(EngineError::AudioCleanup(_))
            ));
        }
        bounded(engine.close()).await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    }
}

struct ContextBlocked(Arc<Semaphore>);
impl ContextProvider for ContextBlocked {
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
        Box::pin(async {
            self.0.add_permits(1);
            std::future::pending().await
        })
    }
}

#[tokio::test]
async fn audio_eof_is_distinct_from_request_source_exhaustion_during_late_context() {
    let started = Arc::new(Semaphore::new(0));
    let entered = started.clone();
    let server = TestServer::new(move |mut request| {
        let entered = entered.clone();
        async move {
            // init + two nonempty encoded chunks; no context or END is available.
            for _ in 0..3 {
                assert!(request.get_mut().message().await?.is_some());
            }
            entered.acquire().await.unwrap().forget();
            Ok(replies(stream::iter([Ok(Bytes::from_static(b"\x08\x01"))])))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut session = engine
        .start(
            Box::new(Audio::new(false, false)),
            SessionOptions {
                context: ContextInput::Provider(Arc::new(ContextBlocked(started))),
                ..Default::default()
            },
        )
        .unwrap();
    let diagnostic = loop {
        match bounded(session.next()).await {
            Ok(Some(_)) => {}
            Err(EngineError::IncompleteSession(diagnostic)) => break diagnostic,
            other => panic!("unexpected {other:?}"),
        }
    };
    assert!(diagnostic.saw_s3_end);
    assert!(!diagnostic.completion.source_exhausted);
    assert!(!diagnostic.completion.upload_eof_polled);
    assert_eq!(diagnostic.metrics.audio, expected(true, true));
    assert_eq!(session.audio_progress(), expected(true, true));
    bounded(engine.close()).await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}
