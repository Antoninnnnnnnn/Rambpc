// Recent results (« Récents »): dictated text, asked with `get_recent_drafts`
// by the view that displays it, kept in that view's state only and dropped
// as soon as a result expires, is cleared or the view goes away.

import { useCallback, useEffect, useRef, useState } from "react";
import { useBridge } from "../ipc/bridge";
import type { DraftView, TakeId, UiState } from "../ipc/contract";
import { useUiCommands } from "./commands";
import { useUiState } from "./ui";

export interface RecentsStore {
  /** Newest first; null until the first answer. Expired results are never included. */
  items: DraftView[] | null;
  failed: boolean;
  /** Ticks every second while results are shown (countdowns). */
  now: number;
  reload: () => void;
  /** Copy one result (`copy_draft { take }`); rejects with a `CommandError`. */
  copy: (take: TakeId) => Promise<void>;
  /** Clear one result, or all of them when `take` is omitted. */
  clear: (take?: TakeId) => Promise<void>;
}

/** Changes whenever the native side may have added, replaced or dropped a result. */
function recentsKey(state: UiState, takesEnded: number): string {
  const draft = state.draft;
  return [
    state.recentCount,
    draft ? `${draft.take}:${draft.createdAtMs}:${draft.complete}` : "-",
    state.lastOutcome ? JSON.stringify(state.lastOutcome) : "-",
    takesEnded,
  ].join("|");
}

export function useRecents(): RecentsStore {
  const bridge = useBridge();
  const state = useUiState();
  const commands = useUiCommands();
  const [items, setItems] = useState<DraftView[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [reloads, setReloads] = useState(0);

  // Two takes with the same outcome leave the same state: count the take ends.
  const [takesEnded, setTakesEnded] = useState(0);
  const previousPhase = useRef(state.phase.kind);
  useEffect(() => {
    if (previousPhase.current !== "idle" && state.phase.kind === "idle") setTakesEnded((count) => count + 1);
    previousPhase.current = state.phase.kind;
  }, [state.phase.kind]);

  const key = recentsKey(state, takesEnded);
  const hasAny = state.recentCount > 0 || state.draft !== null;

  useEffect(() => {
    let cancelled = false;
    if (!hasAny) {
      setItems([]);
      setFailed(false);
      return;
    }
    setFailed(false);
    bridge
      .invoke("get_recent_drafts")
      .then((list) => {
        if (!cancelled) setItems(list);
      })
      .catch(() => {
        if (!cancelled) {
          setItems((current) => current ?? []);
          setFailed(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [bridge, key, hasAny, reloads]);

  const showing = items !== null && items.length > 0;
  useEffect(() => {
    if (!showing) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [showing]);

  // Drop the text of expired results from memory, not only from the screen.
  useEffect(() => {
    if (items?.some((item) => item.meta.expiresAtMs <= now)) {
      setItems((current) => current?.filter((item) => item.meta.expiresAtMs > now) ?? null);
    }
  }, [items, now]);

  const reload = useCallback(() => setReloads((count) => count + 1), []);

  const copy = useCallback(
    async (take: TakeId) => {
      await bridge.invoke("copy_draft", { take });
    },
    [bridge],
  );

  const clear = useCallback(
    async (take?: TakeId) => {
      await commands.clearDraft(take);
      setItems((current) => (take === undefined ? [] : (current?.filter((item) => item.meta.take !== take) ?? null)));
    },
    [commands],
  );

  const visible = items?.filter((item) => item.meta.expiresAtMs > now) ?? null;
  return { items: visible, failed, now, reload, copy, clear };
}
