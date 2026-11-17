//! Real Engine/loopback-transport qualification of invented field-4 wire previews.
//! Reproduce after shared validation release:
//! `cargo test -p rambler-engine --test partial_stream_presentation`.
//! No real service, external network, audio, or device is used. This does not assert real
//! provider behavior, confidence semantics, cross-response preview accumulation,
//! or late-failure handling (already covered by engine/segments integration tests).

mod common;

use bytes::Bytes;
use common::*;
use futures_util::stream;
use rambler_engine::{
    Engine, EngineSession, PartialTranscript, Presentation, PresentationContent,
    RecognitionCompletion, S3Response, SessionOptions, StreamAudio,
};
use rambler_protocol::{
    RECOGNITION_RESPONSE,
    wire::{bytes_field as b, encode_varint, string_field as s, varint_field as v},
};
use std::sync::Arc;
use tokio::sync::Semaphore;

const FIRST_STABLE: &str = "Quill ";
const FIRST_TENTATIVE: &str = "brisk";
const SECOND_STABLE: &str = "Quill briskly ";
const SECOND_TENTATIVE: &str = "dances";
const FINAL_TEXT: &str = "Quill takes the lantern home.";

#[derive(Clone)]
struct Gate(Arc<Semaphore>);
impl Gate {
    fn new() -> Self {
        Self(Arc::new(Semaphore::new(0)))
    }
    fn open(&self) {
        self.0.add_permits(1);
    }
    async fn wait(&self) {
        self.0.acquire().await.unwrap().forget();
    }
}

fn double_field(number: u32, value: f64) -> Vec<u8> {
    let mut out = encode_varint(u64::from(number) << 3 | 1);
    out.extend(value.to_le_bytes());
    out
}

fn response(recognition: Vec<u8>) -> Bytes {
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}

fn partial(parts: &[(&str, f64)]) -> Bytes {
    let container = parts
        .iter()
        .map(|(text, confidence)| {
            let mut hypothesis = s(1, text).unwrap();
            hypothesis.extend(double_field(2, *confidence));
            b(1, &hypothesis).unwrap()
        })
        .collect::<Vec<_>>()
        .concat();
    response([v(1, 0).unwrap(), b(4, &container).unwrap()].concat())
}

fn final_recognition() -> Bytes {
    let hypothesis = s(1, FINAL_TEXT).unwrap();
    let hypotheses = b(3, &hypothesis).unwrap();
    response([v(1, 1).unwrap(), b(3, &hypotheses).unwrap()].concat())
}

fn replace_preview<'a>(response: &'a S3Response, displayed: &mut String) -> PartialTranscript<'a> {
    let PresentationContent::Recognition {
        completion,
        has_final_segment,
        has_partials,
        result,
        ..
    } = Presentation::new(response).content
    else {
        panic!("expected recognition preview");
    };
    assert_eq!(response.status, None);
    assert_eq!(completion, RecognitionCompletion::Other);
    assert_eq!(result.state, Some(0));
    assert!(!has_final_segment);
    assert!(has_partials);
    assert!(result.hypotheses().is_empty());
    let preview = PartialTranscript::from_result(result.partial_result.as_ref().unwrap());
    // Each response replaces the whole preview; parts are joined only within it.
    *displayed = preview.text();
    preview
}

async fn next_response(session: &mut EngineSession) -> S3Response {
    bounded(session.next())
        .await
        .unwrap()
        .unwrap()
        .response
        .unwrap()
}

#[tokio::test]
async fn ordered_wire_previews_replace_before_distinct_final_and_clean_eof() {
    let release_second = Gate::new();
    let release_final = Gate::new();
    let release_end = Gate::new();
    let server = TestServer::new({
        let release_second = release_second.clone();
        let release_final = release_final.clone();
        let release_end = release_end.clone();
        move |request| {
            let release_second = release_second.clone();
            let release_final = release_final.clone();
            let release_end = release_end.clone();
            async move {
                drain(request.into_inner()).await;
                let frames = vec![
                    (
                        partial(&[(FIRST_STABLE, 0.97), (FIRST_TENTATIVE, 0.82)]),
                        None,
                    ),
                    (
                        partial(&[
                            (FIRST_STABLE, 0.99),
                            ("briskly ", 0.95),
                            (SECOND_TENTATIVE, 0.71),
                        ]),
                        Some(release_second),
                    ),
                    (final_recognition(), Some(release_final)),
                    (Bytes::from_static(b"\x08\x01"), Some(release_end)),
                ];
                Ok(replies(stream::unfold(
                    frames.into_iter(),
                    |mut frames| async move {
                        let (frame, gate) = frames.next()?;
                        if let Some(gate) = gate {
                            gate.wait().await;
                        }
                        Some((Ok(frame), frames))
                    },
                )))
            }
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let audio = StreamAudio::new(stream::iter([Ok(Bytes::from_static(
        b"invented encoded fixture",
    ))]));
    let mut session = engine
        .start(Box::new(audio), SessionOptions::default())
        .unwrap();
    let mut displayed = String::new();

    let first_response = next_response(&mut session).await;
    let first = replace_preview(&first_response, &mut displayed);
    assert_eq!(first.stable_prefix, FIRST_STABLE);
    assert_eq!(first.tentative_suffix, FIRST_TENTATIVE);
    assert_eq!(displayed, "Quill brisk");
    release_second.open();

    let second_response = next_response(&mut session).await;
    let second = replace_preview(&second_response, &mut displayed);
    assert_eq!(second.stable_prefix, SECOND_STABLE);
    assert_eq!(second.tentative_suffix, SECOND_TENTATIVE);
    assert_eq!(displayed, "Quill briskly dances");
    assert_ne!(displayed, "Quill briskQuill briskly dances");
    release_final.open();

    let final_response = next_response(&mut session).await;
    let PresentationContent::Recognition {
        completion,
        has_final_segment,
        has_partials,
        result,
        ..
    } = Presentation::new(&final_response).content
    else {
        panic!("expected final recognition");
    };
    assert_eq!(final_response.status, None);
    assert_eq!(completion, RecognitionCompletion::Complete);
    assert_eq!(result.state, Some(1));
    assert!(has_final_segment);
    assert!(!has_partials);
    assert_eq!(result.hypotheses().len(), 1);
    assert_eq!(result.hypotheses()[0].text.as_deref(), Some(FINAL_TEXT));
    assert_ne!(displayed, FINAL_TEXT);
    release_end.open();

    // S3 END is another response, not the unique successful gRPC EOF event.
    let end = next_response(&mut session).await;
    assert_eq!(end.status, Some(1));
    assert!(matches!(
        Presentation::new(&end).content,
        PresentationContent::None
    ));
    let eof = bounded(session.next()).await.unwrap().unwrap();
    assert!(eof.response.is_none());
    assert_eq!(eof.metrics.responses_received, 4);
    assert!(bounded(session.next()).await.unwrap().is_none());

    bounded(engine.close()).await.unwrap();
    bounded(transport.close()).await.unwrap();
    bounded(server.close()).await;
}
