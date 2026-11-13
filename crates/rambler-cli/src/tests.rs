use super::supervise;
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
