use crate::{
    wire::{bytes_field as b, fixed32_field, string_field as s, varint_field as v},
    *,
};
use std::fmt;

/// Public fields are convenient for struct-update syntax; `validate` and request
/// assembly check all bounds before wire allocation. Hints are excluded from Debug.
#[derive(Clone, PartialEq, Eq)]
pub struct ProtocolConfig {
    pub language: String,
    pub application_id: String,
    pub sample_rate: i32,
    pub encoding: i32,
    pub channels: i32,
    pub routing: String,
    pub platform: String,
    pub bias_terms: Vec<String>,
    pub preferences: Vec<String>,
    pub secondary_languages: Vec<String>,
}
impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            language: "fr-FR".into(),
            application_id: "gboard".into(),
            sample_rate: 16000,
            encoding: 10,
            channels: 1,
            routing: "gboard_gemini_v3_streaming_voice_edit_mul".into(),
            platform: "Rust".into(),
            bias_terms: vec![],
            preferences: vec![],
            secondary_languages: vec![],
        }
    }
}
impl fmt::Debug for ProtocolConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtocolConfig")
            .field("language", &self.language)
            .field("application_id", &self.application_id)
            .field("sample_rate", &self.sample_rate)
            .field("encoding", &self.encoding)
            .field("channels", &self.channels)
            .field("routing", &self.routing)
            .field("platform", &self.platform)
            .finish()
    }
}
impl ProtocolConfig {
    pub fn validate(&self) -> Result<(), Error> {
        for value in [
            &self.language,
            &self.application_id,
            &self.routing,
            &self.platform,
        ] {
            if value.is_empty() {
                return Err(Error::InvalidConfig("nonempty text required"));
            }
            bounded(value.len())?;
        }
        if self.sample_rate < 1 || self.encoding < 0 || self.channels < 1 {
            return Err(Error::InvalidConfig("audio integer range"));
        }
        let remaining = strings(
            &self.bias_terms,
            MAX_HINTS_PER_CATEGORY,
            MAX_HINT_UTF8_BYTES,
            false,
        )?;
        strings(&self.preferences, MAX_HINTS_PER_CATEGORY, remaining, false)?;
        strings(
            &self.secondary_languages,
            MAX_SECONDARY_LANGUAGES,
            MAX_SECONDARY_LANGUAGE_UTF8_BYTES,
            true,
        )?;
        Ok(())
    }
}
fn strings(
    values: &[String],
    count: usize,
    mut remaining: usize,
    nonempty: bool,
) -> Result<usize, Error> {
    if values.len() > count {
        return Err(Error::InvalidConfig("SDK string count limit"));
    }
    for value in values {
        if nonempty && value.is_empty() {
            return Err(Error::InvalidConfig("nonempty secondary language required"));
        }
        remaining = remaining
            .checked_sub(value.len())
            .ok_or(Error::InvalidConfig("SDK total UTF-8 byte limit"))?;
    }
    Ok(remaining)
}
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
/// Verified init subset, not a claim of backend support for custom audio enums.
/// Native defaults honestly declare platform Rust. Hints occur only at init.
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
    append(&mut recognizer, s(50, &config.routing)?)?;
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
fn varint_len(mut value: u64) -> usize {
    let mut len = 1;
    while value >= 128 {
        value >>= 7;
        len += 1;
    }
    len
}
fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Wrap bytes from one continuous Ogg/Opus stream; no re-encoding or splitting.
pub fn audio_request(chunk: &[u8], elapsed_ms: u64) -> Result<Vec<u8>, Error> {
    bounded(chunk.len())?;
    elapsed(elapsed_ms)?;
    // Validate every nested wrapper and the complete message before allocation.
    // This is the same field order and canonical wire encoding as b/v assembly,
    // but copies the opaque audio only once instead of through three Vecs.
    let audio_info = 1 + varint_len(elapsed_ms);
    let audio_data = chunk
        .len()
        .checked_add(1 + varint_len(chunk.len() as u64))
        .ok_or(Error::SizeLimit)?;
    bounded(audio_data)?;
    let wrapped_data = audio_data
        .checked_add(varint_len((u64::from(AUDIO_DATA) << 3) | 2) + varint_len(audio_data as u64))
        .ok_or(Error::SizeLimit)?;
    bounded(wrapped_data)?;
    let total = wrapped_data
        .checked_add(
            varint_len((u64::from(AUDIO_INFO) << 3) | 2)
                + varint_len(audio_info as u64)
                + audio_info,
        )
        .ok_or(Error::SizeLimit)?;
    bounded(total)?;
    let mut out = Vec::with_capacity(total);
    push_varint(&mut out, (u64::from(AUDIO_INFO) << 3) | 2);
    push_varint(&mut out, audio_info as u64);
    push_varint(&mut out, 9 << 3);
    push_varint(&mut out, elapsed_ms);
    push_varint(&mut out, (u64::from(AUDIO_DATA) << 3) | 2);
    push_varint(&mut out, audio_data as u64);
    push_varint(&mut out, (1 << 3) | 2);
    push_varint(&mut out, chunk.len() as u64);
    out.extend_from_slice(chunk);
    debug_assert_eq!(out.len(), total);
    Ok(out)
}
/// Build extension1203 only, after audio and before end. No hints are copied here.
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
/// Transport must half-close after sending this exact marker.
pub const fn end_request() -> [u8; 4] {
    [0x10, 0x01, 0x18, 0x01]
}
