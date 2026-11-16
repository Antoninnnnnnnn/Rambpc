//! Static response schema, not evidence of service support or a selection policy.
//! Independent test encoder and operation-level oracle; no production encoders.
use rambler_protocol::{wire::parse_fields, *};

// Immutable raw migration: the independent oracle builds its own copied bytes.
fn append_raw(raw: &mut bytes::Bytes, suffix: impl AsRef<[u8]>) {
    let mut owned = raw.to_vec();
    owned.extend_from_slice(suffix.as_ref());
    *raw = owned.into();
}
fn uint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while n > 127 {
        out.push((n as u8 & 127) | 128);
        n >>= 7;
    }
    out.push(n as u8);
    out
}
fn scalar(n: u32, value: u64) -> Vec<u8> {
    [uint(u64::from(n) * 8), uint(value)].concat()
}
fn blob(n: u32, value: impl AsRef<[u8]>) -> Vec<u8> {
    let value = value.as_ref();
    [
        uint(u64::from(n) * 8 + 2),
        uint(value.len() as u64),
        value.to_vec(),
    ]
    .concat()
}
fn fixed(n: u32, wire: u64, value: &[u8]) -> Vec<u8> {
    [uint(u64::from(n) * 8 + wire), value.to_vec()].concat()
}
fn response(recognition: impl AsRef<[u8]>) -> Vec<u8> {
    blob(RECOGNITION_RESPONSE, blob(1, recognition))
}
fn recognition(decoded: &S3Response) -> &RecognitionResult {
    decoded
        .result
        .as_ref()
        .unwrap()
        .recognition
        .as_ref()
        .unwrap()
}
fn containers(rec: &RecognitionResult) -> [Option<&HypothesisResult>; 3] {
    [
        rec.hypothesis_result.as_ref(),
        rec.hypothesis_result_field5.as_ref(),
        rec.hypothesis_result_field7.as_ref(),
    ]
}
fn with_hypothesis(field: u32, fallback: bool, hypothesis: impl AsRef<[u8]>) -> Vec<u8> {
    let list = if fallback {
        blob(5, blob(1, hypothesis))
    } else {
        blob(3, hypothesis)
    };
    response(blob(field, list))
}

#[test]
fn empty_presence_at_each_level_is_distinct_and_never_selects_text() {
    assert_eq!(decode_response(b"").unwrap().all_hypotheses().count(), 0);
    for field in [3, 5, 7] {
        for (list_raw, wrapper, hypothesis) in [
            (vec![], false, false),
            (blob(5, b""), true, false),
            (blob(5, blob(1, b"")), true, true),
        ] {
            let decoded = decode_response(&response(blob(field, &list_raw))).unwrap();
            let rec = recognition(&decoded);
            let lists = containers(rec);
            let list = lists[[3, 5, 7].iter().position(|n| *n == field).unwrap()].unwrap();
            assert_eq!(lists.iter().filter(|v| v.is_some()).count(), 1);
            assert_eq!(list.raw, list_raw);
            assert_eq!(list.fallback_field5.is_some(), wrapper);
            assert_eq!(list.all_hypotheses().count(), usize::from(hypothesis));
            if let Some(fallback) = &list.fallback_field5 {
                assert_eq!(fallback.hypothesis.is_some(), hypothesis);
                if let Some(hyp) = &fallback.hypothesis {
                    assert_eq!(*hyp, Hypothesis::default());
                }
            }
            assert!(decoded.hypotheses().is_empty());
            assert!(decoded.partials().is_empty());
            assert_eq!(decoded.all_hypotheses().count(), usize::from(hypothesis));
        }
        let decoded =
            decode_response(&with_hypothesis(field, false, blob(1, "candidate"))).unwrap();
        assert_eq!(decoded.hypotheses().len(), usize::from(field == 3));
        assert_eq!(decoded.all_hypotheses().count(), 1);
    }
    // All six hypothesis paths and partials coexist. Reverse wire container
    // order, put fallbacks first, and keep repeated duplicates: traversal is
    // structural, never global wire order, ranking, deduplication or selection.
    let mut raw = blob(4, blob(1, blob(1, "partial")));
    for field in [7, 5, 3] {
        raw.extend(blob(
            field,
            [
                blob(5, blob(1, blob(1, format!("fallback-{field}")))),
                blob(3, blob(1, format!("list-{field}"))),
                blob(3, blob(1, format!("list-{field}"))),
            ]
            .concat(),
        ));
    }
    let decoded = decode_response(&response(raw)).unwrap();
    assert_eq!(
        decoded
            .all_hypotheses()
            .map(|h| h.text.as_deref().unwrap())
            .collect::<Vec<_>>(),
        [
            "list-3",
            "list-3",
            "fallback-3",
            "list-5",
            "list-5",
            "fallback-5",
            "list-7",
            "list-7",
            "fallback-7"
        ]
    );
    assert_eq!(decoded.hypotheses().len(), 2);
    assert!(
        decoded
            .hypotheses()
            .iter()
            .all(|h| h.text.as_deref() == Some("list-3"))
    );
    assert_eq!(decoded.partials()[0].text.as_deref(), Some("partial"));
}

#[test]
fn exhaustive_small_interleavings_merge_independent_containers_and_fallbacks() {
    // Ten operations: repeated/fallback/empty-fallback for each of 3/5/7,
    // plus partial field 4. Enumerate all 11111 sequences of length 0..=4.
    for len in 0..=4_u32 {
        for mut sequence in 0..10_usize.pow(len) {
            let mut expected = RecognitionResult::default();
            let mut wire = Vec::new();
            let mut recognition_raw = Vec::new();
            for step in 0..len {
                let op = sequence % 10;
                sequence /= 10;
                let text = format!("item-{step}");
                let hyp_raw = [
                    blob(1, &text),
                    scalar(HYPOTHESIS_SAFETY_STATUS, u64::from(step % 2)),
                ]
                .concat();
                let rec_part = if op == 9 {
                    let item_raw = blob(1, &text);
                    let list_raw = blob(1, &item_raw);
                    let list = expected.partial_result.get_or_insert_with(Default::default);
                    list.partials.push(PartialHypothesis {
                        text: Some(text),
                        raw: item_raw.into(),
                        ..Default::default()
                    });
                    append_raw(&mut list.raw, &list_raw);
                    blob(4, list_raw)
                } else {
                    let index = op % 3;
                    let list = match index {
                        0 => &mut expected.hypothesis_result,
                        1 => &mut expected.hypothesis_result_field5,
                        _ => &mut expected.hypothesis_result_field7,
                    }
                    .get_or_insert_with(Default::default);
                    let list_raw = if op < 3 {
                        list.hypotheses.push(Hypothesis {
                            text: Some(text),
                            safety_status: Some((step % 2) as i32),
                            raw: hyp_raw.clone().into(),
                            ..Default::default()
                        });
                        blob(3, &hyp_raw)
                    } else {
                        let fallback = list.fallback_field5.get_or_insert_with(Default::default);
                        let fallback_raw = if op < 6 {
                            let hyp = fallback.hypothesis.get_or_insert_with(Default::default);
                            hyp.text = Some(text);
                            hyp.safety_status = Some((step % 2) as i32);
                            append_raw(&mut hyp.raw, &hyp_raw);
                            blob(1, &hyp_raw)
                        } else {
                            vec![]
                        };
                        append_raw(&mut fallback.raw, &fallback_raw);
                        blob(5, fallback_raw)
                    };
                    append_raw(&mut list.raw, &list_raw);
                    blob([3, 5, 7][index], list_raw)
                };
                recognition_raw.extend_from_slice(&rec_part);
                // Also split both enclosing singular messages at every operation.
                wire.extend(response(rec_part));
            }
            if len == 0 {
                wire = response(b"");
            }
            expected.raw = recognition_raw.into();
            let decoded = decode_response(&wire).unwrap();
            let rec = recognition(&decoded);
            assert_eq!(*rec, expected, "len={len}, wire={wire:?}");
            let canonical: Vec<_> = containers(&expected)
                .into_iter()
                .flatten()
                .flat_map(|list| {
                    list.hypotheses.iter().chain(
                        list.fallback_field5
                            .as_ref()
                            .and_then(|f| f.hypothesis.as_ref()),
                    )
                })
                .collect();
            assert_eq!(decoded.all_hypotheses().collect::<Vec<_>>(), canonical);
            assert_eq!(
                rec.hypotheses(),
                expected
                    .hypothesis_result
                    .as_ref()
                    .map_or(&[][..], |l| &l.hypotheses)
            );
            assert_eq!(
                rec.partials(),
                expected
                    .partial_result
                    .as_ref()
                    .map_or(&[][..], |l| &l.partials)
            );
        }
    }
}

#[test]
fn fallback_reuses_full_hypothesis_decoder_and_recursive_proto2_merges() {
    for field in [3, 5, 7] {
        let context_a = [blob(1, "before"), scalar(80, 1)].concat();
        let context_b = [blob(2, "selected"), blob(3, "after")].concat();
        let edit_a = [blob(1, "transcript"), blob(7, &context_a)].concat();
        let edit_b = [blob(7, &context_b), scalar(4, 2)].concat();
        let hyp_a = [
            blob(1, "old"),
            fixed(2, 5, &0.5_f32.to_le_bytes()),
            scalar(HYPOTHESIS_SAFETY_STATUS, 1),
            blob(VOICE_EDITING_RESULT, &edit_a),
            scalar(90, 7),
        ]
        .concat();
        let hyp_b = [
            blob(1, "new"),
            blob(VOICE_EDITING_RESULT, &edit_b),
            scalar(HYPOTHESIS_SAFETY_STATUS, 9),
            scalar(91, 8),
        ]
        .concat();
        let fallback_a = [blob(1, &hyp_a), scalar(92, 9)].concat();
        let fallback_b = [scalar(1, 17), blob(1, &hyp_b), blob(1, b""), scalar(93, 10)].concat();
        let list_a = [
            blob(5, &fallback_a),
            blob(3, blob(1, "primary")),
            scalar(94, 11),
        ]
        .concat();
        let list_b = [blob(5, b""), blob(5, &fallback_b), scalar(95, 12)].concat();
        let payload = [
            response(blob(field, &list_a)),
            response(blob(field, &list_b)),
        ]
        .concat();
        let decoded = decode_response(&payload).unwrap();
        let rec = recognition(&decoded);
        let list = containers(rec)[[3, 5, 7].iter().position(|n| *n == field).unwrap()].unwrap();
        let fallback = list.fallback_field5.as_ref().unwrap();
        let hyp = fallback.hypothesis.as_ref().unwrap();
        let edit = hyp.voice_edit.as_ref().unwrap();
        let context = edit.edited_context.as_ref().unwrap();
        assert_eq!(hyp.text.as_deref(), Some("new"));
        assert_eq!(hyp.confidence, Some(0.5));
        // Later unknown safety enum and empty occurrence do not clear blocked=1.
        assert_eq!(hyp.safety_status, Some(1));
        assert_eq!(
            hyp.unknown_fields,
            parse_fields(
                &[
                    scalar(90, 7),
                    scalar(HYPOTHESIS_SAFETY_STATUS, 9),
                    scalar(91, 8)
                ]
                .concat()
            )
            .unwrap()
        );
        assert_eq!(hyp.raw, [&hyp_a[..], &hyp_b].concat());
        assert_eq!(edit.transcript.as_deref(), Some("transcript"));
        assert_eq!((edit.intent, edit.active_field), (Some(2), Some(7)));
        assert_eq!(edit.raw, [edit_a, edit_b].concat());
        assert_eq!(
            (&*context.before, &*context.selected, &*context.after),
            ("before", "selected", "after")
        );
        assert_eq!(context.raw, [context_a, context_b].concat());
        assert_eq!(
            context.unknown_fields,
            parse_fields(&scalar(80, 1)).unwrap()
        );
        assert_eq!(fallback.raw, [fallback_a, fallback_b].concat());
        assert_eq!(
            fallback.unknown_fields,
            parse_fields(&[scalar(92, 9), scalar(1, 17), scalar(93, 10)].concat()).unwrap()
        );
        assert_eq!(list.raw, [list_a, list_b].concat());
        assert_eq!(
            list.unknown_fields,
            parse_fields(&[scalar(94, 11), scalar(95, 12)].concat()).unwrap()
        );
        assert_eq!(
            decoded
                .all_hypotheses()
                .map(|h| h.text.as_deref())
                .collect::<Vec<_>>(),
            [Some("primary"), Some("new")]
        );
        assert_eq!(decoded.hypotheses().len(), usize::from(field == 3));
    }
}

fn wrong_wires(n: u32) -> Vec<u8> {
    [
        scalar(n, 7),
        fixed(n, 1, &[0xff; 8]),
        fixed(n, 5, &[0xff; 4]),
    ]
    .concat()
}

#[test]
fn wrong_wire_occurrences_remain_unknown_and_do_not_change_presence_or_values() {
    for field in [3, 5, 7] {
        let rec_unknown = wrong_wires(field);
        let list_unknown = [wrong_wires(3), wrong_wires(5)].concat();
        let fallback_unknown = wrong_wires(1);
        let hyp_unknown = [
            blob(HYPOTHESIS_SAFETY_STATUS, b"\xff\x08"),
            fixed(HYPOTHESIS_SAFETY_STATUS, 5, &[0; 4]),
        ]
        .concat();
        let hyp = [
            scalar(HYPOTHESIS_SAFETY_STATUS, 1),
            hyp_unknown.clone(),
            blob(1, "candidate"),
        ]
        .concat();
        let fallback = [
            fallback_unknown.clone(),
            blob(1, &hyp),
            fallback_unknown.clone(),
        ]
        .concat();
        let list = [
            list_unknown.clone(),
            blob(5, &fallback),
            list_unknown.clone(),
        ]
        .concat();
        let rec = [rec_unknown.clone(), blob(field, &list), rec_unknown.clone()].concat();
        let decoded = decode_response(&response(rec)).unwrap();
        let rec = recognition(&decoded);
        assert_eq!(
            rec.unknown_fields,
            parse_fields(&[rec_unknown.clone(), rec_unknown.clone()].concat()).unwrap()
        );
        let list = containers(rec).into_iter().flatten().next().unwrap();
        assert_eq!(
            list.unknown_fields,
            parse_fields(&[list_unknown.clone(), list_unknown.clone()].concat()).unwrap()
        );
        let fallback = list.fallback_field5.as_ref().unwrap();
        assert_eq!(
            fallback.unknown_fields,
            parse_fields(&[fallback_unknown.clone(), fallback_unknown.clone()].concat()).unwrap()
        );
        let hyp = fallback.hypothesis.as_ref().unwrap();
        assert_eq!(hyp.safety_status, Some(1));
        assert_eq!(hyp.text.as_deref(), Some("candidate"));
        assert_eq!(hyp.unknown_fields, parse_fields(&hyp_unknown).unwrap());

        let absent = decode_response(&response(rec_unknown)).unwrap();
        assert!(containers(recognition(&absent)).iter().all(|v| v.is_none()));
        let absent = decode_response(&response(blob(field, list_unknown))).unwrap();
        let list = containers(recognition(&absent))
            .into_iter()
            .flatten()
            .next()
            .unwrap();
        assert!(list.hypotheses.is_empty() && list.fallback_field5.is_none());
        let absent = decode_response(&response(blob(field, blob(5, fallback_unknown)))).unwrap();
        assert_eq!(absent.all_hypotheses().count(), 0);
        assert!(
            containers(recognition(&absent))
                .into_iter()
                .flatten()
                .next()
                .unwrap()
                .fallback_field5
                .as_ref()
                .unwrap()
                .hypothesis
                .is_none()
        );
    }
}

// Split one field into independent occurrences at each newly decoded boundary.
fn split_at_levels(field: u32, a: &[u8], b: &[u8]) -> Vec<Vec<u8>> {
    vec![
        response([blob(field, a), blob(field, b)].concat()),
        response(blob(field, [blob(5, a), blob(5, b)].concat())),
        response(blob(field, blob(5, [blob(1, a), blob(1, b)].concat()))),
        response(blob(
            field,
            blob(
                5,
                blob(
                    1,
                    [blob(VOICE_EDITING_RESULT, a), blob(VOICE_EDITING_RESULT, b)].concat(),
                ),
            ),
        )),
        response(blob(
            field,
            blob(
                5,
                blob(
                    1,
                    blob(VOICE_EDITING_RESULT, [blob(7, a), blob(7, b)].concat()),
                ),
            ),
        )),
    ]
}

#[test]
fn every_interior_cut_fails_independently_at_new_singular_boundaries() {
    let fields = [
        scalar(50, u64::MAX),
        blob(50, "é/test"),
        fixed(50, 1, &[0xff; 8]),
        fixed(50, 5, &[0xff; 4]),
    ];
    for field in [3, 5, 7] {
        for raw in &fields {
            for cut in 0..=raw.len() {
                for payload in split_at_levels(field, &raw[..cut], &raw[cut..]) {
                    assert_eq!(
                        decode_response(&payload).is_ok(),
                        cut == 0 || cut == raw.len(),
                        "field={field}, cut={cut}, raw={raw:?}"
                    );
                    // Invalid runs are still validated before a different oneof wins.
                    let replaced = [payload, blob(RECOGNITION_RESPONSE, blob(2, b""))].concat();
                    assert_eq!(
                        decode_response(&replaced).is_ok(),
                        cut == 0 || cut == raw.len()
                    );
                }
            }
        }
    }
}

#[test]
fn malformed_and_utf8_invalid_occurrences_cannot_be_hidden_or_repaired() {
    for field in [3, 5, 7] {
        for fallback in [false, true] {
            for hyp in [
                vec![0],
                vec![8],
                vec![0x80],
                vec![15],
                vec![13, 0],
                blob(1, b"\xff"),
                [blob(1, b"\xff"), blob(1, "valid")].concat(),
                blob(VOICE_EDITING_RESULT, b"\x08"),
                blob(
                    VOICE_EDITING_RESULT,
                    [blob(3, b"\xff"), blob(2, "valid")].concat(),
                ),
                blob(
                    VOICE_EDITING_RESULT,
                    [blob(7, blob(1, b"\xff")), blob(3, "valid")].concat(),
                ),
            ] {
                let payload = with_hypothesis(field, fallback, hyp);
                assert!(decode_response(&payload).is_err());
                let overwritten = [payload, blob(RECOGNITION_RESPONSE, blob(2, b""))].concat();
                assert!(decode_response(&overwritten).is_err());
            }
        }
        // aalo field 1 is a full message, NOT a string.
        assert_eq!(
            decode_response(&response(blob(field, blob(5, blob(1, "not a hypothesis"))))),
            Err(Error::WireType)
        );
    }
}

#[test]
fn global_field_budget_counts_new_nested_fields_once_even_when_overwritten() {
    // 13 interpreted fields per pair: first costs 7, second 6.
    // The closed safety marker is one field, not a second pass; opaque bytes
    // under unknown field 90 contain protobuf-looking fields but cost only one.
    for field in [3, 5, 7] {
        let first_hyp = [
            scalar(HYPOTHESIS_SAFETY_STATUS, 1),
            blob(90, [8, 0].repeat(20)),
        ]
        .concat();
        let second_hyp = blob(1, "merged");
        let merged = [
            with_hypothesis(field, true, first_hyp),
            with_hypothesis(field, true, second_hyp),
        ]
        .concat();
        for replaced in [false, true] {
            let suffix = if replaced {
                blob(RECOGNITION_RESPONSE, blob(2, scalar(1, 3)))
            } else {
                vec![]
            };
            let cost = 13 + if replaced { 3 } else { 0 };
            let exact = [[8, 0].repeat(MAX_FIELDS - cost), merged.clone(), suffix].concat();
            let decoded = decode_response(&exact).unwrap();
            if replaced {
                assert_eq!(decoded.all_hypotheses().count(), 0);
            } else {
                let hyp = decoded.all_hypotheses().next().unwrap();
                assert_eq!(hyp.safety_status, Some(1));
                assert_eq!(hyp.text.as_deref(), Some("merged"));
            }
            assert_eq!(
                decode_response(&[exact, scalar(1, 0)].concat()),
                Err(Error::FieldLimit)
            );
        }
    }
}

#[test]
fn exact_eight_mib_bound_in_new_container_and_field10_remains_opaque() {
    let make = |size| with_hypothesis(7, true, blob(90, vec![0xff; size]));
    let probe = make(MAX_MESSAGE_BYTES);
    let overhead = probe.len() - MAX_MESSAGE_BYTES;
    let payload = make(MAX_MESSAGE_BYTES - overhead);
    assert_eq!(payload.len(), MAX_MESSAGE_BYTES);
    let decoded = decode_response(&payload).unwrap();
    assert_eq!(decoded.all_hypotheses().count(), 1);
    assert_eq!(decoded.raw, payload);
    drop(decoded);
    assert_eq!(
        decode_response(&[payload, vec![0]].concat()),
        Err(Error::SizeLimit)
    );

    let opaque = blob(10, b"\xff\x08");
    let decoded = decode_response(&response(
        [blob(5, b""), blob(7, b""), opaque.clone()].concat(),
    ))
    .unwrap();
    let rec = recognition(&decoded);
    assert!(rec.hypothesis_result_field5.is_some() && rec.hypothesis_result_field7.is_some());
    assert_eq!(rec.unknown_fields, parse_fields(&opaque).unwrap());
    assert_eq!(decoded.all_hypotheses().count(), 0);
}
