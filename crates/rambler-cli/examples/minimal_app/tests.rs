//! Synthetic WAVs and independent response/upload oracles; no host credentials.
use super::*;
use crate::common::{TestServer, bounded, drain, replies};
use futures_util::stream;
use rambler_protocol::{
    AUDIO_DATA, AUDIO_INFO, RECOGNITION_RESPONSE, VOICE_EDITING_RESULT, context_request,
    end_request, initial_request,
    wire::{Field, Value, bytes_field as b, parse_fields, string_field as s, varint_field as v},
};
use std::{io::Cursor, sync::Mutex};
use tonic::Status;

const SAMPLES: usize = 4099; // Multiple writes, with a one-sample tail at 48 kHz.

fn sample_wav(hz: u32) -> Vec<u8> {
    let mut wav = b"RIFF\0\0\0\0WAVEJUNK\x01\0\0\0x\0fmt \x10\0\0\0\x01\0\x01\0".to_vec();
    wav.extend(hz.to_le_bytes());
    wav.extend((hz * 2).to_le_bytes());
    wav.extend(b"\x02\0\x10\0data");
    wav.extend((2 * SAMPLES as u32).to_le_bytes());
    wav.extend((0..SAMPLES).flat_map(|n| ((n % 37) as i16 * 250 - 4500).to_le_bytes()));
    let riff_len = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff_len.to_le_bytes());
    wav
}

fn recognition(state: u64, member: Vec<u8>) -> Bytes {
    let recognition = [v(1, state).unwrap(), member].concat();
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}

fn preview(text: &str) -> Bytes {
    recognition(0, b(4, &b(1, &s(1, text).unwrap()).unwrap()).unwrap())
}

fn final_response(rewrite: bool) -> Bytes {
    let mut hypothesis = s(1, "The kite stays aloft.").unwrap();
    if rewrite {
        let returned_window = [
            s(1, "🪁 The kite is ").unwrap(),
            s(2, "green").unwrap(),
            s(3, ". Marker stays.").unwrap(),
        ]
        .concat();
        let edit = [v(4, 3).unwrap(), b(7, &returned_window).unwrap()].concat();
        hypothesis.extend(b(VOICE_EDITING_RESULT, &edit).unwrap());
    }
    recognition(1, b(3, &b(3, &hypothesis).unwrap()).unwrap())
}

fn verify_upload(messages: &[Bytes], pcm16_samples: usize) -> Vec<Bytes> {
    assert!(messages.len() >= 4);
    assert_eq!(
        messages[0],
        initial_request(&Default::default(), 0).unwrap()
    );
    let [context, end] = &messages[messages.len() - 2..] else {
        unreachable!()
    };
    assert_eq!(*context, context_request(&demo_window().0).unwrap());
    assert_eq!(end.as_ref(), end_request().as_slice());

    let mut ogg = Vec::new();
    let mut chunks = Vec::new();
    for message in &messages[1..messages.len() - 2] {
        let fields = parse_fields(message).unwrap();
        let [
            Field {
                number: AUDIO_INFO, ..
            },
            Field {
                number: AUDIO_DATA,
                value: Value::Bytes(data),
                ..
            },
        ] = fields.as_slice()
        else {
            panic!("expected an audio message");
        };
        let fields = parse_fields(data).unwrap();
        let [
            Field {
                number: 1,
                value: Value::Bytes(chunk),
                ..
            },
        ] = fields.as_slice()
        else {
            panic!("expected an Ogg audio chunk");
        };
        ogg.extend_from_slice(chunk);
        chunks.push(Bytes::copy_from_slice(chunk));
    }
    let mut ogg = ogg::reading::PacketReader::new(Cursor::new(ogg));
    let head = ogg.read_packet().unwrap().unwrap();
    assert!(head.data.starts_with(b"OpusHead"));
    let pre_skip = u16::from_le_bytes(head.data[10..12].try_into().unwrap()) as u64;
    assert!(
        ogg.read_packet()
            .unwrap()
            .unwrap()
            .data
            .starts_with(b"OpusTags")
    );
    let mut end_granule = None;
    while let Some(packet) = ogg.read_packet().unwrap() {
        assert!(end_granule.is_none());
        assert!(!packet.data.is_empty());
        if packet.last_in_stream() {
            end_granule = Some(packet.absgp_page());
        }
    }
    // The Ogg/Opus timebase is always 48 kHz, independent of input PCM rate.
    assert_eq!(end_granule.unwrap() - pre_skip, pcm16_samples as u64 * 3);
    chunks
}

#[tokio::test]
async fn wav_to_strict_text_and_compatible_review_on_owned_loopback() {
    for (hz, pcm16_samples, rewrite) in [(16_000, SAMPLES, false), (48_000, 1367, true)] {
        let server = TestServer::new(move |request| async move {
            assert!(!request.metadata().contains_key("x-goog-api-key"));
            verify_upload(&drain(request.into_inner()).await, pcm16_samples);
            Ok(replies(stream::iter([
                Ok(preview("The kite")),
                Ok(preview("The kite stays")),
                Ok(final_response(rewrite)),
                Ok(Bytes::from_static(b"\x08\x01")),
            ])))
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.wav");
        std::fs::write(&path, sample_wav(hz)).unwrap();
        let bytes = wav::read(&path).await.unwrap();
        let wav = Wav::parse(&bytes).unwrap();
        let (window, revision) = demo_window();
        let mut current_preview = None;
        let mut previous_previews = Vec::new();
        let engine = bounded(Engine::connect_ready(
            TransportConfig::loopback(server.address).unwrap(),
        ))
        .await
        .unwrap();
        let result = bounded(collect_wav(&engine, None, wav, &window, None, |next| {
            previous_previews.push(std::mem::replace(&mut current_preview, next))
        }))
        .await
        .unwrap();
        assert_eq!(current_preview, None);
        assert_eq!(
            previous_previews,
            [None, Some("The kite".into()), Some("The kite stays".into())]
        );
        assert_eq!(result.context.as_ref(), Some(&window));
        let review = propose_for_review(&result, revision.clone(), &window, &revision);
        if rewrite {
            assert!(matches!(result.text(), Err(EngineError::AmbiguousResult)));
            let review = review.unwrap();
            assert_eq!(review.splice, Some((15..19, "green".into())));
            assert_eq!(review.resulting_selection, Some((15, 20)));
            assert!(matches!(
                propose_for_review(&result, revision, &window, &Revision::from(2_i64)),
                Err(EngineError::ContextConflict)
            ));
            assert_eq!(window.selected, "blue");
        } else {
            assert_eq!(result.text().unwrap(), "The kite stays aloft.");
            assert!(matches!(review, Err(EngineError::UnsupportedEdit)));
        }
        bounded(engine.close()).await.unwrap();
        bounded(server.close()).await;
        dir.close().unwrap();
    }
}

#[tokio::test]
async fn wav_retry_requires_approval_and_reuses_encoded_pages() {
    for allow in [false, true] {
        let uploads = Arc::new(Mutex::new(Vec::new()));
        let server = TestServer::new({
            let uploads = uploads.clone();
            move |request| {
                let uploads = uploads.clone();
                async move {
                    let chunks = verify_upload(&drain(request.into_inner()).await, SAMPLES);
                    let mut uploads = uploads.lock().unwrap();
                    uploads.push(chunks);
                    if uploads.len() == 1 {
                        Err(Status::unavailable("synthetic failure"))
                    } else {
                        Ok(replies(stream::iter([
                            Ok(final_response(false)),
                            Ok(Bytes::from_static(b"\x08\x01")),
                        ])))
                    }
                }
            }
        })
        .await;
        let engine = bounded(Engine::connect_ready(
            TransportConfig::loopback(server.address).unwrap(),
        ))
        .await
        .unwrap();
        let bytes = sample_wav(16_000);
        let approve: fn() -> bool = if allow { || true } else { || false };
        let result = bounded(collect_wav(
            &engine,
            None,
            Wav::parse(&bytes).unwrap(),
            &demo_window().0,
            Some(approve),
            |_| {},
        ))
        .await;
        if allow {
            assert_eq!(result.unwrap().text().unwrap(), "The kite stays aloft.");
            let uploads = uploads.lock().unwrap();
            assert_eq!(uploads.len(), 2);
            assert_eq!(uploads[0], uploads[1]);
        } else {
            assert!(result.is_err());
            assert_eq!(uploads.lock().unwrap().len(), 1);
        }
        assert_eq!(server.accepted_connections(), 1);
        bounded(engine.close()).await.unwrap();
        bounded(server.close()).await;
    }
}

#[test]
fn rejects_truncation_wrong_rate_and_stereo() {
    let valid = sample_wav(48_000);
    assert!(Wav::parse(&valid[..valid.len() - 1]).is_err());
    assert!(Wav::parse(&sample_wav(44_100)).is_err());
    let mut stereo = valid;
    stereo[32] = 2; // fmt tag follows the odd-sized, padded JUNK chunk.
    assert!(Wav::parse(&stereo).is_err());
}
