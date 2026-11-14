//! Full Engine -> plaintext h2 loopback -> active event consumer / collect_session.
//! No global allocator instrumentation. Synthetic fixture verification is timed
//! server work, so these numbers are not production/network-service latency.
#[path = "../tests/common/mod.rs"]
mod loopback;
mod profile_support;

use bytes::Bytes;
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, Engine, SessionMetrics, SessionOptions,
    SourceError, SourceFuture, collect_session,
};
use rambler_protocol::{
    AUDIO_DATA, AUDIO_INFO, ProtocolConfig, context_request, decode_response, end_request,
    initial_request,
    wire::{Value, parse_fields},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;

struct SyntheticAudio {
    chunks: Arc<Vec<Bytes>>,
    index: usize,
    closes: Arc<AtomicUsize>,
}
impl AudioSource for SyntheticAudio {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            let next = self.chunks.get(self.index).cloned();
            if next.is_some() {
                self.index += 1;
            }
            Ok(next)
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.closes.fetch_add(1, SeqCst);
        Box::pin(async { Ok(()) })
    }
}

fn verify_audio(payload: &[u8], expected: &[u8]) {
    let fields = parse_fields(payload).unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].number, AUDIO_INFO);
    assert_eq!(fields[1].number, AUDIO_DATA);
    let Value::Bytes(info) = &fields[0].value else {
        panic!("audio info wire")
    };
    let info = parse_fields(info).unwrap();
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].number, 9);
    let Value::Varint(elapsed) = info[0].value else {
        panic!("elapsed wire")
    };
    // Exact canonical payload equality, including elapsed, nesting and field order.
    // Keep the server verifier on frozen pre-optimization assembly so a client
    // serializer improvement is not counted a second time as server work.
    assert_eq!(
        payload,
        profile_support::reference_audio_request(expected, elapsed).unwrap()
    );
}

#[derive(Clone, Copy)]
struct Mode {
    name: &'static str,
    collect: bool,
    reader_stall: bool,
    consumer_stall: bool,
}

async fn run(
    mode: Mode,
    repeat: usize,
    chunks: Arc<Vec<Bytes>>,
    responses: Arc<Vec<Bytes>>,
    final_every: usize,
) {
    let config = ProtocolConfig::default();
    let context = profile_support::context();
    let init = Bytes::from(initial_request(&config, 0).unwrap());
    let late_context = Bytes::from(context_request(&context).unwrap());
    let expected = chunks.clone();
    let outgoing = responses.clone();
    let (complete, completed) = oneshot::channel();
    let complete = Arc::new(std::sync::Mutex::new(Some(complete)));
    let server = loopback::TestServer::new(move |request| {
        let expected = expected.clone();
        let outgoing = outgoing.clone();
        let init = init.clone();
        let late_context = late_context.clone();
        let complete = complete.clone();
        async move {
            let (tx, rx) = mpsc::channel(1);
            tokio::spawn(async move {
                let mut request = request.into_inner();
                let first = request.message().await.unwrap().unwrap();
                assert_eq!(first, init);
                let mut count = 1usize;
                let mut bytes = first.len();
                let mut reader_stalls = 0;
                for (index, chunk) in expected.iter().enumerate() {
                    if mode.reader_stall && index % 64 == 0 {
                        reader_stalls += 1;
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                    let payload = request.message().await.unwrap().unwrap();
                    verify_audio(&payload, chunk);
                    count += 1;
                    bytes += payload.len();
                    tx.send(Ok(outgoing[index].clone())).await.unwrap();
                }
                let payload = request.message().await.unwrap().unwrap();
                assert_eq!(payload, late_context);
                count += 1;
                bytes += payload.len();
                let payload = request.message().await.unwrap().unwrap();
                assert_eq!(payload.as_ref(), end_request());
                count += 1;
                bytes += payload.len();
                assert!(
                    request.message().await.unwrap().is_none(),
                    "missing upload EOF"
                );
                tx.send(Ok(Bytes::from_static(b"\x08\x01"))).await.unwrap();
                complete
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap()
                    .send((count, bytes, reader_stalls))
                    .unwrap();
            });
            Ok(loopback::replies(ReceiverStream::new(rx)))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let closes = Arc::new(AtomicUsize::new(0));
    let options = SessionOptions {
        config,
        context: ContextInput::Static(context.clone()),
        timeout: Duration::from_secs(120),
        ..Default::default()
    };
    let source = Box::new(SyntheticAudio {
        chunks: chunks.clone(),
        index: 0,
        closes: closes.clone(),
    });
    let mut consumer_stalls = 0;
    let start = Instant::now();
    let mut session = engine.start(source, options).unwrap();
    let metrics: SessionMetrics;
    let finals;
    if mode.collect {
        let result = collect_session(session, CollectorLimits::default())
            .await
            .unwrap();
        assert_eq!(result.context, Some(context));
        finals = result.final_updates.len();
        assert_eq!(finals, chunks.len() / final_every);
        for (index, update) in result.final_updates.iter().enumerate() {
            assert_eq!(
                update.raw,
                responses[(index + 1) * final_every - 1].as_ref()
            );
            assert_eq!(update.hypotheses()[0].safety_status, Some(0));
            assert_eq!(update.unknown_fields.len(), 1);
        }
        result.check_safety().unwrap();
        metrics = result.metrics.unwrap();
    } else {
        let mut index = 0;
        let mut final_count = 0;
        let mut eof = 0;
        let mut final_metrics = None;
        while let Some(event) = session.next().await.unwrap() {
            if let Some(response) = event.response {
                assert_eq!(eof, 0);
                if index < responses.len() {
                    assert_eq!(response.raw, responses[index].as_ref());
                    final_count += usize::from(response.recognition_state() == Some(1));
                } else {
                    assert_eq!(response.raw.as_ref(), b"\x08\x01");
                }
                index += 1;
                if mode.consumer_stall && index % 64 == 0 {
                    consumer_stalls += 1;
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            } else {
                eof += 1;
                final_metrics = Some(event.metrics);
            }
        }
        assert_eq!(index, responses.len() + 1);
        assert_eq!(eof, 1);
        metrics = final_metrics.unwrap();
        finals = final_count;
        session.close().await.unwrap();
    }
    // Full local lifecycle joins are in the timed region. Connection/listener
    // establishment and fixture construction are intentionally outside it.
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    let (requests, request_bytes, reader_stalls) = completed.await.unwrap();
    server.close().await;
    let wall = start.elapsed();
    assert_eq!(closes.load(SeqCst), 1);
    assert_eq!(metrics.audio.chunks, chunks.len() as u64);
    assert_eq!(
        metrics.audio.bytes,
        chunks.iter().map(|b| b.len() as u64).sum::<u64>()
    );
    assert!(metrics.audio.eof && metrics.audio.cleanup_finished && metrics.audio.cleanup_succeeded);
    assert!(metrics.eof_seconds.is_some());
    assert_eq!(requests, chunks.len() + 3);
    assert_eq!(metrics.requests_produced, requests as u64);
    assert_eq!(metrics.request_protobuf_bytes, request_bytes as u64);
    assert_eq!(metrics.responses_received, responses.len() as u64 + 1);
    assert_eq!(
        metrics.response_protobuf_bytes,
        responses.iter().map(|b| b.len() as u64).sum::<u64>() + 2
    );
    assert_eq!(finals, chunks.len() / final_every);
    println!(
        "{}",
        serde_json::json!({"case":mode.name, "repeat":repeat, "wall_ns":wall.as_nanos(),
        "chunks":chunks.len(), "chunk_bytes":chunks[0].len(), "audio_bytes":metrics.audio.bytes,
        "requests":requests, "request_protobuf_bytes":request_bytes, "responses":metrics.responses_received,
        "response_protobuf_bytes":metrics.response_protobuf_bytes, "final_updates":finals,
        "reader_stalls":reader_stalls, "consumer_stalls":consumer_stalls, "stall_requested_ms":1,
        "source_close_calls":closes.load(SeqCst), "unique_engine_eof":true,
        "upload_eof_verified":true, "cleanup_joined":true})
    );
}

// Runtime rejection keeps debug test builds possible; timing requires release.
#[allow(clippy::assertions_on_constants)]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    assert!(!cfg!(debug_assertions), "use --release");
    let count = profile_support::arg("--chunks", 16_384);
    let repeats = profile_support::arg("--repeats", 5);
    let final_every = count.div_ceil(128);
    let chunks = Arc::new(
        (0..count)
            .map(|i| profile_support::audio(i, 640))
            .collect::<Vec<_>>(),
    );
    let responses = Arc::new(
        (0..count)
            .map(|i| profile_support::response(i, 128, (i + 1) % final_every == 0))
            .collect::<Vec<_>>(),
    );
    for raw in responses.iter() {
        let decoded = decode_response(raw).unwrap();
        assert_eq!(decoded.raw, raw.as_ref());
    }
    let modes = [
        Mode {
            name: "collect_active",
            collect: true,
            reader_stall: false,
            consumer_stall: false,
        },
        Mode {
            name: "observe_active",
            collect: false,
            reader_stall: false,
            consumer_stall: false,
        },
        Mode {
            name: "collect_reader_stall",
            collect: true,
            reader_stall: true,
            consumer_stall: false,
        },
        Mode {
            name: "observe_consumer_stall",
            collect: false,
            reader_stall: false,
            consumer_stall: true,
        },
    ];
    // Rotate order to reduce systematic warm-up/order bias.
    for repeat in 0..repeats {
        for offset in 0..modes.len() {
            let mode = modes[(repeat + offset) % modes.len()];
            tokio::time::timeout(
                Duration::from_secs(150),
                run(mode, repeat, chunks.clone(), responses.clone(), final_every),
            )
            .await
            .expect("bounded profiling deadline");
        }
    }
}
