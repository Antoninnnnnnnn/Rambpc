//! Independent offline vectors. Explicit Python platform is test-only parity data.
use rambler_protocol::{
    wire::{Value, parse_fields},
    *,
};

fn hex(s: &str) -> Vec<u8> {
    hex::decode(s).unwrap()
}
fn uint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while value > 127 {
        out.push((value % 128) as u8 + 128);
        value /= 128;
    }
    out.push(value as u8);
    out
}
fn blob(number: u32, value: impl AsRef<[u8]>) -> Vec<u8> {
    let value = value.as_ref();
    let mut out = uint(u64::from(number) * 8 + 2);
    out.extend(uint(value.len() as u64));
    out.extend(value);
    out
}
fn fields(data: &[u8]) -> Vec<wire::Field> {
    parse_fields(data).unwrap()
}
fn data(data: &[u8], number: u32) -> Vec<u8> {
    fields(data)
        .into_iter()
        .find_map(|f| match f.value {
            Value::Bytes(b) if f.number == number => Some(b),
            _ => None,
        })
        .unwrap()
}
fn python() -> ProtocolConfig {
    ProtocolConfig {
        platform: "Python".into(),
        ..Default::default()
    }
}
const GOLDEN_DEFAULT: &str = concat!(
    "0a156265796f6e642d6132612d7265636f676e697a65721001",
    "c2888f010912070a0566722d4652e28e8f010b1500007a46180a20014800",
    "82c78f0113120667626f6172644206507974686f6ed00101",
    "a2e68f013a0a02200028016801a00101c80201920329",
    "67626f6172645f67656d696e695f76335f73747265616d696e675f766f6963655f656469745f6d756c",
    "a2bfb7710308f403"
);
const GOLDEN_CUSTOM: &str = concat!(
    "0a156265796f6e642d6132612d7265636f676e697a65721001",
    "c2888f010912070a05656e2d4742e28e8f010c1500803b47182a200248b960",
    "82c78f011c12106d792d6e61746976652d636c69656e7442054c696e7578d00101",
    "a2e68f011f0a02200028016801a00101c8020192030e63616c6c65722d726f7574696e67",
    "a2bfb7710308f403"
);

#[test]
fn init_two_frozen_python_goldens_and_honest_native_default() {
    assert_eq!(initial_request(&python(), 0).unwrap(), hex(GOLDEN_DEFAULT));
    let custom = ProtocolConfig {
        language: "en-GB".into(),
        application_id: "my-native-client".into(),
        sample_rate: 48000,
        encoding: 42,
        channels: 2,
        routing: "caller-routing".into(),
        platform: "Linux".into(),
        ..Default::default()
    };
    assert_eq!(initial_request(&custom, 12345).unwrap(), hex(GOLDEN_CUSTOM));
    let native = ProtocolConfig::default();
    assert_eq!(native.platform, "Rust");
    assert_eq!(
        (native.sample_rate, native.encoding, native.channels),
        (16000, 10, 1)
    );
    let payload = initial_request(&native, 0).unwrap();
    assert_eq!(data(&data(&payload, CLIENT_INFO), 8), b"Rust");
    assert_ne!(payload, hex(GOLDEN_DEFAULT));
    assert_eq!(
        fields(&payload)
            .iter()
            .map(|f| f.number)
            .collect::<Vec<_>>(),
        [
            1,
            2,
            USER_INFO,
            AUDIO_INFO,
            CLIENT_INFO,
            RECOGNIZER_INFO,
            INITIAL_PARAMETER
        ]
    );
    assert_eq!(data(&data(&data(&payload, USER_INFO), 2), 1), b"fr-FR");
    assert_eq!(data(&data(&payload, RECOGNIZER_INFO), 1), [0x20, 0]);
    assert_eq!(data(&payload, INITIAL_PARAMETER), [0x08, 0xf4, 0x03]);
}

#[test]
fn audio_exact_and_opaque_continuous_stream() {
    assert_eq!(
        audio_request(b"abc", 150).unwrap(),
        hex("e28e8f0103489601ea8e8f01050a03616263")
    );
    let chunk = b"OggS\x00\xff\x80\x00not-a-packet-boundary";
    let payload = audio_request(chunk, 1 << 40).unwrap();
    assert_eq!(data(&data(&payload, AUDIO_DATA), 1), chunk);
    assert_eq!(
        fields(&data(&payload, AUDIO_INFO))[0].value,
        Value::Varint(1 << 40)
    );
    assert_eq!(
        data(&data(&audio_request(b"", 0).unwrap(), AUDIO_DATA), 1),
        b""
    );
    assert_eq!(end_request(), hex("10011801").as_slice());
}

#[test]
fn context_goldens_and_utf16_text_preserved_without_normalization() {
    for (context, expected) in [
        (
            EditingContext {
                before: "abc".into(),
                ..Default::default()
            },
            "9a4b050a03616263",
        ),
        (
            EditingContext {
                before: "a".into(),
                selected: "b".into(),
                after: "c".into(),
                cursor_aware: true,
                ..Default::default()
            },
            "9a4b0b22090a01611201621a0163",
        ),
        (EditingContext::default(), "9a4b020a00"),
        (
            EditingContext {
                cursor_aware: true,
                ..Default::default()
            },
            "9a4b0822060a0012001a00",
        ),
    ] {
        assert_eq!(context_request(&context).unwrap(), hex(expected));
    }
    let c = EditingContext {
        before: "avant é e\u{301}".into(),
        selected: "𝄞".into(),
        after: "après\0".into(),
        cursor_aware: true,
        ..Default::default()
    };
    assert_eq!(c.selected.encode_utf16().count(), 2);
    let cursor = data(&data(&context_request(&c).unwrap(), INPUT_BOX_INFO), 4);
    assert_eq!(
        fields(&cursor)
            .iter()
            .map(|f| match &f.value {
                Value::Bytes(b) => b.clone(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>(),
        [
            c.before.as_bytes(),
            c.selected.as_bytes(),
            c.after.as_bytes()
        ]
    );
    for c in [
        EditingContext {
            selected: "selection".into(),
            ..Default::default()
        },
        EditingContext {
            after: "after".into(),
            ..Default::default()
        },
    ] {
        assert!(matches!(context_request(&c), Err(Error::InvalidConfig(_))));
    }
}

#[test]
fn hint_goldens_empty_entries_unicode_order_duplicates() {
    for (bias, pref, expected) in [
        (vec!["A"], vec![], "1a0141"),
        (vec![], vec!["P"], "2a0150"),
        (vec![""], vec![], "1a00"),
        (vec![], vec![""], "2a00"),
        (
            vec!["A", "", "A"],
            vec!["P", "", "P"],
            "1a01411a001a01412a01502a002a0150",
        ),
        (
            vec!["é", "e\u{301}", "𝄞", "é"],
            vec!["  brut\n", "東京", ""],
            "1a02c3a91a0365cc811a04f09d849e1a02c3a92a072020627275740a2a06e69db1e4baac2a00",
        ),
    ] {
        let cfg = ProtocolConfig {
            bias_terms: bias.into_iter().map(str::to_owned).collect(),
            preferences: pref.into_iter().map(str::to_owned).collect(),
            ..python()
        };
        let payload = initial_request(&cfg, 0).unwrap();
        let inner = hex(expected);
        assert_eq!(
            payload,
            [hex(GOLDEN_DEFAULT), blob(INPUT_BOX_INFO, &inner)].concat()
        );
        assert_eq!(data(&payload, INPUT_BOX_INFO), inner);
    }
}

#[test]
fn hints_are_private_in_debug_and_stay_out_of_late_context() {
    let cfg = ProtocolConfig {
        bias_terms: vec!["private contact".into()],
        preferences: vec!["private preference".into()],
        secondary_languages: vec!["private locale".into()],
        ..Default::default()
    };
    assert_eq!(
        format!("{cfg:?}"),
        format!("{:?}", ProtocolConfig::default())
    );
    let original = cfg.clone();
    let context = EditingContext {
        before: "abc".into(),
        ..Default::default()
    };
    let before = context_request(&context).unwrap();
    initial_request(&cfg, 0).unwrap();
    assert_eq!(context_request(&context).unwrap(), before);
    assert_eq!(before, hex("9a4b050a03616263"));
    assert_eq!(cfg, original);
}

#[test]
fn hint_count_limits_are_per_category_and_keep_empty_entries() {
    for (bias, pref) in [(1024, 0), (0, 1024), (1024, 1024)] {
        let cfg = ProtocolConfig {
            bias_terms: vec![String::new(); bias],
            preferences: vec![String::new(); pref],
            ..Default::default()
        };
        let inner = fields(&data(&initial_request(&cfg, 0).unwrap(), INPUT_BOX_INFO));
        assert_eq!(inner.len(), bias + pref);
        assert!(
            inner[..bias]
                .iter()
                .all(|f| f.number == 3 && f.value == Value::Bytes(vec![]))
        );
        assert!(
            inner[bias..]
                .iter()
                .all(|f| f.number == 5 && f.value == Value::Bytes(vec![]))
        );
    }
    for cfg in [
        ProtocolConfig {
            bias_terms: vec![String::new(); MAX_HINTS_PER_CATEGORY + 1],
            ..Default::default()
        },
        ProtocolConfig {
            preferences: vec![String::new(); MAX_HINTS_PER_CATEGORY + 1],
            ..Default::default()
        },
    ] {
        assert!(cfg.validate().is_err());
        assert!(initial_request(&cfg, 0).is_err());
    }
}

#[test]
fn combined_hint_byte_boundary_counts_utf8_not_characters() {
    let mut cfg = ProtocolConfig {
        bias_terms: vec!["é".repeat(MAX_HINT_UTF8_BYTES / 2 - 1)],
        preferences: vec!["é".into()],
        ..Default::default()
    };
    cfg.validate().unwrap();
    let payload = initial_request(&cfg, 0).unwrap();
    let inner = fields(&data(&payload, INPUT_BOX_INFO));
    assert_eq!(
        inner
            .iter()
            .map(|f| match &f.value {
                Value::Bytes(b) => b.len(),
                _ => 0,
            })
            .sum::<usize>(),
        MAX_HINT_UTF8_BYTES
    );
    cfg.preferences[0].push('x');
    assert!(cfg.validate().is_err());
    assert!(initial_request(&cfg, 0).is_err());
    for cfg in [
        ProtocolConfig {
            bias_terms: vec![format!("{}x", "𝄞".repeat(MAX_HINT_UTF8_BYTES / 4))],
            ..Default::default()
        },
        ProtocolConfig {
            preferences: vec![format!("{}x", "𝄞".repeat(MAX_HINT_UTF8_BYTES / 4))],
            ..Default::default()
        },
    ] {
        assert!(initial_request(&cfg, 0).is_err());
    }
}

#[test]
fn secondary_languages_exact_raw_order_duplicates_and_primary_unchanged() {
    for primary in ["fr-FR", "zh-CN", "zh-Hans"] {
        let languages = ["en-US", "zh-CN", "zh-Hans", "en-US", " raw ", "é"];
        let cfg = ProtocolConfig {
            language: primary.into(),
            secondary_languages: languages.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let request = initial_request(&cfg, 0).unwrap();
        let mut expected_user = blob(2, blob(1, primary));
        for language in languages {
            expected_user.extend(blob(4, [blob(1, language), vec![0x10, 0x01]].concat()));
        }
        assert_eq!(data(&request, USER_INFO), expected_user);
        let baseline = initial_request(
            &ProtocolConfig {
                language: primary.into(),
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let unchanged = |bytes: &[u8]| {
            fields(bytes)
                .into_iter()
                .filter(|f| f.number != USER_INFO)
                .collect::<Vec<_>>()
        };
        assert_eq!(unchanged(&request), unchanged(&baseline));
        assert!(fields(&request).iter().all(|f| f.number != INPUT_BOX_INFO));
    }
}

#[test]
fn secondary_count_nonempty_and_total_byte_bounds() {
    assert_eq!(
        (MAX_SECONDARY_LANGUAGES, MAX_SECONDARY_LANGUAGE_UTF8_BYTES),
        (16, 1024)
    );
    let cfg = ProtocolConfig {
        secondary_languages: vec!["en-US".into(); 16],
        ..Default::default()
    };
    let user = data(&initial_request(&cfg, 0).unwrap(), USER_INFO);
    assert_eq!(fields(&user).iter().filter(|f| f.number == 4).count(), 16);
    let mut cfg = ProtocolConfig {
        secondary_languages: vec!["é".repeat(512)],
        ..Default::default()
    };
    initial_request(&cfg, 0).unwrap();
    cfg.secondary_languages.push("x".into());
    assert!(cfg.validate().is_err());
    for secondary_languages in [
        vec!["en-US".into(); 17],
        vec![String::new()],
        vec!["𝄞".repeat(257)],
    ] {
        assert!(
            initial_request(
                &ProtocolConfig {
                    secondary_languages,
                    ..Default::default()
                },
                0
            )
            .is_err()
        );
    }
}

#[test]
fn request_validation_and_wrapper_size_bounds() {
    for cfg in [
        ProtocolConfig {
            language: String::new(),
            ..Default::default()
        },
        ProtocolConfig {
            application_id: String::new(),
            ..Default::default()
        },
        ProtocolConfig {
            routing: String::new(),
            ..Default::default()
        },
        ProtocolConfig {
            platform: String::new(),
            ..Default::default()
        },
        ProtocolConfig {
            sample_rate: 0,
            ..Default::default()
        },
        ProtocolConfig {
            encoding: -1,
            ..Default::default()
        },
        ProtocolConfig {
            channels: 0,
            ..Default::default()
        },
    ] {
        assert!(cfg.validate().is_err());
        assert!(initial_request(&cfg, 0).is_err());
    }
    assert!(initial_request(&ProtocolConfig::default(), 1 << 63).is_err());
    assert!(audio_request(b"", 1 << 63).is_err());
    initial_request(&ProtocolConfig::default(), i64::MAX as u64).unwrap();
    audio_request(b"", i64::MAX as u64).unwrap();
    let oversized = "x".repeat(MAX_MESSAGE_BYTES + 1);
    assert!(
        initial_request(
            &ProtocolConfig {
                routing: oversized,
                ..Default::default()
            },
            0
        )
        .is_err()
    );
    assert_eq!(
        audio_request(&vec![0; MAX_MESSAGE_BYTES], 0),
        Err(Error::SizeLimit)
    );
    assert_eq!(
        context_request(&EditingContext {
            before: "x".repeat(MAX_MESSAGE_BYTES),
            ..Default::default()
        }),
        Err(Error::SizeLimit)
    );
}
