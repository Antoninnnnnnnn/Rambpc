use rambler_protocol::{
    wire::{Value, parse_fields},
    *,
};

// Deliberately independent, minimal test encoder (not production wire helpers).
fn uint(mut n: u64) -> Vec<u8> {
    let mut out = vec![];
    while n > 127 {
        out.push((n % 128) as u8 + 128);
        n /= 128;
    }
    out.push(n as u8);
    out
}
fn scalar(number: u32, value: u64) -> Vec<u8> {
    [uint(u64::from(number) * 8), uint(value)].concat()
}
fn blob(number: u32, value: impl AsRef<[u8]>) -> Vec<u8> {
    let value = value.as_ref();
    [
        uint(u64::from(number) * 8 + 2),
        uint(value.len() as u64),
        value.to_vec(),
    ]
    .concat()
}
fn fixed(number: u32, value: f32) -> Vec<u8> {
    [
        uint(u64::from(number) * 8 + 5),
        value.to_le_bytes().to_vec(),
    ]
    .concat()
}
fn double(number: u32, value: f64) -> Vec<u8> {
    [
        uint(u64::from(number) * 8 + 1),
        value.to_le_bytes().to_vec(),
    ]
    .concat()
}
fn join(parts: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    parts.into_iter().flatten().collect()
}
fn response(recognition: impl AsRef<[u8]>) -> Vec<u8> {
    blob(RECOGNITION_RESPONSE, blob(1, recognition))
}
fn with_hypothesis(hypothesis: impl AsRef<[u8]>) -> Vec<u8> {
    response(blob(3, blob(3, hypothesis)))
}
fn with_edit(edit: impl AsRef<[u8]>) -> Vec<u8> {
    with_hypothesis(blob(VOICE_EDITING_RESULT, edit))
}
fn voice(decoded: &S3Response) -> &VoiceEditResult {
    decoded.hypotheses()[0].voice_edit.as_ref().unwrap()
}
fn unknown(number: u32, value: u64) -> wire::Field {
    parse_fields(&scalar(number, value)).unwrap().remove(0)
}

#[test]
fn presence_defaults_errors_unknown_enums_and_signed_int32() {
    let empty = decode_response(b"").unwrap();
    assert_eq!(
        (
            empty.status,
            empty.error_code,
            empty.error_message.as_deref()
        ),
        (None, None, None)
    );
    assert_eq!(
        (empty.recognition_state(), empty.speech_event()),
        (None, None)
    );
    assert!(
        empty.hypotheses().is_empty()
            && empty.partials().is_empty()
            && empty.raw.is_empty()
            && empty.unknown_fields.is_empty()
    );
    let decoded = decode_response(&hex::decode("080210071a046f6f7073").unwrap()).unwrap();
    assert_eq!(
        (decoded.status, decoded.error_code, decoded.error_message),
        (Some(S3Status::Error as i32), Some(7), Some("oops".into()))
    );
    let decoded = decode_response(&join([
        scalar(1, 0),
        scalar(1, 3),
        scalar(2, 1),
        scalar(2, u64::MAX),
        blob(3, "old"),
        blob(3, "new"),
    ]))
    .unwrap();
    assert_eq!(
        (decoded.status, decoded.error_code, decoded.error_message),
        (Some(3), Some(-1), Some("new".into()))
    );
    assert_eq!(decode_response(&scalar(1, 317)).unwrap().status, Some(317));
    assert_eq!(
        decode_response(&scalar(1, 1 << 31)).unwrap().status,
        Some(i32::MIN)
    );
    assert_eq!(
        decode_response(&scalar(1, 1 << 32)).unwrap().status,
        Some(0)
    );
    let explicit = decode_response(&join([scalar(1, 0), scalar(2, 0), blob(3, "")])).unwrap();
    assert_eq!(
        (explicit.status, explicit.error_code, explicit.error_message),
        (Some(0), Some(0), Some(String::new()))
    );
    let empty_msg = decode_response(&blob(RECOGNITION_RESPONSE, b"")).unwrap();
    assert!(empty_msg.result.is_some());
    assert_eq!(empty_msg.result.unwrap().active_field, None);
    let empty_rec = decode_response(&response(b"")).unwrap();
    assert!(empty_rec.result.unwrap().recognition.is_some());
    let empty_hyp = decode_response(&with_hypothesis(b"")).unwrap();
    assert_eq!(empty_hyp.hypotheses().len(), 1);
    assert_eq!(empty_hyp.hypotheses()[0].text, None);
}

#[test]
fn speech_events_preserve_raw_numbers_not_finality_inference() {
    assert_eq!(
        [
            S3Status::Normal as i32,
            S3Status::End as i32,
            S3Status::Error as i32,
            S3Status::NotStarted as i32
        ],
        [0, 1, 2, 3]
    );
    assert_eq!(
        [
            SpeechEvent::StartSpeech as i32,
            SpeechEvent::EndSpeech as i32,
            SpeechEvent::EndUtterance as i32
        ],
        [0, 1, 3]
    );
    for event in [0, 1, 3, 1234] {
        let decoded =
            decode_response(&blob(RECOGNITION_RESPONSE, blob(2, scalar(1, event)))).unwrap();
        assert_eq!(decoded.speech_event(), Some(event as i32));
        assert_eq!(decoded.recognition_state(), None);
        assert!(decoded.hypotheses().is_empty());
    }
}

#[test]
fn nested_models_raw_unknowns_float_and_double_confidences() {
    let cursor = join([
        blob(1, "avant "),
        blob(2, "sélection"),
        blob(3, " après"),
        scalar(77, 9),
    ]);
    let edit = join([
        blob(1, "transcript <cursor>brut</cursor>"),
        blob(7, &cursor),
        scalar(4, 6),
        blob(99, b"unknown-edit"),
    ]);
    let hyp = join([
        blob(1, "ASR candidat"),
        fixed(2, 0.75),
        blob(VOICE_EDITING_RESULT, &edit),
        scalar(80, 4),
    ]);
    let hyp2 = join([blob(1, "autre candidat"), fixed(2, 0.5)]);
    let list = join([blob(3, &hyp), blob(3, &hyp2), scalar(81, 5)]);
    let partial = join([blob(1, "partiel"), double(2, 0.95), scalar(82, 6)]);
    let partial2 = join([blob(1, " suite"), double(2, 0.125)]);
    let partial_list = join([blob(1, &partial), blob(1, &partial2), blob(83, b"opaque")]);
    let recognition_raw = join([
        scalar(1, 23),
        blob(3, &list),
        blob(4, &partial_list),
        scalar(84, 7),
    ]);
    let result = join([
        blob(1, &recognition_raw),
        blob(3, "result-id"),
        blob(4, "auxiliary"),
        scalar(85, 8),
    ]);
    let root = join([
        scalar(1, 0),
        blob(RECOGNITION_RESPONSE, &result),
        scalar(86, 9),
    ]);
    let d = decode_response(&root).unwrap();
    assert_eq!(d.status, Some(0));
    assert_eq!(d.recognition_state(), Some(23));
    assert_eq!(d.speech_event(), None);
    assert_eq!(
        d.hypotheses()
            .iter()
            .map(|h| h.text.as_deref())
            .collect::<Vec<_>>(),
        [Some("ASR candidat"), Some("autre candidat")]
    );
    assert_eq!(
        d.hypotheses()
            .iter()
            .map(|h| h.confidence)
            .collect::<Vec<_>>(),
        [Some(0.75), Some(0.5)]
    );
    assert_eq!(
        d.partials()
            .iter()
            .map(|h| h.text.as_deref())
            .collect::<Vec<_>>(),
        [Some("partiel"), Some(" suite")]
    );
    assert_eq!(
        d.partials()
            .iter()
            .map(|h| h.confidence)
            .collect::<Vec<_>>(),
        [Some(0.95), Some(0.125)]
    );
    let v = voice(&d);
    assert_eq!(
        v.transcript.as_deref(),
        Some("transcript <cursor>brut</cursor>")
    );
    assert_eq!((v.intent, v.active_field), (Some(6), Some(7)));
    assert_eq!((&v.alternative_text, &v.edited_text), (&None, &None));
    let c = v.edited_context.as_ref().unwrap();
    assert_eq!(
        (&*c.before, &*c.selected, &*c.after, c.cursor_aware),
        ("avant ", "sélection", " après", true)
    );
    assert_eq!(c.raw, cursor);
    assert_eq!(c.unknown_fields, [unknown(77, 9)]);
    assert_eq!(v.raw, edit);
    assert_eq!(
        v.unknown_fields,
        parse_fields(&blob(99, b"unknown-edit")).unwrap()
    );
    assert_eq!(d.hypotheses()[0].raw, hyp);
    assert_eq!(d.hypotheses()[0].unknown_fields, [unknown(80, 4)]);
    assert_eq!(d.partials()[0].raw, partial);
    assert_eq!(d.partials()[0].unknown_fields, [unknown(82, 6)]);
    let r = d.result.as_ref().unwrap();
    let rec = r.recognition.as_ref().unwrap();
    assert_eq!(
        rec.hypothesis_result.as_ref().unwrap().unknown_fields,
        [unknown(81, 5)]
    );
    assert_eq!(rec.hypothesis_result.as_ref().unwrap().raw, list);
    assert_eq!(
        rec.partial_result.as_ref().unwrap().unknown_fields,
        parse_fields(&blob(83, b"opaque")).unwrap()
    );
    assert_eq!(rec.partial_result.as_ref().unwrap().raw, partial_list);
    assert_eq!(rec.unknown_fields, [unknown(84, 7)]);
    assert_eq!(rec.raw, recognition_raw);
    assert_eq!(r.raw, result);
    assert_eq!(r.unknown_fields, [unknown(85, 8)]);
    assert_eq!(r.language.as_deref(), Some("result-id"));
    assert_eq!(r.spoken_language.as_deref(), Some("auxiliary"));
    assert_eq!(d.raw, root);
    assert_eq!(d.unknown_fields, [unknown(86, 9)]);
}

#[test]
fn verified_intents_and_unknown_intent_are_data_only() {
    for intent in [0, 1, 2, 3, 4, 5, 6, 1234] {
        let d = decode_response(&with_edit(join([
            blob(1, "raw"),
            blob(3, "candidate edit"),
            scalar(4, intent),
        ])))
        .unwrap();
        let v = voice(&d);
        assert_eq!(v.intent, Some(intent as i32));
        assert_eq!(v.transcript.as_deref(), Some("raw"));
        assert_eq!(v.edited_text.as_deref(), Some("candidate edit"));
        assert_eq!(d.hypotheses()[0].text, None);
    }
}

#[test]
fn voice_oneof_last_member_wins_and_empty_string_presence() {
    let members = [
        (2, blob(2, "alternative")),
        (3, blob(3, "edited")),
        (7, blob(7, blob(1, "before"))),
    ];
    for last in [2, 3, 7] {
        let sequence = members
            .iter()
            .filter(|(n, _)| *n != last)
            .chain(members.iter().filter(|(n, _)| *n == last));
        let d = decode_response(&with_edit(join(sequence.map(|(_, b)| b.clone())))).unwrap();
        let v = voice(&d);
        assert_eq!(v.active_field, Some(last));
        assert_eq!(v.alternative_text.is_some(), last == 2);
        assert_eq!(v.edited_text.is_some(), last == 3);
        assert_eq!(v.edited_context.is_some(), last == 7);
    }
    let d = decode_response(&with_edit(blob(3, b""))).unwrap();
    assert_eq!(voice(&d).edited_text, Some(String::new()));
}

#[test]
fn response_language_fields_are_independent_and_preserve_values() {
    let empty = decode_response(&blob(RECOGNITION_RESPONSE, b""))
        .unwrap()
        .result
        .unwrap();
    assert_eq!((empty.language, empty.spoken_language), (None, None));

    let payload = join([
        blob(3, "fr-FR"),
        blob(4, "fr-FR"),
        blob(3, ""),
        blob(3, "en-US"),
    ]);
    let decoded = decode_response(&blob(RECOGNITION_RESPONSE, &payload))
        .unwrap()
        .result
        .unwrap();
    assert_eq!(decoded.language.as_deref(), Some("en-US"));
    assert_eq!(decoded.spoken_language.as_deref(), Some("fr-FR"));
    assert_eq!(decoded.raw, payload);

    let same = decode_response(&blob(
        RECOGNITION_RESPONSE,
        join([blob(3, "fr-FR"), blob(4, "fr-FR")]),
    ))
    .unwrap()
    .result
    .unwrap();
    assert_eq!(same.language.as_deref(), Some("fr-FR"));
    assert_eq!(same.spoken_language.as_deref(), Some("fr-FR"));

    let present_empty = decode_response(&blob(
        RECOGNITION_RESPONSE,
        join([blob(3, ""), blob(4, "")]),
    ))
    .unwrap()
    .result
    .unwrap();
    assert_eq!(present_empty.language.as_deref(), Some(""));
    assert_eq!(present_empty.spoken_language.as_deref(), Some(""));
}

#[test]
fn all_verified_response_oneof_members_clear_and_keep_independent_strings() {
    for last in [1, 2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 18, 19, 20, 21] {
        let result = join([
            blob(1, scalar(1, 17)),
            blob(2, scalar(1, 3)),
            blob(last, scalar(1, 44)),
            blob(3, "id"),
            blob(4, "independent"),
        ]);
        let d = decode_response(&blob(RECOGNITION_RESPONSE, &result)).unwrap();
        let r = d.result.as_ref().unwrap();
        assert_eq!(r.active_field, Some(last));
        assert_eq!(
            r.active_raw,
            Some(if last == 2 {
                join([scalar(1, 3), scalar(1, 44)])
            } else {
                scalar(1, 44)
            })
        );
        assert_eq!(r.language.as_deref(), Some("id"));
        assert_eq!(r.spoken_language.as_deref(), Some("independent"));
        assert_eq!(
            d.recognition_state(),
            if last == 1 { Some(44) } else { None }
        );
        assert_eq!(d.speech_event(), if last == 2 { Some(44) } else { None });
        if last > 2 {
            assert_eq!(
                r.unknown_fields,
                parse_fields(&blob(last, scalar(1, 44))).unwrap()
            );
        }
    }
}

#[test]
fn unknown_non_oneof_and_wrong_wire_do_not_clear_result_and_stay_opaque() {
    let result = join([blob(1, scalar(1, 9)), blob(14, b"future"), scalar(2, 22)]);
    let d = decode_response(&blob(RECOGNITION_RESPONSE, &result)).unwrap();
    assert_eq!(d.recognition_state(), Some(9));
    assert_eq!(
        d.result.unwrap().unknown_fields,
        parse_fields(&join([blob(14, b"future"), scalar(2, 22)])).unwrap()
    );
    // Unknown contents need not be protobuf or UTF-8, including unknown oneof members.
    let d = decode_response(&blob(
        RECOGNITION_RESPONSE,
        join([blob(1, scalar(1, 9)), blob(5, b"\xff\x08")]),
    ))
    .unwrap();
    assert_eq!(d.recognition_state(), None);
    assert_eq!(d.result.unwrap().active_raw, Some(vec![255, 8]));
}

#[test]
fn repeated_items_duplicates_scalar_last_wins_and_singular_messages_merge() {
    let hyp = join([
        blob(1, "old"),
        blob(1, "new"),
        fixed(2, 0.25),
        fixed(2, 0.5),
    ]);
    let p = join([
        blob(1, "old"),
        blob(1, "new"),
        double(2, 0.25),
        double(2, 0.75),
    ]);
    let old = blob(3, blob(1, "retained"));
    let new = join([
        blob(3, hyp),
        blob(3, blob(1, "second")),
        blob(3, blob(1, "second")),
    ]);
    let rec = join([
        scalar(1, 0),
        scalar(1, 77),
        blob(3, old),
        blob(3, new),
        blob(4, blob(1, p)),
    ]);
    let d = decode_response(&join([response(scalar(1, 0)), response(rec)])).unwrap();
    assert_eq!(d.recognition_state(), Some(77));
    assert_eq!(
        d.hypotheses()
            .iter()
            .map(|h| h.text.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("retained"),
            Some("new"),
            Some("second"),
            Some("second")
        ]
    );
    assert_eq!(d.hypotheses()[1].confidence, Some(0.5));
    assert_eq!(d.partials()[0].text.as_deref(), Some("new"));
    assert_eq!(d.partials()[0].confidence, Some(0.75));
}

#[test]
fn literal_singular_response_extension_merge_preserves_root_and_logical_raw() {
    let payload = hex::decode("ca8fe4040b0a0908011a051a030a0141ca8fe404041a026964").unwrap();
    let d = decode_response(&payload).unwrap();
    assert_eq!(d.recognition_state(), Some(1));
    assert_eq!(d.hypotheses()[0].text.as_deref(), Some("A"));
    assert_eq!(d.result.as_ref().unwrap().language.as_deref(), Some("id"));
    assert_eq!(d.raw, payload);
    assert_eq!(
        d.result.unwrap().raw,
        hex::decode("0a0908011a051a030a01411a026964").unwrap()
    );
}

#[test]
fn same_oneof_message_merges_across_independent_and_wrong_wire_fields() {
    let first = join([scalar(1, 1), scalar(88, 5)]);
    let second = join([blob(3, blob(3, blob(1, "A"))), scalar(89, 6)]);
    let result = join([
        blob(1, &first),
        blob(3, "id"),
        scalar(2, 123),
        blob(1, &second),
    ]);
    let d = decode_response(&blob(RECOGNITION_RESPONSE, result)).unwrap();
    assert_eq!(d.recognition_state(), Some(1));
    assert_eq!(d.hypotheses()[0].text.as_deref(), Some("A"));
    let r = d.result.unwrap();
    assert_eq!(r.language.as_deref(), Some("id"));
    assert_eq!(r.active_raw, Some(join([first.clone(), second.clone()])));
    assert_eq!(r.recognition.as_ref().unwrap().raw, join([first, second]));
    assert_eq!(
        r.recognition.unwrap().unknown_fields,
        [unknown(88, 5), unknown(89, 6)]
    );
    assert_eq!(r.unknown_fields, [unknown(2, 123)]);
}

#[test]
fn oneof_member_change_resets_previous_message_before_later_merge() {
    let old = join([scalar(1, 1), blob(3, blob(3, blob(1, "discarded")))]);
    let new = blob(3, blob(3, blob(1, "retained")));
    let result = join([
        blob(1, old),
        blob(2, scalar(1, 3)),
        blob(1, &new),
        blob(1, scalar(1, 7)),
    ]);
    let d = decode_response(&blob(RECOGNITION_RESPONSE, result)).unwrap();
    assert_eq!(d.recognition_state(), Some(7));
    assert_eq!(d.speech_event(), None);
    assert_eq!(d.hypotheses().len(), 1);
    assert_eq!(d.hypotheses()[0].text.as_deref(), Some("retained"));
    assert_eq!(
        d.result.unwrap().active_raw,
        Some(join([new, scalar(1, 7)]))
    );
}

#[test]
fn voice_extension_and_context_same_oneof_member_merge_across_occurrences() {
    let first_context = join([blob(1, "before"), scalar(90, 1)]);
    let second_context = join([blob(2, "selection"), blob(3, "after")]);
    let first = join([blob(1, "transcript"), blob(7, &first_context)]);
    let second = join([scalar(4, 2), blob(7, &second_context)]);
    let d = decode_response(&with_hypothesis(join([
        blob(VOICE_EDITING_RESULT, &first),
        blob(VOICE_EDITING_RESULT, &second),
    ])))
    .unwrap();
    let v = voice(&d);
    assert_eq!(v.transcript.as_deref(), Some("transcript"));
    assert_eq!(v.intent, Some(2));
    assert_eq!(v.raw, join([first, second]));
    let c = v.edited_context.as_ref().unwrap();
    assert_eq!(
        (&*c.before, &*c.selected, &*c.after),
        ("before", "selection", "after")
    );
    assert_eq!(c.raw, join([first_context, second_context]));
    assert_eq!(c.unknown_fields, [unknown(90, 1)]);
    let d = decode_response(&with_edit(join([
        blob(7, blob(1, "discarded")),
        blob(3, "also discarded"),
        blob(7, blob(2, "selected")),
        blob(7, blob(3, "after")),
    ])))
    .unwrap();
    let v = voice(&d);
    let c = v.edited_context.as_ref().unwrap();
    assert_eq!(v.edited_text, None);
    assert_eq!(
        (&*c.before, &*c.selected, &*c.after),
        ("", "selected", "after")
    );
}

#[test]
fn malformed_occurrences_never_heal_each_other_or_hide_in_overwritten_runs() {
    for payload in [
        join([
            blob(RECOGNITION_RESPONSE, b"\x0a"),
            blob(RECOGNITION_RESPONSE, b"\x00"),
        ]),
        response(join([blob(3, b"\x1a"), blob(3, b"\x00")])),
        blob(
            RECOGNITION_RESPONSE,
            join([blob(1, b"\x08"), blob(1, b"\x01")]),
        ),
        with_edit(join([blob(7, b"\x0a\x03a"), blob(7, b"bc")])),
        blob(
            RECOGNITION_RESPONSE,
            join([
                blob(1, b"\x08"),
                blob(2, scalar(1, 3)),
                blob(1, scalar(1, 1)),
            ]),
        ),
        with_edit(join([blob(7, blob(1, b"\xff")), blob(3, "valid")])),
        with_hypothesis(join([
            blob(VOICE_EDITING_RESULT, b"\x08"),
            blob(VOICE_EDITING_RESULT, b"\x01"),
        ])),
    ] {
        assert!(
            decode_response(&payload).is_err(),
            "unexpected success {payload:?}"
        );
    }
}

#[test]
fn known_strings_utf8_strict_even_if_overwritten_at_every_level() {
    for payload in [
        blob(3, b"\xff"),
        join([blob(3, b"\xff"), blob(3, "valid")]),
        with_hypothesis(blob(1, b"\xff")),
        with_edit(blob(1, b"\xff")),
        with_edit(join([blob(2, b"\xff"), blob(3, "valid")])),
        with_edit(blob(7, blob(1, b"\xff"))),
        response(blob(4, blob(1, blob(1, b"\xff")))),
        blob(RECOGNITION_RESPONSE, blob(3, b"\xff")),
        blob(RECOGNITION_RESPONSE, blob(4, b"\xff")),
        with_edit(blob(7, blob(2, b"\xff"))),
        with_edit(blob(7, blob(3, b"\xff"))),
        with_hypothesis(join([blob(1, b"\xff"), blob(1, "valid")])),
        with_edit(join([blob(3, b"\xff"), blob(3, "valid")])),
        blob(3, b"\xed\xa0\x80"),     // Encoded surrogate.
        blob(3, b"\xc0\x80"),         // Overlong UTF-8 NUL.
        blob(3, b"\xf4\x90\x80\x80"), // Beyond Unicode maximum.
    ] {
        assert_eq!(decode_response(&payload), Err(Error::Utf8));
    }
}

#[test]
fn malformed_wire_at_known_nested_levels_is_rejected() {
    for payload in [
        vec![0],
        vec![8],
        vec![128],
        vec![10, 5, b'x'],
        vec![15],
        vec![11],
        vec![12],
        vec![9, 0],
        vec![13, 0],
        join([vec![8], vec![255; 10], vec![1]]),
        blob(RECOGNITION_RESPONSE, b"\x0a\x02\x08"),
        with_hypothesis(b"\x15\x00"),
        response(blob(4, blob(1, b"\x11\x00"))),
        with_edit(blob(7, b"\x0a\x08bad")),
    ] {
        assert!(decode_response(&payload).is_err());
    }
}

#[test]
fn unknown_fields_order_wrong_wire_and_root_noncanonical_bytes_exact() {
    let payload = join([
        vec![8, 128, 0],
        blob(1, b"not-status"),
        scalar(100, 7),
        blob(101, b"\xff\x00"),
        scalar(100, 8),
    ]);
    let d = decode_response(&payload).unwrap();
    assert_eq!(d.status, Some(0));
    assert_eq!(d.raw, payload);
    assert_eq!(
        d.unknown_fields,
        parse_fields(&join([
            blob(1, b"not-status"),
            scalar(100, 7),
            blob(101, b"\xff\x00"),
            scalar(100, 8)
        ]))
        .unwrap()
    );
    let d = decode_response(&join([
        vec![0x88, 0, 0x80, 0],
        vec![0xa0, 0x86, 0x00, 0x81, 0],
    ]))
    .unwrap();
    assert_eq!(d.status, Some(0));
    assert_eq!(d.unknown_fields[0].raw, [0xa0, 0x86, 0, 0x81, 0]);
    assert_eq!(d.unknown_fields[0].value, Value::Varint(1));
}

#[test]
fn exact_global_budget_counts_merged_and_overwritten_occurrences_once() {
    // Two root fields plus two fields at four nested levels = ten interpreted fields.
    let merged = join([with_hypothesis(blob(1, "A")), with_hypothesis(blob(1, "B"))]);
    let payload = join([[8, 0].repeat(MAX_FIELDS - 10), merged]);
    let d = decode_response(&payload).unwrap();
    assert_eq!(
        d.hypotheses()
            .iter()
            .map(|h| h.text.as_deref())
            .collect::<Vec<_>>(),
        [Some("A"), Some("B")]
    );
    assert_eq!(
        decode_response(&join([scalar(1, 0), payload])),
        Err(Error::FieldLimit)
    );
    // One root, three oneof wrapper fields, three nested scalar fields = 7.
    let runs = blob(
        RECOGNITION_RESPONSE,
        join([
            blob(1, scalar(1, 1)),
            blob(2, scalar(1, 3)),
            blob(1, scalar(1, 7)),
        ]),
    );
    decode_response(&join([[8, 0].repeat(MAX_FIELDS - 7), runs.clone()])).unwrap();
    assert_eq!(
        decode_response(&join([[8, 0].repeat(MAX_FIELDS - 6), runs])),
        Err(Error::FieldLimit)
    );
    assert_eq!(
        decode_response(&[8, 0].repeat(MAX_FIELDS + 1)),
        Err(Error::FieldLimit)
    );
    assert_eq!(
        decode_response(&join([
            [8, 0].repeat(MAX_FIELDS - 1),
            response(scalar(1, 0))
        ])),
        Err(Error::FieldLimit)
    );
    assert_eq!(
        decode_response(&vec![0; MAX_MESSAGE_BYTES + 1]),
        Err(Error::SizeLimit)
    );
}

#[test]
fn editing_context_equality_excludes_only_raw_like_python() {
    let first_payload = join([
        blob(1, "before"),
        blob(2, "selected"),
        blob(3, "after"),
        scalar(90, 1),
    ]);
    let reordered_payload = join([
        blob(3, "after"),
        blob(2, "selected"),
        blob(1, "before"),
        scalar(90, 1),
    ]);
    let first_response = decode_response(&with_edit(blob(7, &first_payload))).unwrap();
    let reordered_response = decode_response(&with_edit(blob(7, &reordered_payload))).unwrap();
    let first = voice(&first_response).edited_context.as_ref().unwrap();
    let reordered = voice(&reordered_response).edited_context.as_ref().unwrap();
    assert_eq!(first.raw, first_payload);
    assert_eq!(reordered.raw, reordered_payload);
    assert_ne!(first.raw, reordered.raw);
    assert_eq!(first, reordered);

    let mut changed_raw = first.clone();
    changed_raw.raw.clear();
    assert_eq!(*first, changed_raw);
    for index in 0..3 {
        let mut changed_text = first.clone();
        match index {
            0 => changed_text.before.push('!'),
            1 => changed_text.selected.push('!'),
            _ => changed_text.after.push('!'),
        }
        assert_ne!(*first, changed_text);
    }
    let mut changed_cursor = first.clone();
    changed_cursor.cursor_aware = !changed_cursor.cursor_aware;
    assert_ne!(*first, changed_cursor);
    let mut changed_unknown = first.clone();
    changed_unknown.unknown_fields[0] = unknown(90, 2);
    assert_ne!(*first, changed_unknown);
    let mut removed_unknown = first.clone();
    removed_unknown.unknown_fields.clear();
    assert_ne!(*first, removed_unknown);
}

#[test]
fn float_bit_patterns_are_not_sanitized() {
    let d = decode_response(&with_hypothesis(join([
        fixed(2, f32::NAN),
        fixed(2, f32::NEG_INFINITY),
    ])))
    .unwrap();
    assert_eq!(d.hypotheses()[0].confidence, Some(f32::NEG_INFINITY));
    let d = decode_response(&response(blob(4, blob(1, double(2, -0.0))))).unwrap();
    assert_eq!(
        d.partials()[0].confidence.unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    let d = decode_response(&with_hypothesis(fixed(2, f32::NAN))).unwrap();
    assert!(d.hypotheses()[0].confidence.unwrap().is_nan());
}
