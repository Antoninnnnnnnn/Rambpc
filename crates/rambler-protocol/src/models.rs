use crate::wire::Field;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum S3Status {
    Normal = 0,
    End = 1,
    Error = 2,
    NotStarted = 3,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum SpeechEvent {
    StartSpeech = 0,
    EndSpeech = 1,
    EndUtterance = 3,
}

/// Caller context and decoded cursor-context data. No normalization or edit policy.
/// Equality follows Python's context model: only `raw` is excluded; text,
/// cursor-awareness and unknown fields all participate.
#[derive(Clone, Debug, Default, Eq)]
pub struct EditingContext {
    pub before: String,
    pub selected: String,
    pub after: String,
    pub cursor_aware: bool,
    pub raw: Vec<u8>,
    pub unknown_fields: Vec<Field>,
}
impl PartialEq for EditingContext {
    fn eq(&self, other: &Self) -> bool {
        self.before == other.before
            && self.selected == other.selected
            && self.after == other.after
            && self.cursor_aware == other.cursor_aware
            && self.unknown_fields == other.unknown_fields
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoiceEditResult {
    pub transcript: Option<String>,
    pub alternative_text: Option<String>,
    pub edited_text: Option<String>,
    pub edited_context: Option<EditingContext>,
    pub intent: Option<i32>,
    /// Active oneof member 2, 3, or 7. These candidates are never auto-applied.
    pub active_field: Option<u32>,
    pub unknown_fields: Vec<Field>,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hypothesis {
    pub text: Option<String>,
    pub confidence: Option<f32>,
    pub voice_edit: Option<VoiceEditResult>,
    pub unknown_fields: Vec<Field>,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartialHypothesis {
    pub text: Option<String>,
    pub confidence: Option<f64>,
    pub unknown_fields: Vec<Field>,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HypothesisResult {
    pub hypotheses: Vec<Hypothesis>,
    pub unknown_fields: Vec<Field>,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartialResult {
    pub partials: Vec<PartialHypothesis>,
    pub unknown_fields: Vec<Field>,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecognitionResult {
    pub state: Option<i32>,
    pub hypothesis_result: Option<HypothesisResult>,
    pub partial_result: Option<PartialResult>,
    pub unknown_fields: Vec<Field>,
    pub raw: Vec<u8>,
}
impl RecognitionResult {
    pub fn hypotheses(&self) -> &[Hypothesis] {
        self.hypothesis_result
            .as_ref()
            .map_or(&[], |r| &r.hypotheses)
    }
    pub fn partials(&self) -> &[PartialHypothesis] {
        self.partial_result.as_ref().map_or(&[], |r| &r.partials)
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpeechEventResult {
    pub event: Option<i32>,
    pub unknown_fields: Vec<Field>,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecognitionResponse {
    pub recognition: Option<RecognitionResult>,
    pub speech: Option<SpeechEventResult>,
    pub result_id: Option<String>,
    pub alternative_text: Option<String>,
    pub active_field: Option<u32>,
    pub active_raw: Option<Vec<u8>>,
    pub unknown_fields: Vec<Field>,
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct S3Response {
    pub status: Option<i32>,
    pub error_code: Option<i32>,
    pub error_message: Option<String>,
    pub result: Option<RecognitionResponse>,
    pub unknown_fields: Vec<Field>,
    pub raw: Vec<u8>,
}
impl S3Response {
    pub fn recognition_state(&self) -> Option<i32> {
        self.result.as_ref()?.recognition.as_ref()?.state
    }
    pub fn speech_event(&self) -> Option<i32> {
        self.result.as_ref()?.speech.as_ref()?.event
    }
    pub fn hypotheses(&self) -> &[Hypothesis] {
        self.result
            .as_ref()
            .and_then(|r| r.recognition.as_ref())
            .map_or(&[], RecognitionResult::hypotheses)
    }
    pub fn partials(&self) -> &[PartialHypothesis] {
        self.result
            .as_ref()
            .and_then(|r| r.recognition.as_ref())
            .map_or(&[], RecognitionResult::partials)
    }
}
