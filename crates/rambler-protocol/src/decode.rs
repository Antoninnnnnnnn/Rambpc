use crate::{
    wire::{Field, RefField, RefValue, parse_ref},
    *,
};

use bytes::Bytes;

struct Budget<'a> {
    root: &'a Bytes,
    fields: usize,
}

// Single occurrences view the exact, independently owned root payload. A true
// proto2 merge needs a contiguous logical concatenation, allocated exactly once.
fn raw_parts(parts: &[&[u8]], root: &Bytes) -> Result<Bytes, Error> {
    let mut len = 0usize;
    for part in parts {
        len = len.checked_add(part.len()).ok_or(Error::SizeLimit)?;
        bounded(len)?;
    }
    if let [part] = parts {
        return Ok(root.slice_ref(part));
    }
    let mut raw = Vec::with_capacity(len);
    for part in parts {
        raw.extend_from_slice(part);
    }
    Ok(raw.into())
}

enum Parts<'a> {
    Single([&'a [u8]; 1]),
    Multiple(Vec<&'a [u8]>),
}
impl<'a> Parts<'a> {
    fn as_slice(&self) -> &[&'a [u8]] {
        match self {
            Self::Single(parts) => parts,
            Self::Multiple(parts) => parts,
        }
    }
}

struct Message<'a> {
    fields: Vec<RefField<'a>>,
    raw: Bytes,
    unknown: Vec<Field>,
}
impl<'a> Message<'a> {
    fn new(
        parts: &[&'a [u8]],
        known: &[(u32, u8)],
        budget: &mut Budget<'_>,
    ) -> Result<Self, Error> {
        Self::new_matching(
            parts,
            |field| known.contains(&(field.number, field.wire_type)),
            budget,
        )
    }
    fn new_matching(
        parts: &[&'a [u8]],
        is_known: impl Fn(&RefField<'a>) -> bool,
        budget: &mut Budget<'_>,
    ) -> Result<Self, Error> {
        let mut parts_iter = parts.iter();
        // Reuse the first parse allocation rather than copying its field vector.
        let mut fields = match parts_iter.next() {
            Some(part) => parse_ref(part, &mut budget.fields)?,
            None => Vec::new(),
        };
        for part in parts_iter {
            // Each occurrence is validated separately; fragments cannot heal.
            fields.extend(parse_ref(part, &mut budget.fields)?);
        }
        let raw = raw_parts(parts, budget.root)?;
        let unknown = fields
            .iter()
            .filter(|field| !is_known(field))
            .map(|f| f.owned())
            .collect();
        Ok(Self {
            fields,
            raw,
            unknown,
        })
    }
    fn data(
        &self,
        number: u32,
        wire: u8,
    ) -> impl DoubleEndedIterator<Item = &'a [u8]> + Clone + '_ {
        self.fields.iter().filter_map(move |f| match f.value {
            RefValue::Data(d) if f.number == number && f.wire_type == wire => Some(d),
            _ => None,
        })
    }
    fn integer(&self, number: u32) -> Option<i32> {
        self.fields.iter().rev().find_map(|f| match f.value {
            RefValue::Varint(v) if f.number == number => Some(v as u32 as i32),
            _ => None,
        })
    }
    fn string(&self, number: u32) -> Result<Option<String>, Error> {
        text_last(self.data(number, 2))
    }
    fn float(&self, number: u32) -> Option<f32> {
        self.data(number, 5).next_back().map(|d| {
            let mut b = [0; 4];
            b.copy_from_slice(d);
            f32::from_le_bytes(b)
        })
    }
    fn double(&self, number: u32) -> Option<f64> {
        self.data(number, 1).next_back().map(|d| {
            let mut b = [0; 8];
            b.copy_from_slice(d);
            f64::from_le_bytes(b)
        })
    }
    fn message<T>(
        &self,
        number: u32,
        budget: &mut Budget<'_>,
        parser: impl FnOnce(&[&'a [u8]], &mut Budget<'_>) -> Result<T, Error>,
    ) -> Result<Option<T>, Error> {
        let mut parts = self.data(number, 2);
        let Some(first) = parts.next() else {
            return Ok(None);
        };
        if let Some(second) = parts.next() {
            let mut merged = vec![first, second];
            merged.extend(parts);
            parser(&merged, budget).map(Some)
        } else {
            parser(&[first], budget).map(Some)
        }
    }
    // Validate the first child before allocating output storage. The hint looks
    // at at most four parsed fields BEFORE filtering, never all unknown fields
    // or all wrappers whose children have not yet been validated.
    fn repeated<T>(
        &self,
        number: u32,
        budget: &mut Budget<'_>,
        mut parser: impl FnMut(&[&'a [u8]], &mut Budget<'_>) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        let mut parts = self.data(number, 2);
        let Some(part) = parts.next() else {
            return Ok(Vec::new());
        };
        let first = parser(&[part], budget)?;
        let capacity = self
            .fields
            .iter()
            .take(4)
            .filter(|field| field.number == number && field.wire_type == 2)
            .count()
            .max(1);
        let mut items = Vec::with_capacity(capacity);
        items.push(first);
        for part in parts {
            let item = parser(&[part], budget)?;
            items.push(item);
        }
        Ok(items)
    }
    fn oneof<'m>(&'m self, numbers: &'m [u32]) -> impl Iterator<Item = (u32, Parts<'a>)> + 'm {
        let mut selected = self
            .fields
            .iter()
            .filter_map(move |field| match field.value {
                RefValue::Data(data) if field.wire_type == 2 && numbers.contains(&field.number) => {
                    Some((field.number, data))
                }
                _ => None,
            })
            .peekable();
        // Noise and wrong-wire fields do not split a selected-member run. Only
        // real same-member merges need heap storage for their occurrence parts.
        std::iter::from_fn(move || {
            let (number, first) = selected.next()?;
            let Some((_, second)) = selected.next_if(|(next, _)| *next == number) else {
                return Some((number, Parts::Single([first])));
            };
            let mut parts = vec![first, second];
            while let Some((_, data)) = selected.next_if(|(next, _)| *next == number) {
                parts.push(data);
            }
            Some((number, Parts::Multiple(parts)))
        })
    }
}
fn text_last<'a>(values: impl IntoIterator<Item = &'a [u8]>) -> Result<Option<String>, Error> {
    let mut last = None;
    for value in values {
        last = Some(std::str::from_utf8(value).map_err(|_| Error::Utf8)?);
    }
    Ok(last.map(str::to_owned))
}
fn context(parts: &[&[u8]], budget: &mut Budget<'_>) -> Result<EditingContext, Error> {
    let m = Message::new(parts, &[(1, 2), (2, 2), (3, 2)], budget)?;
    Ok(EditingContext {
        before: m.string(1)?.unwrap_or_default(),
        selected: m.string(2)?.unwrap_or_default(),
        after: m.string(3)?.unwrap_or_default(),
        cursor_aware: true,
        raw: m.raw,
        unknown_fields: m.unknown,
    })
}
fn voice_edit(parts: &[&[u8]], budget: &mut Budget<'_>) -> Result<VoiceEditResult, Error> {
    let m = Message::new(parts, &[(1, 2), (2, 2), (3, 2), (4, 0), (7, 2)], budget)?;
    let mut out = VoiceEditResult {
        transcript: m.string(1)?,
        intent: m.integer(4),
        ..Default::default()
    };
    for (number, parts) in m.oneof(&[2, 3, 7]) {
        let values = parts.as_slice();
        out.active_field = Some(number);
        out.alternative_text = None;
        out.edited_text = None;
        out.edited_context = None;
        match number {
            2 => out.alternative_text = text_last(values.iter().copied())?,
            3 => out.edited_text = text_last(values.iter().copied())?,
            _ => out.edited_context = Some(context(values, budget)?),
        }
    }
    out.raw = m.raw;
    out.unknown_fields = m.unknown;
    Ok(out)
}
fn hypothesis(parts: &[&[u8]], budget: &mut Budget<'_>) -> Result<Hypothesis, Error> {
    // Classify verified values while fields are borrowed, before unknown fields
    // acquire owned storage. Other values and wire types must remain opaque.
    let m = Message::new_matching(
        parts,
        |field| {
            [(1, 2), (2, 5), (VOICE_EDITING_RESULT, 2)].contains(&(field.number, field.wire_type))
                || (field.number == HYPOTHESIS_SAFETY_STATUS
                    && matches!(field.value, RefValue::Varint(0 | 1)))
        },
        budget,
    )?;
    // Unlike open numeric enums, this safety marker only recognizes the two
    // verified wire values. Unknown occurrences remain opaque and never clear
    // an earlier recognized value (closed proto2 enum merge semantics).
    let safety_status = m.fields.iter().rev().find_map(|field| match field.value {
        RefValue::Varint(value @ (0 | 1)) if field.number == HYPOTHESIS_SAFETY_STATUS => {
            Some(value as i32)
        }
        _ => None,
    });
    Ok(Hypothesis {
        text: m.string(1)?,
        confidence: m.float(2),
        safety_status,
        voice_edit: m.message(VOICE_EDITING_RESULT, budget, voice_edit)?,
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
fn partial(parts: &[&[u8]], budget: &mut Budget<'_>) -> Result<PartialHypothesis, Error> {
    let m = Message::new(parts, &[(1, 2), (2, 1)], budget)?;
    Ok(PartialHypothesis {
        text: m.string(1)?,
        confidence: m.double(2),
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
fn hypothesis_fallback(
    parts: &[&[u8]],
    budget: &mut Budget<'_>,
) -> Result<HypothesisFallback, Error> {
    let m = Message::new(parts, &[(1, 2)], budget)?;
    Ok(HypothesisFallback {
        hypothesis: m.message(1, budget, hypothesis)?,
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
fn hypotheses(parts: &[&[u8]], budget: &mut Budget<'_>) -> Result<HypothesisResult, Error> {
    let m = Message::new(parts, &[(3, 2), (5, 2)], budget)?;
    let items = m.repeated(3, budget, hypothesis)?;
    Ok(HypothesisResult {
        hypotheses: items,
        fallback_field5: m.message(5, budget, hypothesis_fallback)?,
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
fn partials(parts: &[&[u8]], budget: &mut Budget<'_>) -> Result<PartialResult, Error> {
    let m = Message::new(parts, &[(1, 2)], budget)?;
    let items = m.repeated(1, budget, partial)?;
    Ok(PartialResult {
        partials: items,
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
fn recognition(parts: &[&[u8]], budget: &mut Budget<'_>) -> Result<RecognitionResult, Error> {
    let m = Message::new(parts, &[(1, 0), (3, 2), (4, 2), (5, 2), (7, 2)], budget)?;
    Ok(RecognitionResult {
        state: m.integer(1),
        hypothesis_result: m.message(3, budget, hypotheses)?,
        partial_result: m.message(4, budget, partials)?,
        hypothesis_result_field5: m.message(5, budget, hypotheses)?,
        hypothesis_result_field7: m.message(7, budget, hypotheses)?,
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
fn speech(parts: &[&[u8]], budget: &mut Budget<'_>) -> Result<SpeechEventResult, Error> {
    let m = Message::new(parts, &[(1, 0)], budget)?;
    Ok(SpeechEventResult {
        event: m.integer(1),
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
const RESULT_ONEOF: &[u32] = &[1, 2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 18, 19, 20, 21];
fn response_result(parts: &[&[u8]], budget: &mut Budget<'_>) -> Result<RecognitionResponse, Error> {
    let m = Message::new(parts, &[(1, 2), (2, 2), (3, 2), (4, 2)], budget)?;
    let mut out = RecognitionResponse {
        language: m.string(3)?,
        spoken_language: m.string(4)?,
        ..Default::default()
    };
    for (number, parts) in m.oneof(RESULT_ONEOF) {
        let values = parts.as_slice();
        out.active_field = Some(number);
        out.recognition = None;
        out.speech = None;
        out.active_raw = Some(match number {
            1 => {
                let parsed = recognition(values, budget)?;
                let raw = parsed.raw.clone();
                out.recognition = Some(parsed);
                raw
            }
            2 => {
                let parsed = speech(values, budget)?;
                let raw = parsed.raw.clone();
                out.speech = Some(parsed);
                raw
            }
            _ => raw_parts(values, budget.root)?, // Opaque oneofs still clear the preceding member.
        });
    }
    out.raw = m.raw;
    out.unknown_fields = m.unknown;
    Ok(out)
}
/// Decode with a single 100000-field global budget and an 8MiB root bound.
/// Every known occurrence is validated, even when overwritten. S3 errors are data.
pub fn decode_response(payload: &[u8]) -> Result<S3Response, Error> {
    bounded(payload.len())?;
    // Never retain a transport/framer allocation beyond this exact payload.
    let root = Bytes::copy_from_slice(payload);
    let mut budget = Budget {
        root: &root,
        fields: MAX_FIELDS,
    };
    let m = Message::new(
        &[root.as_ref()],
        &[(1, 0), (2, 0), (3, 2), (RECOGNITION_RESPONSE, 2)],
        &mut budget,
    )?;
    Ok(S3Response {
        status: m.integer(1),
        error_code: m.integer(2),
        error_message: m.string(3)?,
        result: m.message(RECOGNITION_RESPONSE, &mut budget, response_result)?,
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}

#[cfg(test)]
mod oneof_run_tests {
    use super::*;

    fn data(number: u32, wire_type: u8, value: &'static [u8]) -> RefField<'static> {
        RefField {
            number,
            wire_type,
            value: RefValue::Data(value),
            raw: &[],
        }
    }
    fn scalar(number: u32) -> RefField<'static> {
        RefField {
            number,
            wire_type: 0,
            value: RefValue::Varint(9),
            raw: &[],
        }
    }

    #[test]
    fn selected_runs_match_explicit_tables_and_borrow_original_parts() {
        const A: &[u8] = b"a";
        const B: &[u8] = b"b";
        const C: &[u8] = b"c";
        const D: &[u8] = b"d";
        struct Case {
            members: &'static [u32],
            fields: Vec<RefField<'static>>,
            runs: Vec<(u32, Vec<&'static [u8]>)>,
        }
        let cases = [
            Case {
                members: RESULT_ONEOF,
                fields: vec![],
                runs: vec![],
            },
            Case {
                members: RESULT_ONEOF,
                fields: vec![data(3, 2, A), data(14, 2, B), scalar(2)],
                runs: vec![],
            },
            Case {
                members: RESULT_ONEOF,
                fields: vec![data(1, 2, A)],
                runs: vec![(1, vec![A])],
            },
            Case {
                members: RESULT_ONEOF,
                fields: vec![
                    data(1, 2, A),
                    data(3, 2, C),
                    data(14, 2, D),
                    scalar(2),
                    data(1, 1, &[0xff; 8]),
                    data(2, 5, &[0xff; 4]),
                    data(1, 2, B),
                ],
                runs: vec![(1, vec![A, B])],
            },
            Case {
                members: RESULT_ONEOF,
                fields: vec![data(1, 2, A), data(2, 2, B), data(1, 2, C), data(1, 2, D)],
                runs: vec![(1, vec![A]), (2, vec![B]), (1, vec![C, D])],
            },
            Case {
                members: RESULT_ONEOF,
                fields: vec![data(1, 2, A), data(5, 2, B), data(1, 2, C)],
                runs: vec![(1, vec![A]), (5, vec![B]), (1, vec![C])],
            },
            Case {
                members: RESULT_ONEOF,
                fields: vec![data(5, 2, A), data(4, 2, D), data(5, 2, B), data(6, 2, C)],
                runs: vec![(5, vec![A, B]), (6, vec![C])],
            },
            Case {
                members: &[2, 3, 7],
                fields: vec![
                    data(7, 2, A),
                    data(1, 2, C),
                    scalar(4),
                    scalar(3),
                    data(7, 2, B),
                ],
                runs: vec![(7, vec![A, B])],
            },
            Case {
                members: &[2, 3, 7],
                fields: vec![data(7, 2, A), data(3, 2, B), data(7, 2, C), data(7, 2, D)],
                runs: vec![(7, vec![A]), (3, vec![B]), (7, vec![C, D])],
            },
        ];
        for (index, case) in cases.into_iter().enumerate() {
            let message = Message {
                fields: case.fields,
                raw: Bytes::new(),
                unknown: Vec::new(),
            };
            let mut runs = message.oneof(case.members);
            for (number, expected_parts) in case.runs {
                let (actual_number, parts) = runs.next().expect("missing selected run");
                assert_eq!(actual_number, number, "case {index}");
                assert_eq!(parts.as_slice(), expected_parts.as_slice(), "case {index}");
                assert_eq!(
                    matches!(&parts, Parts::Single(_)),
                    expected_parts.len() == 1
                );
                for (actual, expected) in parts.as_slice().iter().zip(expected_parts) {
                    assert_eq!(actual.as_ptr(), expected.as_ptr(), "case {index}");
                }
            }
            assert!(runs.next().is_none(), "extra selected run in case {index}");
            assert!(
                runs.next().is_none(),
                "non-exhausted iterator in case {index}"
            );
        }
    }
}
