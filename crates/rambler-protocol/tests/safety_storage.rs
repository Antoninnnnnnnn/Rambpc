//! Storage regressions for recognized safety fields; independent wire fixtures.
use rambler_protocol::{
    wire::{Field, Value},
    *,
};

fn uint(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        bytes.push((value & 127) as u8 | if value >= 128 { 128 } else { 0 });
        value >>= 7;
        if value == 0 {
            return bytes;
        }
    }
}

fn blob(number: u32, bytes: &[u8]) -> Vec<u8> {
    [
        uint(u64::from(number) * 8 + 2),
        uint(bytes.len() as u64),
        bytes.to_vec(),
    ]
    .concat()
}

fn marker(value: u64) -> Vec<u8> {
    [vec![0xf0, 0x01], uint(value)].concat()
}

fn response(hypothesis: &[u8]) -> Vec<u8> {
    blob(
        RECOGNITION_RESPONSE,
        &blob(1, &blob(3, &blob(3, hypothesis))),
    )
}

#[test]
fn verified_safety_markers_do_not_retain_unknown_storage() {
    let cases = [
        (Vec::new(), None),
        (marker(0), Some(0)),
        (marker(1), Some(1)),
        ([marker(0), marker(1)].concat().repeat(8), Some(1)),
        (vec![0xf0, 0x81, 0, 0x80, 0], Some(0)),
        (vec![0xf0, 0x81, 0, 0x81, 0], Some(1)),
    ];
    for with_text in [false, true] {
        for (safety, expected_status) in &cases {
            let text = if with_text { blob(1, b"x") } else { Vec::new() };
            let body = [text, safety.clone()].concat();
            let payload = response(&body);
            let decoded = decode_response(&payload).unwrap();
            let hypothesis = &decoded.hypotheses()[0];
            assert_eq!(decoded.raw, payload);
            assert_eq!(hypothesis.raw, body);
            assert_eq!(hypothesis.text.as_deref(), with_text.then_some("x"));
            assert_eq!(hypothesis.safety_status, *expected_status);
            assert!(hypothesis.unknown_fields.is_empty());
            assert_eq!(
                hypothesis.unknown_fields.capacity(),
                0,
                "recognized-only markers must not retain an empty unknown vector"
            );
        }
    }
}

#[test]
fn recognized_markers_do_not_inflate_genuine_unknown_storage() {
    let unknowns = vec![
        Field {
            number: 31,
            wire_type: 0,
            value: Value::Varint(7),
            raw: vec![0xf8, 0x01, 7],
        },
        Field {
            number: 30,
            wire_type: 0,
            value: Value::Varint(42),
            raw: vec![0xf0, 0x81, 0, 0xaa, 0],
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
            value: Value::Fixed32([1, 2, 3, 4]),
            raw: vec![0xf5, 0x01, 1, 2, 3, 4],
        },
    ];
    let plain: Vec<u8> = unknowns
        .iter()
        .flat_map(|field| field.raw.clone())
        .collect();
    let reference = decode_response(&response(&plain)).unwrap();
    assert_eq!(reference.hypotheses()[0].unknown_fields, unknowns);
    assert_eq!(reference.hypotheses()[0].safety_status, None);

    let mut mixed = blob(1, b"x");
    for field in &unknowns {
        mixed.extend([marker(0), marker(1)].concat().repeat(8));
        mixed.extend_from_slice(&field.raw);
    }
    mixed.extend([0xf0, 0x81, 0, 0x80, 0]);
    let payload = response(&mixed);
    let decoded = decode_response(&payload).unwrap();
    let hypothesis = &decoded.hypotheses()[0];
    assert_eq!(decoded.raw, payload);
    assert_eq!(hypothesis.raw, mixed);
    assert_eq!(hypothesis.text.as_deref(), Some("x"));
    assert_eq!(hypothesis.safety_status, Some(0));
    assert_eq!(hypothesis.unknown_fields, unknowns);
    assert_eq!(
        hypothesis.unknown_fields.capacity(),
        reference.hypotheses()[0].unknown_fields.capacity(),
        "recognized markers must not inflate storage for genuine opaque fields"
    );
}
