//! The safety marker is intentionally a conservative closed proto2 subset.
use rambler_protocol::{
    wire::{Field, Value},
    *,
};

fn uint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        out.push((n & 127) as u8 | if n >= 128 { 128 } else { 0 });
        n >>= 7;
        if n == 0 {
            return out;
        }
    }
}
fn scalar(value: u64) -> Vec<u8> {
    [vec![0xf0, 0x01], uint(value)].concat() // Hypothesis.field30, wire 0.
}
fn blob(n: u32, value: &[u8]) -> Vec<u8> {
    [
        uint(u64::from(n) * 8 + 2),
        uint(value.len() as u64),
        value.to_vec(),
    ]
    .concat()
}
fn response(hypothesis: &[u8]) -> Vec<u8> {
    blob(
        RECOGNITION_RESPONSE,
        &blob(1, &blob(3, &blob(3, hypothesis))),
    )
}
fn unknown(value: u64) -> Field {
    Field {
        number: HYPOTHESIS_SAFETY_STATUS,
        wire_type: 0,
        value: Value::Varint(value),
        raw: scalar(value),
    }
}

#[test]
fn verified_safety_enum_values_are_known_not_opaque_unknowns() {
    assert_eq!(HYPOTHESIS_SAFETY_STATUS, 30);
    for value in [0, 1] {
        let raw = scalar(value);
        let decoded = decode_response(&response(&raw)).unwrap();
        let hypothesis = &decoded.hypotheses()[0];
        assert_eq!(hypothesis.raw, raw);
        assert!(
            hypothesis.unknown_fields.is_empty(),
            "verified safety value {value} must be classified as a known field"
        );
        assert_eq!(hypothesis.safety_status, Some(value as i32));
    }
    let absent = decode_response(&response(b"")).unwrap();
    assert_eq!(absent.hypotheses()[0].safety_status, None);
    // Overlong key and value still represent the exact verified numeric values.
    for value in [0, 1] {
        let raw = [0xf0, 0x81, 0, 0x80 | value, 0];
        let decoded = decode_response(&response(&raw)).unwrap();
        assert_eq!(
            decoded.hypotheses()[0].safety_status,
            Some(i32::from(value))
        );
        assert_eq!(decoded.hypotheses()[0].raw, raw);
        assert!(decoded.hypotheses()[0].unknown_fields.is_empty());
    }
}

#[test]
fn safety_last_recognized_wins_and_unknown_values_never_clear_known_presence() {
    // Exhaust all short traces: absence, explicit zero/one, duplicates, unknown
    // before/after known and values that would alias 0/1 if truncated to int32.
    let alphabet = [0, 1, 2, 42, 1_u64 << 32, (1_u64 << 32) + 1, u64::MAX];
    for a in alphabet {
        for b in alphabet {
            for c in alphabet {
                let values = [a, b, c];
                let raw: Vec<u8> = values.into_iter().flat_map(scalar).collect();
                let payload = response(&raw);
                let decoded = decode_response(&payload).unwrap();
                let h = &decoded.hypotheses()[0];
                let expected = values.into_iter().rev().find(|v| *v <= 1).map(|v| v as i32);
                assert_eq!(h.safety_status, expected, "values={values:?}");
                assert_eq!(
                    h.unknown_fields,
                    values
                        .into_iter()
                        .filter(|v| *v > 1)
                        .map(unknown)
                        .collect::<Vec<_>>(),
                    "values={values:?}"
                );
                assert_eq!(h.raw, raw);
                assert_eq!(decoded.raw, payload);
            }
        }
    }
    for value in alphabet.into_iter().filter(|v| *v > 1) {
        let decoded = decode_response(&response(&scalar(value))).unwrap();
        assert_eq!(decoded.hypotheses()[0].safety_status, None);
        assert_eq!(decoded.hypotheses()[0].unknown_fields, [unknown(value)]);
    }
}

#[test]
fn safety_wrong_wire_and_noncanonical_unknowns_remain_exact_and_ordered() {
    let wrong = [
        Field {
            number: 30,
            wire_type: 1,
            value: Value::Fixed64([0; 8]),
            raw: [vec![0xf1, 1], vec![0; 8]].concat(),
        },
        Field {
            number: 30,
            wire_type: 2,
            value: Value::Bytes(vec![0xff]),
            raw: blob(30, &[0xff]),
        },
        Field {
            number: 30,
            wire_type: 5,
            value: Value::Fixed32([1; 4]),
            raw: [vec![0xf5, 1], vec![1; 4]].concat(),
        },
        Field {
            number: 30,
            wire_type: 0,
            value: Value::Varint(2),
            raw: vec![0xf0, 0x81, 0, 0x82, 0],
        },
    ];
    let raw = [
        scalar(1),
        wrong.iter().flat_map(|f| f.raw.clone()).collect(),
    ]
    .concat();
    let decoded = decode_response(&response(&raw)).unwrap();
    let h = &decoded.hypotheses()[0];
    assert_eq!(h.safety_status, Some(1));
    assert_eq!(h.unknown_fields, wrong);
    assert_eq!(h.raw, raw);
    for field in &wrong {
        let decoded = decode_response(&response(&field.raw)).unwrap();
        assert_eq!(decoded.hypotheses()[0].safety_status, None);
        assert_eq!(
            decoded.hypotheses()[0].unknown_fields,
            std::slice::from_ref(field)
        );
    }
}

#[test]
fn singular_parent_merges_preserve_each_repeated_hypothesis_safety_independently() {
    let hypotheses = [
        vec![],
        scalar(0),
        scalar(1),
        scalar(42),
        [scalar(1), scalar(42), scalar(0)].concat(),
    ];
    let mut payload = Vec::new();
    // Repeated root extension and recognition oneof message occurrences merge;
    // the hypotheses themselves are repeated items and must never merge/bleed.
    for h in &hypotheses {
        payload.extend(response(h));
    }
    let decoded = decode_response(&payload).unwrap();
    assert_eq!(
        decoded
            .hypotheses()
            .iter()
            .map(|h| h.safety_status)
            .collect::<Vec<_>>(),
        [None, Some(0), Some(1), None, Some(0)]
    );
    for (h, raw) in decoded.hypotheses().iter().zip(&hypotheses) {
        assert_eq!(&h.raw, raw);
    }
    assert_eq!(decoded.hypotheses()[3].unknown_fields, [unknown(42)]);
    assert_eq!(decoded.hypotheses()[4].unknown_fields, [unknown(42)]);
    assert_eq!(decoded.raw, payload);
}

#[test]
fn safety_occurrences_share_global_budget_and_malformed_ones_cannot_be_overwritten() {
    let raw = [scalar(1), scalar((1_u64 << 32) + 1)].concat();
    // Four wrappers plus two safety fields, each charged exactly once.
    let mut exact = [8, 0].repeat(MAX_FIELDS - 6);
    exact.extend(response(&raw));
    let decoded = decode_response(&exact).unwrap();
    assert_eq!(decoded.hypotheses()[0].safety_status, Some(1));
    assert_eq!(
        decoded.hypotheses()[0].unknown_fields,
        [unknown((1_u64 << 32) + 1)]
    );
    exact.extend([8, 0]);
    assert_eq!(decode_response(&exact), Err(Error::FieldLimit));
    for malformed in [vec![0xf0, 1], [vec![0xf0, 1], vec![0xff; 10]].concat()] {
        let payload = response(&[malformed, scalar(1)].concat());
        assert!(decode_response(&payload).is_err());
    }
}
