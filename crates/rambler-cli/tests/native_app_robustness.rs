#![cfg(feature = "audio")]

#[path = "../../rambler-audio/tests/common/mod.rs"]
mod audio_common;
#[path = "../../rambler-engine/tests/common/mod.rs"]
mod common;
#[path = "support/native_app_cancellation.rs"]
mod native_app_cancellation;
#[path = "support/native_app_reuse.rs"]
mod native_app_reuse;

use bytes::Bytes;
use common::{Replies, TestServer, replies};
use futures_util::{StreamExt, future::join_all, poll, stream};
use rambler_audio::{
    AudioError, EncoderConfig, FRAME_SAMPLES, Pcm16Opus, Pcm48kTo16k, PcmInputError, PcmSender,
    pcm_channel, start_pcm_dictation, start_pcm_dictation_with_observer,
};
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, ContextProvider, DictationObservation,
    DictationResult, EditingContext, Engine, EngineError, OggAudio, PartialTranscript,
    PresentationContent, SessionMetrics, SessionOptions, SourceError, SourceFuture, StreamAudio,
};
use rambler_protocol::{
    AUDIO_DATA, AUDIO_INFO, ProtocolConfig, RECOGNITION_RESPONSE, audio_request, context_request,
    end_request, initial_request,
    wire::{Value, bytes_field, parse_fields, string_field, varint_field},
};
use rambler_transport::TransportConfig;
use std::{
    collections::HashSet,
    future::{Future, pending},
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
    time::Duration,
};
use tokio::sync::{Barrier, Notify, oneshot};
use tonic::{Request, Response, Status, Streaming};

const END: Bytes = Bytes::from_static(b"\x08\x01");
const TEST_TIMEOUT: Duration = Duration::from_secs(40);
const SESSION_TIMEOUT: Duration = Duration::from_secs(30);

async fn timed<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(TEST_TIMEOUT, future)
        .await
        .expect("native app test deadline")
}

fn recognition(final_update: bool, text: &str) -> Bytes {
    let text = string_field(1, text).unwrap();
    let (container, item) = if final_update { (3, 3) } else { (4, 1) };
    let value = bytes_field(container, &bytes_field(item, &text).unwrap()).unwrap();
    let recognition = [varint_field(1, u64::from(final_update)).unwrap(), value].concat();
    bytes_field(RECOGNITION_RESPONSE, &bytes_field(1, &recognition).unwrap())
        .unwrap()
        .into()
}

fn preview(event: &DictationObservation<'_>) -> String {
    let PresentationContent::Recognition { result, .. } = event.presentation.content else {
        panic!("expected recognition preview");
    };
    PartialTranscript::from_result(result.partial_result.as_ref().unwrap()).text()
}

fn pcm(start: usize, count: usize, seed: usize) -> Vec<u8> {
    (start..start + count)
        .flat_map(|i| {
            let sample = ((i * 487 + i % 7 * 1009 + seed * 991) % 28001) as i32 - 14000;
            (sample as i16).to_le_bytes()
        })
        .collect()
}

#[derive(Default)]
struct PcmLifetime {
    live: AtomicUsize,
    peak: AtomicUsize,
}

struct OwnedPcm {
    pcm: Vec<u8>,
    lifetime: Arc<PcmLifetime>,
}

impl AsRef<[u8]> for OwnedPcm {
    fn as_ref(&self) -> &[u8] {
        &self.pcm
    }
}

impl Drop for OwnedPcm {
    fn drop(&mut self) {
        self.lifetime.live.fetch_sub(1, SeqCst);
    }
}

impl PcmLifetime {
    async fn send(self: &Arc<Self>, writer: &mut PcmSender, pcm: Vec<u8>) {
        let permit = writer.reserve().await.unwrap();
        let live = self.live.fetch_add(1, SeqCst) + 1;
        self.peak.fetch_max(live, SeqCst);
        permit
            .send(Bytes::from_owner(OwnedPcm {
                pcm,
                lifetime: self.clone(),
            }))
            .unwrap();
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
struct Receipt {
    messages: u64,
    bytes: u64,
    pages: u64,
    ogg_bytes: u64,
}

#[derive(Default)]
struct Pages {
    serial: Option<[u8; 4]>,
    count: u64,
    pre_skip: u64,
    granule: u64,
    eos: bool,
}

impl Pages {
    fn check(&mut self, page: &[u8]) {
        assert!(page.len() <= rambler_audio::MAX_PENDING_BYTES);
        assert_eq!(&page[..5], b"OggS\0");
        assert!(!self.eos, "audio after Ogg EOS");
        assert_eq!(
            u32::from_le_bytes(page[18..22].try_into().unwrap()) as u64,
            self.count
        );
        let serial: [u8; 4] = page[14..18].try_into().unwrap();
        assert_eq!(self.serial.get_or_insert(serial), &serial);
        let (mut parser, n) =
            ogg::reading::PageParser::new(page[..27].try_into().unwrap()).unwrap();
        let body = &page[27 + n..];
        assert_eq!(parser.parse_segments(page[27..27 + n].to_vec()), body.len());
        parser.parse_packet_data(body.to_vec()).unwrap(); // Independent CRC check.
        let granule = u64::from_le_bytes(page[6..14].try_into().unwrap());
        match self.count {
            0 => {
                assert_eq!(page[5], 2);
                assert!(body.starts_with(b"OpusHead"));
                self.pre_skip = u16::from_le_bytes(body[10..12].try_into().unwrap()) as u64;
                assert!(self.pre_skip > 0);
            }
            1 => {
                assert_eq!(page[5], 0);
                assert!(body.starts_with(b"OpusTags"));
            }
            _ => {
                assert!(matches!(page[5], 0 | 4));
                assert!(granule >= self.granule);
                self.eos = page[5] == 4;
            }
        }
        self.granule = granule;
        self.count += 1;
    }

    fn assert_duration(&self, samples: usize) {
        assert!(self.eos);
        assert_eq!(self.granule - self.pre_skip, samples as u64 * 3);
    }
}

struct Upload {
    input: Streaming<Bytes>,
    receipt: Receipt,
    pages: Pages,
    elapsed: u64,
}

impl Upload {
    fn new(input: Streaming<Bytes>) -> Self {
        Self {
            input,
            receipt: Receipt::default(),
            pages: Pages::default(),
            elapsed: 0,
        }
    }

    async fn next(&mut self) -> Result<Option<Bytes>, Status> {
        let message = self.input.message().await?;
        if let Some(payload) = &message {
            self.receipt.messages += 1;
            self.receipt.bytes += payload.len() as u64;
        }
        Ok(message)
    }

    async fn required(&mut self) -> Bytes {
        self.next().await.unwrap().expect("request ended early")
    }

    fn page(&mut self, payload: &[u8]) -> Vec<u8> {
        fn number(payload: &[u8], field: u32) -> Value {
            parse_fields(payload)
                .unwrap()
                .into_iter()
                .find(|value| value.number == field)
                .expect("missing request field")
                .value
        }
        fn blob(payload: &[u8], field: u32) -> Vec<u8> {
            let Value::Bytes(bytes) = number(payload, field) else {
                panic!("expected request bytes")
            };
            bytes
        }
        let page = blob(&blob(payload, AUDIO_DATA), 1);
        let Value::Varint(elapsed) = number(&blob(payload, AUDIO_INFO), 9) else {
            panic!("missing audio time")
        };
        assert!(elapsed >= self.elapsed);
        self.elapsed = elapsed;
        assert_eq!(payload, audio_request(&page, elapsed).unwrap());
        self.pages.check(&page);
        self.receipt.pages += 1;
        self.receipt.ogg_bytes += page.len() as u64;
        page
    }

    async fn expect_page(&mut self, expected: &[u8], exact: bool) {
        let request = self.required().await;
        let page = self.page(&request);
        if exact {
            assert_eq!(page, expected);
        } else {
            // Independently encoded streams use different serials and hence CRCs.
            // Validate continuity and CRC above; compare every other byte.
            assert_eq!(&page[..14], &expected[..14]);
            assert_eq!(&page[18..22], &expected[18..22]);
            assert_eq!(&page[26..], &expected[26..]);
        }
    }

    async fn finish(&mut self, context: Option<&EditingContext>) {
        if let Some(context) = context {
            assert_eq!(self.required().await, context_request(context).unwrap());
        }
        assert_eq!(self.required().await.as_ref(), end_request());
        assert!(
            self.next().await.unwrap().is_none(),
            "data after request END"
        );
    }
}

fn assert_complete<'a>(
    result: &'a DictationResult,
    receipt: &Receipt,
    responses: u64,
) -> &'a SessionMetrics {
    let metrics = result.metrics.as_ref().unwrap();
    assert_eq!(metrics.requests_produced, receipt.messages);
    assert_eq!(metrics.request_protobuf_bytes, receipt.bytes);
    assert_eq!(metrics.audio.chunks, receipt.pages);
    assert_eq!(metrics.audio.bytes, receipt.ogg_bytes);
    assert_eq!(metrics.responses_received, responses);
    assert!(metrics.audio.eof && metrics.audio.cleanup_finished && metrics.audio.cleanup_succeeded);
    assert!(metrics.eof_seconds.is_some());
    metrics
}

async fn engine(server: &TestServer) -> Engine {
    Engine::connect(TransportConfig::loopback(server.address).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn three_minutes_of_paced_pcm_and_partials_have_bounded_retention_and_exact_upload() {
    timed(async {
        const TURNS: usize = 1800;
        const FRAMES_PER_TURN: usize = 5;
        const SAMPLES_PER_TURN: usize = FRAMES_PER_TURN * FRAME_SAMPLES;
        const TAIL: usize = 17;
        let final_frame = recognition(true, "long session final");
        let limits = CollectorLimits {
            max_final_updates: 1,
            max_result_bytes: final_frame.len(),
        };
        let (receipt_tx, receipt_rx) = oneshot::channel();
        let receipt_tx = Mutex::new(Some(receipt_tx));
        let calls = Arc::new(AtomicUsize::new(0));
        let server = TestServer::new({
            let calls = calls.clone();
            let final_frame = final_frame.clone();
            move |request| {
                assert_eq!(calls.fetch_add(1, SeqCst), 0);
                let receipt_tx = receipt_tx.lock().unwrap().take().unwrap();
                let final_frame = final_frame.clone();
                async move {
                    let mut upload = Upload::new(request.into_inner());
                    assert_eq!(
                        upload.required().await,
                        initial_request(&ProtocolConfig::default(), 0).unwrap()
                    );
                    // A differently partitioned pull encoder, not a second SDK session.
                    // Both the reference and the verifier stream in constant space.
                    let reference_pcm = StreamAudio::new(stream::iter(
                        (0..TURNS)
                            .map(|i| Ok(pcm(i * SAMPLES_PER_TURN, SAMPLES_PER_TURN, 0).into()))
                            .chain(std::iter::once_with(|| {
                                Ok(pcm(TURNS * SAMPLES_PER_TURN, TAIL, 0).into())
                            })),
                    ));
                    let mut reference = Pcm16Opus::new(reference_pcm, EncoderConfig::default());
                    for _ in 0..2 {
                        upload
                            .expect_page(&reference.next().await.unwrap().unwrap(), false)
                            .await;
                    }
                    Ok(replies(stream::unfold(
                        (0, upload, reference, Some((receipt_tx, final_frame))),
                        |(i, mut upload, mut reference, mut terminal)| async move {
                            let frame = if i < TURNS {
                                for _ in 0..FRAMES_PER_TURN {
                                    upload
                                        .expect_page(
                                            &reference.next().await.unwrap().unwrap(),
                                            false,
                                        )
                                        .await;
                                }
                                recognition(false, &format!("synthetic draft {i}"))
                            } else if i == TURNS {
                                while let Some(page) = reference.next().await.unwrap() {
                                    upload.expect_page(&page, false).await;
                                }
                                reference.close().await.unwrap();
                                upload
                                    .pages
                                    .assert_duration(TURNS * SAMPLES_PER_TURN + TAIL);
                                upload.finish(None).await;
                                let (receipt_tx, final_frame) = terminal.take().unwrap();
                                receipt_tx
                                    .send(std::mem::take(&mut upload.receipt))
                                    .unwrap();
                                final_frame
                            } else if i == TURNS + 1 {
                                END
                            } else {
                                return None;
                            };
                            Some((Ok(frame), (i + 1, upload, reference, terminal)))
                        },
                    )))
                }
            }
        })
        .await;
        let engine = engine(&server).await;
        let next_turn = Notify::new();
        let lifetime = Arc::new(PcmLifetime::default());
        let mut seen = 0;
        let mut bytes = 0;
        let mut observe = |event: DictationObservation<'_>| {
            assert!(event.metrics.eof_seconds.is_none());
            bytes += event.presentation.response.raw.len() as u64;
            assert_eq!(event.metrics.response_protobuf_bytes, bytes);
            seen += 1;
            assert_eq!(event.metrics.responses_received, seen as u64);
            if seen <= TURNS {
                assert_eq!(preview(&event), format!("synthetic draft {}", seen - 1));
                next_turn.notify_one();
            } else if seen == TURNS + 1 {
                assert_eq!(event.presentation.response.raw, final_frame);
            } else {
                assert_eq!(seen, TURNS + 2);
                assert_eq!(event.presentation.response.raw, END);
            }
        };
        let (mut writer, result) = start_pcm_dictation_with_observer(
            &engine,
            SessionOptions::default(),
            limits,
            &mut observe,
        );
        let feed = async {
            for i in 0..TURNS {
                let block = pcm(i * SAMPLES_PER_TURN, SAMPLES_PER_TURN, 0);
                for fragment in block.chunks(641) {
                    lifetime.send(&mut writer, fragment.to_vec()).await;
                }
                // An app-level pace: the next 100ms block cannot be produced
                // until the peer consumed and the app observed this one's draft.
                next_turn.notified().await;
            }
            lifetime
                .send(&mut writer, pcm(TURNS * SAMPLES_PER_TURN, TAIL, 0))
                .await;
            writer.finish().unwrap();
            Ok::<(), EngineError>(())
        };
        let ((), result) = tokio::try_join!(feed, result).unwrap();
        assert_eq!(seen, TURNS + 2);
        assert_eq!(lifetime.live.load(SeqCst), 0);
        assert!((1..=2).contains(&lifetime.peak.load(SeqCst)));
        assert_eq!(result.text().unwrap(), "long session final");
        assert_eq!(result.final_updates.len(), 1);
        assert_eq!(result.final_updates[0].raw, final_frame);
        let receipt = receipt_rx.await.unwrap();
        assert_eq!(receipt.messages, receipt.pages + 2);
        assert_eq!(
            assert_complete(&result, &receipt, (TURNS + 2) as u64).response_protobuf_bytes,
            bytes
        );
        engine.close().await.unwrap();
        server.close().await;
        assert_eq!(calls.load(SeqCst), 1);
    })
    .await;
}
