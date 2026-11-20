// Was a passage of the current draft refused by Rambler's safety filter?
//
// UiState carries no part statuses, so for a `no-text` outcome that left a
// draft, this asks `get_draft` once per draft and keeps ONLY a boolean
// (`parts.some(status === "blocked")`). The DraftView, and its text, is
// dropped inside the callback: nothing here stores, displays or announces it.

import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { useBridge } from "../ipc/bridge";
import type { UiState } from "../ipc/contract";
import { useUiStore } from "./ui";

export interface DraftSafety {
  /** Identity of the draft checked (`take:createdAtMs`), null when not applicable. */
  key: string | null;
  /** null while checking; false when not applicable. */
  blocked: boolean | null;
}

const NOT_APPLICABLE: DraftSafety = { key: null, blocked: false };
const DraftSafetyContext = createContext<DraftSafety>(NOT_APPLICABLE);

/** The draft to check, if the latest outcome is `no-text` and a draft exists. */
export function safetyKey(state: UiState | null): string | null {
  if (!state || state.lastOutcome?.kind !== "no-text" || !state.draft) return null;
  return `${state.draft.take}:${state.draft.createdAtMs}`;
}

export function DraftSafetyProvider({ children }: { children: ReactNode }) {
  const bridge = useBridge();
  const { state } = useUiStore();
  const key = safetyKey(state);
  const [safety, setSafety] = useState<DraftSafety>(NOT_APPLICABLE);

  useEffect(() => {
    if (!key) {
      setSafety(NOT_APPLICABLE);
      return;
    }
    let cancelled = false;
    setSafety({ key, blocked: null });
    bridge
      .invoke("get_draft")
      .then((view) => {
        const blocked = view?.parts.some((part) => part.status === "blocked") ?? false;
        if (!cancelled) setSafety({ key, blocked });
      })
      .catch(() => {
        if (!cancelled) setSafety({ key, blocked: false });
      });
    return () => {
      cancelled = true;
    };
  }, [bridge, key]);

  return <DraftSafetyContext.Provider value={safety}>{children}</DraftSafetyContext.Provider>;
}

export function useDraftSafety(): DraftSafety {
  return useContext(DraftSafetyContext);
}

/** True when the latest outcome of `state` is a safety-filter refusal. */
export function useSafetyBlocked(state: UiState): boolean {
  const safety = useDraftSafety();
  const key = safetyKey(state);
  return key !== null && safety.key === key && safety.blocked === true;
}
