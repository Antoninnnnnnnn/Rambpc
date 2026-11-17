//! Independent byte/model fixtures for selected oneof runs; no production wire
//! encoder/parser is used as an oracle. Broader reducers/cut/8MiB tests stay intact.
use rambler_protocol::{
    wire::{Field, Value},
    *,
};

fn uint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 127) as u8;
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 128 });
        if value == 0 {
            return out;
        }
    }
}
fn scalar(number: u32, value: u64) -> Vec<u8> {
    [uint(u64::from(number) * 8), uint(value)].concat()
}
fn blob(number: u32, body: &[u8]) -> Vec<u8> {
    [
        uint(u64::from(number) * 8 + 2),
        uint(body.len() as u64),
        body.to_vec(),
    ]
    .concat()
}
fn varint_unknown(number: u32, value: u64) -> Field {
    Field {
        number,
        wire_type: 0,
        value: Value::Varint(value),
        raw: scalar(number, value),
    }
}
fn bytes_unknown(number: u32, body: &[u8]) -> Field {
    Field {
        number,
        wire_type: 2,
        value: Value::Bytes(body.to_vec()),
        raw: blob(number, body),
    }
}
fn result_wire(body: &[u8]) -> Vec<u8> {
    blob(RECOGNITION_RESPONSE, body)
}
fn edit_wire(body: &[u8]) -> Vec<u8> {
    result_wire(&blob(
        1,
        &blob(3, &blob(3, &blob(VOICE_EDITING_RESULT, body))),
    ))
}
fn result_case(body: Vec<u8>, mut result: RecognitionResponse) -> (Vec<u8>, S3Response) {
    let raw = result_wire(&body);
    result.raw = body.into();
    let expected = S3Response {
        raw: raw.clone().into(),
        result: Some(result),
        ..Default::default()
    };
    (raw, expected)
}
fn recognition_case(body: Vec<u8>, mut recognition: RecognitionResult) -> (Vec<u8>, S3Response) {
    recognition.raw = body.clone().into();
    result_case(
        blob(1, &body),
        RecognitionResponse {
            active_field: Some(1),
            active_raw: Some(body.into()),
            recognition: Some(recognition),
            ..Default::default()
        },
    )
}
fn edit_case(parts: &[Vec<u8>], mut edit: VoiceEditResult) -> (Vec<u8>, S3Response) {
    edit.raw = parts.concat().into();
    let hypothesis_raw: Vec<_> = parts
        .iter()
        .flat_map(|p| blob(VOICE_EDITING_RESULT, p))
        .collect();
    let list_raw = blob(3, &hypothesis_raw);
    recognition_case(
        blob(3, &list_raw),
        RecognitionResult {
            hypothesis_result: Some(HypothesisResult {
                raw: list_raw.into(),
                hypotheses: vec![Hypothesis {
                    voice_edit: Some(edit),
                    raw: hypothesis_raw.into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
    )
}
fn check((raw, expected): (Vec<u8>, S3Response)) -> S3Response {
    let actual = decode_response(&raw).unwrap();
    assert_eq!(actual, expected, "wire {raw:?}");
    // EditingContext equality intentionally excludes raw. Check it separately.
    for (actual, expected) in actual.all_hypotheses().zip(expected.all_hypotheses()) {
        if let Some(context) = expected
            .voice_edit
            .as_ref()
            .and_then(|e| e.edited_context.as_ref())
        {
            let actual = actual
                .voice_edit
                .as_ref()
                .unwrap()
                .edited_context
                .as_ref()
                .unwrap();
            assert_eq!(actual.raw, context.raw);
        }
    }
    actual
}
fn inside(root: &bytes::Bytes, child: &bytes::Bytes) -> bool {
    let start = root.as_ptr() as usize;
    let child_start = child.as_ptr() as usize;
    child_start >= start && child_start + child.len() <= start + root.len()
}

#[test]
fn literal_empty_and_all_selected_singletons_have_explicit_full_models() {
    assert_eq!(uint(1253625 * 8 + 2), [0xca, 0x8f, 0xe4, 0x04]);
    assert_eq!(blob(1, &[8, 1]), [0x0a, 2, 8, 1]);
    check((vec![], S3Response::default()));
    check(result_case(vec![], RecognitionResponse::default()));
    let literal = vec![0xca, 0x8f, 0xe4, 0x04, 4, 0x0a, 2, 8, 1];
    let (raw, expected) = recognition_case(
        vec![8, 1],
        RecognitionResult {
            state: Some(1),
            ..Default::default()
        },
    );
    assert_eq!(raw, literal);
    let decoded = check((raw, expected));
    assert_ne!(decoded.raw.as_ptr(), literal.as_ptr());
    let result = decoded.result.as_ref().unwrap();
    let recognition = result.recognition.as_ref().unwrap();
    assert!(inside(&decoded.raw, &result.raw));
    assert!(inside(&decoded.raw, &recognition.raw));
    assert_eq!(
        result.active_raw.as_ref().unwrap().as_ptr(),
        recognition.raw.as_ptr()
    );

    for member in [1, 2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 18, 19, 20, 21] {
        for empty in [false, true] {
            let body = if empty {
                vec![]
            } else if member <= 2 {
                scalar(1, 3)
            } else {
                vec![0xff, 8]
            };
            let mut expected = RecognitionResponse {
                active_field: Some(member),
                active_raw: Some(body.clone().into()),
                ..Default::default()
            };
            match member {
                1 => {
                    expected.recognition = Some(RecognitionResult {
                        state: if empty { None } else { Some(3) },
                        raw: body.clone().into(),
                        ..Default::default()
                    })
                }
                2 => {
                    expected.speech = Some(SpeechEventResult {
                        event: if empty { None } else { Some(3) },
                        raw: body.clone().into(),
                        ..Default::default()
                    })
                }
                _ => expected.unknown_fields = vec![bytes_unknown(member, &body)],
            }
            check(result_case(blob(member, &body), expected));
        }
    }
    let unknown = bytes_unknown(14, &[0xff, 8]);
    let wrong = varint_unknown(2, 9);
    check(result_case(
        [
            blob(3, b""),
            blob(4, b"id"),
            unknown.raw.clone(),
            wrong.raw.clone(),
        ]
        .concat(),
        RecognitionResponse {
            language: Some(String::new()),
            spoken_language: Some("id".into()),
            unknown_fields: vec![unknown, wrong],
            ..Default::default()
        },
    ));
}

#[test]
fn same_member_merges_across_languages_unknowns_and_all_wrong_wires() {
    let hypothesis = blob(1, b"A");
    let list = blob(3, &hypothesis);
    let first = [scalar(1, 1), scalar(88, 5)].concat();
    let second = [blob(3, &list), scalar(89, 6)].concat();
    let fixed64 = Field {
        number: 1,
        wire_type: 1,
        value: Value::Fixed64([0xff; 8]),
        raw: [vec![9], vec![0xff; 8]].concat(),
    };
    let fixed32 = Field {
        number: 2,
        wire_type: 5,
        value: Value::Fixed32([0xff; 4]),
        raw: [vec![21], vec![0xff; 4]].concat(),
    };
    let unknown = vec![
        bytes_unknown(14, &[0xff, 8]),
        varint_unknown(2, 9),
        fixed64,
        fixed32,
    ];
    let mut body = [blob(1, &first), blob(3, b"id")].concat();
    for field in &unknown {
        body.extend_from_slice(&field.raw);
    }
    body.extend(blob(1, &second));
    let merged = [first, second].concat();
    let decoded = check(result_case(
        body,
        RecognitionResponse {
            active_field: Some(1),
            active_raw: Some(merged.clone().into()),
            language: Some("id".into()),
            recognition: Some(RecognitionResult {
                state: Some(1),
                raw: merged.into(),
                unknown_fields: vec![varint_unknown(88, 5), varint_unknown(89, 6)],
                hypothesis_result: Some(HypothesisResult {
                    raw: list.into(),
                    hypotheses: vec![Hypothesis {
                        text: Some("A".into()),
                        raw: hypothesis.into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            unknown_fields: unknown,
            ..Default::default()
        },
    ));
    let result = decoded.result.as_ref().unwrap();
    let recognition = result.recognition.as_ref().unwrap();
    assert!(!inside(&decoded.raw, &recognition.raw));
    assert_eq!(
        result.active_raw.as_ref().unwrap().as_ptr(),
        recognition.raw.as_ptr()
    );

    for member in [2, 5] {
        let first = scalar(1, 1);
        let second = scalar(1, 3);
        let wrong = varint_unknown(2, 9);
        let mut expected = RecognitionResponse {
            active_field: Some(member),
            active_raw: Some([first.clone(), second.clone()].concat().into()),
            spoken_language: Some("id".into()),
            ..Default::default()
        };
        if member == 2 {
            expected.speech = Some(SpeechEventResult {
                event: Some(3),
                raw: [first.clone(), second.clone()].concat().into(),
                ..Default::default()
            });
            expected.unknown_fields = vec![wrong.clone()];
        } else {
            expected.unknown_fields = vec![
                bytes_unknown(5, &first),
                wrong.clone(),
                bytes_unknown(5, &second),
            ];
        }
        check(result_case(
            [
                blob(member, &first),
                blob(4, b"id"),
                wrong.raw,
                blob(member, &second),
            ]
            .concat(),
            expected,
        ));
    }
}

#[test]
fn switches_reset_typed_and_opaque_runs_instead_of_globally_merging_them() {
    let old = [scalar(1, 1), blob(3, &blob(3, &blob(1, b"discarded")))].concat();
    let hypothesis = blob(1, b"retained");
    let list = blob(3, &hypothesis);
    let new = blob(3, &list);
    let tail = scalar(1, 7);
    let active = [new.clone(), tail.clone()].concat();
    check(result_case(
        [
            blob(1, &old),
            blob(2, &scalar(1, 3)),
            blob(1, &new),
            blob(1, &tail),
        ]
        .concat(),
        RecognitionResponse {
            active_field: Some(1),
            active_raw: Some(active.clone().into()),
            recognition: Some(RecognitionResult {
                state: Some(7),
                raw: active.into(),
                hypothesis_result: Some(HypothesisResult {
                    raw: list.into(),
                    hypotheses: vec![Hypothesis {
                        text: Some("retained".into()),
                        raw: hypothesis.into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    ));
    let opaque = bytes_unknown(5, &[0xff, 8]);
    let tail = scalar(1, 7);
    check(result_case(
        [blob(1, &scalar(1, 1)), opaque.raw.clone(), blob(1, &tail)].concat(),
        RecognitionResponse {
            active_field: Some(1),
            active_raw: Some(tail.clone().into()),
            recognition: Some(RecognitionResult {
                state: Some(7),
                raw: tail.into(),
                ..Default::default()
            }),
            unknown_fields: vec![opaque],
            ..Default::default()
        },
    ));
    let unknown = vec![
        bytes_unknown(5, b"old"),
        bytes_unknown(6, &[0xff]),
        bytes_unknown(5, b"new"),
    ];
    check(result_case(
        unknown.iter().flat_map(|f| f.raw.iter().copied()).collect(),
        RecognitionResponse {
            active_field: Some(5),
            active_raw: Some(b"new".to_vec().into()),
            unknown_fields: unknown,
            ..Default::default()
        },
    ));
}

#[test]
fn voice_strings_empty_presence_and_context_merges_have_independent_models() {
    for member in [2, 3, 7] {
        let mut expected = VoiceEditResult {
            active_field: Some(member),
            ..Default::default()
        };
        match member {
            2 => expected.alternative_text = Some(String::new()),
            3 => expected.edited_text = Some(String::new()),
            _ => {
                expected.edited_context = Some(EditingContext {
                    cursor_aware: true,
                    ..Default::default()
                })
            }
        }
        check(edit_case(&[blob(member, b"")], expected));
    }
    for member in [2, 3] {
        let edit = [
            blob(member, b"old"),
            blob(1, b"transcript"),
            scalar(7, 9),
            blob(member, b""),
        ]
        .concat();
        let mut expected = VoiceEditResult {
            active_field: Some(member),
            transcript: Some("transcript".into()),
            unknown_fields: vec![varint_unknown(7, 9)],
            ..Default::default()
        };
        if member == 2 {
            expected.alternative_text = Some(String::new());
        } else {
            expected.edited_text = Some(String::new());
        }
        check(edit_case(&[edit], expected));
    }
    let first = [blob(1, b"before"), scalar(90, 1)].concat();
    let second = [blob(2, b"selected"), blob(3, b"after")].concat();
    let parts = [
        [blob(1, b"transcript"), blob(7, &first)].concat(),
        [
            scalar(4, 2),
            scalar(3, 9),
            blob(99, &[0xff]),
            blob(7, &second),
        ]
        .concat(),
    ];
    let decoded = check(edit_case(
        &parts,
        VoiceEditResult {
            active_field: Some(7),
            transcript: Some("transcript".into()),
            intent: Some(2),
            unknown_fields: vec![varint_unknown(3, 9), bytes_unknown(99, &[0xff])],
            edited_context: Some(EditingContext {
                before: "before".into(),
                selected: "selected".into(),
                after: "after".into(),
                cursor_aware: true,
                raw: [first, second].concat().into(),
                unknown_fields: vec![varint_unknown(90, 1)],
            }),
            ..Default::default()
        },
    ));
    let context = decoded.hypotheses()[0]
        .voice_edit
        .as_ref()
        .unwrap()
        .edited_context
        .as_ref()
        .unwrap();
    assert!(!inside(&decoded.raw, &context.raw));
    let kept = context.clone();
    drop(decoded);
    assert_eq!(kept.before, "before");
    assert!(!kept.raw.is_empty());

    let selected = blob(2, b"selected");
    let after = blob(3, b"after");
    check(edit_case(
        &[[
            blob(7, &blob(1, b"discarded")),
            blob(3, b"discarded"),
            blob(7, &selected),
            blob(7, &after),
        ]
        .concat()],
        VoiceEditResult {
            active_field: Some(7),
            edited_context: Some(EditingContext {
                selected: "selected".into(),
                after: "after".into(),
                cursor_aware: true,
                raw: [selected, after].concat().into(),
                ..Default::default()
            }),
            ..Default::default()
        },
    ));
    check(edit_case(
        &[[blob(2, b"old"), blob(3, b"discarded"), blob(2, b"last")].concat()],
        VoiceEditResult {
            active_field: Some(2),
            alternative_text: Some("last".into()),
            ..Default::default()
        },
    ));
}

#[test]
fn noncanonical_headers_child_bytes_and_unknown_raw_are_not_normalized() {
    let first = vec![0x8a, 0, 0x82, 0, 8, 1];
    let second_payload = vec![0x88, 0, 0x81, 0];
    let second = vec![0x8a, 0, 0x84, 0, 0x88, 0, 0x81, 0];
    check(result_case(
        first.clone(),
        RecognitionResponse {
            active_field: Some(1),
            active_raw: Some(vec![8, 1].into()),
            recognition: Some(RecognitionResult {
                state: Some(1),
                raw: vec![8, 1].into(),
                ..Default::default()
            }),
            ..Default::default()
        },
    ));
    let active = [vec![8, 1], second_payload].concat();
    check(result_case(
        [first, blob(3, b"id"), second].concat(),
        RecognitionResponse {
            active_field: Some(1),
            active_raw: Some(active.clone().into()),
            language: Some("id".into()),
            recognition: Some(RecognitionResult {
                state: Some(1),
                raw: active.into(),
                ..Default::default()
            }),
            ..Default::default()
        },
    ));
    let opaque = vec![0xaa, 0, 0x82, 0, 0xff, 8];
    check(result_case(
        opaque.clone(),
        RecognitionResponse {
            active_field: Some(5),
            active_raw: Some(vec![0xff, 8].into()),
            unknown_fields: vec![Field {
                number: 5,
                wire_type: 2,
                value: Value::Bytes(vec![0xff, 8]),
                raw: opaque,
            }],
            ..Default::default()
        },
    ));
    let context_raw = vec![0x8a, 0, 0x81, 0, b'a'];
    let decoded = check(edit_case(
        &[blob(7, &context_raw)],
        VoiceEditResult {
            active_field: Some(7),
            edited_context: Some(EditingContext {
                before: "a".into(),
                cursor_aware: true,
                raw: context_raw.into(),
                ..Default::default()
            }),
            ..Default::default()
        },
    ));
    let context = decoded.hypotheses()[0]
        .voice_edit
        .as_ref()
        .unwrap()
        .edited_context
        .as_ref()
        .unwrap();
    assert!(inside(&decoded.raw, &context.raw));
}

#[test]
fn overwritten_occurrences_do_not_heal_hide_errors_or_change_error_precedence() {
    let cases = [
        (
            result_wire(&[blob(1, &[8]), blob(2, &scalar(1, 3))].concat()),
            Error::Truncated,
        ),
        (
            result_wire(&[blob(2, &[8]), blob(5, &[0xff])].concat()),
            Error::Truncated,
        ),
        (
            edit_wire(&[blob(2, &[0xff]), blob(3, b"valid")].concat()),
            Error::Utf8,
        ),
        (
            edit_wire(&[blob(3, &[0xff]), blob(3, b"valid")].concat()),
            Error::Utf8,
        ),
        (
            edit_wire(&[blob(7, &blob(1, &[0xff])), blob(3, b"valid")].concat()),
            Error::Utf8,
        ),
        (
            edit_wire(&[blob(7, &[0]), blob(2, b"valid")].concat()),
            Error::FieldNumber,
        ),
        (
            result_wire(&[blob(1, &[8]), blob(1, &[1])].concat()),
            Error::Truncated,
        ),
        (
            edit_wire(&[blob(7, &[0x0a, 3, b'a']), blob(7, b"bc")].concat()),
            Error::Truncated,
        ),
        (
            result_wire(&[blob(1, &[8]), vec![0]].concat()),
            Error::FieldNumber,
        ),
        (
            result_wire(&[blob(1, &[8]), blob(3, &[0xff])].concat()),
            Error::Utf8,
        ),
        (
            edit_wire(&[blob(1, &[0xff]), blob(7, &[0])].concat()),
            Error::Utf8,
        ),
        (
            result_wire(&[blob(1, &[8]), blob(2, &[0])].concat()),
            Error::Truncated,
        ),
        (
            result_wire(&[blob(1, &[0]), blob(2, &[8])].concat()),
            Error::FieldNumber,
        ),
        (
            [result_wire(&blob(1, &[8])), vec![0]].concat(),
            Error::FieldNumber,
        ),
    ];
    for (raw, error) in cases {
        assert_eq!(decode_response(&raw), Err(error), "wire {raw:?}");
    }
}

#[test]
fn exact_global_budget_matches_independent_field_costs_including_discarded_context() {
    let tail = scalar(1, 7);
    let runs = result_case(
        [
            blob(1, &scalar(1, 1)),
            blob(2, &scalar(1, 3)),
            blob(1, &tail),
        ]
        .concat(),
        RecognitionResponse {
            active_field: Some(1),
            active_raw: Some(tail.clone().into()),
            recognition: Some(RecognitionResult {
                state: Some(7),
                raw: tail.clone().into(),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let active = [scalar(1, 1), tail.clone()].concat();
    let merged = result_case(
        [blob(1, &scalar(1, 1)), blob(3, b"id"), blob(1, &tail)].concat(),
        RecognitionResponse {
            active_field: Some(1),
            active_raw: Some(active.clone().into()),
            language: Some("id".into()),
            recognition: Some(RecognitionResult {
                state: Some(7),
                raw: active.into(),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let opaque = result_case(
        blob(5, &[0xff, 8]),
        RecognitionResponse {
            active_field: Some(5),
            active_raw: Some(vec![0xff, 8].into()),
            unknown_fields: vec![bytes_unknown(5, &[0xff, 8])],
            ..Default::default()
        },
    );
    // Costs count interpreted fields, not opaque contents or retained models.
    let cases = [
        (
            recognition_case(
                scalar(1, 1),
                RecognitionResult {
                    state: Some(1),
                    ..Default::default()
                },
            ),
            3,
        ),
        (runs, 7),
        (merged, 6),
        (opaque, 2),
        (
            edit_case(
                &[blob(3, b"x")],
                VoiceEditResult {
                    active_field: Some(3),
                    edited_text: Some("x".into()),
                    ..Default::default()
                },
            ),
            6,
        ),
        (
            edit_case(
                &[blob(7, &blob(1, b"a"))],
                VoiceEditResult {
                    active_field: Some(7),
                    edited_context: Some(EditingContext {
                        before: "a".into(),
                        cursor_aware: true,
                        raw: blob(1, b"a").into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ),
            7,
        ),
        (
            edit_case(
                &[[blob(7, &scalar(99, 1)), blob(3, b"x")].concat()],
                VoiceEditResult {
                    active_field: Some(3),
                    edited_text: Some("x".into()),
                    ..Default::default()
                },
            ),
            8,
        ),
    ];
    for ((raw, mut expected), cost) in cases {
        let exact = [[8, 0].repeat(MAX_FIELDS - cost), raw].concat();
        expected.status = Some(0);
        expected.raw = exact.clone().into();
        check((exact.clone(), expected));
        assert_eq!(
            decode_response(&[exact, scalar(1, 0)].concat()),
            Err(Error::FieldLimit)
        );
    }
    let malformed = result_wire(&blob(1, &[8]));
    assert_eq!(
        decode_response(&[[8, 0].repeat(MAX_FIELDS - 2), malformed.clone()].concat()),
        Err(Error::FieldLimit)
    );
    assert_eq!(
        decode_response(&[[8, 0].repeat(MAX_FIELDS - 3), malformed].concat()),
        Err(Error::Truncated)
    );
}
