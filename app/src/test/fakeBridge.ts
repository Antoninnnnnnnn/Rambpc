// Test double for the Tauri bridge. Lives under src/test/ and is imported
// only by *.test.tsx files; the application never references it.

import { vi, type Mock } from "vitest";
import { sampleActivity } from "../dev/sampleActivity";
import type { Bridge } from "../ipc/bridge";
import {
  defaultSettings,
  EVENT_STATE,
  type AccessStatus,
  type ActivityStats,
  type CommandName,
  type DraftView,
  type EventMap,
  type EventName,
  type HistoryEntry,
  type HistoryQuery,
  type HistoryStatus,
  type MicInfo,
  type PersonalRules,
  type PersonalRulesInput,
  type Settings,
  type UiState,
  type UsageStats,
} from "../ipc/contract";

// Handlers receive the command's own argument object.
type Handler = (args: any) => unknown; // eslint-disable-line @typescript-eslint/no-explicit-any

export interface FakeWindow {
  minimize: Mock;
  toggleMaximize: Mock;
  close: Mock;
  isMaximized: Mock;
  onResized: Mock;
  maximized: boolean;
  /** Simulate a native resize (maximize / restore). */
  resize(maximized: boolean): void;
}

export interface FakeBridge extends Bridge {
  invoke: Mock & Bridge["invoke"];
  appWindow: Bridge["appWindow"] & FakeWindow;
  /** Replace a command's behaviour; throw a `{ code, message }` to reject. */
  handlers: Partial<Record<CommandName, Handler>>;
  emit<E extends EventName>(event: E, payload: EventMap[E]): void;
  /** Emit a new `sambler://state` and remember it as the current state. */
  pushState(patch: Partial<UiState>): UiState;
  state: UiState;
  settings: Settings;
  rules: PersonalRules;
  history: HistoryEntry[];
  /** Recent results, newest first (what `get_recent_drafts` returns). */
  recents: DraftView[];
  stats: UsageStats;
  /** What `get_activity` returns (see dev/sampleActivity.ts). */
  activity: ActivityStats;
  historyStatus(): HistoryStatus;
  listenerCount(event: EventName): number;
  /** What the next `get_draft` returns. */
  setDraft(view: DraftView | null): void;
}

export function uiState(patch: Partial<UiState> = {}): UiState {
  return {
    phase: { kind: "idle" },
    take: null,
    destination: null,
    availability: { kind: "ready" },
    lastOutcome: null,
    draft: null,
    listeningMs: null,
    gesture: "hold",
    shortcut: "Ctrl+Shift+F2",
    recentCount: 0,
    appearance: { theme: "verre-aurore", colorScheme: "system" },
    ...patch,
  };
}

export const MICS: MicInfo[] = [
  { id: "mic-1", name: "Réseau de microphones", is_default: true },
  { id: "mic-2", name: "Casque USB", is_default: false },
];

export const CONFIGURED_ACCESS: AccessStatus = {
  configured: true,
  source: "saved",
  authority: "speech.example.test:443",
  canRemember: true,
};

export const STATS: UsageStats = { wordsToday: 1240, wordsThisWeek: 5312, wordsTotal: 48210, takesTotal: 612, averageWpm: 132 };

interface Options {
  state?: Partial<UiState>;
  settings?: Partial<Settings>;
  access?: AccessStatus;
  mics?: MicInfo[];
  /** What `get_draft` returns; also the only recent result unless `recents` is given. */
  draft?: DraftView | null;
  /** Recent results, newest first. */
  recents?: DraftView[];
  rules?: Partial<PersonalRules>;
  history?: HistoryEntry[];
  /** `available: false` simulates an ephemeral session. */
  historyAvailable?: boolean;
  stats?: UsageStats;
  activity?: ActivityStats;
}

export function createFakeBridge(options: Options = {}): FakeBridge {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  let draft = options.draft ?? null;
  let access = options.access ?? CONFIGURED_ACCESS;
  let skipNext = false;
  const resizeHandlers = new Set<() => void>();

  const fake = {
    state: uiState(options.state),
    settings: { ...defaultSettings(), ...options.settings },
    rules: { lexicon: [], expressions: [], unreadable: false, ...options.rules },
    history: [...(options.history ?? [])],
    recents: [...(options.recents ?? (draft ? [draft] : []))],
    stats: options.stats ?? STATS,
    activity: options.activity ?? sampleActivity(),
  } as unknown as FakeBridge;
  // A state that names recent results counts them, unless the test says otherwise.
  if (options.state?.recentCount === undefined) fake.state.recentCount = fake.recents.length;

  fake.historyStatus = () => ({
    enabled: fake.settings.history.enabled,
    retentionDays: fake.settings.history.retentionDays,
    entryCount: fake.history.length,
    skipNext,
    available: options.historyAvailable ?? true,
  });

  const afterRecentsChange = () => {
    fake.state = { ...fake.state, draft: fake.recents[0]?.meta ?? null, recentCount: fake.recents.length };
    draft = fake.recents[0] ?? null;
    return fake.state;
  };

  fake.handlers = {
    get_state: () => fake.state,
    get_settings: () => fake.settings,
    save_settings: ({ settings }: { settings: Settings }) => {
      fake.settings = settings;
      // Like the native side: the floating bar gets the appearance in UiState.
      if (settings.appearance) fake.state = { ...fake.state, appearance: settings.appearance };
      return settings;
    },
    list_microphones: () => options.mics ?? MICS,
    start_mic_test: () => undefined,
    stop_mic_test: () => undefined,
    set_consent: ({ enabled }: { enabled: boolean }) => {
      fake.state = {
        ...fake.state,
        availability: enabled ? { kind: "connecting" } : { kind: "needs-consent" },
      };
      return fake.state;
    },
    get_access: () => access,
    set_access: ({ access: input }: { access: { authority: string; remember: boolean } }) => {
      access = {
        configured: true,
        source: input.remember ? "saved" : "session",
        authority: input.authority || null,
        canRemember: access.canRemember,
      };
      return access;
    },
    clear_access: () => {
      access = { configured: false, source: "none", authority: null, canRemember: access.canRemember };
      return access;
    },
    retry_connection: () => fake.state,
    dictate_in_sambler: () => fake.state,
    finish_take: () => fake.state,
    cancel_take: () => fake.state,
    get_draft: () => draft,
    get_recent_drafts: () => fake.recents,
    copy_draft: () => undefined,
    clear_draft: (args?: { take?: number }) => {
      fake.recents = args?.take === undefined ? [] : fake.recents.filter((item) => item.meta.take !== args.take);
      return afterRecentsChange();
    },
    get_stats: () => fake.stats,
    get_activity: () => fake.activity,
    open_mic_privacy_settings: () => undefined,
    preview_render: ({ sample }: { sample: string }) => sample,
    get_personal_rules: () => fake.rules,
    save_personal_rules: ({ rules }: { rules: PersonalRulesInput }) => {
      fake.rules = { ...rules, unreadable: false };
      return { saved: true, rules: fake.rules, conflicts: [] };
    },
    get_history: ({ query }: { query: HistoryQuery }) => {
      const search = query.search?.toLowerCase() ?? "";
      const matching = fake.history.filter((entry) => !search || entry.text?.toLowerCase().includes(search));
      const start = query.cursor ? Number(query.cursor) : 0;
      const limit = query.limit ?? 50;
      const entries = matching.slice(start, start + limit);
      const next = start + limit < matching.length ? String(start + limit) : null;
      return { entries, nextCursor: next };
    },
    copy_history_entry: () => undefined,
    delete_history_entry: ({ id }: { id: string }) => {
      fake.history = fake.history.filter((entry) => entry.id !== id);
      return fake.historyStatus();
    },
    clear_history: ({ disable }: { disable: boolean }) => {
      fake.history = [];
      if (disable) fake.settings = { ...fake.settings, history: { ...fake.settings.history, enabled: false } };
      return fake.historyStatus();
    },
    get_history_status: () => fake.historyStatus(),
    skip_history_next_take: ({ skip }: { skip: boolean }) => {
      skipNext = skip;
      return fake.historyStatus();
    },
    set_launch_at_startup: ({ enabled }: { enabled: boolean }) => {
      fake.settings = { ...fake.settings, launchAtStartup: enabled };
    },
    // Milestone 5 defaults: empty and harmless. Tests override `fake.handlers.*`.
    get_known_apps: () => [],
    get_voice_commands: () => [],
    list_notes: () => [],
    get_note: () => {
      throw { code: "note-not-found", message: "Cette note n’existe plus." };
    },
    save_note: ({ note }: { note: { id: string | null; text: string } }) => ({
      id: note.id ?? "note-1",
      title: note.text.split("\n")[0].slice(0, 80),
      createdAt: 0,
      updatedAt: 0,
      words: note.text.split(/\s+/).filter(Boolean).length,
    }),
    delete_note: () => undefined,
    copy_note: () => undefined,
    pick_audio_file: () => null,
    transcribe_file: () => {
      throw { code: "transcription-unavailable", message: "Transcription indisponible." };
    },
    cancel_transcription: () => {
      throw { code: "transcription-not-found", message: "Cette transcription n’existe plus." };
    },
    list_transcriptions: () => [],
  };

  const run = async (command: CommandName, args: unknown) => {
    const handler = fake.handlers[command];
    if (!handler) throw new Error(`fake bridge: no handler for ${String(command)}`);
    return handler(args);
  };

  fake.invoke = vi.fn((command: CommandName, args?: unknown) => run(command, args)) as FakeBridge["invoke"];

  fake.listen = vi.fn(async (event: string, handler: (payload: unknown) => void) => {
    if (!listeners.has(event)) listeners.set(event, new Set());
    listeners.get(event)!.add(handler);
    return () => listeners.get(event)?.delete(handler);
  }) as FakeBridge["listen"];

  const appWindow = {
    maximized: false,
    minimize: vi.fn(async () => undefined),
    toggleMaximize: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
    isMaximized: vi.fn(async () => appWindow.maximized),
    onResized: vi.fn(async (handler: () => void) => {
      resizeHandlers.add(handler);
      return () => resizeHandlers.delete(handler);
    }),
    resize(maximized: boolean) {
      appWindow.maximized = maximized;
      resizeHandlers.forEach((handler) => handler());
    },
  };
  fake.appWindow = appWindow as unknown as FakeBridge["appWindow"];

  fake.emit = (event, payload) => {
    listeners.get(event)?.forEach((listener) => listener(payload));
  };

  fake.pushState = (patch) => {
    fake.state = { ...fake.state, ...patch };
    fake.emit(EVENT_STATE, fake.state);
    return fake.state;
  };

  fake.listenerCount = (event) => listeners.get(event)?.size ?? 0;
  fake.setDraft = (value) => {
    draft = value;
  };

  return fake;
}

/** A draft view with admitted text and every kind of gap. */
export function draftWithGaps(now = Date.now(), take = 7): DraftView {
  return {
    meta: { take, createdAtMs: now, expiresAtMs: now + 9 * 60_000 + 30_000, complete: false },
    parts: [
      { status: "admitted", text: "Je ne pourrai pas venir jeudi," },
      { status: "missing", text: null },
      { status: "admitted", text: "mais vendredi me convient." },
      { status: "needs-review", text: "vers dix heures" },
      { status: "blocked", text: null },
      { status: "no-text", text: null },
    ],
    fullText: null,
  };
}

export function completeDraft(text: string, now = Date.now(), take = 9): DraftView {
  return {
    meta: { take, createdAtMs: now, expiresAtMs: now + 10 * 60_000, complete: true },
    parts: [{ status: "admitted", text }],
    fullText: text,
  };
}

export function historyEntry(id: string, text: string | null, ageMinutes = 5, now = Date.now()): HistoryEntry {
  const created = now - ageMinutes * 60_000;
  return {
    id,
    createdAtMs: created,
    expiresAtMs: created + 7 * 86_400_000,
    language: "french",
    profile: "natural",
    text,
  };
}
