use crate::{
    AudioSource, EditingContext, Engine, EngineError, EngineSession, S3Response, SessionMetrics,
    SessionOptions,
};
use rambler_protocol::{HYPOTHESIS_SAFETY_STATUS, Hypothesis};

#[derive(Clone, Copy, Debug)]
pub struct CollectorLimits {
    pub max_final_updates: usize,
    /// Original protobuf bytes, not Rust allocation/object overhead.
    pub max_result_bytes: usize,
}
impl Default for CollectorLimits {
    fn default() -> Self {
        Self {
            max_final_updates: 128,
            max_result_bytes: 8 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct DictationResult {
    /// Every recognition-state-1 response, ordered, without merging/deduplication.
    pub final_updates: Vec<S3Response>,
    /// The actual late snapshot, not a snapshot taken when start was called.
    pub context: Option<EditingContext>,
    pub metrics: Option<SessionMetrics>,
}
impl DictationResult {
    /// Refuse simplified rendering and editor proposals when any retained final
    /// candidate is safety-blocked, or has an uninterpreted field-30 occurrence.
    /// Structured responses remain explicitly available to callers. The Android
    /// evidence covers accepted VoiceEdit candidates; applying this guard to all
    /// candidates and unknown values is intentionally conservative.
    pub fn check_safety(&self) -> Result<(), EngineError> {
        let mut unrecognized = false;
        for hypothesis in self.final_updates.iter().flat_map(S3Response::hypotheses) {
            if hypothesis.safety_status == Some(1) {
                return Err(EngineError::SafetyBlocked);
            }
            unrecognized |= hypothesis.safety_status.is_some_and(|value| value != 0)
                || hypothesis
                    .unknown_fields
                    .iter()
                    .any(|field| field.number == HYPOTHESIS_SAFETY_STATUS);
        }
        if unrecognized {
            Err(EngineError::UnrecognizedSafety)
        } else {
            Ok(())
        }
    }
    pub fn single_hypothesis(&self) -> Result<&Hypothesis, EngineError> {
        self.check_safety()?;
        if self.final_updates.len() != 1 || self.final_updates[0].hypotheses().len() != 1 {
            return Err(EngineError::AmbiguousResult);
        }
        Ok(&self.final_updates[0].hypotheses()[0])
    }
    pub fn text(&self) -> Result<String, EngineError> {
        let hypothesis = self.single_hypothesis()?;
        let Some(edit) = &hypothesis.voice_edit else {
            return hypothesis.text.clone().ok_or(EngineError::AmbiguousResult);
        };
        if matches!(edit.intent, Some(2 | 3))
            && edit.active_field == Some(3)
            && let Some(text) = &edit.edited_text
        {
            return Ok(text.clone());
        }
        if edit.intent == Some(2)
            && edit.active_field == Some(7)
            && let Some(context) = &edit.edited_context
        {
            return Ok(join_context(context));
        }
        Err(EngineError::AmbiguousResult)
    }
}
fn join_context(context: &EditingContext) -> String {
    let mut text =
        String::with_capacity(context.before.len() + context.selected.len() + context.after.len());
    text.push_str(&context.before);
    text.push_str(&context.selected);
    text.push_str(&context.after);
    text
}

/// Collect a started native session. No result is returned until the unique
/// engine EOF, S3 END, and clean terminal completion have all been observed.
/// Failure/limit overflow cancels and awaits cleanup without closing the engine.
/// Dropping the whole collecting future signals cancellation through session Drop;
/// use Engine::close to await cleanup after external cancellation.
pub async fn collect_session(
    mut session: EngineSession,
    limits: CollectorLimits,
) -> Result<DictationResult, EngineError> {
    let result = collect_inner(&mut session, limits).await;
    let cleanup = session.close_unobserved().await;
    match result {
        Err(error) => Err(error),
        Ok(result) => {
            cleanup?;
            Ok(result)
        }
    }
}
/// Takes ownership synchronously, before the returned future's first poll. Even
/// dropping an entirely unpolled collector signals cleanup; Engine::close awaits
/// it. Invalid limits are checked before RPC setup and still clean up audio.
pub fn collect_dictation(
    engine: &Engine,
    audio: Box<dyn AudioSource>,
    options: SessionOptions,
    limits: CollectorLimits,
) -> impl std::future::Future<Output = Result<DictationResult, EngineError>> + Send + 'static {
    let invalid_limits = limits.max_final_updates == 0 || limits.max_result_bytes == 0;
    let (session, rejected_cleanup) = if invalid_limits {
        (
            Err(EngineError::InvalidConfig),
            Some(engine.cleanup_unstarted(audio)),
        )
    } else {
        (engine.start(audio, options), None)
    };
    async move {
        if let Some(cleanup) = rejected_cleanup {
            cleanup.await?;
        }
        collect_session(session?, limits).await
    }
}
async fn collect_inner(
    session: &mut EngineSession,
    limits: CollectorLimits,
) -> Result<DictationResult, EngineError> {
    if limits.max_final_updates == 0 || limits.max_result_bytes == 0 {
        return Err(EngineError::InvalidConfig);
    }
    let mut updates = Vec::new();
    let mut size = 0usize;
    let mut eof = None;
    let mut end = false;
    let mut s3_error = None;
    while let Some(event) = session.next().await? {
        if let Some(response) = event.response {
            if eof.is_some() {
                return Err(session.incomplete_error().await);
            }
            if response.status == Some(1) {
                end = true;
            }
            if response.status == Some(2) || response.error_code.is_some_and(|code| code != 0) {
                s3_error = Some(response.clone());
            }
            if response.recognition_state() == Some(1) {
                size = size
                    .checked_add(response.raw.len())
                    .ok_or(EngineError::ResultLimit)?;
                if updates.len() >= limits.max_final_updates || size > limits.max_result_bytes {
                    return Err(EngineError::ResultLimit);
                }
                updates.push(response);
            }
        } else {
            if eof.is_some() {
                return Err(session.incomplete_error().await);
            }
            eof = Some(event.metrics);
        }
    }
    if let Some(error) = s3_error {
        return Err(EngineError::S3(std::sync::Arc::new(error)));
    }
    if !end || eof.is_none() {
        return Err(session.incomplete_error().await);
    }
    Ok(DictationResult {
        final_updates: updates,
        context: session.captured_context(),
        metrics: eof,
    })
}

/// Strings and arbitrary-size canonical decimal integers are distinct revision
/// types. There is deliberately no bool conversion, float, or numeric arithmetic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Revision {
    String(String),
    Integer(DecimalRevision),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecimalRevision(String);
impl DecimalRevision {
    /// Canonical spelling: 0 or optional '-' followed by nonzero leading digit.
    pub fn new(value: impl Into<String>) -> Result<Self, EngineError> {
        let value = value.into();
        let digits = value.strip_prefix('-').unwrap_or(&value);
        if digits.is_empty()
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
            || (digits.len() > 1 && digits.starts_with('0'))
            || value == "-0"
        {
            return Err(EngineError::InvalidRevision);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<i64> for Revision {
    fn from(value: i64) -> Self {
        Self::Integer(DecimalRevision(value.to_string()))
    }
}
impl From<String> for Revision {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<&str> for Revision {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

/// Describes replacement of the captured context WINDOW, never a text delta or
/// authorization for whole-document replacement. Does not mutate an editor.
#[derive(Clone, Debug)]
pub struct ReplacementProposal {
    pub snapshot: EditingContext,
    pub expected_revision: Option<Revision>,
    pub replacement_text: String,
    pub replacement_context: Option<EditingContext>,
}
impl ReplacementProposal {
    pub fn validate_current(
        &self,
        current: &EditingContext,
        revision: Option<&Revision>,
    ) -> Result<(), EngineError> {
        if current.before != self.snapshot.before
            || current.selected != self.snapshot.selected
            || current.after != self.snapshot.after
        {
            return Err(EngineError::ContextConflict);
        }
        if self
            .expected_revision
            .as_ref()
            .is_some_and(|expected| revision != Some(expected))
        {
            return Err(EngineError::ContextConflict);
        }
        Ok(())
    }
    /// Local UTF-16 code-unit positions in replacement_text, only for field 7.
    pub fn selection_utf16_range(&self) -> Option<(usize, usize)> {
        let context = self.replacement_context.as_ref()?;
        let start = context.before.encode_utf16().count();
        Some((start, start + context.selected.encode_utf16().count()))
    }
}
/// The revision must belong to the captured (possibly late) snapshot. Callers
/// should capture context and revision atomically; text alone cannot detect ABA.
pub fn replacement_proposal(
    result: &DictationResult,
    expected_revision: Option<Revision>,
) -> Result<ReplacementProposal, EngineError> {
    let hypothesis = result.single_hypothesis()?;
    let snapshot = result
        .context
        .as_ref()
        .ok_or(EngineError::UnsupportedEdit)?;
    let edit = hypothesis
        .voice_edit
        .as_ref()
        .ok_or(EngineError::UnsupportedEdit)?;
    if edit.intent != Some(2) {
        return Err(EngineError::UnsupportedEdit);
    }
    let (text, context) = match edit.active_field {
        Some(3) => (
            edit.edited_text
                .clone()
                .ok_or(EngineError::UnsupportedEdit)?,
            None,
        ),
        Some(7) => {
            let context = edit
                .edited_context
                .as_ref()
                .ok_or(EngineError::UnsupportedEdit)?;
            (join_context(context), Some(context.clone()))
        }
        _ => return Err(EngineError::UnsupportedEdit),
    };
    Ok(ReplacementProposal {
        snapshot: snapshot.clone(),
        expected_revision,
        replacement_text: text,
        replacement_context: context,
    })
}
