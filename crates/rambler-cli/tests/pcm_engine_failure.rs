#![cfg(feature = "audio")]

#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;

use bytes::Bytes;
use common::{TestServer, bounded, complete, replies};
use futures_util::{future::poll_fn, stream};
use rambler_audio::{AudioError, EncoderConfig, Pcm16Opus, PcmInputError, pcm_channel};
use rambler_engine::{
    ContextInput, ContextProvider, EditingContext, Engine, EngineError, EngineSession,
    MetadataProvider, SessionOptions, SourceError, SourceFuture,
};
use rambler_protocol::{
    initial_request,
    wire::{Value, parse_fields},
};
use rambler_transport::MetadataFuture;
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
};
use tokio::sync::{Semaphore, oneshot};

// One complete 20 ms PCM16LE/16 kHz mono frame, entirely synthetic silence.
fn frame() -> Bytes {
    Bytes::from_static(&[0; 640])
}

struct CountContext(Arc<AtomicUsize>);
impl ContextProvider for CountContext {
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(None) })
    }
}

struct GatedMetadata(Arc<Semaphore>);
impl MetadataProvider for GatedMetadata {
    fn metadata(&self) -> MetadataFuture<'_> {
        Box::pin(async {
            self.0.add_permits(1);
            // Cancellation must drop this wait; no credentials or RPC are emitted.
            std::future::pending().await
        })
    }
}

async fn failure(session: &mut EngineSession) -> EngineError {
    loop {
        match session.next().await {
            Ok(Some(event)) => {
                assert!(
                    event.response.is_some(),
                    "unexpected healthy terminal event"
                );
                assert!(!event.metrics.audio.eof, "unexpected healthy audio EOF");
            }
            Ok(None) => panic!("failed session ended successfully"),
            Err(error) => return error,
        }
    }
}

fn assert_cleaned_without_eof(session: &EngineSession) {
    let progress = session.audio_progress();
    assert!(!progress.eof);
    assert!(progress.cleanup_finished);
    assert!(progress.cleanup_succeeded);
}

#[tokio::test]
async fn abandoned_producer_preserves_typed_audio_error_and_finishes_cleanup() {
    bounded(async {
        let peer_audio = Arc::new(Semaphore::new(0));
        let observed = peer_audio.clone();
        let server = TestServer::new(move |mut request| {
            let observed = observed.clone();
            async move {
                let mut signalled = false;
                // An aborted upload need not deliver every encoder output. Only
                // this explicitly parsed, peer-consumed audio is acknowledged.
                while let Ok(Some(payload)) = request.get_mut().message().await {
                    for extension in parse_fields(&payload).unwrap() {
                        if extension.number == 293101 {
                            let Value::Bytes(message) = extension.value else {
                                panic!("audio extension must be length-delimited");
                            };
                            let fields = parse_fields(&message).unwrap();
                            let audio = fields.iter().find(|field| field.number == 1).unwrap();
                            let Value::Bytes(encoded) = &audio.value else {
                                panic!("audio field must contain bytes");
                            };
                            assert!(encoded.starts_with(b"OggS"));
                            if !signalled {
                                observed.add_permits(1);
                                signalled = true;
                            }
                        }
                    }
                }
                Ok(replies(stream::empty()))
            }
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let (mut sender, input) = pcm_channel();
        sender.reserve().await.unwrap().send(frame()).unwrap();
        let context_calls = Arc::new(AtomicUsize::new(0));
        let mut session = engine
            .start(
                Box::new(Pcm16Opus::new(input, EncoderConfig::default())),
                SessionOptions {
                    context: ContextInput::Provider(Arc::new(CountContext(context_calls.clone()))),
                    ..Default::default()
                },
            )
            .unwrap();
        peer_audio.acquire().await.unwrap().forget();
        // Deliberately omit finish, after a full frame was queued and at least
        // one real nested S3 audio payload reached the loopback peer.
        drop(sender);
        let error = failure(&mut session).await;
        match error {
            EngineError::Audio(original) => assert!(matches!(
                original.original().downcast_ref::<AudioError>(),
                Some(AudioError::InputAbandoned)
            )),
            other => panic!("expected original InputAbandoned audio error, got {other:?}"),
        }
        assert_cleaned_without_eof(&session);
        assert!(session.audio_progress().chunks > 0);
        assert_eq!(context_calls.load(Ordering::SeqCst), 0);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn engine_cancellation_wakes_full_queue_reservation_without_taking_caller_pcm() {
    bounded(async {
        let rpc_calls = Arc::new(AtomicUsize::new(0));
        let calls = rpc_calls.clone();
        let server = TestServer::new(move |request| {
            calls.fetch_add(1, Ordering::SeqCst);
            complete(request)
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let (mut sender, input) = pcm_channel();
        sender.reserve().await.unwrap().send(frame()).unwrap();
        let metadata_entered = Arc::new(Semaphore::new(0));
        let context_calls = Arc::new(AtomicUsize::new(0));
        let mut session = engine
            .start(
                Box::new(Pcm16Opus::new(input, EncoderConfig::default())),
                SessionOptions {
                    credentials: Some(Arc::new(GatedMetadata(metadata_entered.clone()))),
                    context: ContextInput::Provider(Arc::new(CountContext(context_calls.clone()))),
                    ..Default::default()
                },
            )
            .unwrap();
        metadata_entered.acquire().await.unwrap().forget();

        // With metadata held, init fills the transport's one-slot queue and
        // the first encoder output blocks behind it. After the encoder takes
        // the initial frame, this second PCM frame therefore stays queued.
        sender.reserve().await.unwrap().send(frame()).unwrap();
        let chunk = Bytes::from(vec![0x12; 640]);
        let original = chunk.clone();
        let original_pointer = chunk.as_ptr() as usize;
        let (blocked_tx, blocked_rx) = oneshot::channel();
        let producer = tokio::spawn(async move {
            let mut blocked_tx = Some(blocked_tx);
            let result = {
                let reserve = sender.reserve();
                tokio::pin!(reserve);
                poll_fn(|cx| match reserve.as_mut().poll(cx) {
                    Poll::Pending => {
                        // reserve has registered this producer task's waker on
                        // a genuinely full queue before cancellation is allowed.
                        if let Some(signal) = blocked_tx.take() {
                            signal.send(()).unwrap();
                        }
                        Poll::Pending
                    }
                    Poll::Ready(result) => Poll::Ready(result.map(drop)),
                })
                .await
            };
            (result, chunk, sender)
        });
        blocked_rx.await.unwrap();
        assert_eq!(rpc_calls.load(Ordering::SeqCst), 0);
        engine.close().await.unwrap();
        // No manual producer wake/poll: source closure must wake its registered
        // reservation future, and it must not acquire or consume caller PCM.
        let (reservation, retained, sender) = producer.await.unwrap();
        assert_eq!(reservation, Err(PcmInputError::Closed));
        assert_eq!(retained, original);
        assert_eq!(retained.as_ptr() as usize, original_pointer);
        assert_eq!(sender.finish(), Err(PcmInputError::Closed));
        assert!(matches!(
            failure(&mut session).await,
            EngineError::Cancelled
        ));
        assert_cleaned_without_eof(&session);
        assert_eq!(context_calls.load(Ordering::SeqCst), 0);
        assert_eq!(rpc_calls.load(Ordering::SeqCst), 0);
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn early_healthy_server_end_closes_unfinished_live_producer_as_incomplete() {
    bounded(async {
        let server = TestServer::new(|mut request| async move {
            let init = request.get_mut().message().await?.unwrap();
            assert_eq!(
                init.as_ref(),
                initial_request(&Default::default(), 0).unwrap()
            );
            // Synthetic S3 END followed by healthy gRPC trailers, before the
            // producer supplies intentional EOF or the peer drains the upload.
            Ok(replies(stream::iter([Ok(Bytes::from_static(b"\x08\x01"))])))
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let (mut sender, input) = pcm_channel();
        sender.reserve().await.unwrap().send(frame()).unwrap();
        let context_calls = Arc::new(AtomicUsize::new(0));
        let mut session = engine
            .start(
                Box::new(Pcm16Opus::new(input, EncoderConfig::default())),
                SessionOptions {
                    context: ContextInput::Provider(Arc::new(CountContext(context_calls.clone()))),
                    ..Default::default()
                },
            )
            .unwrap();
        // Keep the unique sender alive and unfinished until AFTER the observed
        // failure; abandonment cannot explain this incomplete-session outcome.
        let error = failure(&mut session).await;
        let EngineError::IncompleteSession(diagnostic) = error else {
            panic!("expected IncompleteSession, got {error:?}");
        };
        assert!(diagnostic.saw_s3_end);
        assert_eq!(diagnostic.completion.terminal_code, Some(tonic::Code::Ok));
        assert!(diagnostic.completion.source_closed);
        assert!(!diagnostic.completion.source_exhausted);
        assert!(!diagnostic.completion.upload_eof_polled);
        assert!(!diagnostic.metrics.audio.eof);
        assert_cleaned_without_eof(&session);
        assert!(matches!(sender.reserve().await, Err(PcmInputError::Closed)));
        assert_eq!(sender.finish(), Err(PcmInputError::Closed));
        assert_eq!(context_calls.load(Ordering::SeqCst), 0);
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}
