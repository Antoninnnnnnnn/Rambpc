// Hand-written mirror of the JSON crossing the WebView boundary.
// Sources of truth (keep in sync):
//   app/src-tauri/src/ipc.rs       commands, events, UiState, outcomes, draft, access
//   app/src-tauri/src/settings.rs  Settings and defaults
//   app/src-tauri/src/model.rs     Language, RenderProfile, Gesture, MicChoice, DestinationKind
//   app/src-tauri/src/ports.rs     MicInfo, MicError, TargetChange, UncertainReason,
//                                  NotWrittenReason, ClipboardError
//   app/src-tauri/src/text/        LexiconEntry, Expression (mod.rs); RuleConflict,
//                                  RuleConflictReason, RuleRef, RuleKind (personal.rs)
//
// History text and rule text are, like dictated text, only ever shown by the
// view that asked for them (get_history, get_personal_rules).
//
// Serde shapes:
//   #[serde(tag = "kind", rename_all = "kebab-case")] enums -> { kind: "...", ...fields }
//   #[serde(rename_all = "kebab-case")] unit enums          -> "..." strings
//   #[serde(rename_all = "camelCase")] structs              -> camelCase keys
//   Option<T>                                               -> T | null
//
// Privacy rule: dictated text only ever arrives in `DraftView` (command
// `get_draft`). Every other type here is text-free by construction.

// ------------------------------------------------------------------ model.rs

/** `TakeId(u64)`, `#[serde(transparent)]`. Monotonic, never reused. */
export type TakeId = number;

export type Language = "french" | "english";

export type RenderProfile = "natural" | "faithful";

export type Gesture = "hold" | "toggle";

export type MicChoice =
  | { kind: "system-default" }
  | { kind: "named"; id: string; name: string };

export type DestinationKind = "field" | "sambler";

// ------------------------------------------------------------------ ports.rs

/**
 * `MicInfo` has no `rename_all` attribute in ports.rs, so its boolean field
 * crosses the boundary in snake_case (`is_default`).
 */
export interface MicInfo {
  id: string;
  name: string;
  is_default: boolean;
}

export type MicError =
  | { kind: "no-input" }
  | { kind: "not-found" }
  | { kind: "access-denied" }
  | { kind: "unsupported-format"; format: string }
  | { kind: "open-timeout" }
  | { kind: "disconnected" }
  | { kind: "other" };

export type UncertainReason =
  | "no-focused-field"
  | "read-only"
  | "selection"
  | "elevated"
  | "terminal"
  | "remote-desktop"
  | "unresponsive"
  | "unsupported";

export type TargetChange =
  | "focus-moved"
  | "selection-changed"
  | "window-gone"
  | "protected"
  | "unknown";

export type NotWrittenReason =
  | "target-changed"
  | "modifiers-held"
  | "unsupported"
  | "refused";

export type ClipboardError = "busy" | "exclusion-failed" | "other";

// ------------------------------------------------------------------ settings.rs

export type IndicatorPosition = "bottom-center" | "top-center" | "near-cursor";

export interface TextOptions {
  lightPunctuation: boolean;
  typographicSpaces: boolean;
  sentenceCase: boolean;
}

export interface IndicatorOptions {
  sound: boolean;
  position: IndicatorPosition;
}

export interface HistoryOptions {
  enabled: boolean;
  /** 1, 7 or 30. */
  retentionDays: number;
}

export interface Settings {
  version: number;
  language: Language;
  microphone: MicChoice;
  gesture: Gesture;
  /** Tauri accelerator syntax, e.g. "Ctrl+Shift+F2". */
  shortcut: string;
  cancelWithEscape: boolean;
  profile: RenderProfile;
  text: TextOptions;
  alwaysKeepInSambler: boolean;
  indicator: IndicatorOptions;
  /** 1, 5 or 10. */
  draftMinutes: number;
  history: HistoryOptions;
  launchAtStartup: boolean;
}

export const SETTINGS_VERSION = 1;
export const DEFAULT_SHORTCUT = "Ctrl+Shift+F2";
export const DRAFT_MINUTES_CHOICES = [1, 5, 10] as const;
export const HISTORY_DAYS_CHOICES = [1, 7, 30] as const;

/** `Settings::default()` in settings.rs. */
export function defaultSettings(): Settings {
  return {
    version: SETTINGS_VERSION,
    language: "french",
    microphone: { kind: "system-default" },
    gesture: "hold",
    shortcut: DEFAULT_SHORTCUT,
    cancelWithEscape: true,
    profile: "natural",
    text: { lightPunctuation: true, typographicSpaces: true, sentenceCase: true },
    alwaysKeepInSambler: false,
    indicator: { sound: false, position: "bottom-center" },
    draftMinutes: 10,
    history: { enabled: false, retentionDays: 7 },
    launchAtStartup: false,
  };
}

// ------------------------------------------------------------------ ipc.rs

export type Phase =
  | { kind: "idle" }
  | { kind: "preparing" }
  | { kind: "listening" }
  | { kind: "finalizing" };

export type PhaseKind = Phase["kind"];

export type UnavailableReason =
  | "network"
  | "access-refused"
  | "throttled"
  | "invalid-access"
  | "service";

export type Availability =
  | { kind: "needs-consent" }
  | { kind: "needs-access" }
  | { kind: "connecting" }
  | { kind: "ready" }
  | { kind: "unavailable"; reason: UnavailableReason };

export type KeptReason =
  | { kind: "sambler-destination" }
  | { kind: "target-changed"; change: TargetChange }
  | { kind: "not-written"; reason: NotWrittenReason }
  | { kind: "incomplete" };

export type InterruptReason =
  | { kind: "microphone-stalled" }
  | { kind: "microphone"; error: MicError }
  | { kind: "capture-overflow" }
  | { kind: "network" }
  | { kind: "access-refused" }
  | { kind: "throttled" }
  | { kind: "timeout" }
  | { kind: "service" }
  | { kind: "system-interrupted" }
  | { kind: "consent-withdrawn" }
  | { kind: "duration-limit" };

export type RefusalReason =
  | { kind: "needs-consent" }
  | { kind: "needs-access" }
  | { kind: "unavailable"; reason: UnavailableReason }
  | { kind: "protected-field" }
  | { kind: "uncertain-target"; reason: UncertainReason }
  | { kind: "pending-draft" }
  | { kind: "microphone"; error: MicError };

export type TakeOutcome =
  | { kind: "inserted" }
  | { kind: "inserted-partially" }
  | { kind: "kept-in-sambler"; reason: KeptReason }
  | { kind: "no-text" }
  | { kind: "cancelled" }
  | { kind: "interrupted"; reason: InterruptReason; recovered: boolean }
  | { kind: "refused"; reason: RefusalReason };

/** Draft metadata (no text). Times are Unix epoch milliseconds. */
export interface DraftMeta {
  take: TakeId;
  createdAtMs: number;
  expiresAtMs: number;
  complete: boolean;
}

/** Text-free state, pushed on `sambler://state`. */
export interface UiState {
  phase: Phase;
  take: TakeId | null;
  destination: DestinationKind | null;
  availability: Availability;
  lastOutcome: TakeOutcome | null;
  draft: DraftMeta | null;
  /** Milliseconds since listening started (5-minute limit warning). */
  listeningMs: number | null;
  gesture: Gesture;
  shortcut: string;
}

export type PartStatus = "admitted" | "no-text" | "needs-review" | "blocked" | "missing";

export interface DraftPart {
  status: PartStatus;
  /** Present only for `admitted` (and `needs-review` when displayable). */
  text: string | null;
}

/** The only type that carries dictated text. Returned by `get_draft` only. */
export interface DraftView {
  meta: DraftMeta;
  parts: DraftPart[];
  /** Rendered full text when the take is complete; null otherwise. */
  fullText: string | null;
}

export type AccessSource = "none" | "environment" | "saved" | "session";

/** Never contains the secret values. */
export interface AccessStatus {
  configured: boolean;
  source: AccessSource;
  /** host:port, not a secret. */
  authority: string | null;
  /** False in the portable edition. */
  canRemember: boolean;
}

/** Sent once to `set_access`; never stored or displayed by the WebView. */
export interface AccessInput {
  authority: string;
  apiKey: string;
  androidPackage: string;
  androidCert: string;
  remember: boolean;
}

/**
 * Error returned by commands. `code` is stable; `message` is French and text-free.
 * Codes used by the UI (shell/commands.rs): `clipboard-busy`,
 * `clipboard-exclusion-failed`, `clipboard-other`, `no-draft`, `mic-<MicError kind>`
 * (e.g. `mic-access-denied`), `shortcut`, `invalid-settings`, `save-failed`
 * (applied for this session but not written), `portable` (set_launch_at_startup).
 */
export interface CommandError {
  code: string;
  message: string;
}

// ------------------------------------------------------------------ text/ (milestone 2)

/** `RuleKind`, `#[serde(rename_all = "camelCase")]` (text/personal.rs). */
export type RuleKind = "lexicon" | "expression";

export interface RuleRef {
  kind: RuleKind;
  id: string;
}

/** `RuleConflictReason`, camelCase strings (not kebab-case). */
export type RuleConflictReason =
  | "emptyMatch"
  | "emptyReplacement"
  | "noMatchableWords"
  | "multilineMatch"
  | "duplicateHeard"
  | "duplicateTrigger"
  | "triggerShadowsHeard"
  /** `rule` is the shorter trigger; `related` contains it at word boundaries. */
  | "triggerOverlap";

export interface RuleConflict {
  reason: RuleConflictReason;
  rule: RuleRef;
  related: RuleRef | null;
}

/** Whole-word replacement: what Rambler writes -> what the person wants. */
export interface LexiconEntry {
  id: string;
  heard: string;
  written: string;
  enabled: boolean;
  /** Substitutions in Faithful need a separate opt-in. */
  applyInFaithful: boolean;
}

/** Explicit trigger phrase -> literal text (may contain line breaks). */
export interface Expression {
  id: string;
  trigger: string;
  text: string;
  enabled: boolean;
}

// ------------------------------------------------------------------ ipc.rs milestone 2

export interface PersonalRules {
  lexicon: LexiconEntry[];
  expressions: Expression[];
  /** Stored rules exist but cannot be decrypted under this Windows account. */
  unreadable: boolean;
}

export interface PersonalRulesInput {
  lexicon: LexiconEntry[];
  expressions: Expression[];
}

/** `saved` is false when enabled rules conflict; nothing is written then. */
export interface SaveRulesResult {
  saved: boolean;
  rules: PersonalRules;
  conflicts: RuleConflict[];
}

/** `#[serde(default)]`: every field may be omitted. */
export interface HistoryQuery {
  search?: string | null;
  /** Default 50, max 200. */
  limit?: number | null;
  /** Opaque cursor from a previous page. */
  cursor?: string | null;
}

export interface HistoryEntry {
  id: string;
  createdAtMs: number;
  expiresAtMs: number;
  language: Language;
  profile: RenderProfile;
  /** null when this entry cannot be decrypted under this account. */
  text: string | null;
}

export interface HistoryPage {
  entries: HistoryEntry[];
  nextCursor: string | null;
}

export interface HistoryStatus {
  enabled: boolean;
  retentionDays: number;
  entryCount: number;
  /** "Sans historique pour la prochaine dictée" is armed. */
  skipNext: boolean;
  /** False in ephemeral sessions (read-only data folder). */
  available: boolean;
}

// ------------------------------------------------------------------ commands

/** Every command of ipc.rs. `args` are camelCase on the JS side. */
export interface CommandMap {
  get_state: { args: void; result: UiState };
  get_settings: { args: void; result: Settings };
  save_settings: { args: { settings: Settings }; result: Settings };
  list_microphones: { args: void; result: MicInfo[] };
  start_mic_test: { args: void; result: void };
  stop_mic_test: { args: void; result: void };
  set_consent: { args: { enabled: boolean }; result: UiState };
  get_access: { args: void; result: AccessStatus };
  set_access: { args: { access: AccessInput }; result: AccessStatus };
  clear_access: { args: void; result: AccessStatus };
  retry_connection: { args: void; result: UiState };
  dictate_in_sambler: { args: void; result: UiState };
  finish_take: { args: { take: TakeId }; result: UiState };
  cancel_take: { args: { take: TakeId }; result: UiState };
  get_draft: { args: void; result: DraftView | null };
  copy_draft: { args: void; result: void };
  clear_draft: { args: void; result: UiState };
  open_mic_privacy_settings: { args: void; result: void };
  /** Text rules on a sample sentence, without microphone or network (max 500 chars). */
  preview_render: { args: { sample: string; settings: Settings }; result: string };
  // milestone 2
  get_personal_rules: { args: void; result: PersonalRules };
  save_personal_rules: { args: { rules: PersonalRulesInput }; result: SaveRulesResult };
  get_history: { args: { query: HistoryQuery }; result: HistoryPage };
  copy_history_entry: { args: { id: string }; result: void };
  delete_history_entry: { args: { id: string }; result: HistoryStatus };
  /** `disable: true` also turns the history off. */
  clear_history: { args: { disable: boolean }; result: HistoryStatus };
  get_history_status: { args: void; result: HistoryStatus };
  skip_history_next_take: { args: { skip: boolean }; result: HistoryStatus };
  /** Installed edition only: the portable edition answers `portable`. */
  set_launch_at_startup: { args: { enabled: boolean }; result: void };
}

export type CommandName = keyof CommandMap;

export const EVENT_STATE = "sambler://state";
export const EVENT_LEVEL = "sambler://level";
export const EVENT_MIC_TEST_LEVEL = "sambler://mic-test-level";

export interface EventMap {
  [EVENT_STATE]: UiState;
  /** 0..=1, about 20 Hz while listening. */
  [EVENT_LEVEL]: number;
  /** 0..=1 during a microphone test. */
  [EVENT_MIC_TEST_LEVEL]: number;
}

export type EventName = keyof EventMap;
