import { useCallback, useEffect, useRef, useState } from "react";
import { toCommandError } from "../ipc/bridge";
import type { CommandError } from "../ipc/contract";

/** Busy / error bookkeeping for one button's command. */
export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CommandError | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const run = useCallback(async <T>(task: () => Promise<T>): Promise<{ ok: true; value: T } | { ok: false; error: CommandError }> => {
    setBusy(true);
    setError(null);
    try {
      const value = await task();
      return { ok: true, value };
    } catch (raw) {
      const failure = toCommandError(raw);
      if (mounted.current) setError(failure);
      return { ok: false, error: failure };
    } finally {
      if (mounted.current) setBusy(false);
    }
  }, []);

  const clearError = useCallback(() => setError(null), []);

  return { busy, error, run, clearError };
}
