use rambler_protocol::{
    PartialHypothesis, PartialResult, RecognitionResult, S3Response, SpeechEventResult,
};

/// Structural view borrowing the original response. No candidate selection,
/// deduplication, global concatenation or authorization to apply text is implied.
#[derive(Clone, Copy, Debug)]
pub struct Presentation<'a> {
    pub response: &'a S3Response,
    pub content: PresentationContent<'a>,
}
/// Recognition state, independent of the presence of a final segment container.
/// Even Complete is not successful session completion: S3 END, full upload,
/// cleanup and terminal gRPC status must still be checked by the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecognitionCompletion {
    Complete,
    Other,
}
#[derive(Clone, Copy, Debug)]
pub enum PresentationContent<'a> {
    None,
    Recognition {
        completion: RecognitionCompletion,
        /// Presence of field3, even when its hypotheses list is empty.
        has_final_segment: bool,
        /// Presence of field4, even when its parts list is empty.
        has_partials: bool,
        /// Presence only; neither implies a Rambler final or automatic fallback.
        has_hypothesis_field5: bool,
        has_hypothesis_field7: bool,
        result: &'a RecognitionResult,
    },
    Speech(&'a SpeechEventResult),
    Opaque {
        field: u32,
        raw: Option<&'a [u8]>,
    },
}
impl<'a> Presentation<'a> {
    pub fn new(response: &'a S3Response) -> Self {
        let content =
            match response.result.as_ref() {
                Some(result) => match result.active_field {
                    Some(1) => result.recognition.as_ref().map_or(
                        PresentationContent::None,
                        |recognition| PresentationContent::Recognition {
                            completion: if recognition.state == Some(1) {
                                RecognitionCompletion::Complete
                            } else {
                                RecognitionCompletion::Other
                            },
                            has_final_segment: recognition.hypothesis_result.is_some(),
                            has_partials: recognition.partial_result.is_some(),
                            has_hypothesis_field5: recognition.hypothesis_result_field5.is_some(),
                            has_hypothesis_field7: recognition.hypothesis_result_field7.is_some(),
                            result: recognition,
                        },
                    ),
                    Some(2) => result
                        .speech
                        .as_ref()
                        .map_or(PresentationContent::None, PresentationContent::Speech),
                    Some(field) => PresentationContent::Opaque {
                        field,
                        raw: result.active_raw.as_deref(),
                    },
                    None => PresentationContent::None,
                },
                None => PresentationContent::None,
            };
        Self { response, content }
    }
}

/// Presentation of the sequential parts WITHIN ONE partial result, following
/// the observed S3 client helper. Never append these previews across responses.
/// Final hypotheses, in contrast, are alternative candidates, not text parts.
#[derive(Clone, Debug)]
pub struct PartialTranscript<'a> {
    pub stable_prefix: String,
    pub tentative_suffix: String,
    pub parts: &'a [PartialHypothesis],
    /// This is a client presentation label, not statistical confidence. The Java
    /// helper tests score < 0.9; NaN/+inf can therefore enter its stable prefix.
    /// We preserve that behavior explicitly and flag it, rather than clamp data.
    pub has_nonfinite_scores: bool,
}
impl<'a> PartialTranscript<'a> {
    pub fn from_result(result: &'a PartialResult) -> Self {
        let mut out = Self {
            stable_prefix: String::new(),
            tentative_suffix: String::new(),
            parts: &result.partials,
            has_nonfinite_scores: false,
        };
        let mut unstable = false;
        for part in &result.partials {
            // Missing text is ignored entirely, including its confidence. An
            // explicitly present empty text still participates in the latch.
            if let Some(text) = &part.text {
                let score = part.confidence.unwrap_or(0.0);
                out.has_nonfinite_scores |= !score.is_finite();
                if unstable || score < 0.9 {
                    unstable = true;
                    out.tentative_suffix.push_str(text);
                } else {
                    out.stable_prefix.push_str(text);
                }
            }
        }
        out
    }
    pub fn text(&self) -> String {
        let mut text =
            String::with_capacity(self.stable_prefix.len() + self.tentative_suffix.len());
        text.push_str(&self.stable_prefix);
        text.push_str(&self.tentative_suffix);
        text
    }
}
