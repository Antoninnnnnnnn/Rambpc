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

struct Message<'a> {
    fields: Vec<RefField<'a>>,
    raw: Bytes,
    unknown: Vec<Field>,
}
impl<'a> Message<'a> {
    fn new(
        parts: Vec<&'a [u8]>,
        known: &[(u32, u8)],
        budget: &mut Budget<'_>,
    ) -> Result<Self, Error> {
        let mut fields = Vec::new();
        for part in &parts {
            // Deliberately parse each occurrence before concatenating its raw bytes.
            fields.extend(parse_ref(part, &mut budget.fields)?);
        }
        let raw = raw_parts(&parts, budget.root)?;
        let unknown = fields
            .iter()
            .filter(|f| !known.contains(&(f.number, f.wire_type)))
            .map(|f| f.owned())
            .collect();
        Ok(Self {
            fields,
            raw,
            unknown,
        })
    }
    fn data(&self, number: u32, wire: u8) -> Vec<&'a [u8]> {
        self.fields
            .iter()
            .filter_map(|f| match f.value {
                RefValue::Data(d) if f.number == number && f.wire_type == wire => Some(d),
                _ => None,
            })
            .collect()
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
        self.data(number, 5).last().map(|d| {
            let mut b = [0; 4];
            b.copy_from_slice(d);
            f32::from_le_bytes(b)
        })
    }
    fn double(&self, number: u32) -> Option<f64> {
        self.data(number, 1).last().map(|d| {
            let mut b = [0; 8];
            b.copy_from_slice(d);
            f64::from_le_bytes(b)
        })
    }
    fn message<T>(
        &self,
        number: u32,
        budget: &mut Budget<'_>,
        parser: impl FnOnce(Vec<&'a [u8]>, &mut Budget<'_>) -> Result<T, Error>,
    ) -> Result<Option<T>, Error> {
        let parts = self.data(number, 2);
        if parts.is_empty() {
            Ok(None)
        } else {
            parser(parts, budget).map(Some)
        }
    }
    fn oneof(&self, numbers: &[u32]) -> Vec<(u32, Vec<&'a [u8]>)> {
        let mut runs: Vec<(u32, Vec<&'a [u8]>)> = Vec::new();
        for field in &self.fields {
            if field.wire_type != 2 || !numbers.contains(&field.number) {
                continue;
            }
            if let RefValue::Data(data) = field.value {
                if let Some((number, values)) = runs.last_mut()
                    && *number == field.number
                {
                    values.push(data);
                    continue;
                }
                runs.push((field.number, vec![data]));
            }
        }
        runs
    }
}
fn text_last(values: Vec<&[u8]>) -> Result<Option<String>, Error> {
    let mut last = None;
    for value in values {
        last = Some(std::str::from_utf8(value).map_err(|_| Error::Utf8)?);
    }
    Ok(last.map(str::to_owned))
}
fn context(parts: Vec<&[u8]>, budget: &mut Budget<'_>) -> Result<EditingContext, Error> {
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
fn voice_edit(parts: Vec<&[u8]>, budget: &mut Budget<'_>) -> Result<VoiceEditResult, Error> {
    let m = Message::new(parts, &[(1, 2), (2, 2), (3, 2), (4, 0), (7, 2)], budget)?;
    let mut out = VoiceEditResult {
        transcript: m.string(1)?,
        intent: m.integer(4),
        ..Default::default()
    };
    for (number, values) in m.oneof(&[2, 3, 7]) {
        out.active_field = Some(number);
        out.alternative_text = None;
        out.edited_text = None;
        out.edited_context = None;
        match number {
            2 => out.alternative_text = text_last(values)?,
            3 => out.edited_text = text_last(values)?,
            _ => out.edited_context = Some(context(values, budget)?),
        }
    }
    out.raw = m.raw;
    out.unknown_fields = m.unknown;
    Ok(out)
}
fn hypothesis(parts: Vec<&[u8]>, budget: &mut Budget<'_>) -> Result<Hypothesis, Error> {
    let mut m = Message::new(parts, &[(1, 2), (2, 5), (VOICE_EDITING_RESULT, 2)], budget)?;
    // Unlike open numeric enums, this safety marker only recognizes the two
    // verified wire values. Unknown occurrences remain opaque and never clear
    // an earlier recognized value (closed proto2 enum merge semantics).
    let safety_status = m.fields.iter().rev().find_map(|field| match field.value {
        RefValue::Varint(value @ (0 | 1)) if field.number == HYPOTHESIS_SAFETY_STATUS => {
            Some(value as i32)
        }
        _ => None,
    });
    m.unknown.retain(|field| {
        field.number != HYPOTHESIS_SAFETY_STATUS
            || !matches!(field.value, wire::Value::Varint(0 | 1))
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
fn partial(parts: Vec<&[u8]>, budget: &mut Budget<'_>) -> Result<PartialHypothesis, Error> {
    let m = Message::new(parts, &[(1, 2), (2, 1)], budget)?;
    Ok(PartialHypothesis {
        text: m.string(1)?,
        confidence: m.double(2),
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
fn hypothesis_fallback(
    parts: Vec<&[u8]>,
    budget: &mut Budget<'_>,
) -> Result<HypothesisFallback, Error> {
    let m = Message::new(parts, &[(1, 2)], budget)?;
    Ok(HypothesisFallback {
        hypothesis: m.message(1, budget, hypothesis)?,
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
fn hypotheses(parts: Vec<&[u8]>, budget: &mut Budget<'_>) -> Result<HypothesisResult, Error> {
    let m = Message::new(parts, &[(3, 2), (5, 2)], budget)?;
    let items = m
        .data(3, 2)
        .into_iter()
        .map(|p| hypothesis(vec![p], budget))
        .collect::<Result<_, _>>()?;
    Ok(HypothesisResult {
        hypotheses: items,
        fallback_field5: m.message(5, budget, hypothesis_fallback)?,
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
fn partials(parts: Vec<&[u8]>, budget: &mut Budget<'_>) -> Result<PartialResult, Error> {
    let m = Message::new(parts, &[(1, 2)], budget)?;
    let items = m
        .data(1, 2)
        .into_iter()
        .map(|p| partial(vec![p], budget))
        .collect::<Result<_, _>>()?;
    Ok(PartialResult {
        partials: items,
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
fn recognition(parts: Vec<&[u8]>, budget: &mut Budget<'_>) -> Result<RecognitionResult, Error> {
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
fn speech(parts: Vec<&[u8]>, budget: &mut Budget<'_>) -> Result<SpeechEventResult, Error> {
    let m = Message::new(parts, &[(1, 0)], budget)?;
    Ok(SpeechEventResult {
        event: m.integer(1),
        unknown_fields: m.unknown,
        raw: m.raw,
    })
}
const RESULT_ONEOF: &[u32] = &[1, 2, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 18, 19, 20, 21];
fn response_result(
    parts: Vec<&[u8]>,
    budget: &mut Budget<'_>,
) -> Result<RecognitionResponse, Error> {
    let m = Message::new(parts, &[(1, 2), (2, 2), (3, 2), (4, 2)], budget)?;
    let mut out = RecognitionResponse {
        language: m.string(3)?,
        spoken_language: m.string(4)?,
        ..Default::default()
    };
    for (number, values) in m.oneof(RESULT_ONEOF) {
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
            _ => raw_parts(&values, budget.root)?, // Opaque oneofs still clear the preceding member.
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
        vec![root.as_ref()],
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
