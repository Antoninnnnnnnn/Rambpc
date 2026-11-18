//! Offline wire assertions independent of the production wire parser/encoders.
use rambler_protocol::{
    EditingContext, Error, MAX_MESSAGE_BYTES, ProtocolConfig, audio_request, context_request,
    end_request, initial_request,
};

#[derive(Debug, PartialEq, Eq)]
enum Value<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

fn uint(input: &mut &[u8]) -> u64 {
    let mut value = 0;
    for shift in (0..=63).step_by(7) {
        let (&byte, rest) = input.split_first().expect("complete varint");
        *input = rest;
        assert!(shift != 63 || byte <= 1);
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return value;
        }
    }
    panic!("oversized varint")
}

// Only the wire types in these init roots/RecognizerInfo payloads are needed.
fn fields(mut input: &[u8]) -> Vec<(u64, Value<'_>)> {
    let mut out = Vec::new();
    while !input.is_empty() {
        let tag = uint(&mut input);
        assert_ne!(tag >> 3, 0);
        let value = match tag & 7 {
            0 => Value::Int(uint(&mut input)),
            2 => {
                let len = usize::try_from(uint(&mut input)).unwrap();
                let (bytes, rest) = input.split_at(len);
                input = rest;
                Value::Bytes(bytes)
            }
            other => panic!("unexpected wire type {other}"),
        };
        out.push((tag >> 3, value));
    }
    out
}

fn recognizer(request: &[u8]) -> &[u8] {
    let mut payloads = fields(request).into_iter().filter_map(|(number, value)| {
        if number == 294500 {
            match value {
                Value::Bytes(bytes) => Some(bytes),
                _ => panic!("RecognizerInfo must be length-delimited"),
            }
        } else {
            None
        }
    });
    let payload = payloads.next().unwrap();
    assert!(payloads.next().is_none(), "one RecognizerInfo extension");
    payload
}

const RECOGNIZER_DEFAULT: &str = concat!(
    "0a02200028016801a00101c80201920329",
    "67626f6172645f67656d696e695f76335f73747265616d696e675f766f6963655f656469745f6d756c"
);
const NATIVE_GOLDEN: &str = concat!(
    "0a156265796f6e642d6132612d7265636f676e697a65721001",
    "c2888f010912070a0566722d4652e28e8f010b1500007a46180a20014800",
    "82c78f0111120667626f617264420452757374d00101",
    "a2e68f013a0a02200028016801a00101c80201920329",
    "67626f6172645f67656d696e695f76335f73747265616d696e675f766f6963655f656469745f6d756c",
    "a2bfb7710308f403"
);

#[test]
fn defaults_omit_all_three_fields_and_preserve_frozen_native_bytes() {
    let config = ProtocolConfig::default();
    assert_eq!(config.block_offensive_words, None);
    assert_eq!(config.auto_punctuation, None);
    assert_eq!(config.secondary_routing, None);
    let request = initial_request(&config, 0).unwrap();
    assert_eq!(request, hex::decode(NATIVE_GOLDEN).unwrap());
    assert_eq!(
        recognizer(&request),
        hex::decode(RECOGNIZER_DEFAULT).unwrap()
    );
    assert!(
        fields(recognizer(&request))
            .iter()
            .all(|(n, _)| ![7, 40, 64].contains(n))
    );
}

#[test]
fn explicit_scalar_values_have_literal_wire_vectors() {
    for (block, punctuation, suffix) in [
        (Some(false), None, "3800"),
        (Some(true), None, "3802"),
        (None, Some(false), "c00200"),
        (None, Some(true), "c00201"),
    ] {
        let config = ProtocolConfig {
            block_offensive_words: block,
            auto_punctuation: punctuation,
            ..Default::default()
        };
        // Compare the complete submessage to a frozen vector, not our own encoder.
        let expected = format!(
            "0a02200028016801a00101c80201{suffix}920329{}",
            "67626f6172645f67656d696e695f76335f73747265616d696e675f766f6963655f656469745f6d756c"
        );
        let request = initial_request(&config, 0).unwrap();
        assert_eq!(recognizer(&request), hex::decode(expected).unwrap());
    }
}

#[test]
fn all_presence_combinations_only_change_recognizer_info() {
    let baseline_config = ProtocolConfig {
        bias_terms: vec!["contact é".into()],
        preferences: vec!["préférence".into()],
        secondary_languages: vec!["en-US".into(), "zh-Hans".into()],
        ..Default::default()
    };
    let baseline = initial_request(&baseline_config, 0).unwrap();
    fn without_recognizer(request: &[u8]) -> Vec<(u64, Value<'_>)> {
        fields(request)
            .into_iter()
            .filter(|(n, _)| *n != 294500)
            .collect()
    }
    let default_recognizer = hex::decode(RECOGNIZER_DEFAULT).unwrap();
    for block in [None, Some(false), Some(true)] {
        for punctuation in [None, Some(false), Some(true)] {
            for secondary in [None, Some(" clé-e\u{301}-東京-𝄞\0 ")] {
                let config = ProtocolConfig {
                    block_offensive_words: block,
                    auto_punctuation: punctuation,
                    secondary_routing: secondary.map(str::to_owned),
                    ..baseline_config.clone()
                };
                let saved = config.clone();
                let request = initial_request(&config, 0).unwrap();
                assert_eq!(config, saved);
                assert_eq!(without_recognizer(&request), without_recognizer(&baseline));
                let mut expected = fields(&default_recognizer);
                if let Some(block) = block {
                    expected.push((7, Value::Int(if block { 2 } else { 0 })));
                }
                if let Some(punctuation) = punctuation {
                    expected.push((40, Value::Int(u64::from(punctuation))));
                }
                if let Some(secondary) = secondary {
                    expected.push((64, Value::Bytes(secondary.as_bytes())));
                }
                let mut actual = fields(recognizer(&request));
                expected.sort_by_key(|(n, _)| *n);
                actual.sort_by_key(|(n, _)| *n);
                assert_eq!(actual, expected);
            }
        }
    }
}

#[test]
fn secondary_routing_has_a_literal_utf8_vector_and_keeps_primary_verbatim() {
    let request = initial_request(
        &ProtocolConfig {
            routing: "primary".into(),
            secondary_routing: Some("é𝄞".into()),
            ..Default::default()
        },
        0,
    )
    .unwrap();
    assert_eq!(
        recognizer(&request),
        hex::decode("0a02200028016801a00101c802019203077072696d617279820406c3a9f09d849e").unwrap()
    );
}

#[test]
fn secondary_routing_matches_primary_validation_and_errors_are_private() {
    for value in [String::new(), "é".repeat(MAX_MESSAGE_BYTES / 2 + 1)] {
        let primary = ProtocolConfig {
            routing: value.clone(),
            ..Default::default()
        };
        let secondary = ProtocolConfig {
            secondary_routing: Some(value),
            ..Default::default()
        };
        assert!(secondary.validate().is_err());
        assert_eq!(secondary.validate(), primary.validate());
        assert_eq!(initial_request(&secondary, 0), initial_request(&primary, 0));
    }
    let config = ProtocolConfig {
        secondary_routing: Some("private-route".repeat(MAX_MESSAGE_BYTES / 13 + 1)),
        ..Default::default()
    };
    let error = initial_request(&config, 0).unwrap_err();
    assert!(!format!("{error:?} {error}").contains("private-route"));
}

#[test]
fn utf8_and_combined_wrapper_bounds_are_enforced() {
    // At this size, field64 has a 2-byte key and 4-byte length. The enclosing
    // RecognizerInfo length grows from 1 to 4 bytes: 9 added overhead bytes.
    let exact = MAX_MESSAGE_BYTES - hex::decode(NATIVE_GOLDEN).unwrap().len() - 9;
    let mut secondary = "é".repeat(exact / 2);
    if !exact.is_multiple_of(2) {
        secondary.push('x');
    }
    let mut config = ProtocolConfig {
        secondary_routing: Some(secondary),
        ..Default::default()
    };
    config.validate().unwrap();
    let request = initial_request(&config, 0).unwrap();
    assert_eq!(request.len(), MAX_MESSAGE_BYTES);
    assert!(fields(recognizer(&request)).contains(&(
        64,
        Value::Bytes(config.secondary_routing.as_ref().unwrap().as_bytes())
    )));
    config.secondary_routing.as_mut().unwrap().push('x');
    config.validate().unwrap();
    assert_eq!(initial_request(&config, 0), Err(Error::SizeLimit));

    config.secondary_routing = Some("x".repeat(MAX_MESSAGE_BYTES));
    config.validate().unwrap(); // Individual string allowed; wrapper still too large.
    assert_eq!(initial_request(&config, 0), Err(Error::SizeLimit));
    config.secondary_routing.as_mut().unwrap().push('x');
    assert_eq!(config.validate(), Err(Error::SizeLimit));

    let combined = ProtocolConfig {
        routing: "x".repeat(MAX_MESSAGE_BYTES / 2),
        secondary_routing: Some("y".repeat(MAX_MESSAGE_BYTES / 2)),
        block_offensive_words: Some(true),
        auto_punctuation: Some(false),
        ..Default::default()
    };
    combined.validate().unwrap();
    assert_eq!(initial_request(&combined, 0), Err(Error::SizeLimit));
}

#[test]
fn new_options_stay_private_and_init_only_without_legacy_selection_changes() {
    let config = ProtocolConfig {
        block_offensive_words: Some(false),
        auto_punctuation: Some(true),
        secondary_routing: Some("private-secondary-routing".into()),
        ..Default::default()
    };
    assert_eq!(
        format!("{config:?}"),
        format!("{:?}", ProtocolConfig::default())
    );
    // The legacy selection is already represented by before with cursor_aware=false.
    let context = EditingContext {
        before: "sélection 𝄞".into(),
        cursor_aware: false,
        ..Default::default()
    };
    let before = context_request(&context).unwrap();
    initial_request(&config, 0).unwrap();
    assert_eq!(context_request(&context).unwrap(), before);
    assert_eq!(
        before,
        hex::decode("9a4b110a0f73c3a96c656374696f6e20f09d849e").unwrap()
    );
    assert_eq!(
        audio_request(b"abc", 150).unwrap(),
        hex::decode("e28e8f0103489601ea8e8f01050a03616263").unwrap()
    );
    assert_eq!(end_request(), [0x10, 1, 0x18, 1]);
}

#[test]
fn trigger_application_id_is_an_optional_client_info_field_16() {
    fn client(request: &[u8]) -> Vec<u8> {
        let mut payloads = fields(request)
            .into_iter()
            .filter_map(|(n, v)| match (n, v) {
                (294000, Value::Bytes(bytes)) => Some(bytes.to_vec()),
                _ => None,
            });
        let payload = payloads.next().unwrap();
        assert!(payloads.next().is_none(), "one ClientInfo extension");
        payload
    }
    fn others(request: &[u8]) -> Vec<String> {
        fields(request)
            .into_iter()
            .filter(|(n, _)| *n != 294000)
            .map(|(n, v)| format!("{n}:{v:?}"))
            .collect()
    }
    let baseline = initial_request(&ProtocolConfig::default(), 0).unwrap();
    assert_eq!(
        client(&baseline),
        hex::decode("120667626f617264420452757374d00101").unwrap()
    );
    let config = ProtocolConfig {
        trigger_application_id: Some("com.example.notes".into()),
        ..Default::default()
    };
    let request = initial_request(&config, 0).unwrap();
    // Frozen vector: field2 "gboard", field8 "Rust", field16 package, field26 = 1.
    assert_eq!(
        client(&request),
        hex::decode("120667626f617264420452757374820111636f6d2e6578616d706c652e6e6f746573d00101")
            .unwrap()
    );
    assert_eq!(others(&request), others(&baseline));
    let empty = ProtocolConfig {
        trigger_application_id: Some(String::new()),
        ..Default::default()
    };
    assert!(empty.validate().is_err());
}
