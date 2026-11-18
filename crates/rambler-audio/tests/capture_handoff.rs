use bytes::Bytes;
use rambler_audio::{
    CaptureConfig, CaptureError, FRAME_BYTES, FRAME_SAMPLES, MAX_CAPTURE_BYTES, MAX_CAPTURE_CHUNKS,
    capture_channel,
};
use rambler_engine::{AudioSource, SourceError};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Wake, Waker},
    time::Duration,
};

fn config(max_chunks: usize) -> CaptureConfig {
    CaptureConfig {
        max_chunks,
        max_chunk_bytes: FRAME_BYTES,
    }
}

fn samples(start: usize, count: usize) -> Vec<u8> {
    (start..start + count)
        .flat_map(|index| (index as i16).to_le_bytes())
        .collect()
}

fn assert_error(result: Result<Option<Bytes>, SourceError>, expected: CaptureError) {
    assert_eq!(
        result.unwrap_err().original().downcast_ref(),
        Some(&expected)
    );
}

#[tokio::test]
async fn gated_twenty_ms_blocks_wrap_without_gaps_and_finish_with_a_short_tail() {
    let (mut sender, mut input) = capture_channel(config(40)).unwrap();
    let mut cursor = 0;
    let mut held = None;
    // The consumer does not poll during these batches. They represent stalls
    // from 20 ms to 800 ms, including 600 ms; no wall-clock sleeps are needed.
    for blocks in [1, 7, 30, 40, 2, 31, 40] {
        for block in 0..blocks {
            sender
                .offer(&samples(cursor + block * FRAME_SAMPLES, FRAME_SAMPLES))
                .unwrap();
        }
        for _ in 0..blocks {
            let chunk = input.next().await.unwrap().unwrap();
            assert_eq!(chunk, samples(cursor, FRAME_SAMPLES));
            cursor += FRAME_SAMPLES;
            if held.is_none() {
                held = Some(chunk);
            }
        }
    }
    // Retaining output Bytes does not reserve a queue slot or alias its reuse.
    assert_eq!(held.unwrap(), samples(0, FRAME_SAMPLES));
    let tail = samples(cursor, 37);
    sender.offer(&tail).unwrap();
    sender.finish().unwrap();
    assert_eq!(input.next().await.unwrap().unwrap(), tail);
    assert!(input.next().await.unwrap().is_none());
    assert!(input.next().await.unwrap().is_none());
    input.close().await.unwrap();
}

#[tokio::test]
async fn concurrent_producer_and_consumer_reuse_slots_without_losing_wakeups() {
    const BATCH: usize = 5;
    const ROUNDS: usize = 61;
    let (mut sender, mut input) = capture_channel(config(BATCH)).unwrap();
    let (start, batches) = std::sync::mpsc::sync_channel(1);
    let producer = std::thread::spawn(move || {
        for round in 0..ROUNDS {
            batches.recv().unwrap();
            for block in 0..BATCH {
                sender
                    .offer(&samples(
                        (round * BATCH + block) * FRAME_SAMPLES,
                        FRAME_SAMPLES,
                    ))
                    .unwrap();
            }
        }
        sender.finish().unwrap();
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut cursor = 0;
        for _ in 0..ROUNDS {
            // The previous batch was completely consumed, so this control
            // channel has space. Only the test producer waits for each grant.
            start.try_send(()).unwrap();
            for _ in 0..BATCH {
                assert_eq!(
                    input.next().await.unwrap().unwrap(),
                    samples(cursor, FRAME_SAMPLES)
                );
                cursor += FRAME_SAMPLES;
            }
        }
        assert!(input.next().await.unwrap().is_none());
    })
    .await
    .expect("test deadline");
    producer.join().unwrap();
}

#[tokio::test]
async fn exceeding_six_hundred_ms_budget_rejects_the_whole_offer_and_latches() {
    let (mut sender, mut input) = capture_channel(config(30)).unwrap();
    for block in 0..30 {
        sender
            .offer(&samples(block * FRAME_SAMPLES, FRAME_SAMPLES))
            .unwrap();
    }
    let rejected = samples(30 * FRAME_SAMPLES, FRAME_SAMPLES);
    assert_eq!(sender.offer(&rejected), Err(CaptureError::Overflow));
    assert_eq!(
        input.next().await.unwrap().unwrap(),
        samples(0, FRAME_SAMPLES)
    );
    // Once there is room again, a retry cannot splice audio across a gap.
    assert_eq!(sender.offer(&rejected), Err(CaptureError::Overflow));
    assert_eq!(sender.finish(), Err(CaptureError::Overflow));
    for block in 1..30 {
        assert_eq!(
            input.next().await.unwrap().unwrap(),
            samples(block * FRAME_SAMPLES, FRAME_SAMPLES)
        );
    }
    for _ in 0..2 {
        assert_error(input.next().await, CaptureError::Overflow);
    }
    input.close().await.unwrap();
    assert!(input.next().await.unwrap().is_none());
}

#[tokio::test]
async fn invalid_and_oversize_offers_latch_without_splitting_samples() {
    for (chunk, expected) in [
        (vec![], CaptureError::InvalidChunk),
        (vec![42], CaptureError::InvalidChunk),
        (vec![42; FRAME_BYTES + 2], CaptureError::Overflow),
    ] {
        let (mut sender, mut input) = capture_channel(config(2)).unwrap();
        let accepted = samples(0, FRAME_SAMPLES);
        sender.offer(&accepted).unwrap();
        assert_eq!(sender.offer(&chunk), Err(expected));
        assert_eq!(sender.abort(), Err(expected));
        assert_eq!(input.next().await.unwrap().unwrap(), accepted);
        for _ in 0..2 {
            assert_error(input.next().await, expected);
        }
    }
}

#[tokio::test]
async fn normal_finish_abort_drop_and_consumer_close_are_distinct() {
    let (sender, mut input) = capture_channel(config(1)).unwrap();
    sender.finish().unwrap();
    assert!(input.next().await.unwrap().is_none());

    for explicit in [true, false] {
        let (mut sender, mut input) = capture_channel(config(1)).unwrap();
        sender.offer(&[1, 2]).unwrap();
        if explicit {
            sender.abort().unwrap();
        } else {
            drop(sender);
        }
        for _ in 0..2 {
            assert_error(input.next().await, CaptureError::Aborted);
        }
        input.close().await.unwrap();
        assert!(input.next().await.unwrap().is_none());
    }

    let (mut sender, mut input) = capture_channel(config(1)).unwrap();
    sender.offer(&[1, 2]).unwrap();
    drop(input.close()); // Close performs the transition before its future polls.
    assert_eq!(sender.offer(&[3, 4]), Err(CaptureError::Closed));
    assert_eq!(sender.finish(), Err(CaptureError::Closed));
    assert!(input.next().await.unwrap().is_none());
    input.close().await.unwrap();

    let (mut sender, input) = capture_channel(config(1)).unwrap();
    drop(input);
    assert_eq!(sender.offer(&[1, 2]), Err(CaptureError::Closed));
    assert_eq!(sender.abort(), Err(CaptureError::Closed));

    let (mut sender, mut input) = capture_channel(config(1)).unwrap();
    sender.offer(&[1, 2]).unwrap();
    assert_eq!(sender.offer(&[3, 4]), Err(CaptureError::Overflow));
    input.close().await.unwrap();
    assert_eq!(sender.offer(&[3, 4]), Err(CaptureError::Overflow));
    assert_eq!(sender.finish(), Err(CaptureError::Overflow));
}

#[test]
fn bounds_are_checked_before_any_reservation() {
    for config in [
        config(0),
        config(MAX_CAPTURE_CHUNKS + 1),
        CaptureConfig {
            max_chunks: 1,
            max_chunk_bytes: 0,
        },
        CaptureConfig {
            max_chunks: 1,
            max_chunk_bytes: 3,
        },
        CaptureConfig {
            max_chunks: 2,
            max_chunk_bytes: MAX_CAPTURE_BYTES,
        },
        CaptureConfig {
            max_chunks: 2,
            max_chunk_bytes: usize::MAX - 1,
        },
    ] {
        assert!(matches!(
            capture_channel(config),
            Err(CaptureError::InvalidConfig)
        ));
    }
}

#[derive(Default)]
struct Counter(AtomicUsize);
impl Wake for Counter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[tokio::test]
async fn callback_on_another_thread_wakes_a_cancelled_wait_and_close_releases_waker() {
    let (mut sender, mut input) = capture_channel(config(1)).unwrap();
    let count = Arc::new(Counter::default());
    let waker = Waker::from(Arc::clone(&count));
    let mut next = input.next();
    assert!(
        next.as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    drop(next);
    let sender = std::thread::spawn(move || {
        sender.offer(&[1, 2]).unwrap();
        sender
    })
    .join()
    .unwrap();
    assert_eq!(count.0.load(Ordering::Relaxed), 1);
    assert_eq!(input.next().await.unwrap().unwrap().as_ref(), &[1, 2]);

    let mut next = input.next();
    assert!(
        next.as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    drop(next);
    let weak = Arc::downgrade(&count);
    drop(waker);
    drop(count);
    assert!(weak.upgrade().is_some());
    input.close().await.unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(sender.finish(), Err(CaptureError::Closed));
}

struct Backing(Vec<u8>, Arc<AtomicBool>);
impl AsRef<[u8]> for Backing {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}
impl Drop for Backing {
    fn drop(&mut self) {
        self.1.store(true, Ordering::Relaxed);
    }
}

#[tokio::test]
async fn offering_a_short_slice_does_not_pin_its_caller_owned_backing() {
    let (mut sender, mut input) = capture_channel(config(1)).unwrap();
    let dropped = Arc::new(AtomicBool::new(false));
    let pcm = Bytes::from_owner(Backing(vec![8; 1024 * 1024], Arc::clone(&dropped)));
    let slice = pcm.slice(24..26);
    sender.offer(&slice).unwrap();
    drop(slice);
    drop(pcm);
    assert!(dropped.load(Ordering::Relaxed));
    sender.finish().unwrap();
    assert_eq!(input.next().await.unwrap().unwrap().as_ref(), &[8, 8]);
    assert!(input.next().await.unwrap().is_none());
}
