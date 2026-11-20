// Visual-only microphone level (aria-hidden): updated outside React renders
// at the event rate (~20 Hz).

import { useEffect, useRef } from "react";
import { useBridge } from "../ipc/bridge";
import { EVENT_LEVEL, EVENT_MIC_TEST_LEVEL } from "../ipc/contract";
import "./LevelMeter.css";

interface Props {
  event: typeof EVENT_LEVEL | typeof EVENT_MIC_TEST_LEVEL;
  active: boolean;
  /** Raw level for callers that derive a text status (throttle there). */
  onLevel?: (level: number) => void;
  label?: string;
}

export function LevelMeter({ event, active, onLevel, label }: Props) {
  const bridge = useBridge();
  const fill = useRef<HTMLDivElement>(null);
  const onLevelRef = useRef(onLevel);
  useEffect(() => {
    onLevelRef.current = onLevel;
  });

  useEffect(() => {
    const set = (level: number) => {
      const clamped = Math.min(1, Math.max(0, Number.isFinite(level) ? level : 0));
      if (fill.current) fill.current.style.transform = `scaleX(${clamped})`;
      onLevelRef.current?.(clamped);
    };
    set(0);
    if (!active) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    bridge
      .listen(event, (level) => {
        if (!disposed) set(level);
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
      set(0);
    };
  }, [bridge, event, active]);

  return (
    <div className={active ? "level-meter level-meter--active" : "level-meter"} aria-hidden="true" data-testid="level-meter">
      {label ? <span className="level-meter__label">{label}</span> : null}
      <div className="level-meter__track">
        <div className="level-meter__fill" ref={fill} />
      </div>
    </div>
  );
}
