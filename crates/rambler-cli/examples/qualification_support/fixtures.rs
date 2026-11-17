//! Deterministic synthetic PCM and hand-built S3 protobuf oracles.
//!
//! The PCM is raw signed PCM16LE, mono, 16 kHz, without a WAV header. Every case
//! has an odd number of samples, so its byte length is even and its sample tail
//! cannot be mistaken for a byte tail. Audio samples and response text are
//! independently invented; neither is derived from the other.
//!
//! | id | samples | replies before S3 END | independent final oracle |
//! |---|---:|---|---|
//! | `literal` | 17 | one state-1 ASR result | hypothesis text |
//! | `rewrite` | 33 | one state-1 intent-3/member-3 result | edited text |
//! | `selection` | 49 | one state-1 intent-2/member-7 result | returned context text |
//! | `preview` | 641 | two state-0 field-4 previews, then a literal final | hypothesis text |
//!
//! Corpus-local guards are at most 641 samples, four replies, and 512 bytes per
//! reply. The preview input supplies a non-silent 320-sample initial frame plus
//! a non-silent 321-sample odd tail. Each reply sequence has exactly one state-1
//! result followed by a separate raw S3 END response. Preview pairs are
//! `(stable, tentative)` within each update; updates are replacements, not
//! fragments to concatenate.

use bytes::Bytes;
use rambler_engine::{EditingContext, TextProvenance};

const RECOGNITION_RESPONSE: u32 = 1_253_625;
const VOICE_EDITING_RESULT: u32 = 74_956_224;
const S3_END: &[u8] = b"\x08\x01";

const SELECTION_BEFORE: &str = "Préface 🧭 | ";
const SELECTION_AFTER: &str = " | épilogue 📚";
const RETURNED_SELECTION: &str = "révisé 🪐";

pub struct Case {
    pub id: &'static str,
    pub pcm: Vec<u8>,
    pub context: EditingContext,
    pub replies: Vec<Bytes>,
    pub expected: Expected,
}

pub struct Expected {
    pub partials: Vec<(&'static str, &'static str)>,
    pub final_text: &'static str,
    pub provenance: TextProvenance,
    pub intent: Option<i32>,
    /// Selection in the returned member-7 context, in UTF-16 code units.
    pub selection_utf16: Option<(usize, usize)>,
}

pub fn cases() -> Vec<Case> {
    let rewrite_edit = [
        varint_field(4, 3),
        string_field(3, "Rewrite keeps this synthetic wording 🪶."),
    ]
    .concat();
    let returned_context = context_wire(SELECTION_BEFORE, RETURNED_SELECTION, SELECTION_AFTER);
    let selection_edit = [varint_field(4, 2), bytes_field(7, &returned_context)].concat();

    vec![
        Case {
            id: "literal",
            pcm: pcm(1, 17),
            context: context("Literal synthetic context.", "", "", false),
            replies: vec![literal_final("Literal transcript seventeen."), end()],
            expected: Expected {
                partials: vec![],
                final_text: "Literal transcript seventeen.",
                provenance: TextProvenance::HypothesisText,
                intent: None,
                selection_utf16: None,
            },
        },
        Case {
            id: "rewrite",
            pcm: pcm(2, 33),
            context: context("Rewrite synthetic context.", "", "", false),
            replies: vec![
                edited_final("ASR wording is deliberately not the rewrite.", rewrite_edit),
                end(),
            ],
            expected: Expected {
                partials: vec![],
                final_text: "Rewrite keeps this synthetic wording 🪶.",
                provenance: TextProvenance::VoiceEditText,
                intent: Some(3),
                selection_utf16: None,
            },
        },
        Case {
            id: "selection",
            pcm: pcm(3, 49),
            context: context(SELECTION_BEFORE, "choix 🦀", SELECTION_AFTER, true),
            replies: vec![
                edited_final(
                    "ASR wording is deliberately not the selection.",
                    selection_edit,
                ),
                end(),
            ],
            expected: Expected {
                partials: vec![],
                final_text: "Préface 🧭 | révisé 🪐 | épilogue 📚",
                provenance: TextProvenance::VoiceEditContext,
                intent: Some(2),
                selection_utf16: Some((13, 22)),
            },
        },
        Case {
            id: "preview",
            pcm: pcm(4, 641),
            context: context("Preview synthetic context.", "", "", false),
            replies: vec![
                preview("Amber ", 0.97, "wren", 0.76),
                preview("Amber wren ", 0.99, "circles", 0.71),
                literal_final("Amber wren rests."),
                end(),
            ],
            expected: Expected {
                partials: vec![("Amber ", "wren"), ("Amber wren ", "circles")],
                final_text: "Amber wren rests.",
                provenance: TextProvenance::HypothesisText,
                intent: None,
                selection_utf16: None,
            },
        },
    ]
}

fn context(before: &str, selected: &str, after: &str, cursor_aware: bool) -> EditingContext {
    EditingContext {
        before: before.into(),
        selected: selected.into(),
        after: after.into(),
        cursor_aware,
        ..Default::default()
    }
}

fn pcm(seed: i32, samples: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples * 2);
    for index in 0..samples {
        let square = (index * index) % 97;
        let sample = ((index * 733 + seed as usize * 1597 + square) % 8192) as i32 - 4096;
        out.extend_from_slice(&(sample as i16).to_le_bytes());
    }
    out
}

fn literal_final(text: &str) -> Bytes {
    final_with_hypothesis(string_field(1, text))
}

fn edited_final(asr: &str, edit: Vec<u8>) -> Bytes {
    final_with_hypothesis(
        [
            string_field(1, asr),
            bytes_field(VOICE_EDITING_RESULT, &edit),
        ]
        .concat(),
    )
}

fn final_with_hypothesis(hypothesis: Vec<u8>) -> Bytes {
    let hypotheses = bytes_field(3, &hypothesis);
    response([varint_field(1, 1), bytes_field(3, &hypotheses)].concat())
}

fn preview(stable: &str, stable_score: f64, tentative: &str, tentative_score: f64) -> Bytes {
    let parts = [
        bytes_field(1, &partial_part(stable, stable_score)),
        bytes_field(1, &partial_part(tentative, tentative_score)),
    ]
    .concat();
    response([varint_field(1, 0), bytes_field(4, &parts)].concat())
}

fn partial_part(text: &str, score: f64) -> Vec<u8> {
    let mut out = string_field(1, text);
    push_varint(&mut out, (2 << 3) | 1);
    out.extend_from_slice(&score.to_le_bytes());
    out
}

fn response(recognition: Vec<u8>) -> Bytes {
    Bytes::from(bytes_field(
        RECOGNITION_RESPONSE,
        &bytes_field(1, &recognition),
    ))
}

fn end() -> Bytes {
    Bytes::from_static(S3_END)
}

fn context_wire(before: &str, selected: &str, after: &str) -> Vec<u8> {
    [
        string_field(1, before),
        string_field(2, selected),
        string_field(3, after),
    ]
    .concat()
}

fn string_field(number: u32, value: &str) -> Vec<u8> {
    bytes_field(number, value.as_bytes())
}

fn bytes_field(number: u32, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    push_varint(&mut out, (u64::from(number) << 3) | 2);
    push_varint(&mut out, value.len() as u64);
    out.extend_from_slice(value);
    out
}

fn varint_field(number: u32, value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    push_varint(&mut out, u64::from(number) << 3);
    push_varint(&mut out, value);
    out
}

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum Value<'a> {
        Varint(u64),
        Fixed64(&'a [u8]),
        Bytes(&'a [u8]),
    }

    #[derive(Clone, Copy)]
    struct Field<'a> {
        number: u32,
        value: Value<'a>,
    }

    #[test]
    fn corpus_is_ordered_deterministic_bounded_and_directly_distinct() {
        let cases = cases();
        assert_eq!(
            cases.iter().map(|case| case.id).collect::<Vec<_>>(),
            ["literal", "rewrite", "selection", "preview"]
        );
        for (case, (seed, samples)) in cases.iter().zip([(1, 17), (2, 33), (3, 49), (4, 641)]) {
            assert_eq!(case.pcm.len(), samples * 2, "{}", case.id);
            assert_eq!(case.pcm.len() / 2 % 2, 1, "{}", case.id);
            assert!(case.pcm.len() <= 641 * 2, "{}", case.id);
            assert!(
                case.pcm
                    .chunks_exact(2)
                    .any(|sample| sample.iter().any(|&byte| byte != 0))
            );
            assert_eq!(case.pcm, pcm(seed, samples));
            assert!(case.context.raw.is_empty());
            assert!(case.context.unknown_fields.is_empty());
            assert_eq!(
                case.context.cursor_aware,
                !case.context.selected.is_empty() || !case.context.after.is_empty()
            );
        }
        let preview = &cases[3];
        assert_eq!(preview.id, "preview");
        assert_eq!(preview.pcm.len() / 2, 641);
        assert!(preview.pcm.len() / 2 > 320);
        let initial_frame = &preview.pcm[..320 * 2];
        let odd_tail = &preview.pcm[320 * 2..];
        assert_eq!(odd_tail.len() / 2, 321);
        assert_eq!(odd_tail.len() % 2, 0);
        assert!(
            initial_frame
                .chunks_exact(2)
                .any(|sample| sample.iter().any(|&byte| byte != 0))
        );
        assert!(
            odd_tail
                .chunks_exact(2)
                .any(|sample| sample.iter().any(|&byte| byte != 0))
        );
        for (index, left) in cases.iter().enumerate() {
            for right in &cases[index + 1..] {
                assert_ne!(left.pcm, right.pcm, "{} / {}", left.id, right.id);
                assert_ne!(
                    left.expected.final_text, right.expected.final_text,
                    "{} / {}",
                    left.id, right.id
                );
            }
        }
        let repeated = super::cases();
        for (left, right) in cases.iter().zip(&repeated) {
            assert_eq!(left.pcm, right.pcm);
            assert_eq!(left.replies, right.replies);
            assert_eq!(left.expected.final_text, right.expected.final_text);
            assert_eq!(left.expected.partials, right.expected.partials);
        }
    }

    #[test]
    fn response_stages_intents_active_members_and_limits_are_exact() {
        for case in cases() {
            assert!((2..=4).contains(&case.replies.len()), "{}", case.id);
            assert!(case.replies.iter().all(|reply| reply.len() <= 512));
            assert_eq!(case.replies.last().unwrap().as_ref(), S3_END);
            let final_reply = &case.replies[case.replies.len() - 2];
            let recognition = recognition(final_reply);
            assert_eq!(varint(field(recognition, 1)), 1, "{}", case.id);
            assert!(fields(recognition).iter().all(|field| field.number != 4));
            let hypothesis = final_hypothesis(final_reply);

            match case.id {
                "literal" | "preview" => {
                    assert!(
                        fields(hypothesis)
                            .iter()
                            .all(|field| field.number != VOICE_EDITING_RESULT)
                    );
                    assert_eq!(text(field(hypothesis, 1)), case.expected.final_text);
                    assert_eq!(case.expected.intent, None);
                    assert_eq!(case.expected.provenance, TextProvenance::HypothesisText);
                }
                "rewrite" | "selection" => {
                    let edit = bytes(field(hypothesis, VOICE_EDITING_RESULT));
                    assert_eq!(varint(field(edit, 4)), case.expected.intent.unwrap() as u64);
                    assert_ne!(text(field(hypothesis, 1)), case.expected.final_text);
                    let active = fields(edit)
                        .iter()
                        .filter(|field| [2, 3, 7].contains(&field.number))
                        .map(|field| field.number)
                        .collect::<Vec<_>>();
                    if case.id == "rewrite" {
                        assert_eq!(active, [3]);
                        assert_eq!(text(field(edit, 3)), case.expected.final_text);
                        assert_eq!(case.expected.provenance, TextProvenance::VoiceEditText);
                    } else {
                        assert_eq!(active, [7]);
                        assert_eq!(case.expected.provenance, TextProvenance::VoiceEditContext);
                    }
                }
                other => panic!("unexpected case {other}"),
            }
        }
    }

    #[test]
    fn selection_returns_preserved_sides_and_independent_utf16_range() {
        let cases = cases();
        let case = &cases[2];
        assert_eq!(case.id, "selection");
        let hypothesis = final_hypothesis(&case.replies[0]);
        let edit = bytes(field(hypothesis, VOICE_EDITING_RESULT));
        let returned = bytes(field(edit, 7));
        assert_eq!(text(field(returned, 1)), SELECTION_BEFORE);
        assert_eq!(text(field(returned, 2)), RETURNED_SELECTION);
        assert_eq!(text(field(returned, 3)), SELECTION_AFTER);
        assert_eq!(text(field(returned, 1)), case.context.before);
        assert_eq!(text(field(returned, 3)), case.context.after);
        assert_ne!(text(field(returned, 2)), case.context.selected);
        assert_eq!(case.context.selected.encode_utf16().count(), 8);
        assert_eq!(RETURNED_SELECTION.encode_utf16().count(), 9);

        // Independent arithmetic for the static oracle: 13 before, 9 selected.
        assert_eq!(SELECTION_BEFORE.encode_utf16().count(), 13);
        assert_eq!(case.expected.selection_utf16, Some((13, 22)));
        assert_eq!(
            case.expected.final_text,
            "Préface 🧭 | révisé 🪐 | épilogue 📚"
        );
    }

    #[test]
    fn preview_updates_are_distinct_stable_tentative_replacements() {
        let cases = cases();
        let case = &cases[3];
        assert_eq!(case.id, "preview");
        assert_eq!(case.replies.len(), 4);
        assert_ne!(case.replies[0], case.replies[1]);

        for (reply, &(stable, tentative)) in case.replies[..2].iter().zip(&case.expected.partials) {
            let recognition = recognition(reply);
            assert_eq!(varint(field(recognition, 1)), 0);
            assert!(fields(recognition).iter().all(|field| field.number != 3));
            let partials = bytes(field(recognition, 4));
            let parts = fields(partials);
            assert_eq!(parts.len(), 2);
            assert!(parts.iter().all(|field| field.number == 1));
            let stable_part = bytes(parts[0]);
            let tentative_part = bytes(parts[1]);
            assert_eq!(text(field(stable_part, 1)), stable);
            assert_eq!(text(field(tentative_part, 1)), tentative);
            assert!(fixed64(field(stable_part, 2)) >= 0.9);
            assert!(fixed64(field(tentative_part, 2)) < 0.9);
        }
        assert_eq!(case.expected.partials.len(), 2);
        assert_ne!(case.expected.partials[0], case.expected.partials[1]);
        assert_eq!(
            text(field(final_hypothesis(&case.replies[2]), 1)),
            "Amber wren rests."
        );
    }

    fn recognition(reply: &[u8]) -> &[u8] {
        let result = bytes(field(reply, RECOGNITION_RESPONSE));
        bytes(field(result, 1))
    }

    fn final_hypothesis(reply: &[u8]) -> &[u8] {
        let recognition = recognition(reply);
        let hypotheses = bytes(field(recognition, 3));
        bytes(field(hypotheses, 3))
    }

    fn field(message: &[u8], number: u32) -> Field<'_> {
        let matching = fields(message)
            .into_iter()
            .filter(|field| field.number == number)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "field {number}");
        matching[0]
    }

    fn varint(field: Field<'_>) -> u64 {
        match field.value {
            Value::Varint(value) => value,
            _ => panic!("field {} is not a varint", field.number),
        }
    }

    fn fixed64(field: Field<'_>) -> f64 {
        match field.value {
            Value::Fixed64(value) => f64::from_le_bytes(value.try_into().unwrap()),
            _ => panic!("field {} is not fixed64", field.number),
        }
    }

    fn bytes<'a>(field: Field<'a>) -> &'a [u8] {
        match field.value {
            Value::Bytes(value) => value,
            _ => panic!("field {} is not bytes", field.number),
        }
    }

    fn text(field: Field<'_>) -> &str {
        std::str::from_utf8(bytes(field)).unwrap()
    }

    fn fields(mut message: &[u8]) -> Vec<Field<'_>> {
        let mut out = Vec::new();
        while !message.is_empty() {
            let key = take_varint(&mut message);
            let number = u32::try_from(key >> 3).unwrap();
            let value = match key & 7 {
                0 => Value::Varint(take_varint(&mut message)),
                1 => {
                    let (value, rest) = message.split_at(8);
                    message = rest;
                    Value::Fixed64(value)
                }
                2 => {
                    let length = take_varint(&mut message) as usize;
                    assert!(length <= message.len());
                    let (value, rest) = message.split_at(length);
                    message = rest;
                    Value::Bytes(value)
                }
                wire => panic!("unsupported test wire type {wire}"),
            };
            out.push(Field { number, value });
        }
        out
    }

    fn take_varint(input: &mut &[u8]) -> u64 {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = input[0];
            *input = &input[1..];
            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return value;
            }
            shift += 7;
            assert!(shift < 64);
        }
    }
}
