// DEV PREVIEW ONLY: drives the floating bar in a plain browser
// (`npm run dev`, /overlay.html) with simulated states and levels, over a
// light, dark or photo-like wallpaper, inside a frame the size of the real
// window (360 x 72). Loaded by overlay/main.tsx only when
// `import.meta.env.DEV` is true outside Tauri: never part of a build.

import type { AppearanceSettings, ColorSchemeChoice, TakeOutcome, UiState } from "../ipc/contract";
import type { OverlaySource } from "../overlay/bar";
import { DEFAULT_THEME, THEMES, THEME_IDS } from "../themes/registry";

export const OVERLAY_PREVIEW_MARKER = "DEV OVERLAY PREVIEW (simulated, never shipped)";

const OUTCOMES: { id: string; label: string; outcome: TakeOutcome }[] = [
  { id: "inserted", label: "Inséré", outcome: { kind: "inserted" } },
  { id: "kept", label: "Texte dans Sambler", outcome: { kind: "kept-in-sambler", reason: { kind: "uncertain-target", reason: "terminal" } } },
  { id: "no-text", label: "Aucun texte", outcome: { kind: "no-text" } },
  { id: "cancelled", label: "Annulé", outcome: { kind: "cancelled" } },
  { id: "unavailable", label: "Indisponible", outcome: { kind: "refused", reason: { kind: "unavailable", reason: "network" } } },
  { id: "protected", label: "Champ protégé", outcome: { kind: "refused", reason: { kind: "protected-field" } } },
  { id: "mic-denied", label: "Micro bloqué", outcome: { kind: "refused", reason: { kind: "microphone", error: { kind: "access-denied" } } } },
  { id: "partial", label: "Partiel", outcome: { kind: "inserted-partially" } },
];

const CSS = `
.ovp-stage { position: fixed; inset: 0; display: grid; place-items: center; transition: background 300ms; }
.ovp-stage[data-wall="light"] { background: linear-gradient(135deg, #f7f7f4, #e8edf5); }
.ovp-stage[data-wall="dark"] { background: linear-gradient(135deg, #0b0d12, #1b1f2a); }
.ovp-stage[data-wall="photo"] { background:
  radial-gradient(40rem 20rem at 20% 30%, #f6c38b 0%, transparent 60%),
  radial-gradient(40rem 24rem at 80% 70%, #3a6ea5 0%, transparent 60%),
  linear-gradient(160deg, #e6a26a, #2d4f7c); }
.ovp-stage[data-wall="white"] { background: #ffffff; }
.ovp-stage[data-wall="black"] { background: #000000; }
.ovp-frame { width: 360px; height: 72px; display: flex; align-items: flex-end; outline: 1px dashed rgba(127,127,127,.55); }
.ovp-frame .bar { width: 360px; }
.ovp-panel { position: fixed; top: 12px; left: 12px; display: flex; flex-wrap: wrap; gap: 6px; max-width: calc(100vw - 24px);
  font: 12px system-ui, sans-serif; padding: 8px; background: rgba(255,255,255,.9); color: #111; border-radius: 10px;
  box-shadow: 0 2px 12px rgba(0,0,0,.2); }
.ovp-panel button { font: inherit; padding: 4px 8px; border-radius: 6px; border: 1px solid #bbb; background: #fff; cursor: pointer; }
.ovp-panel strong { width: 100%; }
`;

const SCHEMES: readonly ColorSchemeChoice[] = ["system", "light", "dark"];

/**
 * `&theme=<id>&scheme=light|dark|system` in the URL: the appearance the
 * simulated native sends with every state, as a native from milestone 4 does.
 * Neither given: none is sent, as an older native (the bar keeps its cached
 * one). One given: the other is the default. An unknown theme id is sent
 * as is, to see the bar fall back.
 */
export function previewAppearance(params: URLSearchParams): AppearanceSettings | undefined {
  const theme = params.get("theme");
  const scheme = params.get("scheme");
  if (!theme && !scheme) return undefined;
  return {
    theme: theme || DEFAULT_THEME,
    colorScheme: SCHEMES.includes(scheme as ColorSchemeChoice) ? (scheme as ColorSchemeChoice) : "system",
  };
}

function baseState(): UiState {
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
  };
}

export function installOverlayPreview(): OverlaySource {
  let onState: (state: UiState) => void = () => {};
  let onLevel: (level: number) => void = () => {};
  // For captures: `?state=preparing|listening|near|finalizing|<outcome id>&wall=light|dark|photo|white|black&panel=0`,
  // plus `&theme=<id>&scheme=light|dark|system` (see previewAppearance).
  const params = new URLSearchParams(window.location.search);
  const forced = previewAppearance(params);
  let state: UiState = forced ? { ...baseState(), appearance: forced } : baseState();
  let take = 1;
  let outcomeIndex = 0;
  let levelTimer = 0;
  let cycle = 0;
  let auto = true;

  const push = (patch: Partial<UiState>) => {
    state = { ...state, ...patch };
    onState(state);
  };

  const stopLevels = () => window.clearInterval(levelTimer);
  const startLevels = () => {
    stopLevels();
    let t = 0;
    levelTimer = window.setInterval(() => {
      t += 1;
      const syllables = Math.max(0, Math.sin(t / 2.3)) * Math.abs(Math.sin(t / 7));
      onLevel(0.32 + syllables * 0.5 + Math.random() * 0.08);
    }, 33);
  };

  const prepare = () => {
    take += 1;
    stopLevels();
    push({ phase: { kind: "preparing" }, take, destination: "field", listeningMs: null });
  };
  const listen = () => {
    take += 1;
    startLevels();
    push({ phase: { kind: "listening" }, take, destination: "field", listeningMs: 0 });
  };
  const finalize = () => {
    stopLevels();
    push({ phase: { kind: "finalizing" } });
  };
  const finish = (outcome: TakeOutcome) => {
    stopLevels();
    push({ phase: { kind: "idle" }, take: null, destination: null, listeningMs: null, lastOutcome: outcome });
  };

  const runCycle = () => {
    window.clearTimeout(cycle);
    if (!auto) return;
    listen();
    cycle = window.setTimeout(() => {
      finalize();
      cycle = window.setTimeout(() => {
        finish(OUTCOMES[outcomeIndex % OUTCOMES.length].outcome);
        outcomeIndex += 1;
        cycle = window.setTimeout(runCycle, 3600);
      }, 1100);
    }, 4200);
  };

  // Stage: wallpaper + a frame the size of the real window.
  const style = document.createElement("style");
  style.textContent = CSS;
  document.head.appendChild(style);
  const stage = document.createElement("div");
  stage.className = "ovp-stage";
  stage.dataset.wall = "photo";
  const frame = document.createElement("div");
  frame.className = "ovp-frame";
  stage.appendChild(frame);
  document.body.appendChild(stage);
  const bar = document.getElementById("bar");
  if (bar) frame.appendChild(bar);

  const panel = document.createElement("div");
  panel.className = "ovp-panel";
  panel.lang = "en";
  const button = (label: string, action: () => void) => {
    const element = document.createElement("button");
    element.type = "button";
    element.textContent = label;
    element.addEventListener("click", action);
    panel.appendChild(element);
  };
  const title = document.createElement("strong");
  title.textContent = OVERLAY_PREVIEW_MARKER;
  panel.appendChild(title);
  button("Auto cycle", () => {
    auto = true;
    runCycle();
  });
  const manual = (action: () => void) => () => {
    auto = false;
    window.clearTimeout(cycle);
    action();
  };
  const nearLimit = () => {
    take += 1;
    startLevels();
    push({ phase: { kind: "listening" }, take, destination: "field", listeningMs: 4 * 60_000 + 40_000 });
  };
  button("Preparing", manual(prepare));
  button("Listening", manual(listen));
  button("Near 5:00", manual(nearLimit));
  button("Finalizing", manual(finalize));
  for (const item of OUTCOMES) button(item.label, manual(() => finish(item.outcome)));
  for (const wall of ["light", "dark", "photo", "white", "black"]) {
    button(`Wall: ${wall}`, () => (stage.dataset.wall = wall));
  }
  // Theme and scheme travel in the next state, as from the native side.
  const appearance = (patch: Partial<AppearanceSettings>) => () => {
    const current = state.appearance ?? { theme: DEFAULT_THEME, colorScheme: "system" };
    push({ appearance: { ...current, ...patch } });
  };
  for (const id of THEME_IDS) button(`Theme: ${THEMES[id].label}`, appearance({ theme: id }));
  for (const scheme of SCHEMES) button(`Scheme: ${scheme}`, appearance({ colorScheme: scheme }));
  console.info(`Sambler: ${OVERLAY_PREVIEW_MARKER}.`);

  const wall = params.get("wall");
  if (wall) stage.dataset.wall = wall;
  if (params.get("panel") !== "0") document.body.appendChild(panel);
  const fixed = params.get("state");

  // Start once the bar has subscribed.
  window.setTimeout(() => {
    onState(state);
    if (!fixed) {
      runCycle();
      return;
    }
    auto = false;
    if (fixed === "preparing") prepare();
    else if (fixed === "listening") listen();
    else if (fixed === "near") nearLimit();
    else if (fixed === "finalizing") {
      listen();
      finalize();
    } else {
      const item = OUTCOMES.find((entry) => entry.id === fixed);
      if (item) {
        listen();
        finish(item.outcome);
      }
    }
  }, 300);

  return {
    onState: (handler) => {
      onState = handler;
    },
    onLevel: (handler) => {
      onLevel = handler;
    },
  };
}
