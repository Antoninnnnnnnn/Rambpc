//! Differential encoding oracle copied from the frozen d9af4f5 wire implementation.
use rambler_protocol::{Error, MAX_MESSAGE_BYTES, wire};

// Keep this implementation independent of the production wire helpers. The limit
// check mirrors the baseline's crate::bounded from lib.rs.
mod legacy {
    use super::{Error, MAX_MESSAGE_BYTES};

    fn bounded(len: usize) -> Result<(), Error> {
        if len > MAX_MESSAGE_BYTES {
            Err(Error::SizeLimit)
        } else {
            Ok(())
        }
    }

    pub fn encode_varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(10);
        while value >= 128 {
            out.push((value as u8 & 127) | 128);
            value >>= 7;
        }
        out.push(value as u8);
        out
    }

    fn key(number: u32, wire_type: u8) -> Result<Vec<u8>, Error> {
        if !(1..=0x1fff_ffff).contains(&number) {
            return Err(Error::FieldNumber);
        }
        Ok(encode_varint(
            (u64::from(number) << 3) | u64::from(wire_type),
        ))
    }

    pub fn varint_field(number: u32, value: u64) -> Result<Vec<u8>, Error> {
        let mut out = key(number, 0)?;
        out.extend(encode_varint(value));
        Ok(out)
    }

    pub fn bytes_field(number: u32, value: &[u8]) -> Result<Vec<u8>, Error> {
        bounded(value.len())?;
        let mut out = key(number, 2)?;
        out.extend(encode_varint(value.len() as u64));
        out.extend_from_slice(value);
        bounded(out.len())?;
        Ok(out)
    }

    pub fn string_field(number: u32, value: &str) -> Result<Vec<u8>, Error> {
        bytes_field(number, value.as_bytes())
    }
}

const MAX_FIELD: u32 = 0x1fff_ffff;

#[test]
fn varint_and_key_width_boundaries_match_frozen_encoder() {
    let mut values = vec![0, 1, u64::MAX];
    for shift in (7..64).step_by(7) {
        let boundary = 1_u64 << shift;
        values.extend([boundary - 1, boundary, boundary + 1]);
    }
    let numbers = [
        0,
        1,
        15,
        16,
        17,
        2047,
        2048,
        2049,
        262143,
        262144,
        262145,
        33554431,
        33554432,
        33554433,
        MAX_FIELD,
        MAX_FIELD + 1,
        u32::MAX,
    ];
    for value in values {
        assert_eq!(
            wire::encode_varint(value),
            legacy::encode_varint(value),
            "value={value}"
        );
        for number in numbers {
            assert_eq!(
                wire::varint_field(number, value),
                legacy::varint_field(number, value),
                "number={number} value={value}"
            );
        }
    }
}

#[test]
fn literal_protobuf_wire_goldens() {
    assert_eq!(wire::encode_varint(0), [0x00]);
    assert_eq!(wire::encode_varint(127), [0x7f]);
    assert_eq!(wire::encode_varint(128), [0x80, 0x01]);
    assert_eq!(wire::encode_varint(300), [0xac, 0x02]);
    assert_eq!(
        wire::encode_varint(u64::MAX),
        [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]
    );
    assert_eq!(wire::varint_field(1, 150), Ok(vec![0x08, 0x96, 0x01]));
    assert_eq!(wire::varint_field(16, 1), Ok(vec![0x80, 0x01, 0x01]));
    assert_eq!(
        wire::varint_field(MAX_FIELD, 0),
        Ok(vec![0xf8, 0xff, 0xff, 0xff, 0x0f, 0x00])
    );
    assert_eq!(
        wire::bytes_field(1, &[0, 0xff, 0x80]),
        Ok(vec![0x0a, 0x03, 0x00, 0xff, 0x80])
    );
    assert_eq!(
        wire::string_field(2, "é\0𝄞"),
        Ok(vec![0x12, 0x07, 0xc3, 0xa9, 0x00, 0xf0, 0x9d, 0x84, 0x9e])
    );
    assert_eq!(wire::string_field(1, ""), Ok(vec![0x0a, 0x00]));
}

#[test]
fn byte_and_string_payload_boundaries_match_frozen_encoder() {
    let numbers = [0, 1, 15, 16, 2047, 2048, MAX_FIELD, MAX_FIELD + 1, u32::MAX];
    for len in [0, 1, 126, 127, 128, 129, 16382, 16383, 16384, 16385] {
        let bytes: Vec<u8> = (0..len).map(|i| (i % 256) as u8).collect();
        let text = "x".repeat(len);
        for number in numbers {
            assert_eq!(
                wire::bytes_field(number, &bytes),
                legacy::bytes_field(number, &bytes),
                "number={number} len={len}"
            );
            assert_eq!(
                wire::string_field(number, &text),
                legacy::string_field(number, &text),
                "number={number} len={len}"
            );
        }
    }
    for text in ["", "\0", "a\0b", "é", "漢字", "𝄞🙂", "é\0𝄞"] {
        for number in numbers {
            assert_eq!(
                wire::string_field(number, text),
                legacy::string_field(number, text),
                "number={number} text={text:?}"
            );
        }
    }
}

#[test]
fn complete_message_limit_and_error_precedence_match_frozen_encoder() {
    // At this limit the length prefix is four bytes. Different key widths
    // therefore leave different exact maximum payload lengths.
    for (number, key_len) in [(1, 1), (16, 2), (MAX_FIELD, 5)] {
        let exact = MAX_MESSAGE_BYTES - key_len - 4;
        for len in [
            exact - 1,
            exact,
            exact + 1,
            MAX_MESSAGE_BYTES,
            MAX_MESSAGE_BYTES + 1,
        ] {
            // Compare and release each pair before creating the next large input.
            let bytes = vec![0xa5; len];
            let expected = legacy::bytes_field(number, &bytes);
            let actual = wire::bytes_field(number, &bytes);
            assert_eq!(actual, expected, "number={number} len={len}");
            if len <= exact {
                assert_eq!(actual.unwrap().len(), len + key_len + 4);
            } else {
                assert_eq!(actual, Err(Error::SizeLimit));
            }
        }
    }
    for len in [
        MAX_MESSAGE_BYTES - 5,
        MAX_MESSAGE_BYTES - 4,
        MAX_MESSAGE_BYTES + 1,
    ] {
        let text = "x".repeat(len);
        let expected = legacy::string_field(1, &text);
        let actual = wire::string_field(1, &text);
        assert_eq!(actual, expected, "string len={len}");
        if len == MAX_MESSAGE_BYTES - 5 {
            assert_eq!(actual.unwrap().len(), MAX_MESSAGE_BYTES);
        } else {
            assert_eq!(actual, Err(Error::SizeLimit));
        }
    }
    // Payload validation precedes field-number validation, even for invalid keys.
    for len in [0, MAX_MESSAGE_BYTES, MAX_MESSAGE_BYTES + 1] {
        let bytes = vec![0; len];
        let text = "x".repeat(len);
        for number in [0, MAX_FIELD + 1, u32::MAX] {
            let error = if len > MAX_MESSAGE_BYTES {
                Error::SizeLimit
            } else {
                Error::FieldNumber
            };
            assert_eq!(legacy::bytes_field(number, &bytes), Err(error.clone()));
            assert_eq!(wire::bytes_field(number, &bytes), Err(error.clone()));
            assert_eq!(legacy::string_field(number, &text), Err(error.clone()));
            assert_eq!(wire::string_field(number, &text), Err(error));
        }
    }
}
