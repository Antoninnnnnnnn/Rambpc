// Rambler access status (never the secret values).

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { useBridge } from "../ipc/bridge";
import type { AccessInput, AccessStatus } from "../ipc/contract";

interface AccessStore {
  status: AccessStatus | null;
  loadFailed: boolean;
  /** Rejects with a `CommandError`; the caller drops its copy of the values. */
  save: (input: AccessInput) => Promise<AccessStatus>;
  clear: () => Promise<AccessStatus>;
  refresh: () => void;
}

const AccessContext = createContext<AccessStore | null>(null);

export function AccessProvider({ children }: { children: ReactNode }) {
  const bridge = useBridge();
  const [status, setStatus] = useState<AccessStatus | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);

  const refresh = useCallback(() => {
    setLoadFailed(false);
    bridge
      .invoke("get_access")
      .then(setStatus)
      .catch(() => setLoadFailed(true));
  }, [bridge]);

  useEffect(refresh, [refresh]);

  const save = useCallback(
    async (input: AccessInput) => {
      const next = await bridge.invoke("set_access", { access: input });
      setStatus(next);
      return next;
    },
    [bridge],
  );

  const clear = useCallback(async () => {
    const next = await bridge.invoke("clear_access");
    setStatus(next);
    return next;
  }, [bridge]);

  const store = useMemo(
    () => ({ status, loadFailed, save, clear, refresh }),
    [status, loadFailed, save, clear, refresh],
  );
  return <AccessContext.Provider value={store}>{children}</AccessContext.Provider>;
}

export function useAccess(): AccessStore {
  const store = useContext(AccessContext);
  if (!store) throw new Error("useAccess must be used inside <AccessProvider>");
  return store;
}
