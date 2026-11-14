mod common;
use bytes::Bytes;
use common::*;
use futures_util::stream;
use rambler_engine::*;
use rambler_protocol::{
    RECOGNITION_RESPONSE, VOICE_EDITING_RESULT, decode_response,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};

fn hypothesis(text: &str, safety: u64) -> Vec<u8> {
    [
        s(1, text).unwrap(),
        v(30, safety).unwrap(),
        b(
            VOICE_EDITING_RESULT,
            &[v(4, 2).unwrap(), s(3, text).unwrap()].concat(),
        )
        .unwrap(),
    ]
    .concat()
}
fn response(recognition: &[u8]) -> S3Response {
    decode_response(&b(RECOGNITION_RESPONSE, &b(1, recognition).unwrap()).unwrap()).unwrap()
}
#[test]
fn safety_checks_all_structural_candidates_without_rendering_them() {
    for placement in 0..4 {
        for safety in [0, 1, 2] {
            let primary = b(3, &hypothesis("primary", 0)).unwrap();
            let other = hypothesis("OTHER MUST NOT RENDER", safety);
            let recognition = match placement {
                0 => [
                    v(1, 1).unwrap(),
                    b(3, &primary).unwrap(),
                    b(5, &b(3, &other).unwrap()).unwrap(),
                ]
                .concat(),
                1 => [
                    v(1, 1).unwrap(),
                    b(3, &primary).unwrap(),
                    b(7, &b(3, &other).unwrap()).unwrap(),
                ]
                .concat(),
                2 => [
                    v(1, 1).unwrap(),
                    b(
                        3,
                        &[primary.clone(), b(5, &b(1, &other).unwrap()).unwrap()].concat(),
                    )
                    .unwrap(),
                ]
                .concat(),
                _ => [
                    v(1, 1).unwrap(),
                    b(3, &primary).unwrap(),
                    b(7, &b(5, &b(1, &other).unwrap()).unwrap()).unwrap(),
                ]
                .concat(),
            };
            let result = DictationResult {
                final_updates: vec![response(&recognition)],
                context: Some(EditingContext {
                    before: "captured".into(),
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert_eq!(result.final_updates[0].hypotheses().len(), 1);
            assert_eq!(result.final_updates[0].all_hypotheses().count(), 2);
            for policy in [
                RenderPolicy::Strict,
                RenderPolicy::AsrOnly,
                RenderPolicy::S3ClientCompatible,
            ] {
                let rendered = result.render(policy);
                match safety {
                    0 => assert_eq!(rendered.unwrap().text, "primary"),
                    1 => assert!(matches!(rendered, Err(EngineError::SafetyBlocked))),
                    _ => assert!(matches!(rendered, Err(EngineError::UnrecognizedSafety))),
                }
                if safety != 0 {
                    let proposed = replacement_proposal_with_policy(&result, None, policy);
                    if safety == 1 {
                        assert!(matches!(proposed, Err(EngineError::SafetyBlocked)));
                    } else {
                        assert!(matches!(proposed, Err(EngineError::UnrecognizedSafety)));
                    }
                }
            }
        }
    }
}
#[tokio::test]
async fn segment_collection_keeps_field5_and_field7_updates_without_claiming_finality() {
    let first = response(
        &b(
            5,
            &b(5, &b(1, &hypothesis("field5 fallback", 0)).unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .raw;
    let second = response(
        &[
            v(1, 0).unwrap(),
            b(7, &b(3, &hypothesis("field7", 0)).unwrap()).unwrap(),
        ]
        .concat(),
    )
    .raw;
    let completion = response(&[v(1, 1).unwrap(), b(3, &[]).unwrap()].concat()).raw;
    let messages = vec![
        Bytes::from(first.clone()),
        Bytes::from_static(b"\x08\x00"),
        Bytes::from(second.clone()),
        Bytes::from(completion),
        Bytes::from_static(b"\x08\x01"),
    ];
    let server = TestServer::new(move |request| {
        let messages = messages.clone();
        async move {
            drain(request.into_inner()).await;
            Ok(replies(stream::iter(messages.into_iter().map(Ok))))
        }
    })
    .await;
    let transport = server.transport().await;
    let engine = Engine::new(transport.clone());
    let audio = Box::new(StreamAudio::new(stream::iter([Ok(Bytes::from_static(
        b"encoded controlled source",
    ))])));
    let result = bounded(collect_segments(
        &engine,
        audio,
        SessionOptions::default(),
        CollectorLimits::default(),
    ))
    .await
    .unwrap();
    assert_eq!(
        result
            .updates
            .iter()
            .map(|u| u.response_ordinal)
            .collect::<Vec<_>>(),
        [1, 3, 4]
    );
    assert_eq!(result.updates[0].response.raw, first);
    assert_eq!(result.updates[1].response.raw, second);
    assert_eq!(result.updates[0].response.recognition_state(), None);
    assert_eq!(result.updates[1].response.recognition_state(), Some(0));
    result.check_safety().unwrap();
    engine.close().await.unwrap();
    transport.close().await.unwrap();
    server.close().await;
}
