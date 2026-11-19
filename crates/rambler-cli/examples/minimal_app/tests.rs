//! Synthetic WAVs and independent response/upload oracles; no host credentials.
use super::*;
use crate::common::{TestServer, bounded, drain, replies};
use futures_util::stream;
use rambler_protocol::{
    AUDIO_DATA, AUDIO_INFO, RECOGNITION_RESPONSE, VOICE_EDITING_RESULT, context_request,
    decode_response, end_request, initial_request,
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

fn rewrite_final(before: &str, selected: &str, after: &str) -> Bytes {
    let returned_window = [
        s(1, before).unwrap(),
        s(2, selected).unwrap(),
        s(3, after).unwrap(),
    ]
    .concat();
    let edit = [v(4, 3).unwrap(), b(7, &returned_window).unwrap()].concat();
    let hypothesis = [
        s(1, "The kite stays aloft.").unwrap(),
        b(VOICE_EDITING_RESULT, &edit).unwrap(),
    ]
    .concat();
    recognition(1, b(3, &b(3, &hypothesis).unwrap()).unwrap())
}

fn final_response(rewrite: bool) -> Bytes {
    if rewrite {
        rewrite_final("🪁 The kite is ", "green", ". Marker stays.")
    } else {
        let hypothesis = s(1, "The kite stays aloft.").unwrap();
        recognition(1, b(3, &b(3, &hypothesis).unwrap()).unwrap())
    }
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
    assert_eq!(
        *context,
        context_request(&demo_editor().window().unwrap()).unwrap()
    );
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
        let editor = demo_editor();
        let captured = editor.capture().unwrap();
        let mut current_preview = None;
        let mut previous_previews = Vec::new();
        let engine = bounded(Engine::connect_ready(
            TransportConfig::loopback(server.address).unwrap(),
        ))
        .await
        .unwrap();
        let result = bounded(collect_wav(
            &engine,
            None,
            wav,
            &captured.window,
            None,
            |next| previous_previews.push(std::mem::replace(&mut current_preview, next)),
        ))
        .await
        .unwrap();
        assert_eq!(current_preview, None);
        assert_eq!(
            previous_previews,
            [None, Some("The kite".into()), Some("The kite stays".into())]
        );
        assert_eq!(result.context.as_ref(), Some(&captured.window));
        let review = propose_for_review(&result, Some(&captured), &editor);
        if rewrite {
            assert!(matches!(result.text(), Err(EngineError::AmbiguousResult)));
            assert_eq!(
                review.unwrap(),
                Some(Review::Anchored {
                    splice: Some((22..26, "green".into())),
                    resulting_selection: Some((15, 20)),
                })
            );
            assert_eq!(editor.window().unwrap().selected, "blue");
        } else {
            assert_eq!(result.text().unwrap(), "The kite stays aloft.");
            assert!(matches!(review, Err(EngineError::UnsupportedEdit)));
        }
        bounded(engine.close()).await.unwrap();
        bounded(server.close()).await;
        dir.close().unwrap();
    }
}

fn styled_editor() -> EditorSnapshot<'static> {
    EditorSnapshot {
        target: EditorTarget {
            document_id: 11,
            // The astral prefix is five UTF-8 bytes but only three UTF-16 units.
            window_bytes: 5..16,
            selection_bytes: 5..11,
        },
        revision: Revision::from(7_i64),
        document: "🧭[Paris Paris][Paris Paris]",
    }
}

fn mocked_rewrite(
    captured: &CapturedEdit,
    before: &str,
    selected: &str,
    after: &str,
) -> DictationResult {
    DictationResult {
        final_updates: vec![decode_response(&rewrite_final(before, selected, after)).unwrap()],
        context: Some(captured.window.clone()),
        ..Default::default()
    }
}

#[test]
fn anchored_deletion_preserves_the_second_paris_and_its_link() {
    let editor = styled_editor();
    let captured = editor.capture().unwrap();
    let completed = mocked_rewrite(&captured, "", "", "Paris");
    let Some(Review::Anchored {
        splice: Some((range, text)),
        ..
    }) = propose_for_review(&completed, Some(&captured), &editor).unwrap()
    else {
        panic!("expected a selection-anchored deletion");
    };
    assert_eq!(range, 3..9); // Window-local 0..6, translated to this document.
    assert!(text.is_empty());
    // Mock an exact-range editor deletion, tracking one style per UTF-16 unit.
    let apply = |range| {
        let mut styled: Vec<_> = editor
            .document
            .encode_utf16()
            .enumerate()
            .map(|(i, unit)| {
                (
                    unit,
                    if (9..14).contains(&i) {
                        Some("link")
                    } else {
                        None
                    },
                )
            })
            .collect();
        styled.drain(range);
        let (units, styles): (Vec<_>, Vec<_>) = styled.into_iter().unzip();
        (String::from_utf16(&units).unwrap(), styles)
    };
    let (result, styles) = apply(range);
    assert_eq!(result, "🧭[Paris][Paris Paris]");
    assert_eq!(&styles[3..8], &[Some("link"); 5]);

    // General textual diff would choose window-local 5..11: same characters,
    // wrong occurrence. The rich-edit code path must never fall back to it.
    let (general_result, general_styles) = apply(8..14);
    assert_eq!(general_result, result);
    assert_eq!(&general_styles[3..8], &[None; 5]);
}

#[test]
fn transaction_rejects_wrong_field_window_selection_cursor_or_revision() {
    let editor = styled_editor();
    let captured = editor.capture().unwrap();
    let completed = mocked_rewrite(&captured, "", "", "Paris");
    let reject = |current: &EditorSnapshot<'_>| {
        assert!(matches!(
            propose_for_review(&completed, Some(&captured), current),
            Err(EngineError::ContextConflict)
        ));
    };

    let mut other = editor.clone();
    other.target.document_id += 1;
    assert_eq!(other.window().unwrap(), captured.window);
    assert_eq!(other.revision, captured.revision);
    reject(&other);

    let mut moved_window = editor.clone();
    moved_window.target.window_bytes = 18..29;
    moved_window.target.selection_bytes = 18..24;
    assert_eq!(moved_window.window().unwrap(), captured.window);
    reject(&moved_window);

    for selection in [6..12, 11..11] {
        let mut moved = editor.clone();
        moved.target.selection_bytes = selection;
        reject(&moved);
    }

    let mut revised = editor.clone();
    revised.revision = Revision::from(8_i64);
    reject(&revised); // Same visible text, newer document revision.

    let mut changed_window = editor.clone();
    changed_window.document = "🧭[Paris Lyon!][Paris Paris]";
    reject(&changed_window); // Re-read the actual document, not the captured split.

    assert_eq!(propose_for_review(&completed, None, &editor).unwrap(), None);
}

#[test]
fn rewrite_without_proven_selection_scope_is_for_review_only() {
    let editor = styled_editor();
    let captured = editor.capture().unwrap();
    // Same final text as the verified deletion, but its structured split does
    // not preserve the captured surroundings. It cannot establish a range.
    let completed = mocked_rewrite(&captured, "Paris", "", "");
    assert_eq!(
        propose_for_review(&completed, Some(&captured), &editor).unwrap(),
        Some(Review::Unanchored {
            proposed_window: "Paris".into()
        })
    );
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
            &demo_editor().window().unwrap(),
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
