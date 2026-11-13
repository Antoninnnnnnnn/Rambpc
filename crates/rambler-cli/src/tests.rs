use super::{failure_output, supervise};
use rambler_engine::{DiagnosticMetrics, EngineError, IncompleteSessionDiagnostic, SourceError};
use rambler_transport::Completion;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

struct PendingCollection(Arc<AtomicBool>);

#[test]
fn incomplete_output_contains_only_explicit_completion_diagnostics() {
    let error = EngineError::IncompleteSession(Arc::new(IncompleteSessionDiagnostic {
        saw_s3_end: true,
        completion: Completion {
            requests_produced: 3,
            requests_polled: 3,
            responses_received: 2,
            source_exhausted: false,
            upload_eof_polled: false,
            source_closed: true,
            terminal_code: Some(tonic::Code::Ok),
        },
        metrics: DiagnosticMetrics {
            audio: rambler_engine::EncodedAudioProgress {
                chunks: 1,
                bytes: 19,
                eof: false,
                cleanup_finished: true,
                cleanup_succeeded: true,
            },
            requests_produced: 3,
            request_protobuf_bytes: 100,
            responses_received: 2,
            response_protobuf_bytes: 20,
            elapsed_seconds: 2.0,
            first_response_seconds: Some(0.1),
            eof_seconds: None,
        },
    }));
    let value = failure_output(&error, None);
    assert_eq!(value["category"], "incomplete");
    assert_eq!(value["incomplete"]["saw_s3_end"], true);
    assert_eq!(value["incomplete"]["completion"]["source_exhausted"], false);
    assert_eq!(value["incomplete"]["completion"]["terminal_code"], "Ok");
    assert_eq!(value["incomplete"]["metrics"]["requests_produced"], 3);
    assert_eq!(value["incomplete"]["metrics"]["audio"]["bytes"], 19);
    assert_eq!(value["incomplete"]["metrics"]["audio"]["eof"], false);
    let serialized = value.to_string();
    for forbidden in ["session_id", "raw", "text", "metadata"] {
        assert!(!serialized.contains(forbidden));
    }
    let sensitive = EngineError::Audio(SourceError::new(std::io::Error::other(
        "PRIVATE-AUDIO-DETAIL",
    )));
    let value = failure_output(&sensitive, Some(&sensitive));
    assert_eq!(value["category"], "audio");
    assert!(!value.to_string().contains("PRIVATE-AUDIO-DETAIL"));
    assert!(value.get("incomplete").is_none());
}

impl Future for PendingCollection {
    type Output = Result<u32, &'static str>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}
impl Drop for PendingCollection {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn interruption_drops_collector_before_waiting_for_cleanup() {
    let dropped = Arc::new(AtomicBool::new(false));
    let check = dropped.clone();
    let (result, cleanup, interrupted) = supervise(
        PendingCollection(dropped),
        move || async move {
            assert!(check.load(Ordering::SeqCst));
            tokio::task::yield_now().await;
            Err("cleanup")
        },
        async { true },
    )
    .await;
    assert!(result.is_none());
    assert_eq!(cleanup, Err("cleanup"));
    assert_eq!(interrupted, Some(true));
}

#[tokio::test]
async fn interrupt_during_cleanup_discards_completed_result_and_awaits_cleanup() {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let flag = finished.clone();
    let (result, cleanup, interrupted) = supervise(
        async { Ok::<_, ()>(42) },
        move || async move {
            sender.send(()).unwrap();
            tokio::task::yield_now().await;
            flag.store(true, Ordering::SeqCst);
            Ok(())
        },
        async {
            receiver.await.unwrap();
            true
        },
    )
    .await;
    assert_eq!(result, Some(Ok(42)));
    assert_eq!(cleanup, Ok(()));
    assert_eq!(interrupted, Some(true));
    assert!(finished.load(Ordering::SeqCst));
}
