#![cfg(feature = "audio")]

//! Two live PCM producers share a native engine and transport. The plaintext
//! loopback peer must consume an Opus media packet from each unfinished RPC
//! before sending either draft. Only after explicit cancellation retires one
//! upload can the survivor receive more PCM and finish.
//!
//! Run from the workspace root:
//! `cargo test --locked -p rambler-cli --features audio --test pcm_concurrent_isolation`
//!
//! Packet identity uses isolated encodes with the same codec; independent Ogg
//! parsing and Opus decoding verify the survivor's framing and trim. This does
//! not require a particular Ogg serial or a specific HTTP/2 reset code. The peer
//! checks the remote socket of both RPCs and rejects extra RPCs, but does not
//! count TCP connections that never issue an RPC.

#[path = "../../rambler-audio/tests/common/mod.rs"]
mod audio_common;
#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;

use bytes::Bytes;
use common::{Replies, TestServer, bounded, drain, replies};
use futures_util::{StreamExt, poll, stream};
use rambler_audio::{EncoderConfig, Pcm16Opus, PcmInputError, pcm_channel};
use rambler_engine::{
    AudioSource, ContextInput, EditingContext, Engine, EngineError, EngineEvent, SessionOptions,
};
use rambler_protocol::{
    AUDIO_DATA, AUDIO_INFO, ProtocolConfig, RECOGNITION_RESPONSE, audio_request, context_request,
    end_request, initial_request,
    wire::{Value, bytes_field, parse_fields, string_field, varint_field},
};
use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{Barrier, oneshot};
use tonic::{Request, Response, Status, Streaming};

const CANCELLED: &str = "cancelled";
const SURVIVOR: &str = "survivor";
const SURVIVOR_FINAL: &str = "survivor final";
const SURVIVOR_SAMPLES: usize = 1287;
const PREFIX_BYTES: usize = 640; // One 20 ms PCM16LE frame at 16 kHz, mono.
const S3_END: &[u8] = b"\x08\x01";

fn protocol(name: &str) -> ProtocolConfig {
    ProtocolConfig {
        application_id: format!("synthetic-{name}"),
        ..Default::default()
    }
}

fn context(name: &str) -> EditingContext {
    EditingContext {
        before: format!("before {name}"),
        after: format!("after {name}"),
        cursor_aware: true,
        ..Default::default()
    }
}

fn options(name: &str) -> SessionOptions {
    SessionOptions {
        config: protocol(name),
        context: ContextInput::Static(context(name)),
        ..Default::default()
    }
}

fn recognition(state: u64, text: &str) -> Bytes {
    let hypothesis = string_field(1, text).unwrap();
    let list = bytes_field(3, &hypothesis).unwrap();
    let mut result = varint_field(1, state).unwrap();
    result.extend(bytes_field(3, &list).unwrap());
    bytes_field(RECOGNITION_RESPONSE, &bytes_field(1, &result).unwrap())
        .unwrap()
        .into()
}

fn assert_recognition(event: &EngineEvent, state: i32, text: &str) {
    assert!(event.metrics.eof_seconds.is_none());
    let response = event.response.as_ref().unwrap();
    assert!(response.status.is_none());
    assert_eq!(response.recognition_state(), Some(state));
    let [hypothesis] = response.hypotheses() else {
        panic!("expected a single synthetic hypothesis");
    };
    assert_eq!(hypothesis.text.as_deref(), Some(text));
}

fn field(payload: &[u8], number: u32) -> Value {
    parse_fields(payload)
        .unwrap()
        .into_iter()
        .find(|field| field.number == number)
        .unwrap_or_else(|| panic!("missing field {number}"))
        .value
}

fn blob(payload: &[u8], number: u32) -> Vec<u8> {
    let Value::Bytes(bytes) = field(payload, number) else {
        panic!("expected length-delimited field {number}");
    };
    bytes
}

fn audio_bytes(payload: &[u8]) -> Vec<u8> {
    let chunk = blob(&blob(payload, AUDIO_DATA), 1);
    let Value::Varint(elapsed) = field(&blob(payload, AUDIO_INFO), 9) else {
        panic!("expected elapsed varint");
    };
    assert!(!chunk.is_empty());
    // Reject other message types, extra fields, and mixed audio/context/END.
    assert_eq!(payload, audio_request(&chunk, elapsed).unwrap());
    chunk
}

async fn reference_packets(pcm: Vec<u8>, config: EncoderConfig) -> Vec<Vec<u8>> {
    let mut reference = Pcm16Opus::new(audio_common::Source::new([pcm]), config);
    let ogg = audio_common::collect(&mut reference).await.unwrap();
    reference.close().await.unwrap();
    audio_common::packets(&ogg).split_off(2) // Only codec packets, not headers.
}

struct Peer {
    calls: AtomicUsize,
    remote: Mutex<Option<SocketAddr>>,
    both_uploading: Barrier,
    cancelled_upload: Mutex<Option<oneshot::Sender<Streaming<Bytes>>>>,
    cancelled_packets: Vec<Vec<u8>>,
    survivor_packets: Vec<Vec<u8>>,
}

impl Peer {
    async fn serve(
        self: Arc<Self>,
        request: Request<Streaming<Bytes>>,
    ) -> Result<Response<Replies>, Status> {
        if self.calls.fetch_add(1, Ordering::SeqCst) >= 2 {
            return Err(Status::failed_precondition("unexpected additional PCM RPC"));
        }
        let address = request.remote_addr().expect("loopback TCP peer address");
        assert_eq!(*self.remote.lock().unwrap().get_or_insert(address), address);
        let mut upload = request.into_inner();
        let init = upload.message().await?.expect("missing PCM init");
        let name = [CANCELLED, SURVIVOR]
            .into_iter()
            .find(|name| init.as_ref() == initial_request(&protocol(name), 0).unwrap())
            .expect("unrecognized PCM init");
        let mut ogg = vec![];
        loop {
            let request = upload.message().await?.expect("missing live PCM audio");
            ogg.extend(audio_bytes(&request));
            let packets = audio_common::packets(&ogg);
            if packets.len() > 2 {
                let expected = if name == CANCELLED {
                    &self.cancelled_packets
                } else {
                    &self.survivor_packets
                };
                assert_eq!(&packets[2..], &expected[..1]);
                let pages = audio_common::pages(&ogg);
                for (sequence, page) in pages.iter().enumerate() {
                    assert_eq!(page.serial, pages[0].serial);
                    assert_eq!(page.sequence as usize, sequence);
                    assert_eq!(page.flags & 4, 0);
                }
                break;
            }
        }
        // A serial implementation would stall here instead of delivering drafts.
        self.both_uploading.wait().await;
        let draft = stream::iter([Ok(recognition(0, name))]);
        if name == CANCELLED {
            // Keep the response open; the test now drives this peer's request
            // body directly so it can observe what cancellation leaves behind.
            let observer = self.cancelled_upload.lock().unwrap().take().unwrap();
            assert!(observer.send(upload).is_ok());
            return Ok(replies(draft.chain(stream::pending())));
        }
        let completed = stream::once(async move {
            // This returns only after request half-close, independently of the
            // engine's local EOF bookkeeping. Replies after the draft require it.
            let tail = drain(upload).await;
            let [audio @ .., sent_context, end] = tail.as_slice() else {
                panic!("survivor upload lacks context or END");
            };
            assert!(
                !audio.is_empty(),
                "survivor sent nothing after cancellation"
            );
            assert_eq!(
                sent_context.as_ref(),
                context_request(&context(SURVIVOR)).unwrap()
            );
            assert_eq!(end.as_ref(), end_request());
            for request in audio {
                ogg.extend(audio_bytes(request));
            }
            audio_common::assert_duration(&ogg, SURVIVOR_SAMPLES);
            assert_eq!(audio_common::packets(&ogg)[2..], self.survivor_packets);
            stream::iter([
                Ok(recognition(1, SURVIVOR_FINAL)),
                Ok(Bytes::from_static(S3_END)),
            ])
        });
        Ok(replies(draft.chain(completed.flatten())))
    }
}

#[tokio::test]
async fn cancelling_one_live_pcm_upload_preserves_the_other() {
    bounded(async {
        let config = EncoderConfig {
            page_duration_ms: 20,
            ..Default::default()
        };
        let silence = vec![0; PREFIX_BYTES];
        let tone = audio_common::pcm(SURVIVOR_SAMPLES);
        let (observe_upload, cancelled_upload) = oneshot::channel();
        let peer = Arc::new(Peer {
            calls: AtomicUsize::new(0),
            remote: Mutex::new(None),
            both_uploading: Barrier::new(2),
            cancelled_upload: Mutex::new(Some(observe_upload)),
            cancelled_packets: reference_packets(silence.clone(), config).await,
            survivor_packets: reference_packets(tone.clone(), config).await,
        });
        assert_ne!(peer.cancelled_packets[0], peer.survivor_packets[0]);
        let server = TestServer::new({
            let peer = peer.clone();
            move |request| peer.clone().serve(request)
        })
        .await;
        let transport = server.transport().await;
        let engine = Engine::new(transport.clone());
        let (mut cancelled_sender, cancelled_input) = pcm_channel();
        let (mut survivor_sender, survivor_input) = pcm_channel();
        cancelled_sender
            .reserve()
            .await
            .unwrap()
            .send(silence.into())
            .unwrap();
        survivor_sender
            .reserve()
            .await
            .unwrap()
            .send(Bytes::copy_from_slice(&tone[..PREFIX_BYTES]))
            .unwrap();
        let mut cancelled = engine
            .start(
                Box::new(Pcm16Opus::new(cancelled_input, config)),
                options(CANCELLED),
            )
            .unwrap();
        let mut survivor = engine
            .start(
                Box::new(Pcm16Opus::new(survivor_input, config)),
                options(SURVIVOR),
            )
            .unwrap();

        // Both senders are still live and unfinished. Receiving both drafts
        // proves that both uploads have reached the peer's media-packet gate.
        let (cancelled_draft, survivor_draft) = tokio::join!(cancelled.next(), survivor.next());
        let cancelled_draft = cancelled_draft.unwrap().unwrap();
        let survivor_draft = survivor_draft.unwrap().unwrap();
        assert_recognition(&cancelled_draft, 0, CANCELLED);
        assert_recognition(&survivor_draft, 0, SURVIVOR);
        let survivor_id = survivor_draft.metrics.session_id;
        assert_ne!(cancelled_draft.metrics.session_id, survivor_id);

        let (terminal, peer_tail) = {
            let mut upload = cancelled_upload.await.unwrap();
            let peer_next = upload.message();
            tokio::pin!(peer_next);
            assert!(
                poll!(peer_next.as_mut()).is_pending(),
                "cancelled upload was not live"
            );
            // Signal this session only, then read its outcome instead of using
            // close(), which intentionally treats cancellation as successful cleanup.
            cancelled.request_cancel();
            tokio::join!(cancelled.next(), peer_next)
        };
        assert!(
            matches!(terminal, Err(EngineError::Cancelled)),
            "{terminal:?}"
        );
        assert!(
            !matches!(peer_tail, Ok(Some(_))),
            "cancelled side sent extra audio, context, or END"
        );
        let progress = cancelled.audio_progress();
        assert!(!progress.eof && progress.cleanup_finished && progress.cleanup_succeeded);
        assert!(matches!(
            cancelled_sender.reserve().await,
            Err(PcmInputError::Closed)
        ));
        assert!(!survivor.audio_progress().eof);

        // Fragment the remaining PCM across sample and codec-frame boundaries.
        for chunk in audio_common::fragment(&tone[PREFIX_BYTES..], &[1, 641, 3]) {
            survivor_sender
                .reserve()
                .await
                .unwrap()
                .send(chunk.into())
                .unwrap();
        }
        survivor_sender.finish().unwrap();
        let final_event = survivor.next().await.unwrap().unwrap();
        assert_recognition(&final_event, 1, SURVIVOR_FINAL);
        let end = survivor.next().await.unwrap().unwrap();
        let end_response = end.response.as_ref().unwrap();
        assert_eq!(end_response.raw.as_ref(), S3_END);
        assert_eq!(end_response.status, Some(1));
        let eof = survivor.next().await.unwrap().unwrap();
        assert!(eof.response.is_none() && eof.metrics.eof_seconds.is_some());
        assert!(survivor.next().await.unwrap().is_none());
        let progress = survivor.audio_progress();
        assert!(progress.eof && progress.cleanup_finished && progress.cleanup_succeeded);
        assert_eq!(eof.metrics.audio, progress);
        assert_eq!(eof.metrics.responses_received, 3);
        for event in [final_event, end, eof] {
            assert_eq!(event.metrics.session_id, survivor_id);
        }
        assert_eq!(survivor.captured_context(), Some(context(SURVIVOR)));
        assert_eq!(cancelled.captured_context(), Some(context(CANCELLED)));

        engine.close().await.unwrap();
        transport.close().await.unwrap();
        server.close().await;
        assert_eq!(
            peer.calls.load(Ordering::SeqCst),
            2,
            "unexpected RPC retry or replay"
        );
    })
    .await;
}
