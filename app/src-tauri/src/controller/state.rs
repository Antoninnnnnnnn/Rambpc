//! Pure take transitions. Time and observations are supplied by the runtime.

use crate::{
    ipc::{
        Availability, DraftMeta, DraftPart, DraftView, InterruptReason, KeptReason, PartStatus,
        Phase, RefusalReason, TakeOutcome, UiState, UnavailableReason,
    },
    model::{DestinationKind, Gesture, KeyEdge, Language, MicChoice, SystemEvent, TakeId},
    ports::{InsertOutcome, MicError, TargetChange, TargetToken, TargetVerdict},
    rambler::{FRAME_WAIT_MS, MAX_DRAFT_BYTES, Segment},
    settings::{InsertionTarget, Settings},
    text::{self, TextRules},
};
use rambler_audio::join_segment_texts;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
use unicode_segmentation::UnicodeSegmentation;

const OPEN_MS: u64 = 3_000;
const MAX_LISTENING_MS: u64 = 300_000;
const MAX_RECENT: usize = 5;
const MAX_PARTS: usize = 64;

#[derive(Clone, Copy)]
pub(super) struct Now {
    pub mono: u64,
    pub unix: u64,
}

pub(super) enum Input {
    Hotkey(KeyEdge),
    StartInternal,
    Finish(TakeId),
    Cancel(Option<TakeId>),
    Settings(Settings),
    PersonalRules(Arc<crate::ipc::PersonalRules>),
    Consent(bool),
    Access(bool),
    Retry,
    System(SystemEvent),
    Connected(u64, Result<(), UnavailableReason>),
    Target(TakeId, Result<TargetVerdict, RefusalReason>),
    TargetChanged(TakeId, TargetChange),
    MicOpened(TakeId),
    MicOpenFailed(TakeId, MicError),
    MicFault(TakeId, InterruptReason),
    MicFree(TakeId),
    Tick { last_frame: Option<u64> },
    Delivered(TakeId, Segment),
    Collection(TakeId, Result<(), InterruptReason>),
    Insertion(TakeId, Result<InsertOutcome, KeptReason>),
    ClearDraft(Option<TakeId>),
    Shutdown,
}

pub(super) enum Effect {
    Reconfigure(Option<u64>),
    Probe(TakeId),
    Open { take: TakeId, mic: MicChoice },
    Collect { take: TakeId, language: Language },
    Stop(TakeId),
    Retire(TakeId),
    Completed(Completion),
    Insert(TakeId, InsertionPlan, Arc<str>),
}

pub(super) enum InsertionPlan {
    Original(TargetToken),
    CursorAtEnd(Option<TargetToken>),
}

pub(super) struct Completion {
    pub id: TakeId,
    pub words: u64,
    pub listening_ms: u64,
    pub archive: Option<crate::storage::history::CompletedTake>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Listening,
    Finalizing { collected: bool },
    Inserting,
}

impl Stage {
    fn phase(self) -> Phase {
        match self {
            Self::Listening => Phase::Listening,
            Self::Finalizing { .. } | Self::Inserting => Phase::Finalizing,
        }
    }
}

enum Destination {
    Sambler(KeptReason),
    Field(TargetToken),
}

struct Take {
    id: TakeId,
    stage: Stage,
    gesture: Gesture,
    hotkey: bool,
    mic: MicChoice,
    rules: TextRules,
    always_keep: bool,
    insertion_target: InsertionTarget,
    destination: Option<Destination>,
    pending_insert: Option<(Draft, Completion)>,
    started: u64,
    opened: Option<u64>,
    collecting: bool,
    listening_ms: u64,
    segments: BTreeMap<usize, Segment>,
    bytes: usize,
}

impl Take {
    fn insertion_plan(&self, destination: &Destination) -> Option<InsertionPlan> {
        match (self.insertion_target, destination) {
            (InsertionTarget::OriginalField, Destination::Field(token)) => {
                Some(InsertionPlan::Original(token.clone()))
            }
            (InsertionTarget::CursorAtEnd, Destination::Field(token)) => {
                Some(InsertionPlan::CursorAtEnd(Some(token.clone())))
            }
            (
                InsertionTarget::CursorAtEnd,
                Destination::Sambler(KeptReason::UncertainTarget { .. }),
            ) if self.hotkey && !self.always_keep => Some(InsertionPlan::CursorAtEnd(None)),
            _ => None,
        }
    }
}

struct Draft {
    meta: DraftMeta,
    expires_mono: u64,
    parts: Vec<DraftPart>,
    full: Option<Arc<str>>,
}

impl Draft {
    fn new(take: TakeId, parts: Vec<DraftPart>, full: Option<Arc<str>>) -> Self {
        Self {
            meta: DraftMeta {
                take,
                created_at_ms: 0,
                expires_at_ms: 0,
                complete: full.is_some(),
            },
            expires_mono: 0,
            parts,
            full,
        }
    }

    fn set_lifetime(&mut self, now: Now, minutes: u8) {
        let life = u64::from(minutes.clamp(1, 10)) * 60_000;
        self.meta.created_at_ms = now.unix;
        self.meta.expires_at_ms = now.unix.saturating_add(life);
        self.expires_mono = now.mono.saturating_add(life);
    }

    fn view(&self) -> DraftView {
        DraftView {
            meta: self.meta,
            parts: self.parts.clone(),
            full_text: self.full.as_deref().map(str::to_owned),
        }
    }

    fn copy_text(&self) -> Option<Arc<str>> {
        if let Some(full) = &self.full {
            return Some(full.clone());
        }
        // Copy incomplete ranges verbatim and separately, without suggesting
        // an uncertain boundary was reconstructed (e.g. a split decimal).
        let fragments: Vec<_> = self
            .parts
            .iter()
            .filter(|part| part.status == PartStatus::Admitted)
            .filter_map(|part| part.text.as_deref())
            .collect();
        (!fragments.is_empty()).then(|| Arc::from(fragments.join("\n\n")))
    }
}

pub(super) struct State {
    settings: Settings,
    personal_rules: Option<Arc<crate::ipc::PersonalRules>>,
    archive: bool,
    consent: bool,
    access: bool,
    access_refused: bool,
    availability: Availability,
    epoch: u64,
    next_take: u64,
    take: Option<Take>,
    mic_owner: Option<TakeId>,
    recents: VecDeque<Draft>,
    outcome: Option<TakeOutcome>,
    key_down: bool,
    locked: bool,
    suspended: bool,
}

impl State {
    pub(super) fn new(settings: Settings, consent: bool, access: bool) -> Self {
        Self {
            settings,
            personal_rules: None,
            archive: false,
            consent,
            access,
            access_refused: false,
            availability: if !consent {
                Availability::NeedsConsent
            } else if !access {
                Availability::NeedsAccess
            } else {
                Availability::Connecting
            },
            epoch: 0,
            next_take: 0,
            take: None,
            mic_owner: None,
            recents: VecDeque::new(),
            outcome: None,
            key_down: false,
            locked: false,
            suspended: false,
        }
    }

    pub(super) fn use_profile(&mut self, rules: Arc<crate::ipc::PersonalRules>) {
        self.personal_rules = Some(rules);
        self.archive = true;
    }

    pub(super) fn take_id(&self) -> Option<TakeId> {
        self.take.as_ref().map(|take| take.id)
    }

    pub(super) fn expecting_connection(&self, epoch: u64) -> bool {
        self.epoch == epoch && self.availability == Availability::Connecting
    }

    pub(super) fn ui(&self, now: Now) -> UiState {
        let take = self.take.as_ref();
        UiState {
            phase: take.map_or(Phase::Idle, |take| take.stage.phase()),
            take: self.take_id(),
            destination: take.and_then(|take| match take.destination {
                Some(Destination::Field(_)) => Some(DestinationKind::Field),
                Some(Destination::Sambler(_)) => Some(DestinationKind::Sambler),
                None => None,
            }),
            availability: self.availability.clone(),
            last_outcome: self.outcome.clone(),
            draft: self.recents.front().map(|draft| draft.meta),
            listening_ms: take.and_then(|take| {
                (take.stage == Stage::Listening).then(|| now.mono.saturating_sub(take.started))
            }),
            gesture: take.map_or(self.settings.gesture, |take| take.gesture),
            shortcut: self.settings.shortcut.clone(),
            recent_count: self.recents.len() as u32,
            appearance: self.settings.appearance.clone(),
        }
    }

    pub(super) fn draft(&self) -> Option<DraftView> {
        self.recents.front().map(Draft::view)
    }

    pub(super) fn recent_drafts(&self) -> Vec<DraftView> {
        self.recents.iter().map(Draft::view).collect()
    }

    pub(super) fn has_draft(&self, id: TakeId) -> bool {
        self.recents.iter().any(|draft| draft.meta.take == id)
    }

    pub(super) fn copy_text(&self, id: Option<TakeId>) -> Option<(TakeId, Arc<str>)> {
        let draft = self
            .recents
            .iter()
            .find(|d| id.is_none_or(|id| d.meta.take == id))?;
        Some((draft.meta.take, draft.copy_text()?))
    }

    pub(super) fn step(&mut self, input: Input, now: Now) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.recents.retain(|draft| now.mono < draft.expires_mono);
        match input {
            Input::Hotkey(KeyEdge::Pressed) if !self.key_down => {
                self.key_down = true;
                match &self.take {
                    None => self.start(true, now, &mut effects),
                    Some(take) if take.gesture == Gesture::Toggle => {
                        self.finish(false, now, &mut effects)
                    }
                    _ => (),
                }
            }
            Input::Hotkey(KeyEdge::Released) if self.key_down => {
                self.key_down = false;
                if self
                    .take
                    .as_ref()
                    .is_some_and(|t| t.hotkey && t.gesture == Gesture::Hold)
                {
                    self.finish(false, now, &mut effects);
                }
            }
            Input::Hotkey(_) => (),
            Input::StartInternal => self.start(false, now, &mut effects),
            Input::Finish(id) if self.take_id() == Some(id) => {
                self.finish(false, now, &mut effects)
            }
            Input::Cancel(id) => {
                let id = id.or(self.take_id());
                if let Some(id) = id {
                    self.recents.retain(|draft| draft.meta.take != id);
                    if self.take_id() == Some(id) {
                        self.end(TakeOutcome::Cancelled, &mut effects);
                    }
                }
            }
            Input::System(event) => {
                match event {
                    SystemEvent::SessionLocked => self.locked = true,
                    SystemEvent::Suspending => self.suspended = true,
                    SystemEvent::SessionUnlocked => self.locked = false,
                    SystemEvent::Resumed => self.suspended = false,
                }
                if matches!(event, SystemEvent::SessionLocked | SystemEvent::Suspending) {
                    self.key_down = false;
                    self.recents.clear();
                    if self.take.is_some() {
                        self.end(
                            TakeOutcome::Interrupted {
                                reason: InterruptReason::SystemInterrupted,
                                recovered: false,
                            },
                            &mut effects,
                        );
                    }
                }
            }
            Input::Settings(settings) => self.settings = settings,
            Input::PersonalRules(rules) => self.personal_rules = Some(rules),
            Input::Consent(consent) => {
                if !consent && self.take.is_some() {
                    self.interrupt(InterruptReason::ConsentWithdrawn, now, &mut effects);
                }
                self.consent = consent;
                if self.take.is_none() {
                    self.reconfigure(&mut effects);
                }
            }
            Input::Access(access) => {
                if self.take.is_some() {
                    self.interrupt(
                        if access {
                            InterruptReason::Service
                        } else {
                            InterruptReason::AccessRefused
                        },
                        now,
                        &mut effects,
                    );
                }
                self.access = access;
                self.access_refused = false;
                self.reconfigure(&mut effects);
            }
            Input::Retry if self.take.is_none() => {
                self.access_refused = false;
                self.reconfigure(&mut effects);
            }
            Input::Connected(epoch, result) if self.expecting_connection(epoch) => match result {
                Ok(()) => self.availability = Availability::Ready,
                Err(reason) => {
                    self.access_refused = reason == UnavailableReason::AccessRefused;
                    self.availability = Availability::Unavailable { reason };
                    if self.take.is_some() {
                        self.end(
                            TakeOutcome::Refused {
                                reason: RefusalReason::Unavailable { reason },
                            },
                            &mut effects,
                        );
                    }
                }
            },
            Input::Target(id, verdict) if self.take_id() == Some(id) => {
                let take = self.take.as_mut().unwrap();
                if take.destination.is_none() {
                    let destination = verdict.and_then(|verdict| match verdict {
                        TargetVerdict::Protected => Err(RefusalReason::ProtectedField),
                        TargetVerdict::Uncertain { reason } => {
                            Ok(Destination::Sambler(KeptReason::UncertainTarget { reason }))
                        }
                        TargetVerdict::Eligible { token, .. }
                            if take.hotkey && !take.always_keep =>
                        {
                            Ok(Destination::Field(token))
                        }
                        TargetVerdict::Sambler | TargetVerdict::Eligible { .. } => {
                            Ok(Destination::Sambler(KeptReason::SamblerDestination))
                        }
                    });
                    match destination {
                        Ok(destination) => take.destination = Some(destination),
                        Err(reason) => self.end(TakeOutcome::Refused { reason }, &mut effects),
                    }
                }
            }
            Input::TargetChanged(id, TargetChange::Protected) if self.take_id() == Some(id) => {
                let take = self.take.as_ref().unwrap();
                if take.destination.is_none() {
                    self.end(
                        TakeOutcome::Refused {
                            reason: RefusalReason::ProtectedField,
                        },
                        &mut effects,
                    );
                } else if take.stage == Stage::Listening {
                    self.interrupt(InterruptReason::SystemInterrupted, now, &mut effects);
                } else {
                    self.end(protected_end(), &mut effects);
                }
            }
            Input::MicOpened(id) if self.take_id() == Some(id) => {
                let take = self.take.as_mut().unwrap();
                if take.opened.replace(now.mono).is_none()
                    && matches!(take.stage, Stage::Finalizing { .. })
                {
                    // Release can win the race with open. Stop independently of
                    // the verdict, leaving the closed PCM source available locally.
                    effects.push(Effect::Stop(id));
                }
            }
            Input::MicOpenFailed(id, error) if self.take_id() == Some(id) => {
                self.end(
                    TakeOutcome::Refused {
                        reason: RefusalReason::Microphone { error },
                    },
                    &mut effects,
                );
            }
            Input::MicFault(id, reason) if self.take_id() == Some(id) => {
                self.interrupt(reason, now, &mut effects)
            }
            Input::MicFree(id) if self.mic_owner == Some(id) => self.mic_owner = None,
            Input::Tick { last_frame } => self.tick(now, last_frame, &mut effects),
            Input::Delivered(id, segment) if self.take_id() == Some(id) => {
                let take = self.take.as_mut().unwrap();
                if take.collecting
                    && matches!(
                        take.stage,
                        Stage::Listening | Stage::Finalizing { collected: false }
                    )
                    && !take.segments.contains_key(&segment.index)
                {
                    let bytes = segment.part.text.as_ref().map_or(0, String::len);
                    if take.segments.len() >= MAX_PARTS || take.bytes + bytes > MAX_DRAFT_BYTES {
                        self.interrupt(InterruptReason::Service, now, &mut effects);
                    } else {
                        take.bytes += bytes;
                        take.segments.insert(segment.index, segment);
                    }
                }
            }
            Input::Collection(id, result)
                if self.take.as_ref().is_some_and(|t| {
                    t.id == id
                        && t.collecting
                        && matches!(
                            t.stage,
                            Stage::Listening | Stage::Finalizing { collected: false }
                        )
                }) =>
            {
                match result {
                    Ok(()) => {
                        let take = self.take.as_mut().unwrap();
                        if take.stage == (Stage::Finalizing { collected: false }) {
                            take.stage = Stage::Finalizing { collected: true };
                        } else {
                            // A clean EOF without a requested normal stop is not valid.
                            self.interrupt(InterruptReason::Service, now, &mut effects);
                        }
                    }
                    Err(reason) => {
                        let unavailable = match reason {
                            InterruptReason::AccessRefused => {
                                Some(UnavailableReason::AccessRefused)
                            }
                            InterruptReason::Network => Some(UnavailableReason::Network),
                            InterruptReason::Service => Some(UnavailableReason::Service),
                            _ => None,
                        };
                        self.interrupt(reason, now, &mut effects);
                        if let Some(reason) = unavailable {
                            self.access_refused = reason == UnavailableReason::AccessRefused;
                            self.availability = Availability::Unavailable { reason };
                            effects.push(Effect::Reconfigure(None));
                        }
                    }
                }
            }
            Input::Insertion(id, outcome)
                if self
                    .take
                    .as_ref()
                    .is_some_and(|t| t.id == id && t.stage == Stage::Inserting) =>
            {
                let outcome = match outcome {
                    Ok(InsertOutcome::Delivered) => TakeOutcome::Inserted,
                    Ok(InsertOutcome::Partial) => TakeOutcome::InsertedPartially,
                    Ok(InsertOutcome::NotWritten { reason }) => TakeOutcome::KeptInSambler {
                        reason: KeptReason::NotWritten { reason },
                    },
                    Err(reason) => TakeOutcome::KeptInSambler { reason },
                };
                // Protected focus discards the current result and any pending
                // archive, without disturbing recents from other takes.
                if outcome != protected_end() {
                    let (mut draft, completion) =
                        self.take.as_mut().unwrap().pending_insert.take().unwrap();
                    draft.set_lifetime(now, self.settings.draft_minutes);
                    self.save_draft(draft);
                    effects.push(Effect::Completed(completion));
                }
                self.end(outcome, &mut effects);
            }
            Input::ClearDraft(id) => self
                .recents
                .retain(|draft| id.is_some_and(|id| draft.meta.take != id)),
            Input::Shutdown => {
                self.recents.clear();
                if self.take.is_some() {
                    self.end(TakeOutcome::Cancelled, &mut effects);
                }
                effects.push(Effect::Reconfigure(None));
            }
            _ => (), // Stale or out-of-phase notification.
        }
        self.advance_capture(&mut effects);
        self.complete_if_stopped(now, &mut effects);
        effects
    }

    fn start(&mut self, hotkey: bool, now: Now, effects: &mut Vec<Effect>) {
        if self.take.is_some() || self.locked || self.suspended {
            return;
        }
        let refusal = match self.availability {
            Availability::NeedsConsent => Some(RefusalReason::NeedsConsent),
            Availability::NeedsAccess => Some(RefusalReason::NeedsAccess),
            // The availability banner already reports the connection in progress.
            Availability::Connecting => return,
            Availability::Unavailable { reason } => {
                if matches!(
                    reason,
                    UnavailableReason::Network | UnavailableReason::Service
                ) {
                    self.reconfigure(effects);
                }
                Some(RefusalReason::Unavailable { reason })
            }
            Availability::Ready => None,
        };
        if let Some(reason) = refusal {
            self.outcome = Some(TakeOutcome::Refused { reason });
            return;
        }
        self.next_take = self
            .next_take
            .checked_add(1)
            .expect("take id space exhausted");
        let id = TakeId(self.next_take);
        let mut rules = TextRules::from_settings(&self.settings);
        if let Some(personal) = &self.personal_rules {
            rules.lexicon.clone_from(&personal.lexicon);
            rules.expressions.clone_from(&personal.expressions);
        }
        self.outcome = None;
        self.take = Some(Take {
            id,
            stage: Stage::Listening,
            gesture: self.settings.gesture,
            hotkey,
            mic: self.settings.microphone.clone(),
            rules,
            always_keep: self.settings.always_keep_in_sambler,
            insertion_target: self.settings.insertion_target,
            destination: None,
            pending_insert: None,
            started: now.mono,
            opened: None,
            collecting: false,
            listening_ms: 0,
            segments: BTreeMap::new(),
            bytes: 0,
        });
        // Protection still matters when insertion is disabled.
        effects.push(Effect::Probe(id));
    }

    fn reconfigure(&mut self, effects: &mut Vec<Effect>) {
        self.epoch += 1;
        let epoch = if !self.consent {
            self.availability = Availability::NeedsConsent;
            None
        } else if !self.access {
            self.availability = Availability::NeedsAccess;
            None
        } else if self.access_refused {
            self.availability = Availability::Unavailable {
                reason: UnavailableReason::AccessRefused,
            };
            None
        } else {
            self.availability = Availability::Connecting;
            Some(self.epoch)
        };
        effects.push(Effect::Reconfigure(epoch));
    }

    fn advance_capture(&mut self, effects: &mut Vec<Effect>) {
        let Some(take) = &mut self.take else {
            return;
        };
        if take.stage == Stage::Listening && take.opened.is_none() && self.mic_owner.is_none() {
            self.mic_owner = Some(take.id);
            effects.push(Effect::Open {
                take: take.id,
                mic: take.mic.clone(),
            });
        }
        if take.destination.is_some() && !take.collecting {
            if take.opened.is_some() {
                take.collecting = true;
                effects.push(Effect::Collect {
                    take: take.id,
                    language: take.rules.language,
                });
            } else if matches!(take.stage, Stage::Finalizing { .. })
                && self.mic_owner != Some(take.id)
            {
                // The take was released while a previous microphone was being joined.
                take.stage = Stage::Finalizing { collected: true };
            }
        }
    }

    fn tick(&mut self, now: Now, last_frame: Option<u64>, effects: &mut Vec<Effect>) {
        let Some(take) = &mut self.take else {
            return;
        };
        let elapsed = now.mono.saturating_sub(take.started);
        if take.opened.is_none()
            && elapsed >= OPEN_MS
            && (take.stage == Stage::Listening || self.mic_owner == Some(take.id))
        {
            self.end(
                TakeOutcome::Refused {
                    reason: RefusalReason::Microphone {
                        error: MicError::OpenTimeout,
                    },
                },
                effects,
            );
        } else if take.stage == Stage::Listening {
            if let Some(opened) = take.opened
                && now.mono.saturating_sub(last_frame.unwrap_or(opened)) >= FRAME_WAIT_MS
            {
                self.interrupt(InterruptReason::MicrophoneStalled, now, effects);
            } else if elapsed >= MAX_LISTENING_MS {
                self.finish(true, now, effects);
            }
        }
    }

    fn finish(&mut self, limited: bool, now: Now, effects: &mut Vec<Effect>) {
        let take = self.take.as_mut().unwrap();
        if take.stage != Stage::Listening {
            return;
        }
        take.listening_ms = now.mono.saturating_sub(take.started);
        take.stage = Stage::Finalizing { collected: false };
        if limited {
            // IPC has one outcome; show the limit during finalization,
            // then publish the actual terminal delivery result.
            self.outcome = Some(TakeOutcome::Interrupted {
                reason: InterruptReason::DurationLimit,
                recovered: false,
            });
        }
        effects.push(Effect::Stop(take.id));
    }

    fn complete_if_stopped(&mut self, now: Now, effects: &mut Vec<Effect>) {
        let Some(take) = &mut self.take else {
            return;
        };
        if self.mic_owner == Some(take.id) {
            return;
        }
        let Stage::Finalizing {
            collected: true, ..
        } = take.stage
        else {
            return;
        };
        let parts = parts(std::mem::take(&mut take.segments), false);
        let has_text = parts.iter().any(|p| p.status == PartStatus::Admitted);
        let mut full: Option<Arc<str>> = None;
        let mut insert = None;
        let outcome = if !has_text {
            TakeOutcome::NoText
        } else if parts.iter().any(|p| p.status != PartStatus::Admitted)
            || ambiguous_numeric_seam(&parts)
        {
            TakeOutcome::KeptInSambler {
                reason: KeptReason::Incomplete,
            }
        } else {
            let joined = join_segment_texts(parts.iter().filter_map(|p| p.text.as_deref()));
            let rendered = text::render(&joined, &take.rules);
            if rendered.len() > MAX_DRAFT_BYTES {
                TakeOutcome::KeptInSambler {
                    reason: KeptReason::Incomplete,
                }
            } else if rendered.trim().is_empty() {
                TakeOutcome::NoText
            } else {
                full = Some(Arc::from(rendered));
                let destination = take
                    .destination
                    .as_ref()
                    .expect("collection requires a verdict");
                insert = take.insertion_plan(destination);
                let reason = match destination {
                    Destination::Sambler(reason) => *reason,
                    Destination::Field(_) => KeptReason::Incomplete, // Wait for Windows.
                };
                TakeOutcome::KeptInSambler { reason }
            }
        };
        let id = take.id;
        let completion = full.as_ref().map(|text| Completion {
            id,
            words: text.unicode_words().count() as u64,
            listening_ms: take.listening_ms,
            archive: self
                .archive
                .then(|| crate::storage::history::CompletedTake {
                    id,
                    created: now.unix,
                    text: text.clone(),
                    language: take.rules.language,
                    profile: take.rules.profile,
                }),
        });
        if let Some(plan) = insert {
            // One selection and at most one attempt; late or duplicate
            // collection results cannot start another insertion.
            take.stage = Stage::Inserting;
            take.pending_insert = Some((Draft::new(id, parts, full.clone()), completion.unwrap()));
            effects.push(Effect::Insert(id, plan, full.unwrap()));
        } else {
            if !matches!(outcome, TakeOutcome::NoText)
                || parts.iter().any(|p| {
                    matches!(
                        p.status,
                        PartStatus::Blocked | PartStatus::NeedsReview | PartStatus::Missing
                    )
                })
            {
                let mut draft = Draft::new(id, parts, full);
                draft.set_lifetime(now, self.settings.draft_minutes);
                self.save_draft(draft);
            }
            if let Some(completion) = completion {
                effects.push(Effect::Completed(completion));
            }
            self.end(outcome, effects);
        }
    }

    fn interrupt(&mut self, reason: InterruptReason, now: Now, effects: &mut Vec<Effect>) {
        let take = self.take.as_mut().unwrap();
        let draft = if let Some((draft, _)) = take.pending_insert.take() {
            Some(draft)
        } else {
            let parts = parts(std::mem::take(&mut take.segments), true);
            (parts.len() > 1).then(|| Draft::new(take.id, parts, None))
        };
        let mut recovered = false;
        if let Some(mut draft) = draft {
            recovered = draft.parts.iter().any(|p| p.status == PartStatus::Admitted);
            draft.set_lifetime(now, self.settings.draft_minutes);
            self.save_draft(draft);
        }
        self.end(TakeOutcome::Interrupted { reason, recovered }, effects);
    }

    fn save_draft(&mut self, draft: Draft) {
        self.recents.truncate(MAX_RECENT - 1);
        self.recents.push_front(draft);
    }

    fn end(&mut self, outcome: TakeOutcome, effects: &mut Vec<Effect>) {
        let take = self.take.take().unwrap(); // Invalidate before any cleanup effect.
        effects.push(Effect::Retire(take.id));
        self.outcome = Some(outcome);
    }
}

fn protected_end() -> TakeOutcome {
    TakeOutcome::KeptInSambler {
        reason: KeptReason::TargetChanged {
            change: TargetChange::Protected,
        },
    }
}

fn missing() -> DraftPart {
    DraftPart {
        status: PartStatus::Missing,
        text: None,
    }
}

fn parts(segments: BTreeMap<usize, Segment>, interrupted: bool) -> Vec<DraftPart> {
    let mut parts = Vec::with_capacity(segments.len() + 1);
    let (mut index, mut end) = (0, 0);
    for segment in segments.into_values() {
        if index != segment.index || end != segment.range.start {
            parts.push(missing());
        }
        index = segment.index.saturating_add(1);
        if segment.range.end <= segment.range.start || segment.range.start < end {
            parts.push(missing());
        } else {
            end = segment.range.end;
            parts.push(segment.part);
        }
    }
    if interrupted {
        parts.push(missing());
    }
    parts
}

fn ambiguous_numeric_seam(parts: &[DraftPart]) -> bool {
    parts.windows(2).any(|pair| {
        let left = pair[0].text.as_deref().unwrap_or_default().trim_end();
        let right = pair[1].text.as_deref().unwrap_or_default().trim_start();
        let mut suffix = left.chars().rev();
        let last = suffix.next();
        let number = last.is_some_and(|ch| ch.is_ascii_digit())
            || (matches!(last, Some('.' | ','))
                && suffix.next().is_some_and(|ch| ch.is_ascii_digit()));
        number && right.starts_with(|ch: char| ch.is_ascii_digit())
    })
}
