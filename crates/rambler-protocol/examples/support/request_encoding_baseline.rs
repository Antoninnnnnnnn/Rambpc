//! Frozen unchanged d9af4f5 request-assembly bodies, using frozen wire helpers.
//! Config validation/type and EditingContext are shared unchanged public APIs;
//! the measured functions below are exact git-LF copies, not redesigned builders.
use crate::{
    baseline::{bytes_field as b, fixed32_field, string_field as s, varint_field as v},
    bounded,
};
use rambler_protocol::*;

fn elapsed(value: u64) -> Result<(), Error> {
    if value > i64::MAX as u64 {
        return Err(Error::InvalidConfig("elapsed_ms range"));
    }
    Ok(())
}
fn append(out: &mut Vec<u8>, field: Vec<u8>) -> Result<(), Error> {
    bounded(out.len().checked_add(field.len()).ok_or(Error::SizeLimit)?)?;
    out.extend(field);
    Ok(())
}

pub fn initial_request(config: &ProtocolConfig, elapsed_ms: u64) -> Result<Vec<u8>, Error> {
    config.validate()?;
    elapsed(elapsed_ms)?;
    let mut audio = fixed32_field(2, (config.sample_rate as f32).to_le_bytes())?;
    append(&mut audio, v(3, config.encoding as u64)?)?;
    append(&mut audio, v(4, config.channels as u64)?)?;
    append(&mut audio, v(9, elapsed_ms)?)?;
    let mut recognizer = b(1, &v(4, 0)?)?;
    for number in [5, 13, 20, 41] {
        append(&mut recognizer, v(number, 1)?)?;
    }
    if let Some(block) = config.block_offensive_words {
        append(&mut recognizer, v(7, if block { 2 } else { 0 })?)?;
    }
    if let Some(punctuation) = config.auto_punctuation {
        append(&mut recognizer, v(40, u64::from(punctuation))?)?;
    }
    append(&mut recognizer, s(50, &config.routing)?)?;
    if let Some(routing) = &config.secondary_routing {
        append(&mut recognizer, s(64, routing)?)?;
    }
    let mut client = s(2, &config.application_id)?;
    append(&mut client, s(8, &config.platform)?)?;
    append(&mut client, v(26, 1)?)?;
    let mut user = b(2, &s(1, &config.language)?)?;
    for language in &config.secondary_languages {
        let mut secondary = s(1, language)?;
        append(&mut secondary, v(2, 1)?)?;
        append(&mut user, b(4, &secondary)?)?;
    }
    let mut out = s(1, "beyond-a2a-recognizer")?;
    append(&mut out, v(2, 1)?)?;
    for (number, payload) in [
        (USER_INFO, user),
        (AUDIO_INFO, audio),
        (CLIENT_INFO, client),
        (RECOGNIZER_INFO, recognizer),
        (INITIAL_PARAMETER, v(1, 500)?),
    ] {
        append(&mut out, b(number, &payload)?)?;
    }
    if !config.bias_terms.is_empty() || !config.preferences.is_empty() {
        let mut hints = Vec::new();
        for (number, values) in [(3, &config.bias_terms), (5, &config.preferences)] {
            for value in values {
                append(&mut hints, s(number, value)?)?;
            }
        }
        append(&mut out, b(INPUT_BOX_INFO, &hints)?)?;
    }
    Ok(out)
}

pub fn context_request(context: &EditingContext) -> Result<Vec<u8>, Error> {
    for value in [&context.before, &context.selected, &context.after] {
        bounded(value.len())?;
    }
    let content = if context.cursor_aware {
        let mut cursor = s(1, &context.before)?;
        append(&mut cursor, s(2, &context.selected)?)?;
        append(&mut cursor, s(3, &context.after)?)?;
        b(4, &cursor)?
    } else {
        if !context.selected.is_empty() || !context.after.is_empty() {
            return Err(Error::InvalidConfig("selected/after require cursor_aware"));
        }
        s(1, &context.before)?
    };
    b(INPUT_BOX_INFO, &content)
}
