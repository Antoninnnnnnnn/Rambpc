// Verre et aurore: the aurora decor (styles in aurora.css). The backdrop lives
// behind the whole window; the tile sits in the Accueil status card. Both wake
// while Sambler listens (phase) and breathe with the voice level
// (`sambler://level`, a number: never any dictated text). Purely decorative:
// aria-hidden, paused while the window is hidden, still with reduced motion.

import { useEffect, useRef, type RefObject } from "react";
import { useBridge } from "../../ipc/bridge";
import { EVENT_LEVEL } from "../../ipc/contract";
import { listenShared } from "../../ipc/shared";
import { shortcutKeys } from "../../lib/shortcut";
import { prefersReducedMotion, speechLevel } from "../../lib/waveform";
import { useUiStore } from "../../state/ui";
import type { StatusArtProps } from "../slots";

/** Eases `--voix` on `element` toward the latest level, one write per frame at most. */
export function createVoiceGlow(element: HTMLElement) {
  let target = 0;
  let shown = 0;
  let frame = 0;
  const raf = typeof window.requestAnimationFrame === "function";
  const write = () => element.style.setProperty("--voix", shown.toFixed(3));
  const tick = () => {
    frame = 0;
    shown += (target - shown) * (target > shown ? 0.35 : 0.12);
    if (Math.abs(target - shown) < 0.004) shown = target;
    write();
    if (shown !== target) frame = window.requestAnimationFrame(tick);
  };
  return {
    push(level: number) {
      target = speechLevel(level);
      if (!raf) {
        shown = target;
        write();
      } else if (!frame) {
        frame = window.requestAnimationFrame(tick);
      }
    },
    reset() {
      if (frame) window.cancelAnimationFrame(frame);
      frame = 0;
      target = 0;
      shown = 0;
      element.style.removeProperty("--voix");
    },
  };
}

/** Pause while the window is hidden; follow the voice level while listening. */
function useAuroraDriver(ref: RefObject<HTMLElement | null>, listening: boolean) {
  const bridge = useBridge();

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const sync = () => element.toggleAttribute("data-paused", document.visibilityState === "hidden");
    sync();
    document.addEventListener("visibilitychange", sync);
    return () => document.removeEventListener("visibilitychange", sync);
  }, [ref]);

  useEffect(() => {
    const element = ref.current;
    if (!element || !listening || prefersReducedMotion()) return;
    const glow = createVoiceGlow(element);
    const stop = listenShared(bridge, EVENT_LEVEL, (level) => {
      if (document.visibilityState !== "hidden") glow.push(level);
    });
    return () => {
      stop();
      glow.reset();
    };
  }, [bridge, listening, ref]);
}

function Veils({ live = false }: { live?: boolean }) {
  return (
    <>
      <i className="aurora__veil aurora__veil--1" />
      <i className="aurora__veil aurora__veil--2" />
      <i className="aurora__veil aurora__veil--3" />
      <i className="aurora__veil aurora__veil--4" />
      <i className="aurora__curtains" />
      <i className="aurora__breath" />
      {live ? null : (
        <>
          <i className="aurora__crest" />
          <i className="aurora__grain" />
        </>
      )}
    </>
  );
}

/** Behind the whole window, title bar included. */
export function AuroraBackdrop() {
  const ref = useRef<HTMLDivElement>(null);
  const { state } = useUiStore();
  const phase = state?.phase.kind ?? "idle";
  useAuroraDriver(ref, phase === "listening");
  return (
    <div ref={ref} className="aurora aurora--backdrop" data-phase={phase} aria-hidden="true">
      <Veils />
    </div>
  );
}

function legendFor({ phase, ready, gesture }: StatusArtProps): string | null {
  switch (phase) {
    case "listening":
      return "À l’écoute";
    case "preparing":
      return "Ouverture du micro";
    case "finalizing":
      return "Transcription";
    case "idle":
      if (!ready) return null;
      return gesture === "hold" ? "Maintenir pour parler" : "Appuyer pour commencer";
  }
}

/** The shortcut as big keys on a vivid aurora tile (Accueil status card). */
export function AuroraStatusArt(props: StatusArtProps) {
  const { phase, ready, gesture, shortcut } = props;
  const ref = useRef<HTMLDivElement>(null);
  useAuroraDriver(ref, phase === "listening");
  const legend = legendFor(props);
  return (
    <div
      className="status-art"
      data-phase={phase}
      data-gesture={gesture}
      data-resting={phase === "idle" && !ready ? "" : undefined}
      aria-hidden="true"
    >
      <div ref={ref} className="aurora aurora--live" data-phase={phase}>
        <Veils live />
      </div>
      <span className="status-art__keys">
        {shortcutKeys(shortcut).map((key, index) => (
          <span key={`${key}-${index}`} className="status-art__key" data-key={key} />
        ))}
      </span>
      {legend ? <span className="status-art__legend" data-text={legend} /> : null}
    </div>
  );
}
