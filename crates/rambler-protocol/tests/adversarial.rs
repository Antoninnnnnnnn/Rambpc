//! Bounded deterministic grammar tests. The oracle reduces generated operations;
//! it never parses bytes or calls production wire/encoding helpers.
use rambler_protocol::{
    wire::{Field, Value},
    *,
};

const SEEDS: [u64; 4] = [1, 0x5eed_cafe, 0xdead_beef_0123_4567, 0x9e37_79b9_7f4a_7c15];
struct Rng(u64);
impl Rng {
    fn pick(&mut self, bound: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % bound
    }
}
// Immutable raw migration: the independent oracle builds its own copied bytes.
fn append_raw(raw: &mut bytes::Bytes, suffix: impl AsRef<[u8]>) {
    let mut owned = raw.to_vec();
    owned.extend_from_slice(suffix.as_ref());
    *raw = owned.into();
}
fn uint(mut n: u64, padded: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        bytes.push((n & 127) as u8);
        n >>= 7;
        if n == 0 {
            break;
        }
        *bytes.last_mut().unwrap() |= 128;
    }
    if padded && bytes.len() < 10 {
        *bytes.last_mut().unwrap() |= 128;
        bytes.push(0);
    }
    bytes
}
fn scalar(n: u32, v: u64, padded: bool) -> Vec<u8> {
    [uint(u64::from(n) * 8, padded), uint(v, padded)].concat()
}
fn blob(n: u32, v: &[u8], padded: bool) -> Vec<u8> {
    [
        uint(u64::from(n) * 8 + 2, padded),
        uint(v.len() as u64, padded),
        v.to_vec(),
    ]
    .concat()
}
fn unknown(rng: &mut Rng, n: u32) -> Field {
    let padded = rng.pick(2) == 0;
    let (wire_type, value, body) = match rng.pick(4) {
        0 => {
            let v = rng.0;
            (0, Value::Varint(v), uint(v, padded))
        }
        1 => {
            let v = rng.0.to_le_bytes();
            (1, Value::Fixed64(v), v.to_vec())
        }
        2 => {
            // Deliberately invalid as UTF-8 and as a nested message: opaque data.
            let v = vec![0xff, 0, 0x80, rng.pick(256) as u8];
            (
                2,
                Value::Bytes(v.clone()),
                [uint(v.len() as u64, padded), v].concat(),
            )
        }
        _ => {
            let v = (rng.0 as u32).to_le_bytes();
            (5, Value::Fixed32(v), v.to_vec())
        }
    };
    let raw = [uint(u64::from(n) * 8 + u64::from(wire_type), padded), body].concat();
    Field {
        number: n,
        wire_type,
        value,
        raw,
    }
}

// A new member clears the previous one; repeating the current member merges it.
// This operation-level model is independent of the decoder's run partitioning.
fn activate(model: &mut RecognitionResponse, n: u32, raw: &[u8]) {
    if model.active_field != Some(n) {
        model.recognition = None;
        model.speech = None;
        model.active_raw = Some(Default::default());
    }
    model.active_field = Some(n);
    append_raw(model.active_raw.as_mut().unwrap(), raw);
}
fn generated_response(seed: u64, sample: usize) -> (Vec<u8>, S3Response, usize) {
    let mut rng = Rng(seed);
    let mut model = RecognitionResponse::default();
    let mut root = S3Response::default();
    let mut payload = Vec::new();
    let mut part = Vec::new();
    let mut cost = 0;
    for step in 0..64 {
        let padded = rng.pick(2) == 0;
        let mut nested_cost = 0;
        let raw = match rng.pick(9) {
            0 | 1 => {
                let mut rec_raw = Vec::new();
                let state = rng.pick(3) != 0;
                let v = rng.0;
                if state {
                    rec_raw.extend(scalar(1, v, padded));
                    nested_cost += 1;
                }
                let text = format!("{seed:x}/{sample}/{step}/é");
                let hyp_raw = blob(1, text.as_bytes(), padded);
                let add_hyp = rng.pick(2) == 0;
                if add_hyp {
                    rec_raw.extend(blob(3, &blob(3, &hyp_raw, padded), padded));
                    nested_cost += 3;
                }
                let extra = unknown(&mut rng, 55);
                rec_raw.extend_from_slice(&extra.raw);
                nested_cost += 1;
                activate(&mut model, 1, &rec_raw);
                let rec = model.recognition.get_or_insert_with(Default::default);
                if state {
                    rec.state = Some(v as u32 as i32);
                }
                if add_hyp {
                    let list = rec.hypothesis_result.get_or_insert_with(Default::default);
                    list.hypotheses.push(Hypothesis {
                        text: Some(text),
                        raw: hyp_raw.clone().into(),
                        ..Default::default()
                    });
                    append_raw(&mut list.raw, blob(3, &hyp_raw, padded));
                }
                rec.unknown_fields.push(extra);
                append_raw(&mut rec.raw, &rec_raw);
                blob(1, &rec_raw, padded)
            }
            2 => {
                let v = rng.0;
                let speech_raw = scalar(1, v, padded);
                activate(&mut model, 2, &speech_raw);
                let speech = model.speech.get_or_insert_with(Default::default);
                speech.event = Some(v as u32 as i32);
                append_raw(&mut speech.raw, &speech_raw);
                nested_cost = 1;
                blob(2, &speech_raw, padded)
            }
            3 => {
                let n =
                    [5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 18, 19, 20, 21][rng.pick(15) as usize];
                let bytes = vec![0xff, 0, rng.pick(256) as u8];
                let raw = blob(n, &bytes, padded);
                activate(&mut model, n, &bytes);
                model.unknown_fields.push(Field {
                    number: n,
                    wire_type: 2,
                    value: Value::Bytes(bytes),
                    raw: raw.clone(),
                });
                raw
            }
            4 | 5 => {
                let n = 3 + rng.pick(2) as u32;
                let text = if rng.pick(3) == 0 {
                    String::new()
                } else {
                    format!("independent-{step}")
                };
                let raw = blob(n, text.as_bytes(), padded);
                if n == 3 {
                    model.language = Some(text);
                } else {
                    model.spoken_language = Some(text);
                }
                raw
            }
            6 => {
                // Wrong wire types of known oneof members must not switch it.
                let n = 1 + rng.pick(2) as u32;
                let v = rng.0;
                let raw = scalar(n, v, padded);
                model.unknown_fields.push(Field {
                    number: n,
                    wire_type: 0,
                    value: Value::Varint(v),
                    raw: raw.clone(),
                });
                raw
            }
            _ => {
                let field = unknown(&mut rng, 50);
                let raw = field.raw.clone();
                model.unknown_fields.push(field);
                raw
            }
        };
        cost += 1 + nested_cost;
        append_raw(&mut model.raw, &raw);
        part.extend(raw);
        if rng.pick(5) == 0 || step == 63 {
            payload.extend(blob(RECOGNITION_RESPONSE, &part, padded));
            part.clear();
            cost += 1;
            // Root unknowns interleave singular-message occurrences without
            // changing their merge semantics or spending a nested field budget.
            let field = unknown(&mut rng, 51);
            payload.extend_from_slice(&field.raw);
            root.unknown_fields.push(field);
            cost += 1;
        }
    }
    root.raw = payload.clone().into();
    root.result = Some(model);
    (payload, root, cost)
}

#[test]
fn generated_proto2_operations_match_model_across_occurrence_partitions() {
    for seed in SEEDS {
        for sample in 0..32 {
            let (payload, expected, _) =
                generated_response(seed.wrapping_add(sample as u64), sample);
            assert_eq!(
                decode_response(&payload).unwrap(),
                expected,
                "seed={seed:x}, sample={sample}"
            );
        }
    }
}

#[test]
fn generated_budget_oracle_counts_discarded_members_but_not_opaque_contents() {
    for seed in SEEDS {
        let (payload, _, cost) = generated_response(seed, 0);
        let mut exact = [8, 0].repeat(MAX_FIELDS - cost);
        exact.extend(payload);
        decode_response(&exact).unwrap_or_else(|e| panic!("seed={seed:x}, cost={cost}: {e:?}"));
        exact.extend([8, 0]);
        assert_eq!(
            decode_response(&exact),
            Err(Error::FieldLimit),
            "seed={seed:x}, cost={cost}"
        );
    }
}

// An occurrence boundary may only split between fields. Splitting a single valid
// field into two otherwise concatenable messages must never repair truncation,
// even when the first member is later replaced by another oneof member.
fn split_occurrences_at_known_levels(a: &[u8], b: &[u8]) -> [Vec<u8>; 6] {
    let in_hypothesis = |body: &[u8]| {
        blob(
            RECOGNITION_RESPONSE,
            &blob(1, &blob(3, &blob(3, body, false), false), false),
            false,
        )
    };
    [
        [
            blob(RECOGNITION_RESPONSE, a, false),
            blob(RECOGNITION_RESPONSE, b, false),
        ]
        .concat(),
        blob(
            RECOGNITION_RESPONSE,
            &[blob(1, a, false), blob(1, b, false)].concat(),
            false,
        ),
        blob(
            RECOGNITION_RESPONSE,
            &[blob(1, a, false), blob(2, &[], false), blob(1, b, false)].concat(),
            false,
        ),
        blob(
            RECOGNITION_RESPONSE,
            &blob(1, &[blob(3, a, false), blob(3, b, false)].concat(), false),
            false,
        ),
        in_hypothesis(
            &[
                blob(VOICE_EDITING_RESULT, a, false),
                blob(VOICE_EDITING_RESULT, b, false),
            ]
            .concat(),
        ),
        in_hypothesis(&blob(
            VOICE_EDITING_RESULT,
            &[blob(7, a, false), blob(7, b, false)].concat(),
            false,
        )),
    ]
}

#[test]
fn every_interior_field_cut_is_rejected_at_each_singular_merge_level() {
    let fields = [
        scalar(50, u64::MAX, true),
        blob(50, "é/longer payload".as_bytes(), true),
        [uint(50 * 8 + 1, true), vec![0xff; 8]].concat(),
        [uint(50 * 8 + 5, true), vec![0xff; 4]].concat(),
    ];
    for (case, field) in fields.iter().enumerate() {
        for cut in 0..=field.len() {
            let candidates = split_occurrences_at_known_levels(&field[..cut], &field[cut..]);
            for (level, payload) in candidates.iter().enumerate() {
                // Whole-field occurrences are positive controls for every path.
                let expected_valid = cut == 0 || cut == field.len();
                assert_eq!(
                    decode_response(payload).is_ok(),
                    expected_valid,
                    "case={case}, cut={cut}, level={level}"
                );
            }
        }
    }
}

#[test]
fn exact_eight_mib_opaque_root_retains_noncanonical_length_and_invalid_contents() {
    // Three-byte padded key plus five-byte padded length at this size.
    let contents = vec![0xff; MAX_MESSAGE_BYTES - 8];
    let payload = blob(1000, &contents, true);
    assert_eq!(payload.len(), MAX_MESSAGE_BYTES);
    let decoded = decode_response(&payload).unwrap();
    assert_eq!(decoded.raw, payload);
    assert_eq!(decoded.unknown_fields.len(), 1);
    assert_eq!(decoded.unknown_fields[0].raw, payload);
    assert_eq!(
        decoded.unknown_fields[0].value,
        Value::Bytes(contents.to_vec())
    );
    let mut over = payload;
    over.push(0);
    assert_eq!(decode_response(&over), Err(Error::SizeLimit));
}
