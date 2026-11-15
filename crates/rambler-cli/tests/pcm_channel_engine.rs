#![cfg(feature = "audio")]

#[path = "../../rambler-audio/tests/common/mod.rs"]
mod audio_common;
#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;

use bytes::Bytes;
use common::{TestServer, bounded, drain, replies};
use futures_util::{poll, stream};
use rambler_audio::{EncoderConfig, Pcm16Opus, PcmSender, pcm_channel};
use rambler_engine::{
    AudioSource, ContextInput, ContextProvider, EditingContext, EncodedAudioProgress, Engine,
    EngineError, EngineEvent, EngineSession, ProtocolConfig, SessionOptions, SourceError,
    SourceFuture,
};
use rambler_protocol::{
    audio_request, context_request, end_request, initial_request,
    wire::{Value, parse_fields},
};
use rambler_transport::TransportError;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;
use tonic::{Code, Status};

async fn produce(mut sender: PcmSender, pcm: &[u8]) {
    // The unique writer accepts odd fragments and empty chunks. Neither reserve,
    // send nor finish is used as evidence that the peer consumed any audio.
    sender.reserve().await.unwrap().send(Bytes::new()).unwrap();
    for chunk in audio_common::fragment(pcm, &[1, 3, 641, 1279, 2, 4097]) {
        sender
            .reserve()
            .await
            .unwrap()
            .send(Bytes::from(chunk))
            .unwrap();
    }
    sender.finish().unwrap();
}

async fn events(session: &mut EngineSession) -> Result<Vec<EngineEvent>, EngineError> {
    let mut events = vec![];
    while let Some(event) = session.next().await? {
        events.push(event);
    }
    Ok(events)
}

fn nested_bytes(payload: &[u8], number: u32) -> Vec<u8> {
    let fields = parse_fields(payload).unwrap();
    let matching: Vec<_> = fields
        .iter()
        .filter(|field| field.number == number)
        .collect();
    assert_eq!(matching.len(), 1);
    match &matching[0].value {
        Value::Bytes(bytes) => bytes.clone(),
        other => panic!("expected length-delimited field, got {other:?}"),
    }
}

fn audio_chunk(payload: &[u8]) -> Vec<u8> {
    nested_bytes(&nested_bytes(payload, 293101), 1)
}

fn elapsed(payload: &[u8]) -> u64 {
    let audio_info = nested_bytes(payload, 293100);
    let fields = parse_fields(&audio_info).unwrap();
    match &fields.iter().find(|field| field.number == 9).unwrap().value {
        Value::Varint(value) => *value,
        other => panic!("expected elapsed varint, got {other:?}"),
    }
}

// Validate every request, not just counters: init, only nonempty audio wrappers,
// exactly one context, exact S3 END, and no request after END. Call only after the
// server has independently observed HTTP request EOF.
fn assert_requests(seen: &[Bytes], context: &EditingContext) -> (Vec<u8>, u64) {
    assert!(seen.len() >= 4);
    assert_eq!(
        seen[0].as_ref(),
        initial_request(&ProtocolConfig::default(), elapsed(&seen[0])).unwrap()
    );
    assert_eq!(
        seen[seen.len() - 2].as_ref(),
        context_request(context).unwrap()
    );
    assert_eq!(seen.last().unwrap().as_ref(), end_request());
    let mut ogg = vec![];
    for request in &seen[1..seen.len() - 2] {
        let chunk = audio_chunk(request);
        assert!(!chunk.is_empty());
        assert_eq!(
            request.as_ref(),
            audio_request(&chunk, elapsed(request)).unwrap()
        );
        ogg.extend(chunk);
    }
    (ogg, (seen.len() - 3) as u64)
}

fn assert_progress(progress: &EncodedAudioProgress, bytes: usize, chunks: u64) {
    assert_eq!(
        progress,
        &EncodedAudioProgress {
            chunks,
            bytes: bytes as u64,
            eof: true,
            cleanup_finished: true,
            cleanup_succeeded: true,
        }
    );
}

#[tokio::test]
async fn fragmented_finished_pcm_reaches_peer_as_continuous_ogg_then_normal_engine_eof() {
    bounded(async {
        let samples = 1607;
        let pcm = audio_common::pcm(samples);
        for page_duration_ms in [20, 1000] {
            let config = EncoderConfig {
                page_duration_ms,
                ..Default::default()
            };
            let seen = Arc::new(Mutex::new(vec![]));
            let half_closed = Arc::new(AtomicBool::new(false));
            let server = TestServer::new({
                let seen = seen.clone();
                let half_closed = half_closed.clone();
                move |request| {
                    let seen = seen.clone();
                    let half_closed = half_closed.clone();
                    async move {
                        // drain returns only after the peer's HTTP upload half-close.
                        let requests = drain(request.into_inner()).await;
                        *seen.lock().unwrap() = requests;
                        half_closed.store(true, Ordering::SeqCst);
                        Ok(replies(stream::iter([Ok(Bytes::from_static(b"\x08\x01"))])))
                    }
                }
            })
            .await;
            let transport = server.transport().await;
            let engine = Engine::new(transport.clone());
            let context = EditingContext {
                before: "synthetic context".into(),
                cursor_aware: true,
                ..Default::default()
            };
            let (sender, input) = pcm_channel();
            let mut session = engine
                .start(
                    Box::new(Pcm16Opus::new(input, config)),
                    SessionOptions {
                        context: ContextInput::Static(context.clone()),
                        ..Default::default()
                    },
                )
                .unwrap();
            let ((), result) = tokio::join!(produce(sender, &pcm), events(&mut session));
            let progress = session.audio_progress();
            // Close all three owners before asserting the terminal result.
            let engine_closed = engine.close().await;
            let transport_closed = transport.close().await;
            server.close().await;
            engine_closed.unwrap();
            transport_closed.unwrap();

            let events = result.unwrap();
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].response.as_ref().unwrap().status, Some(1));
            assert!(events[1].response.is_none());
            assert!(events[1].metrics.eof_seconds.is_some());
            assert!(half_closed.load(Ordering::SeqCst));
            let seen = seen.lock().unwrap().clone();
            let (ogg, chunks) = assert_requests(&seen, &context);
            // The shared independent Ogg reader checks CRC, and the Opus decoder
            // checks pre-skip/final-granule trimming against actual PCM duration.
            audio_common::assert_duration(&ogg, samples);
            let mut reference = Pcm16Opus::new(audio_common::Source::new([pcm.clone()]), config);
            let reference_ogg = audio_common::collect(&mut reference).await.unwrap();
            reference.close().await.unwrap();
            assert_eq!(
                audio_common::packets(&ogg)[2..],
                audio_common::packets(&reference_ogg)[2..]
            );
            assert_progress(&progress, ogg.len(), chunks);
            assert_eq!(events[1].metrics.audio, progress);
            assert_eq!(events[1].metrics.requests_produced, seen.len() as u64);
            assert_eq!(
                events[1].metrics.request_protobuf_bytes,
                seen.iter().map(|request| request.len() as u64).sum::<u64>()
            );
        }
    })
    .await;
}

#[derive(Default)]
struct AudioTrace {
    ogg: Vec<u8>,
    eof: bool,
    cleanup_succeeded: bool,
}

// Transparent observation of the real encoder, not an alternate audio engine.
// Record bytes only after next succeeds and cleanup only after real close succeeds.
struct Observed<S> {
    source: S,
    trace: Arc<Mutex<AudioTrace>>,
}
impl<S: AudioSource> AudioSource for Observed<S> {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            let output = self.source.next().await?;
            let mut trace = self.trace.lock().unwrap();
            match &output {
                Some(bytes) => trace.ogg.extend_from_slice(bytes),
                None => trace.eof = true,
            }
            Ok(output)
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.source.close().await?;
            self.trace.lock().unwrap().cleanup_succeeded = true;
            Ok(())
        })
    }
}

struct LateContext {
    trace: Arc<Mutex<AudioTrace>>,
    samples: usize,
    calls: Arc<AtomicUsize>,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
    value: EditingContext,
}
impl ContextProvider for LateContext {
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
        // These assertions run at provider invocation, before its gate or any
        // context work: all finished PCM was encoded, EOS emitted, and close won.
        assert_eq!(self.calls.fetch_add(1, Ordering::SeqCst), 0);
        {
            let trace = self.trace.lock().unwrap();
            assert!(trace.eof);
            assert!(trace.cleanup_succeeded);
            audio_common::assert_duration(&trace.ogg, self.samples);
        }
        self.entered.add_permits(1);
        Box::pin(async move {
            self.release.acquire().await.unwrap().forget();
            Ok(Some(self.value.clone()))
        })
    }
}

#[tokio::test]
async fn late_context_waits_for_real_audio_cleanup_and_s3_end_waits_for_grpc_status() {
    bounded(async {
        // Both terminal outcomes follow the same real PCM/Opus/engine upload.
        // An S3 END candidate must not hide a subsequently failing gRPC status.
        for terminal_failure in [false, true] {
            let samples = 967;
            let pcm = audio_common::pcm(samples);
            let trace = Arc::new(Mutex::new(AudioTrace::default()));
            let seen = Arc::new(Mutex::new(vec![]));
            let half_closed = Arc::new(AtomicBool::new(false));
            let peer_eos = Arc::new(Semaphore::new(0));
            let context_entered = Arc::new(Semaphore::new(0));
            let context_release = Arc::new(Semaphore::new(0));
            let status_waiting = Arc::new(Semaphore::new(0));
            let status_release = Arc::new(Semaphore::new(0));
            let calls = Arc::new(AtomicUsize::new(0));
            let server = TestServer::new({
                let seen = seen.clone();
                let half_closed = half_closed.clone();
                let peer_eos = peer_eos.clone();
                let status_waiting = status_waiting.clone();
                let status_release = status_release.clone();
                move |request| {
                    let seen = seen.clone();
                    let half_closed = half_closed.clone();
                    let peer_eos = peer_eos.clone();
                    let status_waiting = status_waiting.clone();
                    let status_release = status_release.clone();
                    async move {
                        let mut request = request.into_inner();
                        let mut ogg = vec![];
                        let mut eos_seen = false;
                        while let Some(payload) = request.message().await? {
                            if parse_fields(&payload)
                                .unwrap()
                                .iter()
                                .any(|field| field.number == 293101)
                            {
                                ogg.extend(audio_chunk(&payload));
                                if audio_common::pages(&ogg).last().unwrap().flags & 4 != 0 {
                                    assert!(!eos_seen);
                                    eos_seen = true;
                                    peer_eos.add_permits(1);
                                }
                            }
                            seen.lock().unwrap().push(payload);
                        }
                        half_closed.store(true, Ordering::SeqCst);
                        Ok(replies(stream::unfold(0, move |state| {
                            let status_waiting = status_waiting.clone();
                            let status_release = status_release.clone();
                            async move {
                                match state {
                                    0 => Some((Ok(Bytes::from_static(b"\x08\x01")), 1)),
                                    1 => {
                                        status_waiting.add_permits(1);
                                        status_release.acquire().await.unwrap().forget();
                                        if terminal_failure {
                                            Some((
                                                Err(Status::permission_denied(
                                                    "synthetic terminal",
                                                )),
                                                2,
                                            ))
                                        } else {
                                            None
                                        }
                                    }
                                    _ => None,
                                }
                            }
                        })))
                    }
                }
            })
            .await;
            let transport = server.transport().await;
            let engine = Engine::new(transport.clone());
            let context = EditingContext {
                before: "late synthetic context".into(),
                ..Default::default()
            };
            let (sender, input) = pcm_channel();
            let mut session = engine
                .start(
                    Box::new(Observed {
                        source: Pcm16Opus::new(input, EncoderConfig::default()),
                        trace: trace.clone(),
                    }),
                    SessionOptions {
                        context: ContextInput::Provider(Arc::new(LateContext {
                            trace: trace.clone(),
                            samples,
                            calls: calls.clone(),
                            entered: context_entered.clone(),
                            release: context_release.clone(),
                            value: context.clone(),
                        })),
                        ..Default::default()
                    },
                )
                .unwrap();
            // Driving the unique producer is not sufficient: wait separately for
            // provider invocation and for the server to consume real Ogg EOS.
            produce(sender, &pcm).await;
            context_entered.acquire().await.unwrap().forget();
            peer_eos.acquire().await.unwrap().forget();
            let before_context = session.audio_progress();
            let peer_prefix = seen.lock().unwrap().clone();
            assert!(peer_prefix.len() > 1);
            assert!(peer_prefix[1..].iter().all(|request| {
                parse_fields(request)
                    .unwrap()
                    .iter()
                    .any(|field| field.number == 293101)
            }));
            assert!(!half_closed.load(Ordering::SeqCst));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_progress(
                &before_context,
                trace.lock().unwrap().ogg.len(),
                (peer_prefix.len() - 1) as u64,
            );
            {
                let next = session.next();
                tokio::pin!(next);
                assert!(poll!(next.as_mut()).is_pending());
            }
            context_release.add_permits(1);
            let candidate = session.next().await.unwrap().unwrap();
            assert_eq!(candidate.response.as_ref().unwrap().status, Some(1));
            status_waiting.acquire().await.unwrap().forget();
            assert!(half_closed.load(Ordering::SeqCst));
            let terminal = {
                let next = session.next();
                tokio::pin!(next);
                // Even after peer EOF and the S3 END response, normal completion
                // remains unavailable while the server withholds terminal status.
                assert!(poll!(next.as_mut()).is_pending());
                status_release.add_permits(1);
                next.await
            };
            let exhausted = if terminal_failure {
                None
            } else {
                Some(session.next().await)
            };
            let progress = session.audio_progress();
            let engine_closed = engine.close().await;
            let transport_closed = transport.close().await;
            server.close().await;
            engine_closed.unwrap();
            transport_closed.unwrap();

            if terminal_failure {
                assert!(matches!(
                    terminal,
                    Err(EngineError::Transport(TransportError::Grpc(
                        Code::PermissionDenied
                    )))
                ));
            } else {
                let eof = terminal.unwrap().unwrap();
                assert!(eof.response.is_none());
                assert_eq!(eof.metrics.audio, progress);
                assert!(exhausted.unwrap().unwrap().is_none());
            }
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            let seen = seen.lock().unwrap();
            let (ogg, chunks) = assert_requests(&seen, &context);
            assert_eq!(ogg, trace.lock().unwrap().ogg);
            audio_common::assert_duration(&ogg, samples);
            assert_progress(&progress, ogg.len(), chunks);
            assert_eq!(before_context, progress);
        }
    })
    .await;
}
