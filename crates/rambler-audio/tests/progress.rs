mod common;

use common::*;
use rambler_audio::{AudioError, EncoderConfig, MAX_INPUT_BYTES, Pcm16Opus, PcmProgress};
use rambler_engine::{AudioSource, SourceError};
use std::sync::Arc;
use tokio::{
    sync::Notify,
    time::{Duration, timeout},
};

const DEADLINE: Duration = Duration::from_secs(10);

fn monotonic(before: &PcmProgress, after: &PcmProgress) {
    assert!(before.input_bytes <= after.input_bytes);
    assert!(before.encoded_input_samples <= after.encoded_input_samples);
    assert!(before.encoded_frames <= after.encoded_frames);
    assert!(!before.input_eof || after.input_eof);
    assert!(!before.output_eof || after.output_eof);
    assert!(after.encoded_input_samples <= after.input_bytes / 2);
    assert!(!after.output_eof || after.input_eof);
}

async fn failure(audio: &mut impl AudioSource) -> SourceError {
    for _ in 0..1024 {
        match audio.next().await {
            Ok(Some(_)) => (),
            Ok(None) => panic!("failure became EOF"),
            Err(error) => return error,
        }
    }
    panic!("pull bound exceeded")
}

#[tokio::test]
async fn fragmented_partial_frame_matches_unobserved_output_and_duration() {
    timeout(DEADLINE, async {
        let chunks = fragment(&pcm(1017), &[1, 17, 641, 3, 1280]);
        let mut plain = Pcm16Opus::new(Source::new(chunks.clone()), EncoderConfig::default());
        let (mut observed, progress) =
            Pcm16Opus::new_with_progress(Source::new(chunks), EncoderConfig::default());
        assert_eq!(progress.snapshot(), PcmProgress::default());
        let expected = collect(&mut plain).await.unwrap();
        let mut actual = Vec::new();
        let mut previous = progress.snapshot();
        for _ in 0..1024 {
            let chunk = observed.next().await.unwrap();
            let current = progress.snapshot();
            monotonic(&previous, &current);
            previous = current;
            match chunk {
                Some(chunk) => actual.extend(chunk),
                None => break,
            }
        }
        assert_eq!(packets(&actual), packets(&expected));
        assert_duration(&actual, 1017);
        assert_eq!(previous.input_bytes, 2034);
        assert_eq!(previous.encoded_input_samples, 1017);
        // The final data frame also carries padding and codec lookahead.
        assert!(previous.encoded_frames >= 4);
        assert!(previous.input_eof && previous.output_eof);
        assert!(observed.next().await.unwrap().is_none());
        observed.close().await.unwrap();
        observed.close().await.unwrap();
        assert_eq!(progress.snapshot(), previous);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn empty_and_explicit_close_are_distinct_terminal_states() {
    timeout(DEADLINE, async {
        for chunks in [vec![], vec![vec![], vec![]]] {
            let (mut audio, progress) =
                Pcm16Opus::new_with_progress(Source::new(chunks), EncoderConfig::default());
            assert!(audio.next().await.unwrap().is_none());
            let end = progress.snapshot();
            assert_eq!(
                end,
                PcmProgress {
                    input_eof: true,
                    output_eof: true,
                    ..Default::default()
                }
            );
            audio.close().await.unwrap();
            assert!(audio.next().await.unwrap().is_none());
            assert_eq!(progress.snapshot(), end);
        }
        let (mut audio, progress) =
            Pcm16Opus::new_with_progress(Source::new([pcm(320)]), EncoderConfig::default());
        audio.close().await.unwrap();
        assert!(audio.next().await.unwrap().is_none());
        audio.close().await.unwrap();
        assert_eq!(progress.snapshot(), PcmProgress::default());
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn early_close_retains_pulled_bytes_without_claiming_unencoded_samples() {
    timeout(DEADLINE, async {
        let bytes = pcm(320 * 20);
        let (mut audio, progress) =
            Pcm16Opus::new_with_progress(Source::new([bytes.clone()]), EncoderConfig::default());
        audio.next().await.unwrap().unwrap();
        let before = progress.snapshot();
        assert_eq!(before.input_bytes, bytes.len() as u64);
        assert!(before.encoded_input_samples < (bytes.len() / 2) as u64);
        assert!(!before.input_eof && !before.output_eof);
        audio.close().await.unwrap();
        assert!(audio.next().await.unwrap().is_none());
        audio.close().await.unwrap();
        assert_eq!(progress.snapshot(), before);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn errors_stay_sticky_and_never_become_output_eof() {
    timeout(DEADLINE, async {
        for case in 0..4 {
            let mut source = match case {
                0 => Source::new([vec![0; MAX_INPUT_BYTES + 1]]),
                1 => Source::new([vec![1]]),
                _ => Source::new([pcm(320)]),
            };
            if case == 2 {
                source.fail_after = Some(1);
            }
            if case == 3 {
                source.close_fails = true;
            }
            let (mut audio, progress) =
                Pcm16Opus::new_with_progress(source, EncoderConfig::default());
            let first = failure(&mut audio).await;
            if case == 0 || case == 1 {
                let expected = if case == 0 {
                    AudioError::InputLimit
                } else {
                    AudioError::TruncatedPcm
                };
                assert_eq!(
                    first.original().downcast_ref::<AudioError>(),
                    Some(&expected)
                );
            }
            let failed = progress.snapshot();
            assert!(!failed.output_eof);
            assert_eq!(failed.input_eof, case == 1 || case == 3);
            assert_eq!(
                failed.input_bytes,
                match case {
                    0 => 0,
                    1 => 1,
                    _ => 640,
                }
            );
            for _ in 0..3 {
                let again = audio.next().await.unwrap_err();
                assert!(std::ptr::eq(first.original(), again.original()));
                assert_eq!(progress.snapshot(), failed);
            }
            if case == 3 {
                assert!(audio.close().await.is_err());
            } else {
                audio.close().await.unwrap();
            }
            assert!(audio.next().await.unwrap().is_none());
            assert_eq!(progress.snapshot(), failed);
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cancelled_next_and_close_after_upstream_eof_never_invent_output_eof() {
    timeout(DEADLINE, async {
        let mut source = Source::new([pcm(320)]);
        source.block_after = Some(1);
        let entered = source.read_entered.clone();
        let (mut audio, progress) = Pcm16Opus::new_with_progress(source, EncoderConfig::default());
        {
            let mut next = audio.next();
            // Pull pages until a read actually blocks, cancelling that future.
            tokio::select! {
                biased;
                _ = entered.notified() => (),
                result = &mut next => {
                    assert!(result.unwrap().is_some());
                }
            }
        }
        // Continue to the blocked read, if the first pull produced a page.
        loop {
            let mut next = audio.next();
            tokio::select! {
                biased;
                _ = entered.notified() => break,
                result = &mut next => { assert!(result.unwrap().is_some()); }
            }
        }
        let before = progress.snapshot();
        assert_eq!(before.input_bytes, 640);
        assert!(!before.input_eof && !before.output_eof);
        audio.close().await.unwrap();
        assert!(audio.next().await.unwrap().is_none());
        assert_eq!(progress.snapshot(), before);

        let gate = Arc::new(Notify::new());
        let mut source = Source::new([pcm(1)]);
        source.close_gate = Some(gate.clone());
        let entered = source.close_entered.clone();
        let (mut audio, progress) = Pcm16Opus::new_with_progress(source, EncoderConfig::default());
        loop {
            let mut next = audio.next();
            tokio::select! {
                biased;
                _ = entered.notified() => break,
                result = &mut next => { assert!(result.unwrap().is_some()); }
            }
        }
        let before = progress.snapshot();
        assert!(before.input_eof && !before.output_eof);
        gate.notify_one();
        audio.close().await.unwrap();
        assert!(audio.next().await.unwrap().is_none());
        let after = progress.snapshot();
        monotonic(&before, &after);
        assert!(!after.output_eof);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn independent_handles_and_cross_thread_snapshots_are_monotonic() {
    timeout(DEADLINE, async {
        let (mut first, first_handle) = Pcm16Opus::new_with_progress(
            Source::new(fragment(&pcm(1017), &[1, 7, 639])),
            EncoderConfig::default(),
        );
        let (mut second, second_handle) =
            Pcm16Opus::new_with_progress(Source::new([pcm(320)]), EncoderConfig::default());
        let reader = first_handle.clone();
        let thread = std::thread::spawn(move || {
            let mut previous = reader.snapshot();
            for _ in 0..10000 {
                let current = reader.snapshot();
                monotonic(&previous, &current);
                previous = current;
                std::thread::yield_now();
            }
        });
        collect(&mut first).await.unwrap();
        assert_eq!(second_handle.snapshot(), PcmProgress::default());
        collect(&mut second).await.unwrap();
        thread.join().unwrap();
        assert_eq!(first_handle.snapshot().encoded_input_samples, 1017);
        assert_eq!(second_handle.snapshot().encoded_input_samples, 320);
        // A full input frame still needs a lookahead-only flush frame.
        assert!(second_handle.snapshot().encoded_frames >= 2);
        assert!(first_handle.snapshot().output_eof && second_handle.snapshot().output_eof);
        drop(first);
        drop(second);
        assert_eq!(first_handle.snapshot().input_bytes, 2034);
        assert_eq!(second_handle.snapshot().input_bytes, 640);
    })
    .await
    .unwrap();
}
