// Draws the floating dictation bar from state and level events. Plain DOM,
// no framework; nothing here calls a command or receives dictated text.

import type { UiState } from "../ipc/contract";
import { createWaveform, prefersReducedMotion } from "../lib/waveform";
import { clock, expire, HIDDEN, LIMIT_WARNING_MS, nextView, type FlashIcon, type OverlayView } from "./model";

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

export function mountOverlay(root: HTMLElement, source: OverlaySource): void {
  root.innerHTML =
    '<div class="bar__pill"><span class="bar__icon"></span><span class="bar__wave"></span>' +
    '<span class="bar__label"></span><span class="bar__timer"></span></div>';
  const icon = root.querySelector<HTMLElement>(".bar__icon")!;
  const waveRoot = root.querySelector<HTMLElement>(".bar__wave")!;
  const label = root.querySelector<HTMLElement>(".bar__label")!;
  const timer = root.querySelector<HTMLElement>(".bar__timer")!;
  for (let i = 0; i < BARS; i += 1) {
    const bar = document.createElement("span");
    bar.className = "wave__bar";
    // Distance from the center, for the processing ripple.
    bar.style.setProperty("--i", String(Math.abs(i - (BARS - 1) / 2)));
    waveRoot.appendChild(bar);
  }
  const wave = createWaveform(waveRoot, BARS, prefersReducedMotion());

  let view: OverlayView = HIDDEN;
  let previous: UiState | null = null;
  let tick = 0;
  let expiry = 0;

  const paintClock = () => {
    if (view.kind !== "listening") return;
    const elapsed = Date.now() - view.startedAt;
    timer.textContent = clock(Math.min(elapsed, LIMIT_MS));
    root.toggleAttribute("data-warn", elapsed >= LIMIT_WARNING_MS);
  };

  const setContent = (svg: string, text: string, tone?: string) => {
    icon.innerHTML = svg;
    label.textContent = text;
    if (tone) root.dataset.tone = tone;
    else delete root.dataset.tone;
  };

  const render = () => {
    root.dataset.kind = view.kind;
    window.clearInterval(tick);
    window.clearTimeout(expiry);
    if (view.kind !== "listening") {
      wave.reset();
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
        setContent(ICONS[view.flash.icon], view.flash.label, view.flash.tone);
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
    view = nextView(view, previous, next, Date.now());
    previous = next;
    render();
  });
  source.onLevel((level) => {
    if (view.kind === "listening") wave.push(level);
  });
  render();
}
