// Draws the floating dictation bar from state and level events. Plain DOM,
// no framework; nothing here calls a command or receives dictated text.
// One structure for every theme: the theme's overlay.css shows, hides and
// colours its parts (see overlay.css for the tokens).

import type { UiState } from "../ipc/contract";
import { createWaveform, prefersReducedMotion, speechLevel } from "../lib/waveform";
import {
  clock,
  expire,
  FLASH_MS,
  HIDDEN,
  LIMIT_WARNING_MS,
  nextView,
  outcomeMeaning,
  type FlashIcon,
  type OutcomeMeaning,
  type OverlayView,
} from "./model";
import { followAppearance } from "./theme";

export interface OverlaySource {
  onState(handler: (state: UiState) => void): void;
  onLevel(handler: (level: number) => void): void;
}

const SVG = (body: string) =>
  `<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;

const ICONS: Record<FlashIcon | "mic" | "dots", string> = {
  mic: SVG('<rect x="9" y="3" width="6" height="11" rx="3" fill="currentColor" stroke="none"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5v3.5"/>'),
  dots: SVG('<circle cx="6" cy="12" r="1.4" fill="currentColor"/><circle cx="12" cy="12" r="1.4" fill="currentColor"/><circle cx="18" cy="12" r="1.4" fill="currentColor"/>'),
  check: SVG('<path d="M5 12.5l4.5 4.5L19 7.5"/>'),
  inbox: SVG('<path d="M4 13.5l2.2-7.2A1.5 1.5 0 0 1 7.6 5.2h8.8a1.5 1.5 0 0 1 1.4 1.1l2.2 7.2V18a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 18z"/><path d="M4 13.5h4.5l1 2h5l1-2H20"/>'),
  empty: SVG('<circle cx="12" cy="12" r="8.5"/><path d="M8.5 12h7"/>'),
  cancel: SVG('<circle cx="12" cy="12" r="8.5"/><path d="M9.5 9.5l5 5M14.5 9.5l-5 5"/>'),
  warning: SVG('<path d="M10.3 4.4a2 2 0 0 1 3.4 0l7 12.1a2 2 0 0 1-1.7 3H5a2 2 0 0 1-1.7-3z"/><path d="M12 9.5v4"/><circle cx="12" cy="16.6" r="0.6" fill="currentColor"/>'),
  error: SVG('<path d="M8.4 3.5h7.2l4.9 4.9v7.2l-4.9 4.9H8.4l-4.9-4.9V8.4z"/><path d="M9.4 9.4l5.2 5.2M14.6 9.4l-5.2 5.2"/>'),
  lock: SVG('<rect x="5" y="10.5" width="14" height="10" rx="2.5"/><path d="M8.5 10.5V7.5a3.5 3.5 0 0 1 7 0v3"/>'),
  "mic-off": SVG('<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5v3.5M4.5 4.5l15 15"/>'),
};

const BARS = 17;
const LIMIT_MS = 5 * 60_000;

/**
 * `page` gets `data-theme` and `data-scheme`: the cached appearance at once,
 * then the one carried by each state event.
 */
export function mountOverlay(root: HTMLElement, source: OverlaySource, page: HTMLElement = document.documentElement): void {
  const followTheme = followAppearance(page);
  // `bar__aurora` and `bar__dots` are decorations some themes show (hidden by default).
  root.innerHTML =
    '<div class="bar__pill"><span class="bar__aurora"></span><span class="bar__icon"></span>' +
    '<span class="bar__dots"><i></i><i></i><i></i></span><span class="bar__wave"></span>' +
    '<span class="bar__label"></span><span class="bar__timer"></span></div>';
  const pill = root.querySelector<HTMLElement>(".bar__pill")!;
  const icon = root.querySelector<HTMLElement>(".bar__icon")!;
  const waveRoot = root.querySelector<HTMLElement>(".bar__wave")!;
  const label = root.querySelector<HTMLElement>(".bar__label")!;
  const timer = root.querySelector<HTMLElement>(".bar__timer")!;
  waveRoot.style.setProperty("--n", String(BARS));
  for (let i = 0; i < BARS; i += 1) {
    const bar = document.createElement("span");
    bar.className = "wave__bar";
    // Distance from the center, for the processing ripple; index, for a
    // colour gradient spread across the whole wave.
    bar.style.setProperty("--i", String(Math.abs(i - (BARS - 1) / 2)));
    bar.style.setProperty("--k", String(i));
    waveRoot.appendChild(bar);
  }
  const still = prefersReducedMotion();
  const wave = createWaveform(waveRoot, BARS, still);
  // The outcome countdown some themes draw lasts as long as the flash.
  root.style.setProperty("--bar-flash-ms", `${FLASH_MS}ms`);

  let view: OverlayView = HIDDEN;
  let shown: OverlayView = HIDDEN;
  let previous: UiState | null = null;
  let meaning: OutcomeMeaning | null = null;
  let tick = 0;
  let expiry = 0;

  const paintClock = () => {
    if (view.kind !== "listening") return;
    const elapsed = Date.now() - view.startedAt;
    timer.textContent = clock(Math.min(elapsed, LIMIT_MS));
    root.toggleAttribute("data-warn", elapsed >= LIMIT_WARNING_MS);
  };

  const setContent = (svg: string, text: string, tone?: string, sense?: OutcomeMeaning) => {
    icon.innerHTML = svg;
    label.textContent = text;
    if (tone) root.dataset.tone = tone;
    else delete root.dataset.tone;
    if (sense) root.dataset.meaning = sense;
    else delete root.dataset.meaning;
  };

  const render = () => {
    const replacesFlash = view !== shown && shown.kind === "outcome" && view.kind === "outcome";
    shown = view;
    root.dataset.kind = view.kind;
    window.clearInterval(tick);
    window.clearTimeout(expiry);
    if (view.kind !== "listening") {
      wave.reset();
      root.style.removeProperty("--level");
      timer.textContent = "";
      root.removeAttribute("data-warn");
    }
    switch (view.kind) {
      case "hidden":
        // Keep the last content while it fades out.
        return;
      case "preparing":
        setContent(ICONS.mic, "Préparation");
        return;
      case "listening":
        setContent(ICONS.mic, "Écoute");
        root.toggleAttribute("data-sambler", view.toSambler);
        paintClock();
        tick = window.setInterval(paintClock, 250);
        return;
      case "finalizing":
        setContent(ICONS.dots, "Traitement");
        return;
      case "outcome": {
        setContent(ICONS[view.flash.icon], view.flash.label, view.flash.tone, meaning ?? undefined);
        // A new flash straight after another one: its countdown starts over.
        if (replacesFlash) for (const animation of pill.getAnimations?.({ subtree: true }) ?? []) animation.currentTime = 0;
        const until = view.until;
        expiry = window.setTimeout(() => {
          view = expire(view, Math.max(Date.now(), until));
          render();
        }, Math.max(0, until - Date.now()));
        return;
      }
    }
  };

  source.onState((next) => {
    followTheme(next);
    const before = view;
    view = nextView(view, previous, next, Date.now());
    if (view.kind === "outcome" && view !== before && next.lastOutcome) meaning = outcomeMeaning(next.lastOutcome);
    previous = next;
    render();
  });
  source.onLevel((level) => {
    if (view.kind !== "listening") return;
    wave.push(level);
    // The theme's own horizon follows the voice (CSS reads `--level`, 0..1).
    if (!still) root.style.setProperty("--level", speechLevel(level).toFixed(2));
  });
  render();
}
