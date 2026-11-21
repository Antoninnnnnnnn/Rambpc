// `get_stats` and `get_activity` together, refreshed when a take ends and when
// the window comes back into view (the day may have changed). A failed refresh
// keeps the last data; `get_activity` failing on its own (older native side) is
// expected and handled by the screen.

import { useCallback, useEffect, useRef, useState } from "react";
import { useBridge } from "../../ipc/bridge";
import type { ActivityStats, UsageStats } from "../../ipc/contract";
import { useUiState } from "../../state/ui";

export interface ActivityData {
  /** The last answer of each command; null until one succeeded. */
  stats: UsageStats | null;
  activity: ActivityStats | null;
  /** Nothing answered yet. */
  loading: boolean;
  reload: () => void;
}

function isActivity(value: unknown): value is ActivityStats {
  const candidate = value as ActivityStats | null;
  return (
    typeof candidate === "object" &&
    candidate !== null &&
    Array.isArray(candidate.days) &&
    typeof candidate.currentStreak === "number" &&
    typeof candidate.longestStreak === "number"
  );
}

export function useActivity(): ActivityData {
  const bridge = useBridge();
  const state = useUiState();
  const [stats, setStats] = useState<UsageStats | null>(null);
  const [activity, setActivity] = useState<ActivityStats | null>(null);
  const [settled, setSettled] = useState(false);
  const [version, setVersion] = useState(0);
  const previousPhase = useRef(state.phase.kind);

  const reload = useCallback(() => setVersion((count) => count + 1), []);

  useEffect(() => {
    if (previousPhase.current !== "idle" && state.phase.kind === "idle") reload();
    previousPhase.current = state.phase.kind;
  }, [state.phase.kind, reload]);

  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible") reload();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, [reload]);

  useEffect(() => {
    let cancelled = false;
    void Promise.allSettled([bridge.invoke("get_stats"), bridge.invoke("get_activity")]).then(([nextStats, nextActivity]) => {
      if (cancelled) return;
      if (nextStats.status === "fulfilled") setStats(nextStats.value);
      if (nextActivity.status === "fulfilled" && isActivity(nextActivity.value)) setActivity(nextActivity.value);
      setSettled(true);
    });
    return () => {
      cancelled = true;
    };
  }, [bridge, version]);

  return { stats, activity, loading: !settled, reload };
}
