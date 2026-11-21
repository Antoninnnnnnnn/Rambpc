// Doux et rond: the living shape (« la forme vivante ») of the Accueil status
// card, styles in galets.css. Two pebbles, a lilac back and a pêche body, and
// a soft sheen, redrawn as smooth closed curves that breathe. While Sambler
// listens they swell with the voice level (`sambler://level`, a number: never
// any dictated text). Their tint follows the state: pêche ready or listening,
// lilas while processing, brume while dictation is not available.
// Decorative only: aria-hidden, paused while the window is hidden, one still
// frame with reduced motion, hidden in forced colours.

import { useEffect, useId, useRef } from "react";
import { useBridge } from "../../ipc/bridge";
import { EVENT_LEVEL, type PhaseKind } from "../../ipc/contract";
import { listenShared } from "../../ipc/shared";
import { prefersReducedMotion, speechLevel } from "../../lib/waveform";
import type { StatusArtProps } from "../slots";

export type GaletMode = "repos" | "ecoute" | "pense" | "dort";
export type GaletTint = "voix" | "lilas" | "brume";

interface Motion {
  /** Wobble of the outline (share of the radius). */
  amp: number;
  /** Speed of the wobble. */
  speed: number;
  /** Slow swell of the whole shape (share of the radius). */
  breath: number;
  /** Degrees per second the pair turns (processing only). */
  turn?: number;
  /** Frames drawn per second: slow moods do not need sixty. */
  fps: number;
}

const MOTION: Record<GaletMode, Motion> = {
  repos: { amp: 0.1, speed: 0.55, breath: 0.03, fps: 24 },
  ecoute: { amp: 0.08, speed: 1.5, breath: 0.012, fps: 60 },
  pense: { amp: 0.12, speed: 1.15, breath: 0.045, turn: 70, fps: 30 },
  dort: { amp: 0.06, speed: 0.22, breath: 0.035, fps: 12 },
};

/** The shape of the Accueil pebbles (the mockup's hero). */
const SEED = 1;
/** The time drawn when motion is reduced: a pleasant, still pose. */
const STILL_AT = 2.4;

export function galetMode(phase: PhaseKind, ready: boolean): GaletMode {
  switch (phase) {
    case "listening":
    case "preparing":
      return "ecoute";
    case "finalizing":
      return "pense";
    case "idle":
      return ready ? "repos" : "dort";
  }
}

export function galetTint(phase: PhaseKind, ready: boolean): GaletTint {
  if (phase === "finalizing") return "lilas";
  if (phase === "idle" && !ready) return "brume";
  return "voix";
}

const f2 = (value: number) => value.toFixed(2);

/**
 * A smooth closed curve through `points` points on a wobbling circle
 * (Catmull-Rom through the points, written as cubic Béziers).
 */
export function pebblePath(t: number, seed: number, amp: number, radius: number, cx = 0, cy = 0, points = 7): string {
  const p: [number, number][] = [];
  for (let i = 0; i < points; i += 1) {
    const angle = (i / points) * Math.PI * 2 + seed * 0.3;
    const wobble =
      Math.sin(t + i * 2.17 + seed) * 0.55 +
      Math.sin(t * 1.63 + i * 1.31 + seed * 2.3) * 0.3 +
      Math.sin(t * 0.71 + i * 0.57 + seed * 0.7) * 0.15;
    const r = radius * (1 + amp * wobble);
    p.push([cx + Math.cos(angle) * r, cy + Math.sin(angle) * r]);
  }
  let d = `M${f2(p[0][0])},${f2(p[0][1])}`;
  for (let i = 0; i < points; i += 1) {
    const p0 = p[(i - 1 + points) % points];
    const p1 = p[i];
    const p2 = p[(i + 1) % points];
    const p3 = p[(i + 2) % points];
    const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    d += `C${f2(c1[0])},${f2(c1[1])} ${f2(c2[0])},${f2(c2[1])} ${f2(p2[0])},${f2(p2[1])}`;
  }
  return `${d}Z`;
}

export interface GaletFrame {
  back: string;
  body: string;
  sheen: string;
  /** Rotation of the pair, in degrees. */
  turn: number;
}

/** The pebbles at `seconds`, for a mood and a smoothed voice level (0..1). */
export function galetFrame(seconds: number, seed: number, mode: GaletMode, level: number): GaletFrame {
  const motion = MOTION[mode];
  const amp = motion.amp + level * 0.2;
  const t = seconds * motion.speed + seed * 3;
  const swell = 1 + Math.sin(seconds * 1.1 + seed) * motion.breath + level * 0.1;
  return {
    // Two offset pebbles and an inner glow: a shape, not a marble.
    back: pebblePath(t * 0.8 + 2, seed + 5, amp * 1.3, 41 * swell, 5 * swell, 4 * swell),
    body: pebblePath(t, seed, amp, 37 * swell, -2 * swell, -2 * swell),
    sheen: pebblePath(t * 1.3 + 4, seed + 9, amp * 1.4, 18 * swell, -9 * swell, -11 * swell),
    turn: motion.turn ? (seconds * motion.turn) % 360 : 0,
  };
}

/** The living shape in the Accueil status card. */
export function GaletsStatusArt({ phase, ready }: StatusArtProps) {
  const bridge = useBridge();
  const id = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const root = useRef<HTMLDivElement>(null);
  const group = useRef<SVGGElement>(null);
  const back = useRef<SVGPathElement>(null);
  const body = useRef<SVGPathElement>(null);
  const sheen = useRef<SVGPathElement>(null);
  const mode = galetMode(phase, ready);
  const tint = galetTint(phase, ready);
  const listening = phase === "listening";

  useEffect(() => {
    const element = root.current;
    if (!element) return;
    const still = prefersReducedMotion();
    const fps = MOTION[mode].fps;
    const raf = typeof window.requestAnimationFrame === "function";
    let target = 0;
    let shown = 0;
    let frame = 0;
    let last = -Infinity;

    const paint = (seconds: number) => {
      shown += (target - shown) * (target > shown ? 0.3 : 0.12);
      if (Math.abs(target - shown) < 0.004) shown = target;
      const pose = galetFrame(seconds, SEED, mode, shown);
      back.current?.setAttribute("d", pose.back);
      body.current?.setAttribute("d", pose.body);
      sheen.current?.setAttribute("d", pose.sheen);
      if (pose.turn) group.current?.setAttribute("transform", `rotate(${pose.turn.toFixed(1)})`);
      else group.current?.removeAttribute("transform");
      if (shown > 0) element.style.setProperty("--niveau", shown.toFixed(3));
      else element.style.removeProperty("--niveau");
    };

    if (still) {
      paint(STILL_AT);
      return;
    }

    const loop = (ms: number) => {
      frame = window.requestAnimationFrame(loop);
      if (ms - last < 1000 / fps - 4) return;
      last = ms;
      paint(ms / 1000);
    };
    const stop = () => {
      if (frame) window.cancelAnimationFrame(frame);
      frame = 0;
    };
    const sync = () => {
      const hidden = document.visibilityState === "hidden";
      element.toggleAttribute("data-paused", hidden);
      if (hidden) stop();
      else if (raf && !frame) frame = window.requestAnimationFrame(loop);
    };

    paint(performance.now() / 1000);
    sync();
    document.addEventListener("visibilitychange", sync);
    const stopLevel = listening
      ? listenShared(bridge, EVENT_LEVEL, (level) => {
          if (document.visibilityState === "hidden") return;
          target = speechLevel(level);
          // Without animation frames (tests, odd engines), follow at once.
          if (!raf) paint(performance.now() / 1000);
        })
      : undefined;

    return () => {
      stop();
      document.removeEventListener("visibilitychange", sync);
      stopLevel?.();
      element.style.removeProperty("--niveau");
    };
  }, [bridge, mode, listening]);

  return (
    <div ref={root} className="doux-galets" data-phase={phase} data-mode={mode} data-tint={tint} aria-hidden="true">
      <span className="doux-galets__halo" />
      <svg className="doux-galets__shape" viewBox="-60 -60 120 120" focusable="false">
        <defs>
          <linearGradient id={`${id}-body`} x1=".08" y1=".04" x2=".92" y2="1">
            <stop offset="0" className="doux-galets__stop doux-galets__stop--1" />
            <stop offset=".52" className="doux-galets__stop doux-galets__stop--2" />
            <stop offset="1" className="doux-galets__stop doux-galets__stop--3" />
          </linearGradient>
          <linearGradient id={`${id}-back`} x1="1" y1="0" x2=".1" y2="1">
            <stop offset="0" className="doux-galets__stop doux-galets__stop--back-1" />
            <stop offset="1" className="doux-galets__stop doux-galets__stop--back-2" />
          </linearGradient>
          <radialGradient id={`${id}-sheen`} cx=".5" cy=".5" r=".5">
            <stop offset="0" className="doux-galets__stop doux-galets__stop--sheen" />
            <stop offset="1" className="doux-galets__stop doux-galets__stop--sheen-out" />
          </radialGradient>
        </defs>
        <g ref={group}>
          <path ref={back} className="doux-galets__back" fill={`url(#${id}-back)`} />
          <path ref={body} className="doux-galets__body" fill={`url(#${id}-body)`} />
          <path ref={sheen} className="doux-galets__sheen" fill={`url(#${id}-sheen)`} />
        </g>
      </svg>
    </div>
  );
}
