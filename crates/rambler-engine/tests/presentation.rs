use rambler_engine::{Presentation, PresentationContent, RecognitionCompletion};
use rambler_protocol::{RECOGNITION_RESPONSE, decode_response};

fn varint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while n >= 128 {
        out.push((n as u8) | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
    out
}

fn bytes(field: u32, value: &[u8]) -> Vec<u8> {
    let mut out = varint(u64::from(field) << 3 | 2);
    out.extend(varint(value.len() as u64));
    out.extend(value);
    out
}

fn message(field: u32, body: &[u8], status: Option<u8>) -> Vec<u8> {
    let mut response = Vec::new();
    if let Some(status) = status {
        response.extend([8, status]);
    }
    response.extend(bytes(RECOGNITION_RESPONSE, &bytes(field, body)));
    response
}

#[test]
fn both_containers_preserve_order_duplicates_and_distinct_segment_states() {
    let mut hypothesis_container = Vec::new();
    for text in ["same", "same", "other"] {
        hypothesis_container.extend(bytes(3, &bytes(1, text.as_bytes())));
    }
    let mut partial_container = Vec::new();
    for text in ["draft", "draft"] {
        partial_container.extend(bytes(1, &bytes(1, text.as_bytes())));
    }
    for (state, expected) in [
        (None, RecognitionCompletion::Other),
        (Some(0), RecognitionCompletion::Other),
        (Some(1), RecognitionCompletion::Complete),
        (Some(7), RecognitionCompletion::Other),
    ] {
        let mut recognition = Vec::new();
        if let Some(state) = state {
            recognition.extend([8, state]);
        }
        recognition.extend(bytes(3, &hypothesis_container));
        recognition.extend(bytes(4, &partial_container));
        let wire = message(1, &recognition, None);
        let response = decode_response(&wire).unwrap();
        let view = Presentation::new(&response);
        assert!(std::ptr::eq(view.response, &response));
        assert_eq!(view.response.status, None);
        assert_eq!(view.response.raw, wire);
        let PresentationContent::Recognition {
            completion,
            result,
            has_final_segment,
            has_partials,
            ..
        } = view.content
        else {
            panic!("expected recognition");
        };
        assert_eq!(completion, expected);
        assert!(has_final_segment && has_partials);
        assert_eq!(result.state, state.map(i32::from));
        assert_eq!(result.hypotheses().len(), 3);
        assert_eq!(result.partials().len(), 2);
        assert_eq!(result.hypotheses()[0].text.as_deref(), Some("same"));
        assert_eq!(result.hypotheses()[1].text.as_deref(), Some("same"));
        assert_eq!(result.hypotheses()[2].text.as_deref(), Some("other"));
        assert_eq!(result.partials()[0].text.as_deref(), Some("draft"));
        assert_eq!(result.partials()[1].text.as_deref(), Some("draft"));
    }
}

#[test]
fn speech_opaque_end_and_absence_are_not_global_results() {
    let speech = decode_response(&message(2, &[8, 3], None)).unwrap();
    match Presentation::new(&speech).content {
        PresentationContent::Speech(event) => assert_eq!(event.event, Some(3)),
        _ => panic!("expected speech"),
    }
    let opaque = decode_response(&message(5, &[8, 17], Some(1))).unwrap();
    let view = Presentation::new(&opaque);
    assert_eq!(view.response.status, Some(1));
    match view.content {
        PresentationContent::Opaque { field, raw } => {
            assert_eq!(field, 5);
            assert_eq!(raw, Some(&[8, 17][..]));
        }
        _ => panic!("expected opaque oneof"),
    }
    let end = decode_response(&[8, 1]).unwrap();
    assert!(matches!(
        Presentation::new(&end).content,
        PresentationContent::None
    ));
    assert_eq!(Presentation::new(&end).response.status, Some(1));
    let absent = decode_response(&[]).unwrap();
    assert!(matches!(
        Presentation::new(&absent).content,
        PresentationContent::None
    ));
    assert_eq!(Presentation::new(&absent).response.status, None);
}

#[test]
fn final_segment_presence_is_independent_of_recognition_completion() {
    for (body, complete, has_segment) in [
        (vec![8, 1], true, false),
        ([vec![8, 0], bytes(3, &[])].concat(), false, true),
        (bytes(4, &[]), false, false),
    ] {
        let response = decode_response(&message(1, &body, None)).unwrap();
        let PresentationContent::Recognition {
            completion,
            has_final_segment,
            ..
        } = Presentation::new(&response).content
        else {
            panic!("recognition missing");
        };
        assert_eq!(completion == RecognitionCompletion::Complete, complete);
        assert_eq!(has_final_segment, has_segment);
    }
}

fn partials(parts: &[(Option<&str>, Option<f64>)]) -> rambler_protocol::PartialResult {
    rambler_protocol::PartialResult {
        partials: parts
            .iter()
            .map(|(text, confidence)| rambler_protocol::PartialHypothesis {
                text: text.map(str::to_owned),
                confidence: *confidence,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn partial_parts_join_without_separator_and_latch_instability() {
    use rambler_engine::PartialTranscript;
    let parts = partials(&[
        (Some("A😀"), Some(0.95)),
        (Some(" B"), Some(0.4)),
        (Some("C"), Some(1.0)),
    ]);
    let view = PartialTranscript::from_result(&parts);
    assert_eq!(view.stable_prefix, "A😀");
    assert_eq!(view.tentative_suffix, " BC");
    assert_eq!(view.text(), "A😀 BC");
    assert_eq!(view.parts.len(), 3);
    assert!(!view.has_nonfinite_scores);
    let ignored = partials(&[(None, Some(0.0)), (Some("A"), Some(0.9))]);
    assert_eq!(PartialTranscript::from_result(&ignored).stable_prefix, "A");
    let empty = partials(&[(Some(""), None), (Some("A"), Some(1.0))]);
    let view = PartialTranscript::from_result(&empty);
    assert!(view.stable_prefix.is_empty());
    assert_eq!(view.tentative_suffix, "A");
}

#[test]
fn partial_nan_infinity_and_missing_scores_preserve_exact_client_comparison() {
    use rambler_engine::PartialTranscript;
    let parts = partials(&[
        (Some("N"), Some(f64::NAN)),
        (Some("+"), Some(f64::INFINITY)),
        (Some("-"), Some(f64::NEG_INFINITY)),
        (Some("H"), Some(1.0)),
    ]);
    let view = PartialTranscript::from_result(&parts);
    assert_eq!(view.stable_prefix, "N+");
    assert_eq!(view.tentative_suffix, "-H");
    assert!(view.has_nonfinite_scores);
    assert!(view.parts[0].confidence.unwrap().is_nan());
    let absent = partials(&[(Some("zero"), None), (Some("N"), Some(f64::NAN))]);
    assert_eq!(
        PartialTranscript::from_result(&absent).tentative_suffix,
        "zeroN"
    );
    assert_eq!(PartialTranscript::from_result(&partials(&[])).text(), "");
}
