//! Bounded wire oracle built from field descriptors, not the SDK serializer.
use rambler_protocol::{
    Error,
    wire::{Field, Value, parse_fields_with_limits},
};

const PREFIX: [u8; 2] = [0x78, 0x00]; // field 15, varint 0

fn base128(value: u64, width: usize) -> Vec<u8> {
    let value = u128::from(value);
    assert!(width <= 10 && value < 128_u128.pow(width as u32));
    (0..width)
        .map(|i| {
            let digit = (value / 128_u128.pow(i as u32) % 128) as u8;
            digit | if i + 1 < width { 0x80 } else { 0 }
        })
        .collect()
}

fn minimum_width(value: u64) -> usize {
    (1..=10)
        .find(|width| u128::from(value) < 128_u128.pow(*width as u32))
        .unwrap()
}

#[test]
fn complete_fields_keep_exact_width_and_interior_cuts_do_not_complete_a_prefix() {
    // Both sides of every canonical tag-width boundary, including the maximum
    // valid field number. Use every permitted padding width, not just width + 1.
    for number in [
        1,
        15,
        16,
        2_047,
        2_048,
        262_143,
        262_144,
        33_554_431,
        33_554_432,
        0x1fff_ffff,
    ] {
        for (wire_type, value) in [
            (0, Value::Varint(127)),
            (0, Value::Varint(128)),
            (0, Value::Varint(u64::MAX)),
            (1, Value::Fixed64(*b"test-123")),
            (2, Value::Bytes(Vec::new())),
            (2, Value::Bytes(vec![0x00, 0x80, 0xff])),
            (5, Value::Fixed32(*b"test")),
        ] {
            let key = 8 * u64::from(number) + u64::from(wire_type);
            for key_width in minimum_width(key)..=10 {
                let mut field = base128(key, key_width);
                let padded_width = |value| minimum_width(value).max(11 - key_width);
                match &value {
                    Value::Varint(n) => field.extend(base128(*n, padded_width(*n))),
                    Value::Bytes(data) => {
                        let len = data.len() as u64;
                        field.extend(base128(len, padded_width(len)));
                        field.extend_from_slice(data);
                    }
                    Value::Fixed64(data) => field.extend_from_slice(data),
                    Value::Fixed32(data) => field.extend_from_slice(data),
                }
                let packet = [PREFIX.as_slice(), &field].concat();
                let expected = [
                    Field {
                        number: 15,
                        wire_type: 0,
                        value: Value::Varint(0),
                        raw: PREFIX.to_vec(),
                    },
                    Field {
                        number,
                        wire_type,
                        value: value.clone(),
                        raw: field,
                    },
                ];
                let label = (number, wire_type, key_width);
                assert_eq!(
                    parse_fields_with_limits(&packet, packet.len(), 2).unwrap(),
                    expected,
                    "{label:?}"
                );
                assert_eq!(
                    parse_fields_with_limits(&packet, packet.len() - 1, 2),
                    Err(Error::SizeLimit),
                    "{label:?}"
                );
                assert_eq!(
                    parse_fields_with_limits(&packet[..PREFIX.len()], PREFIX.len(), 1).unwrap(),
                    &expected[..1],
                    "{label:?}"
                );
                for end in PREFIX.len() + 1..packet.len() {
                    assert_eq!(
                        parse_fields_with_limits(&packet[..end], end, 2),
                        Err(Error::Truncated),
                        "{label:?}, end={end}"
                    );
                    assert_eq!(
                        parse_fields_with_limits(&packet[..end], end, 1),
                        Err(Error::FieldLimit),
                        "{label:?}, end={end}"
                    );
                }
            }
        }
    }
}

#[test]
fn invalid_tenth_digits_and_invalid_tags_keep_their_exact_errors() {
    // Independently place the same incomplete/overflowing base-128 digits in
    // the tag, value, and length slots after a complete field.
    for slot in [b"".as_slice(), &[0x08], &[0x0a]] {
        let prefix = [PREFIX.as_slice(), slot, &[0x80; 9]].concat();
        assert_eq!(
            parse_fields_with_limits(&prefix, prefix.len(), 2),
            Err(Error::Truncated)
        );
        for tenth in [2, 127, 128, 255] {
            let packet = [prefix.as_slice(), &[tenth]].concat();
            assert_eq!(
                parse_fields_with_limits(&packet, packet.len(), 2),
                Err(Error::VarintOverflow),
                "slot={slot:?}, tenth={tenth}"
            );
        }
    }
    for (number, expected) in [
        (0_u64, Error::FieldNumber),
        (1, Error::WireType),
        (1 << 29, Error::FieldNumber),
    ] {
        for wire_type in [3, 4, 6, 7] {
            let field = base128(8 * number + wire_type, 10);
            let packet = [PREFIX.as_slice(), &field].concat();
            assert_eq!(
                parse_fields_with_limits(&packet, packet.len(), 2),
                Err(expected.clone()),
                "number={number}, type={wire_type}"
            );
        }
    }
}
