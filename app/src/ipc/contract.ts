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
// Privacy rule: dictated text only ever arrives in `DraftView` (commands
// `get_draft` and `get_recent_drafts`, asked by the view that shows it).
// Every other type here is text-free by construction.

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
  /** Milestone 5: spoken layout commands ("à la ligne"…). Absent on older native sides. */
  voiceCommands?: boolean;
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

/**
 * `InsertionMethod`, for fields without an atomic editor API (browsers,
 * Electron apps, Outlook…). Native Edit/RichEdit and Word keep their atomic
 * paths whatever this says.
 * - `paste`: one Ctrl+V, the previous clipboard restored (excluded from Win+V
 *   history and cloud sync). Instant, multi-line capable. Default.
 * - `typing`: paced Unicode typing, never touches the clipboard; single line.
 */
export type InsertionMethod = "paste" | "typing";

/**
 * `InsertionTarget`: which field receives the text when the take ends.
 * - `original-field`: the field where the take started, provided it has the
 *   focus again at the end (leaving it and coming back is fine); otherwise
 *   the text stays in Sambler. Default.
 * - `cursor-at-end`: whatever eligible field has the focus at the end. A
 *   password field or an unrecognized field keeps the text in Sambler.
 */
export type InsertionTarget = "original-field" | "cursor-at-end";

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
  insertion: InsertionMethod;
  insertionTarget: InsertionTarget;
  /** Milestone 4. Absent when the native side predates themes. */
  appearance?: AppearanceSettings;
  /** Milestone 5. At most 50, one per application id. */
  appStyles?: AppStyle[];
}

export type ColorSchemeChoice = "system" | "light" | "dark";

/**
 * Visual theme and colour scheme. `theme` is an id from `themes/registry.ts`;
 * the native side stores any short id (lowercase letters, digits, hyphens,
 * max 32) and the UI falls back to the default theme for an unknown one.
 */
export interface AppearanceSettings {
  theme: string;
  colorScheme: ColorSchemeChoice;
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
    insertion: "paste",
    insertionTarget: "original-field",
    appearance: { theme: "verre-aurore", colorScheme: "system" },
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
  | { kind: "incomplete" }
  /** The field could not be qualified at the trigger: the take went to Sambler instead of refusing. */
  | { kind: "uncertain-target"; reason: UncertainReason };

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
  /** Legacy: an uncertain field now sends the take to Sambler (`KeptReason`). */
  | { kind: "uncertain-target"; reason: UncertainReason }
  /** Legacy (no longer produced): recent results are kept side by side. */
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
  /** Number of recent results kept in RAM (see `get_recent_drafts`). */
  recentCount: number;
  /** Milestone 4: same as `Settings.appearance`, so the floating bar re-themes live. */
  appearance?: AppearanceSettings;
}

export type PartStatus = "admitted" | "no-text" | "needs-review" | "blocked" | "missing";

export interface DraftPart {
  status: PartStatus;
  /** Present only for `admitted` (and `needs-review` when displayable). */
  text: string | null;
}

/** The only type that carries dictated text. Returned by `get_draft` and `get_recent_drafts` only. */
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

// ------------------------------------------------------------------ ipc.rs milestone 3

/** Counters only (no text), persisted as plain JSON in the data root. */
export interface UsageStats {
  wordsToday: number;
  wordsThisWeek: number;
  wordsTotal: number;
  takesTotal: number;
  /** Words per minute over listening time of completed takes, when known. */
  averageWpm: number | null;
}

// ------------------------------------------------------------------ milestone 4 (Activité)
//
// Proposed by the theme work; implemented natively by the windows-v1 session.
// Until the native side answers `get_activity`, the Activité screen falls back
// to `get_stats` (the command error is expected and handled).

/** One local calendar day. Counters only, never text. */
export interface DayActivity {
  /** Local date, "YYYY-MM-DD". */
  date: string;
  words: number;
  takes: number;
}

export interface AppShare {
  /** Display name of the target application, e.g. "Word". */
  app: string;
  words: number;
  /** Milestone 5: the `AppIdentity.id`, to link with per-app styles. */
  id?: string;
}

export interface ActivityStats {
  /** Last 112 local days (16 weeks), oldest first, ending today, zero days included. */
  days: DayActivity[];
  /** Consecutive days with at least one completed take, ending today (or yesterday if none today yet). */
  currentStreak: number;
  longestStreak: number;
  /** Last day of the longest streak, "YYYY-MM-DD", null before any take. */
  longestStreakEnd: string | null;
  /** First day with a completed take, "YYYY-MM-DD", null before any take. */
  firstDay: string | null;
  /** Captured audio time of completed takes, for "temps gagné" (words / 40 wpm − this). */
  listeningMsTotal: number;
  /** Words per target application, largest first; null when not collected. */
  apps: AppShare[] | null;
}

// ------------------------------------------------------------------ milestone 5
//
// Applications (per-app counters and styles), dictation commands and the
// Carnet (dictated notes). Proposed by the theme work, implemented natively by
// the windows-v1 session. Each screen degrades gracefully while a command is
// missing (the command error is expected and handled).

/**
 * A target application as Sambler recognises it. `id` is stable (lowercased
 * executable name without extension, e.g. "winword"); `name` is for display
 * ("Word").
 */
export interface AppIdentity {
  id: string;
  name: string;
}

/**
 * Text style for one application, applied on top of the global profile and
 * text options when a take's target is this application.
 */
export interface AppStyle {
  app: AppIdentity;
  /** Omitted: the global profile. */
  profile?: RenderProfile;
  /** Only the listed options are overridden. */
  text?: Partial<TextOptions>;
}

/** Spoken layout command ("à la ligne"), applied locally after Rambler. */
export interface VoiceCommand {
  /** What the person says, e.g. "à la ligne". */
  spoken: string;
  /** What is written, shown to the person, e.g. "↵" or "« ». */
  result: string;
  description: string;
}

export interface NoteMeta {
  id: string;
  /** First non-empty line, at most 80 characters, computed natively. */
  title: string;
  /** Milliseconds since the Unix epoch. */
  createdAt: number;
  updatedAt: number;
  words: number;
}

/** A note of the Carnet. Carries text: never in events or logs. */
export interface Note extends NoteMeta {
  text: string;
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
  /** `take` omitted: the latest recent result. */
  copy_draft: { args: { take?: TakeId }; result: void };
  /** `take` omitted: every recent result. */
  clear_draft: { args: { take?: TakeId }; result: UiState };
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
  // milestone 3
  /** Newest first, at most 5, RAM only, same lifetime as the draft. */
  get_recent_drafts: { args: void; result: DraftView[] };
  get_stats: { args: void; result: UsageStats };
  // milestone 4
  get_activity: { args: void; result: ActivityStats };
  // milestone 5
  /** Applications seen in the per-app counters, most used first. */
  get_known_apps: { args: void; result: AppIdentity[] };
  /** The fixed French list, for display. */
  get_voice_commands: { args: void; result: VoiceCommand[] };
  /** Most recently updated first. */
  list_notes: { args: void; result: NoteMeta[] };
  get_note: { args: { id: string }; result: Note };
  /** `id: null` creates a note. Returns the stored metadata. */
  save_note: { args: { note: { id: string | null; text: string } }; result: NoteMeta };
  delete_note: { args: { id: string }; result: void };
}

export type CommandName = keyof CommandMap;

export const EVENT_STATE = "sambler://state";
export const EVENT_LEVEL = "sambler://level";
export const EVENT_MIC_TEST_LEVEL = "sambler://mic-test-level";

export interface EventMap {
  [EVENT_STATE]: UiState;
  /** 0..=1 (−60..0 dBFS, linear), about 20–30 Hz while listening. */
  [EVENT_LEVEL]: number;
  /** 0..=1 during a microphone test. */
  [EVENT_MIC_TEST_LEVEL]: number;
}

export type EventName = keyof EventMap;
