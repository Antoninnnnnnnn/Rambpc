// Level-driven voice bars, shared by the main window and the floating bar.
// Plain DOM, no framework: a few `transform: scaleY()` writes per frame.
// The newest level enters at the center and travels outwards, so the bars
// follow the voice rather than flicker. Purely visual (callers hide it from
// assistive technologies); with reduced motion the bars rest.

export interface Waveform {
  /** A raw level in 0..1 (−60..0 dBFS, linear) from `sambler://level`. */
  push(level: number): void;
  /** Back to rest (silence). */
  reset(): void;
  destroy(): void;
}

/** Map the dBFS-linear level to a visual amount: room noise rests, speech fills. */
export function speechLevel(raw: number): number {
  const level = Number.isFinite(raw) ? raw : 0;
  return Math.min(1, Math.max(0, (level - 0.28) / 0.48));
}

const REST = 0.16;
const STEP_MS = 45;

export function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function createWaveform(root: HTMLElement, count: number, reducedMotion = prefersReducedMotion()): Waveform {
  let bars = Array.from(root.children) as HTMLElement[];
  if (bars.length !== count) {
    root.replaceChildren();
    bars = Array.from({ length: count }, () => {
      const bar = document.createElement("span");
      bar.className = "wave__bar";
      root.appendChild(bar);
      return bar;
    });
  }
  const center = (count - 1) / 2;
  const reach = Math.ceil(center) + 1;
  const history = new Float32Array(reach);
  const jitter = new Float32Array(count).fill(1);
  let target = 0;
  let shown = 0;
  let lastStep = 0;
  let frame = 0;
  const raf = typeof window.requestAnimationFrame === "function" && !reducedMotion;

  const paint = () => {
    for (let i = 0; i < count; i += 1) {
      const distance = Math.abs(i - center);
      const envelope = 1 - (distance / (center + 1)) ** 2 * 0.6;
      const amount = Math.min(1, history[Math.min(reach - 1, Math.round(distance))] * envelope * jitter[i]);
      bars[i].style.transform = `scaleY(${(REST + (1 - REST) * amount).toFixed(3)})`;
    }
  };

  const tick = (time: number) => {
    frame = 0;
    shown += (target - shown) * (target > shown ? 0.5 : 0.18);
    if (time - lastStep >= STEP_MS) {
      lastStep = time;
      history.copyWithin(1, 0, reach - 1);
      for (let i = 0; i < count; i += 1) jitter[i] = 0.82 + Math.random() * 0.3;
    }
    history[0] = shown;
    paint();
    const moving = shown > 0.004 || history.some((value) => value > 0.004);
    if (moving) frame = window.requestAnimationFrame(tick);
  };

  const schedule = () => {
    if (raf && !frame) frame = window.requestAnimationFrame(tick);
  };

  const rest = () => {
    for (const bar of bars) bar.style.transform = `scaleY(${REST})`;
  };
  rest();

  return {
    push(level) {
      root.dataset.level = (Number.isFinite(level) ? Math.min(1, Math.max(0, level)) : 0).toFixed(2);
      target = speechLevel(level);
      if (reducedMotion) return;
      if (raf) schedule();
      else {
        history.copyWithin(1, 0, reach - 1);
        history[0] = target;
        paint();
      }
    },
    reset() {
      root.dataset.level = "0.00";
      target = 0;
      shown = 0;
      history.fill(0);
      if (frame) window.cancelAnimationFrame(frame);
      frame = 0;
      rest();
    },
    destroy() {
      if (frame) window.cancelAnimationFrame(frame);
      frame = 0;
    },
  };
}
