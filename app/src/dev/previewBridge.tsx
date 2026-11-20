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
} from "../ipc/contract";
import { matchesShortcut } from "../lib/shortcut";
import { PreviewPanel } from "./PreviewPanel";

export const PREVIEW_MARKER = "DEV PREVIEW (simulated, never shipped)";

const MICS: MicInfo[] = [
  { id: "mic-array", name: "Réseau de microphones (Realtek)", is_default: true },
  { id: "usb-headset", name: "Casque USB", is_default: false },
];

export type CopyMode = "ok" | "busy" | "exclusion-failed";

export interface PreviewControls {
  state: () => UiState;
  setAvailability: (value: "auto" | "connecting" | UnavailableReason) => void;
  setPhase: (phase: UiState["phase"]["kind"]) => void;
  setOutcome: (outcome: TakeOutcome | null, draft: "none" | "complete" | "gaps") => void;
  setCopyMode: (mode: CopyMode) => void;
  setFailNextSave: (fail: boolean) => void;
  resetFirstRun: () => void;
  subscribe: (listener: () => void) => () => void;
}

function sampleDraft(take: number, minutes: number, kind: "complete" | "gaps"): DraftView {
  const now = Date.now();
  const meta = { take, createdAtMs: now, expiresAtMs: now + minutes * 60_000, complete: kind === "complete" };
  if (kind === "complete") {
    return { meta, parts: [{ status: "admitted", text: "Rendez-vous jeudi à 14 h." }], fullText: "Rendez-vous jeudi à 14 h." };
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

export function installPreview(): Bridge {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const watchers = new Set<() => void>();
  const emit = (event: string, payload: unknown) => listeners.get(event)?.forEach((listener) => listener(payload));

  let settings: Settings = defaultSettings();
  let consent = false;
  let access: AccessStatus = { configured: false, source: "none", authority: null, canRemember: true };
  let forced: "auto" | "connecting" | UnavailableReason = "auto";
  let connecting = false;
  let draft: DraftView | null = null;
  let nextTake = 1;
  let copyMode: CopyMode = "ok";
  let failNextSave = false;
  const timers = { level: 0, micTest: 0, phase: 0, draft: 0 };

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
  };

  const publish = (patch: Partial<UiState> = {}) => {
    state = {
      ...state,
      ...patch,
      availability: availability(),
      draft: draft?.meta ?? null,
      gesture: settings.gesture,
      shortcut: settings.shortcut,
    };
    emit(EVENT_STATE, state);
    watchers.forEach((watcher) => watcher());
    return state;
  };

  const setDraft = (next: DraftView | null) => {
    draft = next;
    window.clearTimeout(timers.draft);
    if (next) {
      timers.draft = window.setTimeout(() => {
        draft = null;
        publish();
      }, next.meta.expiresAtMs - Date.now());
    }
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
      emit(EVENT_LEVEL, Math.abs(Math.sin(t / 3)) * 0.7 + Math.random() * 0.25);
    }, 50);
  };

  const warm = () => {
    connecting = true;
    publish();
    window.setTimeout(() => {
      connecting = false;
      publish();
    }, 700);
  };

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
    timers.phase = window.setTimeout(() => {
      startLevels();
      publish({ phase: { kind: "listening" }, listeningMs: 0 });
    }, 450);
    return publish({ phase: { kind: "preparing" }, take, destination, listeningMs: null });
  };

  const finish = (take: number): UiState => {
    if (state.take !== take || state.phase.kind === "idle" || state.phase.kind === "finalizing") return state;
    stopLevels();
    window.clearTimeout(timers.phase);
    timers.phase = window.setTimeout(() => {
      const kept = state.destination === "sambler" || settings.alwaysKeepInSambler;
      if (kept) setDraft(sampleDraft(take, settings.draftMinutes, "complete"));
      publish({
        phase: { kind: "idle" },
        take: null,
        destination: null,
        listeningMs: null,
        lastOutcome: kept ? { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } } : { kind: "inserted" },
      });
    }, 900);
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
        emit(EVENT_MIC_TEST_LEVEL, Math.abs(Math.sin(t / 4)) * 0.6 + Math.random() * 0.2);
      }, 50);
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
      access = {
        configured: true,
        source: input.remember ? "saved" : "session",
        authority: input.authority || null,
        canRemember: true,
      };
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
    get_draft: () => (draft ? structuredClone(draft) : null),
    copy_draft: () => {
      if (!draft) fail("no-draft", "Aucun brouillon.");
      if (copyMode !== "ok") fail(copyMode, "");
    },
    clear_draft: () => {
      setDraft(null);
      return publish();
    },
    open_mic_privacy_settings: () => {
      console.info(`${PREVIEW_MARKER}: would open ms-settings:privacy-microphone`);
    },
    preview_render: (args) => {
      const sample = String(args!.sample);
      const s = args!.settings as Settings;
      if (s.profile === "faithful") return sample;
      let out = sample;
      if (s.text.lightPunctuation) out = out.replace(/ (est-ce|does)/, ". $1") + (/(convient|you)$/.test(out) ? " ?" : ".");
      if (s.text.sentenceCase) out = out.replace(/(^|[.?!] )(\p{L})/gu, (_m, p: string, c: string) => p + c.toUpperCase());
      if (!s.text.typographicSpaces) out = out.replace(/ \?$/, "?");
      if (s.language === "english") out = out.replace(/ \?$/, "?").replace(/\bi\b/g, "I");
      return out;
    },
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
      setDraft(kind === "none" ? null : sampleDraft(nextTake++, settings.draftMinutes, kind));
      publish({ lastOutcome: outcome });
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

  const host = document.createElement("div");
  host.id = "dev-preview-panel";
  document.body.appendChild(host);
  createRoot(host).render(<PreviewPanel controls={controls} />);
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

  const bridge = {
    invoke: (command: string, args?: Record<string, unknown>) => delay(() => handlers[command](args)),
    invokeOptional: (command: string, args?: Record<string, unknown>) =>
      delay(() => (handlers[command] ? handlers[command](args) : undefined)),
    listen: async (event: string, handler: (payload: unknown) => void): Promise<Unlisten> => {
      if (!listeners.has(event)) listeners.set(event, new Set());
      listeners.get(event)!.add(handler);
      return () => listeners.get(event)?.delete(handler);
    },
  };
  return bridge as unknown as Bridge;
}
