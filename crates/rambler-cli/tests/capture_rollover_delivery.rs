//! Capture handoff and segment delivery exercised together on an owned peer.
#![cfg(feature = "audio")]

#[path = "../../rambler-engine/tests/common/mod.rs"]
mod fixture;

use bytes::Bytes;
use fixture::{TestServer, bounded, drain, replies};
use futures_util::stream;
use rambler_audio::{
    CaptureConfig, CompletedDictationSegment, EncoderConfig, LongDictationOptions, Pcm16Opus,
    capture_channel, stream_long_pcm_dictation,
};
use rambler_engine::{AudioSource, CollectorLimits, Engine, SessionOptions, StreamAudio};
use rambler_protocol::{
    AUDIO_DATA, RECOGNITION_RESPONSE, end_request,
    wire::{Value, bytes_field as b, parse_fields, string_field as s, varint_field as v},
};
use rambler_transport::TransportConfig;
use std::{
    array,
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

const BLOCK: usize = 640; // PCM16LE, 20 ms at 16 kHz.
const SEGMENT: usize = 30 * BLOCK;
const SAMPLES: u64 = SEGMENT as u64 / 2;

fn oga(upload: &[Bytes]) -> Vec<u8> {
    assert!(upload.len() >= 4);
    assert_eq!(upload.last().unwrap().as_ref(), end_request());
    let mut bytes = Vec::new();
    for message in &upload[1..upload.len() - 1] {
        let fields = parse_fields(message).unwrap();
        let field = fields
            .iter()
            .find(|field| field.number == AUDIO_DATA)
            .unwrap();
        let Value::Bytes(audio) = &field.value else {
            panic!("not an encoded audio field");
        };
        let child = parse_fields(audio).unwrap();
        assert_eq!(child[0].number, 1);
        let Value::Bytes(payload) = &child[0].value else {
            panic!("not an Ogg payload");
        };
        bytes.extend_from_slice(payload);
    }
    bytes
}

fn packets(ogg: &[u8]) -> Vec<(Vec<u8>, u64, bool)> {
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(ogg));
    let mut result = Vec::new();
    while let Some(packet) = reader.read_packet().unwrap() {
        let eos = packet.last_in_stream();
        let granule = packet.absgp_page();
        result.push((packet.data, granule, eos));
    }
    assert!(result.last().unwrap().2);
    result
}

async fn reference(pcm: &[u8]) -> Vec<u8> {
    let pcm = Bytes::copy_from_slice(pcm);
    let mut source = Pcm16Opus::new(
        StreamAudio::new(stream::iter([Ok(pcm)])),
        EncoderConfig::default(),
    );
    let mut result = Vec::new();
    while let Some(page) = source.next().await.unwrap() {
        result.extend_from_slice(&page);
    }
    source.close().await.unwrap();
    result
}

#[tokio::test]
async fn callback_handoff_preserves_three_rollovers_and_delivers_owned_finals() {
    bounded(async {
        let pcm = (0..(3 * SEGMENT / 2))
            .flat_map(|i| (((i * 193 % 13_001) as i16) - 6500).to_le_bytes())
            .collect::<Vec<u8>>();
        let uploads: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
        let peer_uploads = uploads.clone();
        let request_count = Arc::new(AtomicUsize::new(0));
        let peer_count = request_count.clone();
        let ready = Arc::new(array::from_fn::<_, 2, _>(|_| Notify::new()));
        let release = Arc::new(array::from_fn::<_, 2, _>(|_| Notify::new()));
        let peer_ready = ready.clone();
        let peer_release = release.clone();
        let peer = TestServer::new(move |request| {
            let index = peer_count.fetch_add(1, Ordering::SeqCst);
            let uploads = peer_uploads.clone();
            let ready = peer_ready.clone();
            let release = peer_release.clone();
            async move {
                assert!(index < 3, "no extra session or replay");
                assert!(!request.metadata().contains_key("x-goog-api-key"));
                let encoded = oga(&drain(request.into_inner()).await);
                uploads.lock().unwrap().push(encoded);
                if index < ready.len() {
                    ready[index].notify_one();
                    release[index].notified().await;
                }
                let text = format!("Validated segment {index}.");
                let hypothesis = b(3, &s(1, &text).unwrap()).unwrap();
                let recognition = [v(1, 1).unwrap(), b(3, &hypothesis).unwrap()].concat();
                let final_response = b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap();
                Ok(replies(stream::iter([
                    Ok(Bytes::from(final_response)),
                    Ok(Bytes::from_static(b"\x08\x01")),
                ])))
            }
        })
        .await;
        let engine = Engine::connect(TransportConfig::loopback(peer.address).unwrap())
            .await
            .unwrap();
        let (mut writer, reader) = capture_channel(CaptureConfig {
            max_chunks: 31,
            max_chunk_bytes: BLOCK,
        })
        .unwrap();
        let delivered_count = AtomicUsize::new(0);
        let mut finals = Vec::new();
        let mut sink = |segment: CompletedDictationSegment| {
            assert_eq!(
                segment.segment_index,
                delivered_count.fetch_add(1, Ordering::SeqCst)
            );
            finals.push((segment.sample_range, segment.result.text().unwrap()));
        };
        let capture = async {
            for index in 0..3 {
                if index != 0 {
                    ready[index - 1].notified().await;
                    // The previous RPC has seen END and half-close, but its
                    // final response is deliberately stalled. A callback
                    // supplies the next 600 ms without waiting on the reader.
                    assert_eq!(delivered_count.load(Ordering::SeqCst), index - 1);
                }
                for block in pcm[index * SEGMENT..(index + 1) * SEGMENT].chunks_exact(BLOCK) {
                    writer.offer(block).unwrap();
                }
                if index != 0 {
                    release[index - 1].notify_one();
                }
            }
            writer.finish().unwrap();
        };
        let collect = stream_long_pcm_dictation(
            &engine,
            reader,
            SessionOptions::default(),
            CollectorLimits::default(),
            LongDictationOptions {
                segment_budget: Duration::from_millis(600),
                pause_search: Duration::ZERO,
            },
            &mut sink,
        );
        let ((), result) = tokio::join!(capture, collect);
        result.unwrap();
        assert_eq!(request_count.load(Ordering::SeqCst), 3);
        assert_eq!(peer.accepted_connections(), 1);
        assert_eq!(finals.len(), 3);
        for (index, (range, text)) in finals.into_iter().enumerate() {
            assert_eq!(range, index as u64 * SAMPLES..(index as u64 + 1) * SAMPLES);
            assert_eq!(text, format!("Validated segment {index}."));
        }
        let captured = std::mem::take(&mut *uploads.lock().unwrap());
        assert_eq!(captured.len(), 3);
        let mut serials = std::collections::BTreeSet::new();
        for (index, encoded) in captured.iter().enumerate() {
            assert!(serials.insert(encoded[14..18].to_vec()));
            let expected = reference(&pcm[index * SEGMENT..(index + 1) * SEGMENT]).await;
            assert_eq!(packets(encoded), packets(&expected));
        }
        engine.close().await.unwrap();
        peer.close().await;
    })
    .await;
}
