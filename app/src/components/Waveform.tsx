// Visual-only voice bars (aria-hidden), fed by a level event outside React
// renders. Used while listening and during the microphone test.

import { useEffect, useRef } from "react";
import { useBridge } from "../ipc/bridge";
import { EVENT_LEVEL, EVENT_MIC_TEST_LEVEL } from "../ipc/contract";
import { createWaveform } from "../lib/waveform";
import "./Waveform.css";

interface Props {
  event: typeof EVENT_LEVEL | typeof EVENT_MIC_TEST_LEVEL;
  active: boolean;
  /** Raw level for callers that derive a text status (throttle there). */
  onLevel?: (level: number) => void;
  bars?: number;
  className?: string;
}

export function Waveform({ event, active, onLevel, bars = 23, className }: Props) {
  const bridge = useBridge();
  const root = useRef<HTMLDivElement>(null);
  const onLevelRef = useRef(onLevel);
  useEffect(() => {
    onLevelRef.current = onLevel;
  });

  useEffect(() => {
    const element = root.current;
    if (!element) return;
    const wave = createWaveform(element, bars);
    wave.reset();
    if (!active) return () => wave.destroy();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    bridge
      .listen(event, (level) => {
        if (disposed) return;
        const clamped = Math.min(1, Math.max(0, Number.isFinite(level) ? level : 0));
        wave.push(clamped);
        onLevelRef.current?.(clamped);
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
      wave.reset();
      wave.destroy();
    };
  }, [bridge, event, active, bars]);

  return (
    <div
      ref={root}
      className={["wave", active ? "wave--active" : "", className ?? ""].filter(Boolean).join(" ")}
      aria-hidden="true"
      data-testid="waveform"
      data-level="0.00"
    >
      {Array.from({ length: bars }, (_, index) => (
        <span key={index} className="wave__bar" />
      ))}
    </div>
  );
}
