#![allow(dead_code)]
use bytes::Bytes;
use rambler_protocol::{
    HYPOTHESIS_SAFETY_STATUS, RECOGNITION_RESPONSE, VOICE_EDITING_RESULT,
    wire::{bytes_field as b, string_field as s, varint_field as v},
};

/// Entirely public synthetic text. No captured audio, credentials or corpus.
pub fn response(index: usize, text_bytes: usize, final_update: bool) -> Bytes {
    let text = format!("synthetic-{index:08}-{}", "x".repeat(text_bytes));
    let mut edit = s(1, &text).unwrap();
    edit.extend(v(4, 2).unwrap());
    edit.extend(s(3, &text).unwrap());
    let mut hypothesis = s(1, &text).unwrap();
    hypothesis.extend(v(HYPOTHESIS_SAFETY_STATUS, 0).unwrap());
    hypothesis.extend(b(VOICE_EDITING_RESULT, &edit).unwrap());
    // Retain an opaque unknown field at a nested and root level.
    hypothesis.extend(b(99, b"opaque-nested").unwrap());
    let hypotheses = b(3, &hypothesis).unwrap();
    let mut recognition = v(1, u64::from(final_update)).unwrap();
    recognition.extend(b(3, &hypotheses).unwrap());
    let mut result = b(1, &recognition).unwrap();
    result.extend(s(3, &format!("result-{index:08}")).unwrap());
    let mut root = b(RECOGNITION_RESPONSE, &result).unwrap();
    root.extend(b(101, b"opaque-root").unwrap());
    root.into()
}

pub fn audio(index: usize, size: usize) -> Bytes {
    assert!(size >= 8);
    let mut data = vec![0x5a; size];
    data[..8].copy_from_slice(&(index as u64).to_le_bytes());
    data.into()
}

pub fn context() -> rambler_protocol::EditingContext {
    rambler_protocol::EditingContext {
        before: "synthetic before 𝄞 ".repeat(64),
        selected: "synthetic selection é".into(),
        after: " synthetic after".repeat(64),
        cursor_aware: true,
        ..Default::default()
    }
}

/// Frozen e427fb1 audio request assembly, retained only as a profiling reference.
/// Production always uses rambler_protocol::audio_request.
pub fn reference_audio_request(
    chunk: &[u8],
    elapsed: u64,
) -> Result<Vec<u8>, rambler_protocol::Error> {
    use rambler_protocol::{AUDIO_DATA, AUDIO_INFO, Error, MAX_MESSAGE_BYTES};
    if chunk.len() > MAX_MESSAGE_BYTES {
        return Err(Error::SizeLimit);
    }
    if elapsed > i64::MAX as u64 {
        return Err(Error::InvalidConfig("elapsed_ms range"));
    }
    fn append(out: &mut Vec<u8>, field: Vec<u8>) -> Result<(), Error> {
        if out.len().checked_add(field.len()).ok_or(Error::SizeLimit)? > MAX_MESSAGE_BYTES {
            return Err(Error::SizeLimit);
        }
        out.extend(field);
        Ok(())
    }
    let mut out = b(AUDIO_INFO, &v(9, elapsed)?)?;
    append(&mut out, b(AUDIO_DATA, &b(1, chunk)?)?)?;
    Ok(out)
}

pub fn arg(name: &str, default: usize) -> usize {
    let args: Vec<_> = std::env::args().collect();
    let value = args
        .windows(2)
        .find(|a| a[0] == name)
        .map(|a| a[1].parse::<usize>().expect("integer argument"))
        .unwrap_or(default);
    assert!(value > 0, "arguments must be positive");
    value
}
