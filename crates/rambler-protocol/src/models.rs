use crate::wire::Field;
use bytes::Bytes;

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
    pub raw: Bytes,
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
    pub raw: Bytes,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hypothesis {
    pub text: Option<String>,
    pub confidence: Option<f32>,
    /// Verified closed proto2 safety marker: 0 is unblocked, 1 is blocked.
    /// Only exact unsigned wire values 0/1 are recognized, including overlong
    /// varint encodings of those values; larger values are not truncated to i32.
    /// The last recognized value wins. Unknown values and wrong-wire occurrences
    /// remain in `unknown_fields` for conservative caller policy, and do not clear
    /// an earlier recognized value. Absence is distinct from explicit zero.
    pub safety_status: Option<i32>,
    pub voice_edit: Option<VoiceEditResult>,
    pub unknown_fields: Vec<Field>,
    pub raw: Bytes,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartialHypothesis {
    pub text: Option<String>,
    pub confidence: Option<f64>,
    pub unknown_fields: Vec<Field>,
    pub raw: Bytes,
}
/// Structural wrapper (`aalo`), not an automatic fallback-selection policy.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HypothesisFallback {
    pub hypothesis: Option<Hypothesis>,
    pub unknown_fields: Vec<Field>,
    pub raw: Bytes,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HypothesisResult {
    pub hypotheses: Vec<Hypothesis>,
    /// Independent singular field 5; an empty wrapper still has presence.
    pub fallback_field5: Option<HypothesisFallback>,
    pub unknown_fields: Vec<Field>,
    pub raw: Bytes,
}
impl HypothesisResult {
    /// Structural traversal: repeated field 3, then field 5's optional hypothesis.
    /// This is neither a ranking nor a selection/fallback policy.
    pub fn all_hypotheses(&self) -> impl Iterator<Item = &Hypothesis> {
        self.hypotheses.iter().chain(
            self.fallback_field5
                .as_ref()
                .and_then(|fallback| fallback.hypothesis.as_ref()),
        )
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartialResult {
    pub partials: Vec<PartialHypothesis>,
    pub unknown_fields: Vec<Field>,
    pub raw: Bytes,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecognitionResult {
    pub state: Option<i32>,
    pub hypothesis_result: Option<HypothesisResult>,
    pub partial_result: Option<PartialResult>,
    /// Independent singular containers, not oneof members or ranked alternatives.
    pub hypothesis_result_field5: Option<HypothesisResult>,
    pub hypothesis_result_field7: Option<HypothesisResult>,
    pub unknown_fields: Vec<Field>,
    pub raw: Bytes,
}
impl RecognitionResult {
    /// Visit containers in canonical field order 3, 5, 7, including each fallback.
    /// Not ranking, global wire order, finality, or automatic text selection.
    /// The public containers retain the provenance of each hypothesis.
    pub fn all_hypotheses(&self) -> impl Iterator<Item = &Hypothesis> {
        [
            self.hypothesis_result.as_ref(),
            self.hypothesis_result_field5.as_ref(),
            self.hypothesis_result_field7.as_ref(),
        ]
        .into_iter()
        .flatten()
        .flat_map(HypothesisResult::all_hypotheses)
    }
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
    pub raw: Bytes,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecognitionResponse {
    pub recognition: Option<RecognitionResult>,
    pub speech: Option<SpeechEventResult>,
    /// Recognition response field 3 (`language`); preserved without normalization.
    pub language: Option<String>,
    /// Recognition response field 4 (`spoken_language`); preserved independently.
    pub spoken_language: Option<String>,
    pub active_field: Option<u32>,
    pub active_raw: Option<Bytes>,
    pub unknown_fields: Vec<Field>,
    pub raw: Bytes,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct S3Response {
    pub status: Option<i32>,
    pub error_code: Option<i32>,
    pub error_message: Option<String>,
    pub result: Option<RecognitionResponse>,
    pub unknown_fields: Vec<Field>,
    pub raw: Bytes,
}
impl S3Response {
    /// Structural traversal of the active recognition result; see
    /// [`RecognitionResult::all_hypotheses`] for ordering and policy limits.
    pub fn all_hypotheses(&self) -> impl Iterator<Item = &Hypothesis> {
        self.result
            .as_ref()
            .and_then(|r| r.recognition.as_ref())
            .into_iter()
            .flat_map(RecognitionResult::all_hypotheses)
    }
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
