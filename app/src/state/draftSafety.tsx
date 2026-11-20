// Was a passage of the latest take refused by Rambler's safety filter?
//
// UiState carries no part statuses, so for a `no-text` outcome that left a
// draft, this asks `get_draft` once per draft and keeps ONLY a boolean
// (`parts.some(status === "blocked")`). The DraftView, and its text, is
// dropped inside the callback: nothing here stores, displays or announces it.
//
// Recent results are kept side by side, so `UiState.draft` (the newest one)
// may belong to an earlier take: once a take id has been seen, the check only
// applies when the draft is that take's.

import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { useBridge } from "../ipc/bridge";
import type { TakeId, UiState } from "../ipc/contract";
import { useUiStore } from "./ui";

interface Checked {
  /** Identity of the draft checked (`take:createdAtMs`). */
  key: string | null;
  /** null while checking. */
  blocked: boolean | null;
}

export interface DraftSafety {
  /** The draft that the latest outcome needs checked, null when not applicable. */
  currentKey: string | null;
  checked: Checked;
}

const NOT_CHECKED: Checked = { key: null, blocked: false };
const DraftSafetyContext = createContext<DraftSafety>({ currentKey: null, checked: NOT_CHECKED });

/** The draft to check, if the latest outcome is `no-text` and that take left a draft. */
export function safetyKey(state: UiState | null, lastTake: TakeId | null = null): string | null {
  if (!state || state.lastOutcome?.kind !== "no-text" || !state.draft) return null;
  if (lastTake !== null && state.draft.take !== lastTake) return null;
  return `${state.draft.take}:${state.draft.createdAtMs}`;
}

export function DraftSafetyProvider({ children }: { children: ReactNode }) {
  const bridge = useBridge();
  const { state } = useUiStore();
  // The latest take id seen (UiState.take is null once idle).
  const lastTake = useRef<TakeId | null>(null);
  if (state?.take != null) lastTake.current = state.take;
  const currentKey = safetyKey(state, lastTake.current);
  const [checked, setChecked] = useState<Checked>(NOT_CHECKED);

  useEffect(() => {
    if (!currentKey) {
      setChecked(NOT_CHECKED);
      return;
    }
    let cancelled = false;
    setChecked({ key: currentKey, blocked: null });
    bridge
      .invoke("get_draft")
      .then((view) => {
        const blocked = view?.parts.some((part) => part.status === "blocked") ?? false;
        if (!cancelled) setChecked({ key: currentKey, blocked });
      })
      .catch(() => {
        if (!cancelled) setChecked({ key: currentKey, blocked: false });
      });
    return () => {
      cancelled = true;
    };
  }, [bridge, currentKey]);

  return <DraftSafetyContext.Provider value={{ currentKey, checked }}>{children}</DraftSafetyContext.Provider>;
}

export function useDraftSafety(): DraftSafety {
  return useContext(DraftSafetyContext);
}

/** True when the latest outcome is a safety-filter refusal. */
export function useSafetyBlocked(): boolean {
  const { currentKey, checked } = useDraftSafety();
  return currentKey !== null && checked.key === currentKey && checked.blocked === true;
}
