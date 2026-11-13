use rambler_protocol::{wire::*, *};

#[test]
fn unsigned_varints_roundtrip_and_encoding_bounds() {
    for value in [
        0,
        1,
        127,
        128,
        255,
        16384,
        u32::MAX as u64,
        i64::MAX as u64,
        u64::MAX,
    ] {
        let mut payload = vec![8];
        payload.extend(encode_varint(value));
        assert_eq!(
            parse_fields(&payload).unwrap()[0].value,
            Value::Varint(value)
        );
    }
    assert_eq!(encode_varint(u64::MAX), [vec![255; 9], vec![1]].concat());
    assert_eq!(varint_field(0, 0), Err(Error::FieldNumber));
    assert_eq!(varint_field(1 << 29, 0), Err(Error::FieldNumber));
    assert_eq!(bytes_field(0, b""), Err(Error::FieldNumber));
    assert_eq!(
        parse_fields(&varint_field((1 << 29) - 1, 7).unwrap()).unwrap()[0].number,
        (1 << 29) - 1
    );
}
#[test]
fn unknown_wire_fields_keep_fixed_values_raw_and_noncanonical_varints() {
    let payload = hex::decode("88800080001101020304050607081a80002501020304").unwrap();
    let f = parse_fields(&payload).unwrap();
    assert_eq!(f.iter().map(|f| f.number).collect::<Vec<_>>(), [1, 2, 3, 4]);
    assert_eq!(
        f.iter().map(|f| f.wire_type).collect::<Vec<_>>(),
        [0, 1, 2, 5]
    );
    assert_eq!(f[0].value, Value::Varint(0));
    assert_eq!(f[0].raw, [0x88, 0x80, 0x00, 0x80, 0x00]);
    assert_eq!(f[1].value, Value::Fixed64([1, 2, 3, 4, 5, 6, 7, 8]));
    assert_eq!(f[2].value, Value::Bytes(vec![]));
    assert_eq!(f[2].raw, [0x1a, 0x80, 0]);
    assert_eq!(f[3].value, Value::Fixed32([1, 2, 3, 4]));
    assert_eq!(
        f.iter().flat_map(|f| f.raw.clone()).collect::<Vec<_>>(),
        payload
    );
}
#[test]
fn malformed_wire_and_varint_overflows() {
    for payload in [
        vec![0],
        vec![8],
        vec![128],
        vec![10, 5, b'x'],
        vec![15],
        vec![11],
        vec![12],
        vec![14],
        vec![9, 0],
        vec![13, 0],
        [vec![8], vec![255; 10], vec![1]].concat(),
        [vec![8], vec![255; 9], vec![2]].concat(),
        [vec![8], vec![128; 10], vec![0]].concat(),
        encode_varint(1_u64 << 32), // Field key outside 29-bit range.
        [vec![10], encode_varint(u64::MAX)].concat(),
        [vec![10], vec![255; 9], vec![2]].concat(),
    ] {
        assert!(
            parse_fields(&payload).is_err(),
            "unexpected success: {payload:?}"
        );
    }
}
#[test]
fn field_and_size_budgets_include_exact_boundaries() {
    let payload = [8, 0].repeat(MAX_FIELDS);
    assert_eq!(parse_fields(&payload).unwrap().len(), MAX_FIELDS);
    assert_eq!(
        parse_fields(&[payload, vec![8, 0]].concat()),
        Err(Error::FieldLimit)
    );
    assert_eq!(parse_fields_with_limits(&[8, 0], 2, 1).unwrap().len(), 1);
    assert_eq!(
        parse_fields_with_limits(&[8, 0], 1, 1),
        Err(Error::SizeLimit)
    );
    assert_eq!(
        parse_fields_with_limits(&[8, 0], 2, 0),
        Err(Error::FieldLimit)
    );
    assert!(parse_fields_with_limits(&[], 0, 0).unwrap().is_empty());
    assert_eq!(
        parse_fields(&vec![0; MAX_MESSAGE_BYTES + 1]),
        Err(Error::SizeLimit)
    );
    // A single opaque field is bounded by total bytes, not its arbitrary contents.
    let size = MAX_MESSAGE_BYTES - 5; // one-byte key + four-byte length
    let payload = bytes_field(1, &vec![0; size]).unwrap();
    assert_eq!(payload.len(), MAX_MESSAGE_BYTES);
    assert_eq!(parse_fields(&payload).unwrap().len(), 1);
    assert_eq!(
        parse_fields_with_limits(&vec![0; MAX_MESSAGE_BYTES + 1], usize::MAX, usize::MAX),
        Err(Error::SizeLimit)
    );
}
