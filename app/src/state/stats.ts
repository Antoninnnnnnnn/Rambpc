// Usage counters (`get_stats`): words and takes only, never text. Refreshed
// whenever a take ends.

import { useEffect, useRef, useState } from "react";
import { useBridge } from "../ipc/bridge";
import type { UsageStats } from "../ipc/contract";
import { useUiState } from "./ui";

export function useStats(): { stats: UsageStats | null; failed: boolean } {
  const bridge = useBridge();
  const state = useUiState();
  const [stats, setStats] = useState<UsageStats | null>(null);
  const [failed, setFailed] = useState(false);
  const [version, setVersion] = useState(0);
  const previousPhase = useRef(state.phase.kind);

  useEffect(() => {
    if (previousPhase.current !== "idle" && state.phase.kind === "idle") setVersion((count) => count + 1);
    previousPhase.current = state.phase.kind;
  }, [state.phase.kind]);

  useEffect(() => {
    let cancelled = false;
    bridge
      .invoke("get_stats")
      .then((next) => {
        if (cancelled) return;
        setStats(next);
        setFailed(false);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [bridge, version]);

  return { stats, failed };
}
