// Test double for the Tauri bridge. Lives under src/test/ and is imported
// only by *.test.tsx files; the application never references it.

import { vi, type Mock } from "vitest";
import type { Bridge } from "../ipc/bridge";
import {
  defaultSettings,
  EVENT_STATE,
  type AccessStatus,
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
} from "../ipc/contract";

// Handlers receive the command's own argument object.
type Handler = (args: any) => unknown; // eslint-disable-line @typescript-eslint/no-explicit-any

export interface FakeBridge extends Bridge {
  invoke: Mock & Bridge["invoke"];
  /** Replace a command's behaviour; throw a `{ code, message }` to reject. */
  handlers: Partial<Record<CommandName, Handler>>;
  emit<E extends EventName>(event: E, payload: EventMap[E]): void;
  /** Emit a new `sambler://state` and remember it as the current state. */
  pushState(patch: Partial<UiState>): UiState;
  state: UiState;
  settings: Settings;
  rules: PersonalRules;
  history: HistoryEntry[];
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

interface Options {
  state?: Partial<UiState>;
  settings?: Partial<Settings>;
  access?: AccessStatus;
  mics?: MicInfo[];
  draft?: DraftView | null;
  rules?: Partial<PersonalRules>;
  history?: HistoryEntry[];
  /** `available: false` simulates an ephemeral session. */
  historyAvailable?: boolean;
}

export function createFakeBridge(options: Options = {}): FakeBridge {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  let draft = options.draft ?? null;
  let access = options.access ?? CONFIGURED_ACCESS;
  let skipNext = false;

  const fake = {
    state: uiState(options.state),
    settings: { ...defaultSettings(), ...options.settings },
    rules: { lexicon: [], expressions: [], unreadable: false, ...options.rules },
    history: [...(options.history ?? [])],
  } as unknown as FakeBridge;

  fake.historyStatus = () => ({
    enabled: fake.settings.history.enabled,
    retentionDays: fake.settings.history.retentionDays,
    entryCount: fake.history.length,
    skipNext,
    available: options.historyAvailable ?? true,
  });

  fake.handlers = {
    get_state: () => fake.state,
    get_settings: () => fake.settings,
    save_settings: ({ settings }: { settings: Settings }) => {
      fake.settings = settings;
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
    copy_draft: () => undefined,
    clear_draft: () => {
      draft = null;
      fake.state = { ...fake.state, draft: null };
      return fake.state;
    },
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
export function draftWithGaps(now = Date.now()): DraftView {
  return {
    meta: { take: 7, createdAtMs: now, expiresAtMs: now + 9 * 60_000 + 30_000, complete: false },
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

export function completeDraft(text: string, now = Date.now()): DraftView {
  return {
    meta: { take: 9, createdAtMs: now, expiresAtMs: now + 10 * 60_000, complete: true },
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
