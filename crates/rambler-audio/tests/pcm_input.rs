use bytes::Bytes;
use futures_util::poll;
use rambler_audio::{AudioError, MAX_INPUT_BYTES, PcmInputError, PcmSendError, pcm_channel};
use rambler_engine::AudioSource;
use std::{
    error::Error,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Wake, Waker},
    time::Duration,
};

async fn bounded(test: impl Future<Output = ()>) {
    tokio::time::timeout(Duration::from_secs(5), test)
        .await
        .expect("PCM input test timed out");
}

fn assert_rejected(error: PcmSendError, kind: PcmInputError, original: &Bytes) {
    assert_eq!(error.kind(), kind);
    // Fixed formatting must not expose raw PCM, its length, or a nested error.
    assert_eq!(
        format!("{error:?}"),
        "PcmSendError { chunk: [REDACTED], .. }"
    );
    assert_eq!(error.to_string(), "PCM input chunk rejected");
    assert!(error.source().is_none());
    let recovered = error.into_chunk();
    assert_eq!(recovered.as_ptr(), original.as_ptr());
    assert_eq!(&recovered, original);
}

#[derive(Default)]
struct WakeFlag(AtomicBool);

impl Wake for WakeFlag {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn capacity_is_one_and_canceled_reservation_preserves_caller_bytes() {
    bounded(async {
        let (mut sender, mut source) = pcm_channel();
        let first = Bytes::from_static(b"first");
        sender.reserve().await.unwrap().send(first.clone()).unwrap();
        let caller_held = Bytes::from_static(b"caller-owned PCM marker\0\xff");
        let original = caller_held.clone();
        {
            let mut waiting = Box::pin(sender.reserve());
            assert!(poll!(waiting.as_mut()).is_pending());
        }
        assert_eq!(caller_held.as_ptr(), original.as_ptr());
        assert_eq!(caller_held, original);
        assert_eq!(source.next().await.unwrap(), Some(first));
        sender.reserve().await.unwrap().send(caller_held).unwrap();
        assert_eq!(source.next().await.unwrap(), Some(original));
        sender.finish().unwrap();
        assert_eq!(source.next().await.unwrap(), None);
    })
    .await;
}

#[tokio::test]
async fn dropping_an_unused_permit_restores_the_slot() {
    bounded(async {
        let (mut sender, mut source) = pcm_channel();
        drop(sender.reserve().await.unwrap());
        let chunk = Bytes::from_static(b"after unused permit");
        sender.reserve().await.unwrap().send(chunk.clone()).unwrap();
        sender.finish().unwrap();
        assert_eq!(source.next().await.unwrap(), Some(chunk));
        assert_eq!(source.next().await.unwrap(), None);
    })
    .await;
}

#[tokio::test]
async fn canceling_a_pending_next_does_not_lose_the_following_chunk() {
    bounded(async {
        let (mut sender, mut source) = pcm_channel();
        {
            let mut next = source.next();
            assert!(poll!(next.as_mut()).is_pending());
        }
        let chunk = Bytes::from_static(b"after canceled next");
        sender.reserve().await.unwrap().send(chunk.clone()).unwrap();
        sender.finish().unwrap();
        assert_eq!(source.next().await.unwrap(), Some(chunk));
        assert_eq!(source.next().await.unwrap(), None);
    })
    .await;
}

#[tokio::test]
async fn finish_drains_accepted_pcm_before_stable_eof() {
    bounded(async {
        let (mut sender, mut source) = pcm_channel();
        let chunk = Bytes::from_static(b"accepted before finish");
        sender.reserve().await.unwrap().send(chunk.clone()).unwrap();
        sender.finish().unwrap();
        assert_eq!(source.next().await.unwrap(), Some(chunk));
        assert_eq!(source.next().await.unwrap(), None);
        assert_eq!(source.next().await.unwrap(), None);
    })
    .await;
}

#[tokio::test]
async fn empty_and_odd_chunks_are_accepted_without_rewriting() {
    bounded(async {
        let (mut sender, mut source) = pcm_channel();
        for chunk in [
            Bytes::new(),
            Bytes::from_static(b"\x01"),
            Bytes::from_static(b"\x02\x03\x04"),
        ] {
            sender.reserve().await.unwrap().send(chunk.clone()).unwrap();
            assert_eq!(source.next().await.unwrap(), Some(chunk));
        }
        sender.finish().unwrap();
        assert_eq!(source.next().await.unwrap(), None);
    })
    .await;
}

#[tokio::test]
async fn producer_drop_drains_then_latches_abandonment_until_close() {
    bounded(async {
        for queued in [false, true] {
            let (mut sender, mut source) = pcm_channel();
            let chunk = Bytes::from_static(b"accepted before abandonment");
            if queued {
                sender.reserve().await.unwrap().send(chunk.clone()).unwrap();
            }
            drop(sender);
            if queued {
                assert_eq!(source.next().await.unwrap(), Some(chunk));
            }
            for _ in 0..2 {
                let error = source.next().await.unwrap_err();
                assert!(matches!(
                    error.original().downcast_ref::<AudioError>(),
                    Some(AudioError::InputAbandoned)
                ));
            }
            source.close().await.unwrap();
            assert_eq!(source.next().await.unwrap(), None);
        }
    })
    .await;
}

#[tokio::test]
async fn closing_or_dropping_source_wakes_a_pending_reservation() {
    bounded(async {
        for drop_source in [false, true] {
            let (mut sender, mut source) = pcm_channel();
            sender
                .reserve()
                .await
                .unwrap()
                .send(Bytes::from_static(b"queued"))
                .unwrap();
            let mut waiting = Box::pin(sender.reserve());
            let wake = Arc::new(WakeFlag::default());
            let waker = Waker::from(Arc::clone(&wake));
            let mut context = Context::from_waker(&waker);
            assert!(waiting.as_mut().poll(&mut context).is_pending());
            wake.0.store(false, Ordering::SeqCst);
            if drop_source {
                drop(source);
            } else {
                source.close().await.unwrap();
            }
            assert!(wake.0.load(Ordering::SeqCst), "reservation was not woken");
            assert!(matches!(waiting.await, Err(PcmInputError::Closed)));
        }
    })
    .await;
}

#[tokio::test]
async fn unpolled_close_wakes_a_pending_reservation_without_taking_caller_bytes() {
    bounded(async {
        let (mut sender, mut source) = pcm_channel();
        sender
            .reserve()
            .await
            .unwrap()
            .send(Bytes::from_static(b"queued before unpolled close"))
            .unwrap();
        let caller_held = Bytes::from(b"caller PCM marker\0\xff".to_vec());
        let original = caller_held.clone();
        let mut waiting = Box::pin(sender.reserve());
        let wake = Arc::new(WakeFlag::default());
        let waker = Waker::from(Arc::clone(&wake));
        let mut context = Context::from_waker(&waker);
        assert!(waiting.as_mut().poll(&mut context).is_pending());
        wake.0.store(false, Ordering::SeqCst);

        drop(source.close());

        assert!(wake.0.load(Ordering::SeqCst), "reservation was not woken");
        assert!(matches!(waiting.await, Err(PcmInputError::Closed)));
        assert_eq!(caller_held.as_ptr(), original.as_ptr());
        assert_eq!(caller_held, original);
        assert_eq!(source.next().await.unwrap(), None);
    })
    .await;
}

#[tokio::test]
async fn unpolled_close_is_synchronous_and_finish_reports_closed() {
    bounded(async {
        let (mut sender, mut source) = pcm_channel();
        sender
            .reserve()
            .await
            .unwrap()
            .send(Bytes::from_static(b"discardable"))
            .unwrap();
        let unpolled_close = source.close();
        assert_eq!(sender.finish(), Err(PcmInputError::Closed));
        drop(unpolled_close);
        assert_eq!(source.next().await.unwrap(), None);
        source.close().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn a_held_permit_rejects_after_close_and_returns_the_original_bytes() {
    bounded(async {
        let (mut sender, mut source) = pcm_channel();
        let permit = sender.reserve().await.unwrap();
        source.close().await.unwrap();
        let chunk = Bytes::from(b"raw PCM formatting marker\0\xff".to_vec());
        let original = chunk.clone();
        let error = permit.send(chunk).unwrap_err();
        assert_rejected(error, PcmInputError::Closed, &original);
        assert!(matches!(sender.reserve().await, Err(PcmInputError::Closed)));
        assert_eq!(source.next().await.unwrap(), None);
    })
    .await;
}

#[tokio::test]
async fn oversize_rejection_preserves_bytes_and_allows_a_later_valid_send() {
    bounded(async {
        let (mut sender, mut source) = pcm_channel();
        let chunk = Bytes::from(vec![0xa5; MAX_INPUT_BYTES + 1]);
        let original = chunk.clone();
        let error = sender.reserve().await.unwrap().send(chunk).unwrap_err();
        assert_rejected(error, PcmInputError::ChunkTooLarge, &original);
        let valid = Bytes::from_static(b"valid after rejection");
        sender.reserve().await.unwrap().send(valid.clone()).unwrap();
        sender.finish().unwrap();
        assert_eq!(source.next().await.unwrap(), Some(valid));
        assert_eq!(source.next().await.unwrap(), None);
    })
    .await;
}

#[tokio::test]
async fn exact_maximum_chunk_size_is_accepted_intact() {
    bounded(async {
        let (mut sender, mut source) = pcm_channel();
        let chunk = Bytes::from(vec![0x5a; MAX_INPUT_BYTES]);
        let original = chunk.clone();
        sender.reserve().await.unwrap().send(chunk).unwrap();
        sender.finish().unwrap();
        let received = source.next().await.unwrap().unwrap();
        assert_eq!(received.as_ptr(), original.as_ptr());
        assert_eq!(received, original);
        assert_eq!(source.next().await.unwrap(), None);
    })
    .await;
}

#[tokio::test]
async fn independent_channels_do_not_mix_data_or_terminal_states() {
    bounded(async {
        let (mut first_sender, mut first_source) = pcm_channel();
        let (mut second_sender, mut second_source) = pcm_channel();
        let first = Bytes::from_static(b"first channel");
        let second = Bytes::from_static(b"second channel");
        first_sender
            .reserve()
            .await
            .unwrap()
            .send(first.clone())
            .unwrap();
        second_sender
            .reserve()
            .await
            .unwrap()
            .send(second.clone())
            .unwrap();
        first_sender.finish().unwrap();
        assert_eq!(second_source.next().await.unwrap(), Some(second));
        {
            let mut next = second_source.next();
            assert!(poll!(next.as_mut()).is_pending());
        }
        assert_eq!(first_source.next().await.unwrap(), Some(first));
        assert_eq!(first_source.next().await.unwrap(), None);
        second_sender.finish().unwrap();
        assert_eq!(second_source.next().await.unwrap(), None);
    })
    .await;
}
