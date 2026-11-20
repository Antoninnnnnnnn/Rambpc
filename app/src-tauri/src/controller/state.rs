//! Pure take transitions. Time and observations are supplied by the runtime.

use crate::{
    ipc::{
        Availability, DraftMeta, DraftPart, DraftView, InterruptReason, KeptReason, PartStatus,
        Phase, RefusalReason, TakeOutcome, UiState, UnavailableReason,
    },
    model::{DestinationKind, Gesture, KeyEdge, Language, MicChoice, SystemEvent, TakeId},
    ports::{InsertOutcome, MicError, TargetChange, TargetToken, TargetVerdict},
    rambler::{FRAME_WAIT_MS, MAX_DRAFT_BYTES, Segment},
    settings::Settings,
    text::{self, TextRules},
};
use rambler_audio::join_segment_texts;
use std::{collections::BTreeMap, sync::Arc};

const OPEN_MS: u64 = 3_000;
const MAX_LISTENING_MS: u64 = 300_000;
const REPLACE_MS: u64 = 15_000;
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
    Target(TakeId, TargetVerdict),
    TargetChanged(TakeId, TargetChange),
    MicOpened(TakeId),
    MicOpenFailed(TakeId, MicError),
    MicFault(TakeId, InterruptReason),
    MicFree(TakeId),
    Tick { last_frame: Option<u64> },
    Delivered(TakeId, Segment),
    Collection(TakeId, Result<(), InterruptReason>),
    Insertion(TakeId, InsertOutcome),
    Copied(TakeId),
    ClearDraft,
    Shutdown,
}

pub(super) enum Effect {
    Reconfigure(Option<u64>),
    Probe { take: TakeId, watch: bool },
    Open { take: TakeId, mic: MicChoice },
    Collect { take: TakeId, language: Language },
    Stop(TakeId),
    Retire(TakeId),
    Archive(crate::storage::history::CompletedTake),
    Insert(TakeId, TargetToken, Arc<str>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Checking,
    Opening,
    AwaitingFrames { opened: u64 },
    Listening { since: u64 },
    Finalizing { collected: bool },
    Inserting,
}

impl Stage {
    fn phase(self) -> Phase {
        match self {
            Self::Checking | Self::Opening | Self::AwaitingFrames { .. } => Phase::Preparing,
            Self::Listening { .. } => Phase::Listening,
            Self::Finalizing { .. } | Self::Inserting => Phase::Finalizing,
        }
    }
}

enum Destination {
    Sambler,
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
    replace: Option<TakeId>,
    destination: Option<Destination>,
    change: Option<TargetChange>,
    open_deadline: Option<u64>,
    segments: BTreeMap<usize, Segment>,
    bytes: usize,
}

struct Draft {
    meta: DraftMeta,
    expires_mono: u64,
    parts: Vec<DraftPart>,
    full: Option<Arc<str>>,
    pending: bool,
    replace_by: Option<u64>,
}

impl Draft {
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
    draft: Option<Draft>,
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
            draft: None,
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
                Some(Destination::Sambler) => Some(DestinationKind::Sambler),
                None => None,
            }),
            availability: self.availability.clone(),
            last_outcome: self.outcome.clone(),
            draft: self.draft.as_ref().map(|draft| draft.meta),
            listening_ms: take.and_then(|take| match take.stage {
                Stage::Listening { since } => Some(now.mono.saturating_sub(since)),
                _ => None,
            }),
            gesture: take.map_or(self.settings.gesture, |take| take.gesture),
            shortcut: self.settings.shortcut.clone(),
        }
    }

    pub(super) fn draft(&self) -> Option<DraftView> {
        self.draft.as_ref().map(Draft::view)
    }

    pub(super) fn draft_id(&self) -> Option<TakeId> {
        self.draft.as_ref().map(|draft| draft.meta.take)
    }

    pub(super) fn copy_text(&self) -> Option<(TakeId, Arc<str>)> {
        let draft = self.draft.as_ref()?;
        Some((draft.meta.take, draft.copy_text()?))
    }

    pub(super) fn step(&mut self, input: Input, now: Now) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self
            .draft
            .as_ref()
            .is_some_and(|draft| now.mono >= draft.expires_mono)
        {
            self.draft = None;
        }
        match input {
            Input::Hotkey(KeyEdge::Pressed) if !self.key_down => {
                self.key_down = true;
                match &self.take {
                    None => self.start(true, now, &mut effects),
                    Some(take) if take.gesture == Gesture::Toggle => {
                        self.finish(false, &mut effects)
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
                    self.finish(false, &mut effects);
                }
            }
            Input::Hotkey(_) => (),
            Input::StartInternal => self.start(false, now, &mut effects),
            Input::Finish(id) if self.take_id() == Some(id) => self.finish(false, &mut effects),
            Input::Cancel(id) => {
                if (id.is_none() && self.take.is_some()) || (id.is_some() && id == self.take_id()) {
                    if self.take_id() == self.draft_id() {
                        self.draft = None;
                    }
                    self.end(TakeOutcome::Cancelled, &mut effects);
                } else if id.is_some() && id == self.draft_id() {
                    self.draft = None;
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
                    self.draft = None;
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
                if let Some(take) = &mut self.take
                    && take.stage == Stage::Checking
                    && take.destination.is_none()
                {
                    let destination = match verdict {
                        TargetVerdict::Protected => Err(RefusalReason::ProtectedField),
                        TargetVerdict::Uncertain { reason } => {
                            Err(RefusalReason::UncertainTarget { reason })
                        }
                        TargetVerdict::Sambler => Ok(Destination::Sambler),
                        TargetVerdict::Eligible { .. } if take.always_keep => {
                            Ok(Destination::Sambler)
                        }
                        TargetVerdict::Eligible { token, .. } => Ok(Destination::Field(token)),
                    };
                    match destination {
                        Ok(destination) => take.destination = Some(destination),
                        Err(reason) => self.end(TakeOutcome::Refused { reason }, &mut effects),
                    }
                }
            }
            Input::TargetChanged(id, change) if self.take_id() == Some(id) => {
                if change == TargetChange::Protected {
                    self.interrupt(InterruptReason::SystemInterrupted, now, &mut effects);
                } else if let Some(take) = &mut self.take {
                    take.change.get_or_insert(change);
                }
            }
            Input::MicOpened(id) if self.take_id() == Some(id) => {
                let take = self.take.as_mut().unwrap();
                if take.stage == Stage::Opening {
                    take.stage = Stage::AwaitingFrames { opened: now.mono };
                    effects.push(Effect::Collect {
                        take: id,
                        language: take.rules.language,
                    });
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
            Input::MicFree(id) if self.mic_owner == Some(id) => {
                self.mic_owner = None;
                self.complete_if_stopped(now, &mut effects);
            }
            Input::Tick { last_frame } => self.tick(now, last_frame, &mut effects),
            Input::Delivered(id, segment) if self.take_id() == Some(id) => {
                let take = self.take.as_mut().unwrap();
                if matches!(
                    take.stage,
                    Stage::AwaitingFrames { .. }
                        | Stage::Listening { .. }
                        | Stage::Finalizing {
                            collected: false,
                            ..
                        }
                ) && !take.segments.contains_key(&segment.index)
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
                        && matches!(
                            t.stage,
                            Stage::AwaitingFrames { .. }
                                | Stage::Listening { .. }
                                | Stage::Finalizing { collected: false }
                        )
                }) =>
            {
                match result {
                    Ok(()) => {
                        let take = self.take.as_mut().unwrap();
                        if take.stage == (Stage::Finalizing { collected: false }) {
                            take.stage = Stage::Finalizing { collected: true };
                            self.complete_if_stopped(now, &mut effects);
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
                if let Some(draft) = &mut self.draft
                    && draft.meta.take == id
                {
                    draft.pending &= outcome != InsertOutcome::Delivered;
                }
                self.end(
                    match outcome {
                        InsertOutcome::Delivered => TakeOutcome::Inserted,
                        InsertOutcome::Partial => TakeOutcome::InsertedPartially,
                        InsertOutcome::NotWritten { reason } => TakeOutcome::KeptInSambler {
                            reason: if let Some(change) = self.take.as_ref().and_then(|t| t.change)
                            {
                                KeptReason::TargetChanged { change }
                            } else {
                                KeptReason::NotWritten { reason }
                            },
                        },
                    },
                    &mut effects,
                );
            }
            Input::Copied(id) if self.draft_id() == Some(id) => {
                let draft = self.draft.as_mut().unwrap();
                draft.pending = false;
                draft.replace_by = None;
            }
            Input::ClearDraft => self.draft = None,
            Input::Shutdown => {
                self.draft = None;
                if self.take.is_some() {
                    self.end(TakeOutcome::Cancelled, &mut effects);
                }
                effects.push(Effect::Reconfigure(None));
            }
            _ => (), // Stale or out-of-phase notification.
        }
        self.maybe_open(now, &mut effects);
        effects
    }

    fn start(&mut self, hotkey: bool, now: Now, effects: &mut Vec<Effect>) {
        if self.take.is_some() || self.locked || self.suspended {
            return;
        }
        self.next_take = self
            .next_take
            .checked_add(1)
            .expect("take id space exhausted");
        let id = TakeId(self.next_take);
        let reconnect = matches!(
            self.availability,
            Availability::Unavailable {
                reason: UnavailableReason::Network | UnavailableReason::Service
            }
        );
        let refusal = match self.availability {
            Availability::NeedsConsent => Some(RefusalReason::NeedsConsent),
            Availability::NeedsAccess => Some(RefusalReason::NeedsAccess),
            Availability::Unavailable { reason } if !reconnect => {
                Some(RefusalReason::Unavailable { reason })
            }
            _ => None,
        };
        if let Some(reason) = refusal {
            self.outcome = Some(TakeOutcome::Refused { reason });
            return;
        }
        let replace = match &mut self.draft {
            Some(draft) if draft.pending => {
                if draft
                    .replace_by
                    .take()
                    .is_some_and(|deadline| now.mono <= deadline)
                {
                    Some(draft.meta.take)
                } else {
                    draft.replace_by = Some(now.mono + REPLACE_MS);
                    self.outcome = Some(TakeOutcome::Refused {
                        reason: RefusalReason::PendingDraft,
                    });
                    return;
                }
            }
            _ => None,
        };
        let mut rules = TextRules::from_settings(&self.settings);
        if let Some(personal) = &self.personal_rules {
            rules.lexicon.clone_from(&personal.lexicon);
            rules.expressions.clone_from(&personal.expressions);
        }
        self.outcome = None;
        self.take = Some(Take {
            id,
            stage: Stage::Checking,
            gesture: self.settings.gesture,
            hotkey,
            mic: self.settings.microphone.clone(),
            rules,
            always_keep: self.settings.always_keep_in_sambler,
            replace,
            destination: (!hotkey).then_some(Destination::Sambler),
            change: None,
            open_deadline: None,
            segments: BTreeMap::new(),
            bytes: 0,
        });
        if hotkey {
            effects.push(Effect::Probe {
                take: id,
                watch: !self.settings.always_keep_in_sambler,
            });
        }
        if reconnect {
            self.reconfigure(effects);
        }
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

    fn maybe_open(&mut self, now: Now, effects: &mut Vec<Effect>) {
        if self.availability != Availability::Ready {
            return;
        }
        let Some(take) = &mut self.take else {
            return;
        };
        if take.stage != Stage::Checking || take.destination.is_none() {
            return;
        }
        take.open_deadline.get_or_insert(now.mono + OPEN_MS);
        if self.mic_owner.is_some() {
            return;
        }
        if take.replace.is_some()
            && take.replace == self.draft.as_ref().map(|draft| draft.meta.take)
        {
            self.draft = None;
        }
        take.stage = Stage::Opening;
        self.mic_owner = Some(take.id);
        effects.push(Effect::Open {
            take: take.id,
            mic: take.mic.clone(),
        });
    }

    fn tick(&mut self, now: Now, last_frame: Option<u64>, effects: &mut Vec<Effect>) {
        let Some(take) = &mut self.take else {
            return;
        };
        match take.stage {
            Stage::Checking | Stage::Opening
                if take.open_deadline.is_some_and(|d| now.mono >= d) =>
            {
                self.end(
                    TakeOutcome::Refused {
                        reason: RefusalReason::Microphone {
                            error: MicError::OpenTimeout,
                        },
                    },
                    effects,
                );
            }
            Stage::AwaitingFrames { opened } => {
                if now.mono.saturating_sub(last_frame.unwrap_or(opened)) >= FRAME_WAIT_MS {
                    self.interrupt(InterruptReason::MicrophoneStalled, now, effects);
                } else if last_frame.is_some() {
                    take.stage = Stage::Listening { since: opened };
                }
            }
            Stage::Listening { since } => {
                if now.mono.saturating_sub(last_frame.unwrap_or(since)) >= FRAME_WAIT_MS {
                    self.interrupt(InterruptReason::MicrophoneStalled, now, effects);
                } else if now.mono.saturating_sub(since) >= MAX_LISTENING_MS {
                    self.finish(true, effects);
                }
            }
            _ => (),
        }
    }

    fn finish(&mut self, limited: bool, effects: &mut Vec<Effect>) {
        let take = self.take.as_mut().unwrap();
        match take.stage.phase() {
            Phase::Preparing => self.end(TakeOutcome::Cancelled, effects),
            Phase::Listening => {
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
            _ => (),
        }
    }

    fn complete_if_stopped(&mut self, now: Now, effects: &mut Vec<Effect>) {
        if self.mic_owner.is_some() {
            return;
        }
        let Some(take) = &mut self.take else {
            return;
        };
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
                let reason = match (&take.destination, take.change) {
                    (Some(Destination::Field(token)), None) => {
                        take.stage = Stage::Inserting; // Consume the right before the effect.
                        insert = Some(token.clone());
                        KeptReason::Incomplete // Not published; wait for Windows.
                    }
                    (Some(Destination::Field(_)), Some(change)) => {
                        KeptReason::TargetChanged { change }
                    }
                    _ => KeptReason::SamblerDestination,
                };
                TakeOutcome::KeptInSambler { reason }
            }
        };
        let id = take.id;
        if self.archive
            && let Some(text) = &full
        {
            effects.push(Effect::Archive(crate::storage::history::CompletedTake {
                id,
                created: now.unix,
                text: text.clone(),
                language: take.rules.language,
                profile: take.rules.profile,
            }));
        }
        let pending = has_text
            && !matches!(
                outcome,
                TakeOutcome::NoText
                    | TakeOutcome::KeptInSambler {
                        reason: KeptReason::SamblerDestination
                    }
            );
        if !matches!(outcome, TakeOutcome::NoText)
            || parts.iter().any(|p| {
                matches!(
                    p.status,
                    PartStatus::Blocked | PartStatus::NeedsReview | PartStatus::Missing
                )
            })
        {
            self.save_draft(id, parts, full.clone(), pending, now);
        }
        if let Some(token) = insert {
            effects.push(Effect::Insert(
                id,
                token,
                full.expect("complete text for insertion"),
            ));
        } else {
            self.end(outcome, effects);
        }
    }

    fn interrupt(&mut self, reason: InterruptReason, now: Now, effects: &mut Vec<Effect>) {
        let take = self.take.as_mut().unwrap();
        let id = take.id;
        if take.stage != Stage::Inserting {
            let parts = parts(std::mem::take(&mut take.segments), true);
            if parts.len() > 1 {
                let recovered = parts.iter().any(|p| p.status == PartStatus::Admitted);
                self.save_draft(id, parts, None, recovered, now);
            }
        }
        let recovered = self.draft.as_ref().is_some_and(|d| {
            d.meta.take == id && d.parts.iter().any(|p| p.status == PartStatus::Admitted)
        });
        self.end(TakeOutcome::Interrupted { reason, recovered }, effects);
    }

    fn save_draft(
        &mut self,
        take: TakeId,
        parts: Vec<DraftPart>,
        full: Option<Arc<str>>,
        pending: bool,
        now: Now,
    ) {
        let life = u64::from(self.settings.draft_minutes.clamp(1, 10)) * 60_000;
        self.draft = Some(Draft {
            meta: DraftMeta {
                take,
                created_at_ms: now.unix,
                expires_at_ms: now.unix.saturating_add(life),
                complete: full.is_some(),
            },
            expires_mono: now.mono + life,
            parts,
            full,
            pending,
            replace_by: None,
        });
    }

    fn end(&mut self, outcome: TakeOutcome, effects: &mut Vec<Effect>) {
        let take = self.take.take().unwrap(); // Invalidate before any cleanup effect.
        effects.push(Effect::Retire(take.id));
        self.outcome = Some(outcome);
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
