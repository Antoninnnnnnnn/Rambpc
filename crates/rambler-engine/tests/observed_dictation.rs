//! Public collector coverage using invented responses and a synthetic localhost peer.
mod common;

use bytes::Bytes;
use common::{TestServer, bounded, complete, drain, replies};
use futures_util::stream;
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, ContextProvider, DictationObservation,
    EditingContext, Engine, EngineError, PartialTranscript, PresentationContent,
    RecognitionCompletion, Revision, SessionOptions, SourceError, SourceFuture, StreamAudio,
    collect_dictation, collect_dictation_with_observer, replacement_proposal,
};
use rambler_protocol::{
    RECOGNITION_RESPONSE, VOICE_EDITING_RESULT, context_request, end_request,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};
use rambler_transport::TransportError;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Semaphore, oneshot};
use tonic::{Code, Status};

const END: Bytes = Bytes::from_static(b"\x08\x01");
const ENCODED_FIXTURE: Bytes = Bytes::from_static(b"synthetic encoded fixture");

fn recognition(state: u64, container: Vec<u8>) -> Bytes {
    let recognition = [v(1, state).unwrap(), container].concat();
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}

fn partial(text: &str) -> Bytes {
    recognition(0, b(4, &b(1, &s(1, text).unwrap()).unwrap()).unwrap())
}

fn final_update(text: &str) -> Bytes {
    recognition(1, b(3, &b(3, &s(1, text).unwrap()).unwrap()).unwrap())
}

fn audio() -> Box<dyn AudioSource> {
    Box::new(StreamAudio::new(stream::iter([Ok(ENCODED_FIXTURE)])))
}

#[tokio::test]
async fn same_order_and_limits_as_plain_collection_with_response_progress() {
    let final_a = final_update("first final");
    let final_b = recognition(1, Vec::new()); // A state-1 marker is retained too.
    let frames = [
        partial("first preview"),
        final_a.clone(),
        partial("revised preview"),
        final_a.clone(),
        final_b.clone(),
        END,
    ];
    let response_bytes: u64 = frames.iter().map(|frame| frame.len() as u64).sum();
    let server = TestServer::new(move |request| {
        let frames = frames.clone();
        async move {
            drain(request.into_inner()).await;
            Ok(replies(stream::iter(frames.into_iter().map(Ok))))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let context = EditingContext {
        before: "sample context".into(),
        ..Default::default()
    };
    let options = SessionOptions {
        context: ContextInput::Static(context.clone()),
        ..Default::default()
    };
    let limits = CollectorLimits {
        max_final_updates: 3,
        max_result_bytes: final_a.len() * 2 + final_b.len(),
    };
    let mut ordinals = Vec::new();
    let mut previews = Vec::new();
    let mut completes = Vec::new();
    let mut end_seen = false;
    let mut observe = |observation: DictationObservation<'_>| {
        let ordinal = observation.metrics.responses_received;
        ordinals.push(ordinal);
        assert_eq!(observation.metrics.audio.chunks, 1);
        assert_eq!(
            observation.metrics.audio.bytes,
            ENCODED_FIXTURE.len() as u64
        );
        assert!(observation.metrics.eof_seconds.is_none());
        end_seen |= observation.presentation.response.status == Some(1);
        if let PresentationContent::Recognition {
            completion, result, ..
        } = observation.presentation.content
        {
            match completion {
                RecognitionCompletion::Other => previews.push(
                    PartialTranscript::from_result(result.partial_result.as_ref().unwrap()).text(),
                ),
                RecognitionCompletion::Complete => completes.push(ordinal),
            }
        }
    };
    let observed = bounded(collect_dictation_with_observer(
        &engine,
        audio(),
        options.clone(),
        limits,
        &mut observe,
    ))
    .await
    .unwrap();
    let plain = bounded(collect_dictation(&engine, audio(), options, limits))
        .await
        .unwrap();

    assert_eq!(ordinals, [1, 2, 3, 4, 5, 6]);
    assert_eq!(previews, ["first preview", "revised preview"]);
    assert_eq!(completes, [2, 4, 5]);
    assert!(end_seen);
    for collected in [observed, plain] {
        assert_eq!(
            collected
                .final_updates
                .iter()
                .map(|r| &r.raw)
                .collect::<Vec<_>>(),
            [&final_a, &final_a, &final_b]
        );
        assert_eq!(collected.context.as_ref(), Some(&context));
        assert!(matches!(
            collected.text(),
            Err(EngineError::AmbiguousResult)
        ));
        let metrics = collected.metrics.unwrap();
        assert_eq!(metrics.responses_received, 6);
        assert_eq!(metrics.response_protobuf_bytes, response_bytes);
        assert!(metrics.audio.eof && metrics.audio.cleanup_succeeded);
        assert!(metrics.eof_seconds.is_some());
    }
    bounded(engine.close()).await.unwrap();
    bounded(transport.close()).await.unwrap();
    bounded(server.close()).await;
}

#[tokio::test]
async fn observed_end_cannot_commit_before_late_grpc_status() {
    let release_error = Arc::new(Semaphore::new(0));
    let server = TestServer::new({
        let release_error = release_error.clone();
        move |request| {
            let release_error = release_error.clone();
            async move {
                drain(request.into_inner()).await;
                use futures_util::StreamExt as _;
                let frames =
                    stream::iter([Ok(partial("preview")), Ok(final_update("final")), Ok(END)]);
                let error = stream::once(async move {
                    release_error.acquire().await.unwrap().forget();
                    Err(Status::unavailable("invented late failure"))
                });
                Ok(replies(frames.chain(error)))
            }
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let (notifier, observed_end) = oneshot::channel();
    let mut notifier = Some(notifier);
    let mut observe = |observation: DictationObservation<'_>| {
        if observation.presentation.response.status == Some(1) {
            notifier
                .take()
                .unwrap()
                .send(observation.metrics.responses_received)
                .unwrap();
        }
    };
    let mut collecting = Box::pin(collect_dictation_with_observer(
        &engine,
        audio(),
        SessionOptions::default(),
        CollectorLimits::default(),
        &mut observe,
    ));
    bounded(async {
        tokio::select! {
            _ = &mut collecting => panic!("result returned before gRPC status"),
            ordinal = observed_end => assert_eq!(ordinal.unwrap(), 3),
        }
    })
    .await;
    release_error.add_permits(1);
    assert!(matches!(
        bounded(collecting).await,
        Err(EngineError::Transport(TransportError::Grpc(
            Code::Unavailable
        )))
    ));
    // The delivered failure is not replayed as a background engine failure.
    bounded(engine.close()).await.unwrap();
    bounded(transport.close()).await.unwrap();
    bounded(server.close()).await;
}

#[tokio::test]
async fn failure_before_first_response_and_overflow_do_not_notify() {
    let final_frame = final_update("final");
    let length = final_frame.len();
    let server = TestServer::new(move |request| {
        let frame = final_frame.clone();
        async move {
            drain(request.into_inner()).await;
            Ok(replies(stream::iter([
                Ok(partial("preview")),
                Ok(frame.clone()),
                Ok(frame),
                Ok(END),
            ])))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    for limits in [
        CollectorLimits {
            max_final_updates: 1,
            max_result_bytes: 1024,
        },
        CollectorLimits {
            max_final_updates: 2,
            max_result_bytes: length * 2 - 1,
        },
    ] {
        let mut ordinals = Vec::new();
        let result = bounded(collect_dictation_with_observer(
            &engine,
            audio(),
            SessionOptions::default(),
            limits,
            &mut |observation| ordinals.push(observation.metrics.responses_received),
        ))
        .await;
        assert!(matches!(result, Err(EngineError::ResultLimit)));
        assert_eq!(ordinals, [1, 2]);
    }
    // The same engine is usable after collector-level failures.
    assert!(
        bounded(collect_dictation(
            &engine,
            audio(),
            SessionOptions::default(),
            CollectorLimits::default(),
        ))
        .await
        .is_ok()
    );
    bounded(engine.close()).await.unwrap();
    bounded(transport.close()).await.unwrap();
    bounded(server.close()).await;

    let server =
        TestServer::new(|_| async { Err(Status::permission_denied("invented rejection")) }).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let closed = Arc::new(AtomicUsize::new(0));
    let result = bounded(collect_dictation_with_observer(
        &engine,
        Box::new(Tracked(closed.clone())),
        SessionOptions::default(),
        CollectorLimits::default(),
        &mut |_| panic!("no decoded response to observe"),
    ))
    .await;
    assert!(matches!(
        result,
        Err(EngineError::Transport(TransportError::Grpc(
            Code::PermissionDenied
        )))
    ));
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    bounded(engine.close()).await.unwrap();
    bounded(transport.close()).await.unwrap();
    bounded(server.close()).await;
}

struct Tracked(Arc<AtomicUsize>);
impl AudioSource for Tracked {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async { Ok(None) })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[tokio::test]
async fn unpolled_and_rejected_observations_transfer_audio_immediately() {
    let server = TestServer::new(complete).await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let closed = Arc::new(AtomicUsize::new(0));
    let mut observe =
        |_: DictationObservation<'_>| panic!("unpolled or rejected collector cannot notify");
    drop(collect_dictation_with_observer(
        &engine,
        Box::new(Tracked(closed.clone())),
        SessionOptions::default(),
        CollectorLimits::default(),
        &mut observe,
    ));
    let rejected = bounded(collect_dictation_with_observer(
        &engine,
        Box::new(Tracked(closed.clone())),
        SessionOptions::default(),
        CollectorLimits {
            max_result_bytes: 0,
            ..Default::default()
        },
        &mut observe,
    ))
    .await;
    assert!(matches!(rejected, Err(EngineError::InvalidConfig)));
    bounded(engine.close()).await.unwrap();
    assert_eq!(closed.load(Ordering::SeqCst), 2);
    bounded(transport.close()).await.unwrap();
    bounded(server.close()).await;
}

struct LateWindow {
    current: Mutex<(EditingContext, Revision)>,
    captured_revision: Mutex<Option<Revision>>,
    closed: Arc<AtomicUsize>,
}

impl ContextProvider for LateWindow {
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
        Box::pin(async {
            assert_eq!(self.closed.load(Ordering::SeqCst), 1);
            let (snapshot, revision) = self.current.lock().unwrap().clone();
            *self.captured_revision.lock().unwrap() = Some(revision);
            Ok(Some(snapshot))
        })
    }
}

#[tokio::test]
async fn observed_commit_uses_late_context_and_its_revision_for_proposal() {
    let snapshot = EditingContext {
        selected: "late selection".into(),
        cursor_aware: true,
        ..Default::default()
    };
    let sent_context = context_request(&snapshot).unwrap();
    let server = TestServer::new(move |request| {
        let sent_context = sent_context.clone();
        async move {
            let upload = drain(request.into_inner()).await;
            assert_eq!(upload[upload.len() - 2].as_ref(), sent_context);
            assert_eq!(upload.last().unwrap().as_ref(), end_request());
            let edit = [v(4, 2).unwrap(), s(3, "replacement").unwrap()].concat();
            let hypothesis = b(VOICE_EDITING_RESULT, &edit).unwrap();
            let frame = recognition(1, b(3, &b(3, &hypothesis).unwrap()).unwrap());
            Ok(replies(stream::iter([
                Ok(partial("preview")),
                Ok(frame),
                Ok(END),
            ])))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let closed = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(LateWindow {
        current: Mutex::new((EditingContext::default(), Revision::from(1))),
        captured_revision: Mutex::new(None),
        closed: closed.clone(),
    });
    let mut observe = |_: DictationObservation<'_>| {};
    let collecting = collect_dictation_with_observer(
        &engine,
        Box::new(Tracked(closed)),
        SessionOptions {
            context: ContextInput::Provider(provider.clone()),
            ..Default::default()
        },
        CollectorLimits::default(),
        &mut observe,
    );
    // The collector is already admitted. This test runs on one Tokio thread,
    // so the editor update precedes the supervisor's first poll of the provider.
    let revision = Revision::from(2);
    *provider.current.lock().unwrap() = (snapshot.clone(), revision.clone());
    let result = bounded(collecting).await.unwrap();
    assert_eq!(result.context.as_ref(), Some(&snapshot));
    assert_eq!(result.text().unwrap(), "replacement");
    let captured_revision = provider.captured_revision.lock().unwrap().clone();
    assert_eq!(captured_revision.as_ref(), Some(&revision));
    let proposal = replacement_proposal(&result, captured_revision).unwrap();
    assert_eq!(proposal.replacement_text, "replacement");
    proposal
        .validate_current(&snapshot, Some(&revision))
        .unwrap();
    assert!(matches!(
        proposal.validate_current(&snapshot, Some(&Revision::from(3))),
        Err(EngineError::ContextConflict)
    ));
    bounded(engine.close()).await.unwrap();
    bounded(transport.close()).await.unwrap();
    bounded(server.close()).await;
}

#[tokio::test]
async fn observed_preview_does_not_relax_finalization_deadline() {
    let server = TestServer::new(|request| async move {
        drain(request.into_inner()).await;
        use futures_util::StreamExt as _;
        Ok(replies(
            stream::iter([Ok(partial("preview"))]).chain(stream::pending()),
        ))
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let mut seen = 0;
    let error = bounded(collect_dictation_with_observer(
        &engine,
        audio(),
        SessionOptions {
            timeout: Duration::from_secs(4),
            finalize_timeout: Some(Duration::from_secs(1)),
            ..Default::default()
        },
        CollectorLimits::default(),
        &mut |observation| seen = observation.metrics.responses_received,
    ))
    .await
    .unwrap_err();
    assert_eq!(seen, 1);
    assert!(matches!(error, EngineError::FinalizeTimeout));
    bounded(engine.close()).await.unwrap();
    bounded(transport.close()).await.unwrap();
    bounded(server.close()).await;
}
