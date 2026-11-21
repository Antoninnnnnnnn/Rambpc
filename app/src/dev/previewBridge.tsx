// DEV PREVIEW ONLY. Loaded by main.tsx exclusively when `import.meta.env.DEV`
// is true and the page runs outside Tauri (plain browser on `npm run dev`).
// Production builds never include this module: Sambler ships no simulated
// microphone, recognizer or transcript (docs/windows/08).

import { createRoot } from "react-dom/client";
import type { Bridge, Unlisten } from "../ipc/bridge";
import {
  defaultSettings,
  EVENT_LEVEL,
  EVENT_MIC_TEST_LEVEL,
  EVENT_STATE,
  type AccessStatus,
  type Availability,
  type CommandError,
  type DestinationKind,
  type DraftPart,
  type DraftView,
  type MicInfo,
  type Settings,
  type TakeOutcome,
  type UiState,
  type UnavailableReason,
  type UsageStats,
} from "../ipc/contract";
import { matchesShortcut } from "../lib/shortcut";
import { normalizeAppearance } from "../themes/appearance";
import { PreviewPanel } from "./PreviewPanel";
import { createPreviewApps, previewLanguage, previewVoiceCommands } from "./previewApps";
import { createPreviewCarnet } from "./previewCarnet";
import { createPreviewPersonal } from "./previewPersonal";
import { sampleActivity, sampleStats } from "./sampleActivity";

export const PREVIEW_MARKER = "DEV PREVIEW (simulated, never shipped)";

const MICS: MicInfo[] = [
  { id: "mic-array", name: "Réseau de microphones (Realtek)", is_default: true },
  { id: "usb-headset", name: "Casque USB", is_default: false },
];

const SAMPLES = [
  "Bonjour Claire, je ne pourrai pas venir jeudi à 14 h. Pouvons-nous décaler à vendredi matin ?",
  "Penser à envoyer le devis à Mélusine avant la réunion budget de lundi.",
  "Rendez-vous jeudi à 14 h.",
  "Merci pour votre retour rapide. Je vous propose un point de quinze minutes demain pour valider la maquette, puis je partage la version finale avec l’équipe.",
];

export type CopyMode = "ok" | "busy" | "exclusion-failed";

export interface PreviewControls {
  state: () => UiState;
  setAvailability: (value: "auto" | "connecting" | UnavailableReason) => void;
  setPhase: (phase: UiState["phase"]["kind"]) => void;
  setOutcome: (outcome: TakeOutcome | null, draft: "none" | "complete" | "gaps") => void;
  fillRecents: () => void;
  setCopyMode: (mode: CopyMode) => void;
  setFailNextSave: (fail: boolean) => void;
  resetFirstRun: () => void;
  subscribe: (listener: () => void) => () => void;
}

function sampleDraft(take: number, minutes: number, kind: "complete" | "gaps", ageMs = 0): DraftView {
  const now = Date.now() - ageMs;
  const meta = { take, createdAtMs: now, expiresAtMs: now + minutes * 60_000, complete: kind === "complete" };
  if (kind === "complete") {
    const text = SAMPLES[take % SAMPLES.length];
    return { meta, parts: [{ status: "admitted", text }], fullText: text };
  }
  const parts: DraftPart[] = [
    { status: "admitted", text: "Je ne pourrai pas venir jeudi à 14 h," },
    { status: "missing", text: null },
    { status: "admitted", text: "mais vendredi matin me convient." },
    { status: "needs-review", text: "peut-être dix heures" },
    { status: "blocked", text: null },
  ];
  return { meta, parts, fullText: null };
}

function words(view: DraftView): number {
  const text = view.fullText ?? view.parts.map((part) => part.text ?? "").join(" ");
  return text.split(/\s+/).filter(Boolean).length;
}

export function installPreview(): Bridge {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const watchers = new Set<() => void>();
  const emit = (event: string, payload: unknown) => listeners.get(event)?.forEach((listener) => listener(payload));

  let settings: Settings = defaultSettings();
  let consent = false;
  let access: AccessStatus = { configured: false, source: "none", authority: null, canRemember: true };
  let forced: "auto" | "connecting" | UnavailableReason = "auto";
  let connecting = false;
  let recents: DraftView[] = [];
  let nextTake = 1;
  let copyMode: CopyMode = "ok";
  let failNextSave = false;
  // Counters and per-day activity that agree: 48 312 words, 23 days in a row.
  const activity = sampleActivity();
  let stats: UsageStats = sampleStats(activity);
  const timers = { level: 0, micTest: 0, phase: 0, expiry: 0 };

  const availability = (): Availability => {
    if (!consent) return { kind: "needs-consent" };
    if (!access.configured) return { kind: "needs-access" };
    if (forced === "connecting" || connecting) return { kind: "connecting" };
    if (forced !== "auto") return { kind: "unavailable", reason: forced };
    return { kind: "ready" };
  };

  let state: UiState = {
    phase: { kind: "idle" },
    take: null,
    destination: null,
    availability: availability(),
    lastOutcome: null,
    draft: null,
    listeningMs: null,
    gesture: settings.gesture,
    shortcut: settings.shortcut,
    recentCount: 0,
    appearance: settings.appearance,
  };

  const publish = (patch: Partial<UiState> = {}) => {
    state = {
      ...state,
      ...patch,
      availability: availability(),
      draft: recents[0]?.meta ?? null,
      recentCount: recents.length,
      gesture: settings.gesture,
      shortcut: settings.shortcut,
      appearance: settings.appearance,
    };
    emit(EVENT_STATE, state);
    watchers.forEach((watcher) => watcher());
    return state;
  };

  // Each recent result expires on its own; the next expiry is scheduled.
  const scheduleExpiry = () => {
    window.clearTimeout(timers.expiry);
    if (recents.length === 0) return;
    const next = Math.min(...recents.map((item) => item.meta.expiresAtMs));
    timers.expiry = window.setTimeout(() => {
      recents = recents.filter((item) => item.meta.expiresAtMs > Date.now());
      publish();
      scheduleExpiry();
    }, Math.max(0, next - Date.now()) + 20);
  };

  const addRecent = (view: DraftView) => {
    recents = [view, ...recents].slice(0, 5);
    scheduleExpiry();
  };

  const stopLevels = () => {
    window.clearInterval(timers.level);
    emit(EVENT_LEVEL, 0);
  };

  const startLevels = () => {
    window.clearInterval(timers.level);
    let t = 0;
    timers.level = window.setInterval(() => {
      t += 1;
      const syllables = Math.max(0, Math.sin(t / 2.3)) * Math.abs(Math.sin(t / 7));
      emit(EVENT_LEVEL, 0.32 + syllables * 0.5 + Math.random() * 0.08);
    }, 33);
  };

  const warm = () => {
    connecting = true;
    publish();
    window.setTimeout(() => {
      connecting = false;
      publish();
    }, 700);
  };

  // Instant start: « Écoute » right at the key press.
  const start = (destination: DestinationKind): UiState => {
    if (state.phase.kind !== "idle") return state;
    const a = availability();
    if (a.kind === "needs-consent" || a.kind === "needs-access") {
      return publish({ lastOutcome: { kind: "refused", reason: { kind: a.kind } } });
    }
    if (a.kind === "unavailable") {
      return publish({ lastOutcome: { kind: "refused", reason: { kind: "unavailable", reason: a.reason } } });
    }
    const take = nextTake++;
    window.clearTimeout(timers.phase);
    startLevels();
    return publish({ phase: { kind: "listening" }, take, destination, listeningMs: 0 });
  };

  const finish = (take: number): UiState => {
    if (state.take !== take || state.phase.kind === "idle" || state.phase.kind === "finalizing") return state;
    stopLevels();
    window.clearTimeout(timers.phase);
    timers.phase = window.setTimeout(() => {
      const kept = state.destination === "sambler" || settings.alwaysKeepInSambler;
      const view = sampleDraft(take, settings.draftMinutes, "complete");
      addRecent(view);
      stats = {
        ...stats,
        wordsToday: stats.wordsToday + words(view),
        wordsThisWeek: stats.wordsThisWeek + words(view),
        wordsTotal: stats.wordsTotal + words(view),
        takesTotal: stats.takesTotal + 1,
      };
      publish({
        phase: { kind: "idle" },
        take: null,
        destination: null,
        listeningMs: null,
        lastOutcome: kept ? { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } } : { kind: "inserted" },
      });
    }, 700);
    return publish({ phase: { kind: "finalizing" } });
  };

  const cancel = (take: number): UiState => {
    if (state.take !== take || state.phase.kind === "idle") return state;
    stopLevels();
    window.clearTimeout(timers.phase);
    return publish({ phase: { kind: "idle" }, take: null, destination: null, listeningMs: null, lastOutcome: { kind: "cancelled" } });
  };

  // The global shortcut, simulated while the preview window has the focus.
  let held = false;
  window.addEventListener("keydown", (event) => {
    const input = { key: event.key, code: event.code, ctrlKey: event.ctrlKey, altKey: event.altKey, shiftKey: event.shiftKey, metaKey: event.metaKey };
    if (event.repeat || !matchesShortcut(settings.shortcut, input)) return;
    if ((event.target as HTMLElement | null)?.closest?.("[data-testid='shortcut-capture']")) return;
    event.preventDefault();
    if (settings.gesture === "hold") {
      held = true;
      start("sambler");
    } else if (state.phase.kind === "idle") {
      start("sambler");
    } else if (state.take !== null) {
      finish(state.take);
    }
  });
  window.addEventListener("keyup", (event) => {
    if (!held || settings.gesture !== "hold") return;
    const main = settings.shortcut.split("+").pop()?.toUpperCase();
    if (event.key.toUpperCase() === main || event.code.toUpperCase() === main || event.code === `Key${main}`) {
      held = false;
      if (state.take !== null) finish(state.take);
    }
  });

  const fail = (code: string, message: string): never => {
    throw { code, message } satisfies CommandError;
  };

  const handlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
    get_state: () => state,
    get_settings: () => settings,
    save_settings: (args) => {
      if (failNextSave) {
        failNextSave = false;
        fail("shortcut-unavailable", "Cette combinaison est déjà utilisée par une autre application.");
      }
      settings = structuredClone(args!.settings as Settings);
      publish();
      return settings;
    },
    list_microphones: () => MICS,
    start_mic_test: () => {
      window.clearInterval(timers.micTest);
      let t = 0;
      timers.micTest = window.setInterval(() => {
        t += 1;
        emit(EVENT_MIC_TEST_LEVEL, 0.3 + Math.abs(Math.sin(t / 4)) * 0.45 + Math.random() * 0.15);
      }, 33);
    },
    stop_mic_test: () => {
      window.clearInterval(timers.micTest);
      emit(EVENT_MIC_TEST_LEVEL, 0);
    },
    set_consent: (args) => {
      consent = Boolean(args!.enabled);
      if (!consent && state.take !== null) cancel(state.take);
      if (consent && access.configured) warm();
      return publish();
    },
    get_access: () => access,
    set_access: (args) => {
      const input = args!.access as { authority: string; remember: boolean };
      access = { configured: true, source: input.remember ? "saved" : "session", authority: input.authority || null, canRemember: true };
      if (consent) warm();
      else publish();
      return access;
    },
    clear_access: () => {
      access = { configured: false, source: "none", authority: null, canRemember: true };
      publish();
      return access;
    },
    retry_connection: () => {
      forced = "auto";
      warm();
      return state;
    },
    dictate_in_sambler: () => start("sambler"),
    finish_take: (args) => finish(args!.take as number),
    cancel_take: (args) => cancel(args!.take as number),
    get_draft: () => (recents[0] ? structuredClone(recents[0]) : null),
    get_recent_drafts: () => structuredClone(recents),
    copy_draft: (args) => {
      const take = args?.take as number | undefined;
      const view = take === undefined ? recents[0] : recents.find((item) => item.meta.take === take);
      if (!view) fail("no-draft", "Aucun texte.");
      if (copyMode !== "ok") fail(`clipboard-${copyMode}`, "");
    },
    clear_draft: (args) => {
      const take = args?.take as number | undefined;
      recents = take === undefined ? [] : recents.filter((item) => item.meta.take !== take);
      scheduleExpiry();
      return publish();
    },
    get_stats: () => stats,
    get_activity: () => structuredClone(activity),
    open_mic_privacy_settings: () => {
      console.info(`${PREVIEW_MARKER}: would open ms-settings:privacy-microphone`);
    },
    preview_render: (args) => {
      const s = args!.settings as Settings;
      const sample = previewVoiceCommands(String(args!.sample), s);
      if (s.profile === "faithful") return sample;
      let out = sample.trim();
      if (s.text.lightPunctuation) out = out.replace(/ (est-ce|does)/, ". $1") + (/(convient|you)$/.test(out) ? " ?" : /[.!?]$/.test(out) ? "" : ".");
      if (s.text.sentenceCase) out = out.replace(/(^|[.?!] )(\p{L})/gu, (_m, p: string, c: string) => p + c.toUpperCase());
      if (s.text.typographicSpaces) out = out.replace(/ \?$/, " ?");
      else out = out.replace(/ \?$/, "?");
      if (previewLanguage(sample, s) === "english") out = out.replace(/ \?$/, "?").replace(/\bi\b/g, "I");
      return out;
    },
    ...createPreviewApps(),
    ...createPreviewCarnet(emit, fail),
    ...createPreviewPersonal(
      () => settings,
      (next) => {
        settings = next;
      },
    ),
  };

  const controls: PreviewControls = {
    state: () => state,
    setAvailability: (value) => {
      forced = value;
      if (value !== "auto") {
        consent = true;
        access = { ...access, configured: true, source: access.source === "none" ? "session" : access.source };
      }
      publish();
    },
    setPhase: (phase) => {
      stopLevels();
      if (phase === "listening") startLevels();
      publish({
        phase: { kind: phase },
        take: phase === "idle" ? null : (state.take ?? nextTake++),
        destination: phase === "idle" ? null : "field",
        listeningMs: phase === "listening" ? 0 : null,
      });
    },
    setOutcome: (outcome, kind) => {
      if (kind !== "none") addRecent(sampleDraft(nextTake++, settings.draftMinutes, kind));
      publish({ lastOutcome: outcome });
    },
    fillRecents: () => {
      for (const [index, age] of [9, 6, 3, 1].entries()) {
        addRecent(sampleDraft(nextTake++, settings.draftMinutes, index === 1 ? "gaps" : "complete", age * 60_000 * 0.4));
      }
      publish();
    },
    setCopyMode: (mode) => {
      copyMode = mode;
    },
    setFailNextSave: (value) => {
      failNextSave = value;
    },
    resetFirstRun: () => {
      consent = false;
      access = { configured: false, source: "none", authority: null, canRemember: true };
      forced = "auto";
      publish({ lastOutcome: null });
      window.location.reload();
    },
    subscribe: (watcher) => {
      watchers.add(watcher);
      return () => watchers.delete(watcher);
    },
  };

  // Scenarios for visual checks, e.g. `/?scenario=home&view=Style&panel=0`:
  //   scenario: home | listening | finalizing | kept | gaps | unavailable | needs-access (default: first run)
  //   view:     the sidebar label to open after start
  //   click:    a CSS selector to click once the screen is shown
  //   history:  1 to enable the history; panel: 0 to hide the dev panel
  //   theme:    verre-aurore | doux-rond; scheme: system | light | dark (saved appearance)
  //   legacy:   1 for a native side that predates themes (no `appearance` field)
  const params = new URLSearchParams(window.location.search);
  const scenario = params.get("scenario");
  if (params.has("theme") || params.has("scheme")) {
    settings = {
      ...settings,
      appearance: normalizeAppearance({
        theme: params.get("theme") ?? settings.appearance?.theme,
        colorScheme: (params.get("scheme") ?? settings.appearance?.colorScheme) as never,
      }),
    };
  }
  if (params.get("legacy") === "1") {
    const { appearance: _dropped, ...legacy } = settings;
    settings = legacy;
  }
  state = { ...state, appearance: settings.appearance };
  if (scenario) {
    consent = true;
    if (scenario !== "needs-access") access = { configured: true, source: "saved", authority: null, canRemember: true };
    if (scenario === "unavailable") forced = "network";
    if (params.get("history") === "1") settings = { ...settings, history: { enabled: true, retentionDays: 7 } };
    for (const [index, age] of [9, 6, 3].entries()) {
      addRecent(sampleDraft(nextTake++, settings.draftMinutes, scenario === "gaps" && index === 1 ? "gaps" : "complete", age * 24_000));
    }
    addRecent(sampleDraft(nextTake++, settings.draftMinutes, scenario === "gaps" ? "gaps" : "complete"));
    const outcome: TakeOutcome =
      scenario === "kept"
        ? { kind: "kept-in-sambler", reason: { kind: "uncertain-target", reason: "terminal" } }
        : scenario === "gaps"
          ? { kind: "kept-in-sambler", reason: { kind: "incomplete" } }
          : { kind: "inserted" };
    publish({ lastOutcome: scenario === "unavailable" || scenario === "needs-access" ? null : outcome });
    if (scenario === "listening") {
      startLevels();
      publish({ phase: { kind: "listening" }, take: nextTake++, destination: "field", listeningMs: 7000 });
    }
    if (scenario === "finalizing") publish({ phase: { kind: "finalizing" }, take: nextTake++, destination: "field" });
  }
  const view = params.get("view");
  const click = params.get("click");
  if (view || click) {
    window.setTimeout(() => {
      if (view) document.querySelector<HTMLElement>(`.sidebar button[title="${view}"]`)?.click();
      if (click) window.setTimeout(() => document.querySelector<HTMLElement>(click)?.click(), 300);
    }, 400);
  }

  if (params.get("panel") !== "0") {
    const host = document.createElement("div");
    host.id = "dev-preview-panel";
    document.body.appendChild(host);
    createRoot(host).render(<PreviewPanel controls={controls} />);
  }
  console.info(`Sambler: ${PREVIEW_MARKER}. Use the panel at the bottom right to simulate states.`);

  const delay = <T,>(value: () => T) =>
    new Promise<T>((resolve, reject) => {
      window.setTimeout(() => {
        try {
          resolve(value());
        } catch (error) {
          reject(error);
        }
      }, 60);
    });

  const noop = (what: string) => async () => {
    console.info(`${PREVIEW_MARKER}: would ${what} the window`);
  };

  const bridge = {
    invoke: (command: string, args?: Record<string, unknown>) => delay(() => handlers[command](args)),
    listen: async (event: string, handler: (payload: unknown) => void): Promise<Unlisten> => {
      if (!listeners.has(event)) listeners.set(event, new Set());
      listeners.get(event)!.add(handler);
      return () => listeners.get(event)?.delete(handler);
    },
    appWindow: {
      minimize: noop("minimize"),
      toggleMaximize: noop("toggle-maximize"),
      close: noop("hide"),
      isMaximized: async () => false,
      onResized: async () => () => {},
    },
  };
  return bridge as unknown as Bridge;
}
