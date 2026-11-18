#![cfg(feature = "audio")]

#[path = "../../rambler-audio/tests/common/mod.rs"]
mod audio_common;
#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;

use bytes::Bytes;
use common::{TestServer, bounded, drain, replies};
use futures_util::{StreamExt, poll, stream};
use rambler_audio::{
    AudioError, PcmInputError, start_pcm_dictation, start_pcm_dictation_with_observer,
};
use rambler_engine::{
    CollectorLimits, DictationObservation, Engine, EngineError, PartialTranscript,
    PresentationContent, SessionOptions,
};
use rambler_protocol::{
    AUDIO_DATA, INITIAL_PARAMETER, RECOGNITION_RESPONSE, end_request,
    wire::{Value, bytes_field, parse_fields, string_field, varint_field},
};
use rambler_transport::TransportError;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;
use tonic::{Code, Status};

const SAMPLES: i32 = 1607;
const END: Bytes = Bytes::from_static(b"\x08\x01");

fn synthetic_pcm() -> Vec<u8> {
    (0..SAMPLES)
        .flat_map(|i| (((i * 487 + i % 7 * 1009) % 28001 - 14000) as i16).to_le_bytes())
        .collect()
}

fn recognition(state: u64, field: u32, item_field: u32, text: &str) -> Bytes {
    let text = string_field(1, text).unwrap();
    let item = bytes_field(item_field, &text).unwrap();
    let container = bytes_field(field, &item).unwrap();
    let recognition = [varint_field(1, state).unwrap(), container].concat();
    let extension = bytes_field(1, &recognition).unwrap();
    Bytes::from(bytes_field(RECOGNITION_RESPONSE, &extension).unwrap())
}

fn assert_init(message: &[u8]) {
    let fields = parse_fields(message).unwrap();
    assert!(fields.iter().any(|field| field.number == INITIAL_PARAMETER));
    assert!(!fields.iter().any(|field| field.number == AUDIO_DATA));
}

fn single_bytes_field(message: &[u8], number: u32) -> Vec<u8> {
    let mut values = parse_fields(message).unwrap().into_iter();
    let field = values.next().unwrap();
    assert_eq!(field.number, number);
    assert!(values.next().is_none());
    let Value::Bytes(bytes) = field.value else {
        panic!("expected bytes")
    };
    bytes
}

#[tokio::test]
async fn preview_while_feeding_then_only_clean_status_can_commit() {
    bounded(async {
        for fail_status in [false, true] {
            let release_status = Arc::new(Notify::new());
            let server = TestServer::new({
                let release_status = release_status.clone();
                move |request| {
                    let release_status = release_status.clone();
                    async move {
                        let mut upload = request.into_inner();
                        let init = upload.message().await?.unwrap();
                        assert_init(&init);
                        let preview = stream::iter([Ok(recognition(0, 4, 1, "sample preview"))]);
                        let final_frame = stream::once(async move {
                            let chunks = drain(upload).await;
                            let (end, audio) = chunks.split_last().unwrap();
                            assert_eq!(end.as_ref(), end_request());
                            let ogg: Vec<u8> = audio
                                .iter()
                                .flat_map(|payload| {
                                    let envelope = parse_fields(payload).unwrap();
                                    let data = envelope.iter().find(|f| f.number == AUDIO_DATA).unwrap();
                                    let Value::Bytes(data) = &data.value else { panic!("expected audio bytes") };
                                    single_bytes_field(data, 1)
                                })
                                .collect();
                            audio_common::assert_duration(&ogg, SAMPLES as usize);
                            Ok(recognition(1, 3, 3, "sample final"))
                        });
                        let terminal = stream::once(async move {
                            release_status.notified().await;
                            fail_status.then(|| Err(Status::unavailable("synthetic late failure")))
                        })
                        .filter_map(std::future::ready);
                        Ok(replies(preview.chain(final_frame).chain(stream::iter([Ok(END)])).chain(terminal)))
                    }
                }
            })
            .await;
            let transport = server.transport().await;
            let engine = Engine::new(transport.clone());
            let preview_ready = Notify::new();
            let end_ready = Notify::new();
            let mut preview = None;
            let mut ordinals = Vec::new();
            let result = {
                let mut observe = |event: DictationObservation<'_>| {
                    ordinals.push(event.metrics.responses_received);
                    assert!(event.metrics.eof_seconds.is_none());
                    if let PresentationContent::Recognition { result, .. } = event.presentation.content
                        && let Some(partial) = &result.partial_result
                    {
                        preview = Some(PartialTranscript::from_result(partial).text());
                        preview_ready.notify_one();
                    }
                    if event.presentation.response.status == Some(1) {
                        end_ready.notify_one();
                    }
                };
                let (mut writer, completed) = start_pcm_dictation_with_observer(
                    &engine, SessionOptions::default(), CollectorLimits::default(), &mut observe,
                );
                let preview_ready = &preview_ready;
                let feed = async move {
                    preview_ready.notified().await;
                    for fragment in synthetic_pcm().chunks(641) {
                        writer.reserve().await.unwrap().send(Bytes::copy_from_slice(fragment)).unwrap();
                    }
                    writer.finish().unwrap();
                };
                tokio::pin!(completed);
                tokio::select! {
                    result = &mut completed => panic!("completed before terminal status: {result:?}"),
                    () = async { feed.await; end_ready.notified().await } => {},
                }
                assert!(poll!(completed.as_mut()).is_pending());
                release_status.notify_one();
                completed.await
            };
            engine.close().await.unwrap();
            transport.close().await.unwrap();
            server.close().await;

            assert_eq!(preview.as_deref(), Some("sample preview"));
            assert_eq!(ordinals, [1, 2, 3]);
            if fail_status {
                assert!(matches!(result, Err(EngineError::Transport(TransportError::Grpc(Code::Unavailable)))));
            } else {
                let completed = result.unwrap();
                assert_eq!(completed.final_updates.len(), 1);
                assert_eq!(completed.text().unwrap(), "sample final");
                let metrics = completed.metrics.unwrap();
                assert!(metrics.audio.eof && metrics.audio.cleanup_finished && metrics.audio.cleanup_succeeded);
                assert!(metrics.eof_seconds.is_some());
            }
        }
    }).await;
}

#[tokio::test]
async fn admitted_before_first_poll_and_abandonment_is_not_eof() {
    bounded(async {
        let init_seen = Arc::new(Notify::new());
        let server = TestServer::new({
            let init_seen = init_seen.clone();
            move |request| {
                let init_seen = init_seen.clone();
                async move {
                    let mut upload = request.into_inner();
                    assert_init(&upload.message().await?.unwrap());
                    init_seen.notify_one();
                    // Keep the upload alive, without sending any successful result.
                    Ok(replies(stream::once(async move {
                        while upload.message().await?.is_some() {}
                        Err(Status::internal("unexpected healthy upload EOF"))
                    })))
                }
            }
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let (mut writer, completed) = start_pcm_dictation(
            &engine,
            SessionOptions::default(),
            CollectorLimits::default(),
        );
        init_seen.notified().await; // Do not poll the completion future yet.
        writer
            .reserve()
            .await
            .unwrap()
            .send(Bytes::from(synthetic_pcm()))
            .unwrap();
        drop(writer);
        let result = completed.await;
        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
        let Err(EngineError::Audio(error)) = result else {
            panic!("expected PCM abandonment, got {result:?}")
        };
        assert_eq!(
            error.original().downcast_ref::<AudioError>(),
            Some(&AudioError::InputAbandoned)
        );
    })
    .await;
}

struct QueuedPcm(Arc<AtomicUsize>);
impl AsRef<[u8]> for QueuedPcm {
    fn as_ref(&self) -> &[u8] {
        &[17, 0]
    }
}
impl Drop for QueuedPcm {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_unpolled_completion_wakes_a_blocked_reservation_and_releases_pcm() {
    bounded(async {
        let server = TestServer::new(common::complete).await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let (mut writer, completed) = start_pcm_dictation(
            &engine,
            SessionOptions::default(),
            CollectorLimits::default(),
        );
        let releases = Arc::new(AtomicUsize::new(0));
        writer
            .reserve()
            .await
            .unwrap()
            .send(Bytes::from_owner(QueuedPcm(releases.clone())))
            .unwrap();
        // This task has not yielded since admission. The supervisor cannot have
        // drained the one-slot queue on this current-thread runtime.
        let reservation = writer.reserve();
        tokio::pin!(reservation);
        assert!(poll!(reservation.as_mut()).is_pending());
        assert_eq!(releases.load(Ordering::SeqCst), 0);
        drop(completed);
        assert!(matches!(reservation.await, Err(PcmInputError::Closed)));
        engine.close().await.unwrap();
        assert_eq!(releases.load(Ordering::SeqCst), 1);
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn invalid_limits_close_pcm_before_poll_without_starting_an_rpc() {
    bounded(async {
        let rpc_calls = Arc::new(AtomicUsize::new(0));
        let server = TestServer::new({
            let rpc_calls = rpc_calls.clone();
            move |request| {
                rpc_calls.fetch_add(1, Ordering::SeqCst);
                common::complete(request)
            }
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let (mut writer, completed) = start_pcm_dictation(
            &engine,
            SessionOptions::default(),
            CollectorLimits {
                max_final_updates: 0,
                ..Default::default()
            },
        );
        engine.close().await.unwrap();
        assert!(matches!(writer.reserve().await, Err(PcmInputError::Closed)));
        assert!(matches!(completed.await, Err(EngineError::InvalidConfig)));
        assert_eq!(rpc_calls.load(Ordering::SeqCst), 0);
        transport.close().await.unwrap();
        server.close().await;
    })
    .await;
}
