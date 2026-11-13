use rambler_protocol::{RecognitionResult, S3Response, SpeechEventResult};

/// Structural view of one response, borrowing the original data without selecting,
/// merging, deduplicating, or simplifying text. This is neither authorization to
/// apply an edit nor evidence of a completed session, even for segment state 1 or
/// S3 END. Completion requires the engine's terminal checks.
#[derive(Clone, Copy, Debug)]
pub struct Presentation<'a> {
    /// Original response, including status, errors, unknown fields and raw bytes.
    pub response: &'a S3Response,
    pub content: PresentationContent<'a>,
}

/// State 1 marks a final recognition update, not a globally completed result.
/// Missing and unknown states are preserved in `RecognitionResult::state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Final,
    Other,
}

/// Classification of the active result oneof. A recognition may contain both
/// partials and hypotheses; inspect both original containers independently.
#[derive(Clone, Copy, Debug)]
pub enum PresentationContent<'a> {
    None,
    Recognition {
        segment: SegmentKind,
        result: &'a RecognitionResult,
    },
    Speech(&'a SpeechEventResult),
    /// A decoded but otherwise uninterpreted oneof member.
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
                            segment: if recognition.state == Some(1) {
                                SegmentKind::Final
                            } else {
                                SegmentKind::Other
                            },
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
