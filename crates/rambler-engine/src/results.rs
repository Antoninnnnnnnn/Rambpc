use crate::{
    AudioSource, EditingContext, Engine, EngineError, EngineSession, S3Response, SessionMetrics,
    SessionOptions,
};
use rambler_protocol::{HYPOTHESIS_SAFETY_STATUS, Hypothesis};

/// Explicit text projection; no policy authorizes editor or system effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderPolicy {
    /// Exactly the historical `DictationResult::text` contract.
    Strict,
    /// Only Hypothesis.text, never VoiceEdit.transcript; still safety/uniqueness gated.
    AsrOnly,
    /// Textual helper from ihm:192–224, not Android candidate filtering/selection.
    /// Accepts intents 2/3 and fields 3/7; otherwise uses the ASR getter's default.
    /// This static compatibility evidence does not establish live server behavior.
    S3ClientCompatible,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextProvenance {
    HypothesisText,
    VoiceEditText,
    VoiceEditContext,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackReason {
    VoiceEditAbsent,
    UnsupportedIntent,
    UnsupportedMember,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedText {
    pub text: String,
    pub provenance: TextProvenance,
    /// Raw VoiceEdit intention, even when it was rejected for rendering.
    pub intent: Option<i32>,
    pub fallback_reason: Option<FallbackReason>,
}

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
/// All hypothesis-container responses (fields3/5/7), including those preceding
/// state1. Containers5/7 have no implicit Rambler finality or selection policy. There is
/// deliberately no global text assembly: an edited segment may contain the full
/// history and concatenating it would duplicate content.
#[derive(Clone, Debug, Default)]
pub struct SegmentResult {
    pub updates: Vec<SegmentUpdate>,
    pub context: Option<EditingContext>,
    pub metrics: Option<SessionMetrics>,
}
#[derive(Clone, Debug)]
pub struct SegmentUpdate {
    /// One-based client response ordinal, including unretained heartbeats/partials.
    /// Not a server segment ID. Language fields must never be used as IDs.
    pub response_ordinal: u64,
    pub response: S3Response,
}
impl SegmentResult {
    pub fn check_safety(&self) -> Result<(), EngineError> {
        check_response_safety(self.updates.iter().map(|update| &update.response))
    }
}
fn check_response_safety<'a>(
    responses: impl Iterator<Item = &'a S3Response>,
) -> Result<(), EngineError> {
    let mut unrecognized = false;
    for hypothesis in responses.flat_map(S3Response::all_hypotheses) {
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

impl DictationResult {
    /// Refuse simplified rendering and editor proposals when any retained final
    /// candidate is safety-blocked, or has an uninterpreted field-30 occurrence.
    /// Structured responses remain explicitly available to callers. The Android
    /// evidence covers accepted VoiceEdit candidates; applying this guard to all
    /// candidates and unknown values is intentionally conservative.
    pub fn check_safety(&self) -> Result<(), EngineError> {
        check_response_safety(self.final_updates.iter())
    }
    pub fn single_hypothesis(&self) -> Result<&Hypothesis, EngineError> {
        self.check_safety()?;
        if self.final_updates.len() != 1 || self.final_updates[0].hypotheses().len() != 1 {
            return Err(EngineError::AmbiguousResult);
        }
        Ok(&self.final_updates[0].hypotheses()[0])
    }
    /// Safety is checked across ALL finals before any policy-specific projection.
    /// Multiple finals/hypotheses remain ambiguous, including identical candidates.
    /// An accepted empty edit stays empty: it never falls back to ASR.
    pub fn render(&self, policy: RenderPolicy) -> Result<RenderedText, EngineError> {
        let hypothesis = self.single_hypothesis()?;
        let edit = hypothesis.voice_edit.as_ref();
        let intent = edit.and_then(|edit| edit.intent);
        if policy == RenderPolicy::AsrOnly {
            return Ok(RenderedText {
                text: hypothesis
                    .text
                    .clone()
                    .ok_or(EngineError::AmbiguousResult)?,
                provenance: TextProvenance::HypothesisText,
                intent,
                fallback_reason: None,
            });
        }
        if policy == RenderPolicy::Strict {
            let text = Self::project_strict_text(hypothesis)?;
            return Ok(RenderedText {
                text,
                provenance: match edit.map(|edit| edit.active_field) {
                    Some(Some(3)) => TextProvenance::VoiceEditText,
                    Some(Some(7)) => TextProvenance::VoiceEditContext,
                    _ => TextProvenance::HypothesisText,
                },
                intent,
                fallback_reason: edit.is_none().then_some(FallbackReason::VoiceEditAbsent),
            });
        }
        let fallback_reason = match edit {
            None => FallbackReason::VoiceEditAbsent,
            Some(edit) if !matches!(edit.intent, Some(2 | 3)) => FallbackReason::UnsupportedIntent,
            Some(edit) if !matches!(edit.active_field, Some(3 | 7)) => {
                FallbackReason::UnsupportedMember
            }
            Some(edit) => {
                let (text, provenance) = if edit.active_field == Some(3) {
                    (
                        edit.edited_text
                            .clone()
                            .ok_or(EngineError::AmbiguousResult)?,
                        TextProvenance::VoiceEditText,
                    )
                } else {
                    (
                        join_context(
                            edit.edited_context
                                .as_ref()
                                .ok_or(EngineError::AmbiguousResult)?,
                        ),
                        TextProvenance::VoiceEditContext,
                    )
                };
                return Ok(RenderedText {
                    text,
                    provenance,
                    intent,
                    fallback_reason: None,
                });
            }
        };
        Ok(RenderedText {
            text: hypothesis.text.clone().unwrap_or_default(),
            provenance: TextProvenance::HypothesisText,
            intent,
            fallback_reason: Some(fallback_reason),
        })
    }
    pub fn text(&self) -> Result<String, EngineError> {
        let hypothesis = self.single_hypothesis()?;
        Self::project_strict_text(hypothesis)
    }

    // Call only after checking global safety and uniqueness with single_hypothesis.
    fn project_strict_text(hypothesis: &Hypothesis) -> Result<String, EngineError> {
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
    session: EngineSession,
    limits: CollectorLimits,
) -> Result<DictationResult, EngineError> {
    finish_collection(session, limits, Retention::GlobalFinals)
        .await
        .map(|(result, _)| result)
}

/// Collect hypothesis containers (fields3/5/7), irrespective of recognition state.
/// All completion/error/cleanup checks are shared with `collect_session`.
/// Limits count every retained response, including an empty state1 marker.
pub async fn collect_segment_session(
    session: EngineSession,
    limits: CollectorLimits,
) -> Result<SegmentResult, EngineError> {
    let (result, ordinals) = finish_collection(session, limits, Retention::Segments).await?;
    Ok(SegmentResult {
        updates: result
            .final_updates
            .into_iter()
            .zip(ordinals)
            .map(|(response, response_ordinal)| SegmentUpdate {
                response_ordinal,
                response,
            })
            .collect(),
        context: result.context,
        metrics: result.metrics,
    })
}

#[derive(Clone, Copy)]
enum Retention {
    GlobalFinals,
    Segments,
}
async fn finish_collection(
    mut session: EngineSession,
    limits: CollectorLimits,
    retention: Retention,
) -> Result<(DictationResult, Vec<u64>), EngineError> {
    let result = collect_inner(&mut session, limits, retention).await;
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
    let (session, rejected_cleanup) = prepare_collection(engine, audio, options, limits);
    async move {
        if let Some(cleanup) = rejected_cleanup {
            cleanup.await?;
        }
        collect_session(session?, limits).await
    }
}
/// Ownership is transferred synchronously, including when the returned future
/// is never polled. Like dictation collection, failures do not trigger replay.
pub fn collect_segments(
    engine: &Engine,
    audio: Box<dyn AudioSource>,
    options: SessionOptions,
    limits: CollectorLimits,
) -> impl std::future::Future<Output = Result<SegmentResult, EngineError>> + Send + 'static {
    let (session, rejected_cleanup) = prepare_collection(engine, audio, options, limits);
    async move {
        if let Some(cleanup) = rejected_cleanup {
            cleanup.await?;
        }
        collect_segment_session(session?, limits).await
    }
}
type PreparedCollection = (
    Result<EngineSession, EngineError>,
    Option<crate::SourceFuture<'static, Result<(), EngineError>>>,
);
fn prepare_collection(
    engine: &Engine,
    audio: Box<dyn AudioSource>,
    options: SessionOptions,
    limits: CollectorLimits,
) -> PreparedCollection {
    if limits.max_final_updates == 0 || limits.max_result_bytes == 0 {
        (
            Err(EngineError::InvalidConfig),
            Some(engine.cleanup_unstarted(audio)),
        )
    } else {
        (engine.start(audio, options), None)
    }
}
async fn collect_inner(
    session: &mut EngineSession,
    limits: CollectorLimits,
    retention: Retention,
) -> Result<(DictationResult, Vec<u64>), EngineError> {
    if limits.max_final_updates == 0 || limits.max_result_bytes == 0 {
        return Err(EngineError::InvalidConfig);
    }
    let mut updates = Vec::new();
    let mut ordinals = Vec::new();
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
            let keep = response.recognition_state() == Some(1)
                || matches!(retention, Retention::Segments)
                    && response
                        .result
                        .as_ref()
                        .and_then(|result| result.recognition.as_ref())
                        .is_some_and(|result| {
                            result.hypothesis_result.is_some()
                                || result.hypothesis_result_field5.is_some()
                                || result.hypothesis_result_field7.is_some()
                        });
            if keep {
                if matches!(retention, Retention::Segments) {
                    ordinals.push(event.metrics.responses_received);
                }
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
    Ok((
        DictationResult {
            final_updates: updates,
            context: session.captured_context(),
            metrics: eof,
        },
        ordinals,
    ))
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

/// Explicit opt-in to static client-compatible replacement of the captured
/// context WINDOW (ihm:203–212, ffh:117–145, ffq:64–105). No mutation occurs.
/// Strict delegates unchanged to the historical proposal contract. ASR-only and
/// every ASR fallback are ineligible, even if the ASR text resembles an edit.
pub fn replacement_proposal_with_policy(
    result: &DictationResult,
    expected_revision: Option<Revision>,
    policy: RenderPolicy,
) -> Result<ReplacementProposal, EngineError> {
    if policy == RenderPolicy::Strict {
        return replacement_proposal(result, expected_revision);
    }
    let hypothesis = result.single_hypothesis()?;
    if policy == RenderPolicy::AsrOnly {
        return Err(EngineError::UnsupportedEdit);
    }
    let edit = hypothesis
        .voice_edit
        .as_ref()
        .ok_or(EngineError::UnsupportedEdit)?;
    // The newly established rewrite replacement scope is specifically field7.
    // A renderable rewrite string alone does not establish its editor range.
    if edit.intent != Some(2) && !(edit.intent == Some(3) && edit.active_field == Some(7)) {
        return Err(EngineError::UnsupportedEdit);
    }
    let rendered = result.render(policy)?;
    if rendered.provenance == TextProvenance::HypothesisText {
        return Err(EngineError::UnsupportedEdit);
    }
    let snapshot = result
        .context
        .as_ref()
        .ok_or(EngineError::UnsupportedEdit)?;
    let edit = hypothesis
        .voice_edit
        .as_ref()
        .ok_or(EngineError::UnsupportedEdit)?;
    Ok(ReplacementProposal {
        snapshot: snapshot.clone(),
        expected_revision,
        replacement_text: rendered.text,
        replacement_context: (edit.active_field == Some(7))
            .then(|| edit.edited_context.clone())
            .flatten(),
    })
}

/// Versioned DEFAULT flag patterns from fgc:13–40 and muq:76–78. This is not
/// a claim about runtime flag values, nor a configurable Java regex engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandProfile {
    S3DefaultPatternsV1,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandKind {
    Send,
    DeleteLastSentence,
    DeleteAll,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandDisposition {
    /// ffh:771–773 recognizes CLEAR/CLEAR_ALL but refuses them.
    RejectedByClientBranch,
    /// SEND cannot be offered without the editor's IME action 4.
    RequiresImeAction4,
    /// Classification/preconditions only; the caller must explicitly act.
    EligibleForExplicitSend,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandPreconditions {
    pub required_ime_action: Option<i32>,
    pub observed_ime_action: Option<i32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandCandidate {
    pub profile: CommandProfile,
    pub command: CommandKind,
    pub provenance: TextProvenance,
    pub intent: Option<i32>,
    pub preconditions: CommandPreconditions,
    pub disposition: CommandDisposition,
    /// Always false: classification never sends, clears, or mutates anything.
    pub applied: bool,
}
impl DictationResult {
    /// Opt-in, locale-independent DEFAULT-pattern classification from a unique
    /// final accepted VoiceEdit only (ffh:683–695,767–780). Never uses transcript
    /// or ASR, even when the client text helper would fall back. Does not imply
    /// that Android's general feature gate is enabled or perform a VoiceAction.
    pub fn classify_command(
        &self,
        profile: CommandProfile,
        ime_action: Option<i32>,
    ) -> Result<Option<CommandCandidate>, EngineError> {
        self.single_hypothesis()?;
        if self.final_updates[0].recognition_state() != Some(1) {
            return Err(EngineError::AmbiguousResult);
        }
        let rendered = self.render(RenderPolicy::S3ClientCompatible)?;
        if rendered.provenance == TextProvenance::HypothesisText {
            return Err(EngineError::UnsupportedEdit);
        }
        let command = match profile {
            CommandProfile::S3DefaultPatternsV1 => default_command(&rendered.text),
        };
        Ok(command.map(|command| {
            let send = command == CommandKind::Send;
            CommandCandidate {
                profile,
                command,
                provenance: rendered.provenance,
                intent: rendered.intent,
                preconditions: CommandPreconditions {
                    required_ime_action: send.then_some(4),
                    observed_ime_action: ime_action,
                },
                disposition: if !send {
                    CommandDisposition::RejectedByClientBranch
                } else if ime_action != Some(4) {
                    CommandDisposition::RequiresImeAction4
                } else {
                    CommandDisposition::EligibleForExplicitSend
                },
                applied: false,
            }
        }))
    }
}

fn default_command(text: &str) -> Option<CommandKind> {
    // Java String.trim removes code points <= U+0020, not Unicode whitespace
    // and not just the six ASCII regex whitespace classes.
    let text = text.trim_matches(|ch| ch <= '\u{20}');
    let non_alphanumeric = |ch: char| !ch.is_ascii_alphanumeric();
    let send = text.trim_matches(non_alphanumeric);
    // ^[^a-z0-9]*sen[dt][^a-z0-9]*$; ASCII CASE_INSENSITIVE only.
    if send.eq_ignore_ascii_case("send") || send.eq_ignore_ascii_case("sent") {
        return Some(CommandKind::Send);
    }
    // ^clear[^a-z0-9]*$; then ^clear\sall[^a-z0-9]*$, in that order.
    let clear = text
        .get(..5)
        .filter(|prefix| prefix.eq_ignore_ascii_case("clear"))?;
    let rest = &text[clear.len()..];
    if rest.chars().all(non_alphanumeric) {
        return Some(CommandKind::DeleteLastSentence);
    }
    let bytes = rest.as_bytes();
    if bytes
        .first()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        && rest
            .get(1..4)
            .is_some_and(|word| word.eq_ignore_ascii_case("all"))
        && rest[4..].chars().all(non_alphanumeric)
    {
        Some(CommandKind::DeleteAll)
    } else {
        None
    }
}
