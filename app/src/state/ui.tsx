// Text-free UI state: `get_state` once, then `sambler://state` events.

import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { useBridge } from "../ipc/bridge";
import { EVENT_STATE, type UiState } from "../ipc/contract";

interface UiStore {
  state: UiState | null;
  /** `get_state` failed and no event arrived. */
  failed: boolean;
  /**
   * Run a command that returns a `UiState`. Its result is applied only if no
   * state event arrived meanwhile: events are emitted on every change, so a
   * newer event always wins over an older command reply.
   */
  run: (command: () => Promise<UiState>) => Promise<UiState>;
}

const UiContext = createContext<UiStore | null>(null);

export function UiStateProvider({ children }: { children: ReactNode }) {
  const bridge = useBridge();
  const [state, setState] = useState<UiState | null>(null);
  const [failed, setFailed] = useState(false);
  const eventCount = useRef(0);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    bridge
      .listen(EVENT_STATE, (next) => {
        if (disposed) return;
        eventCount.current += 1;
        setState(next);
        setFailed(false);
      })
      .then((stop) => {
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        // Ask for the current state only once the listener is in place.
        const seen = eventCount.current;
        return bridge.invoke("get_state").then((initial) => {
          if (!disposed && eventCount.current === seen) setState(initial);
        });
      })
      .catch(() => {
        if (!disposed) setFailed(true);
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [bridge]);

  const run = useCallback(async (command: () => Promise<UiState>) => {
    const seen = eventCount.current;
    const next = await command();
    if (eventCount.current === seen) setState(next);
    return next;
  }, []);

  return <UiContext.Provider value={{ state, failed, run }}>{children}</UiContext.Provider>;
}

export function useUiStore(): UiStore {
  const store = useContext(UiContext);
  if (!store) throw new Error("useUiStore must be used inside <UiStateProvider>");
  return store;
}

/** The loaded state; components using it render under the App's loading gate. */
export function useUiState(): UiState {
  const { state } = useUiStore();
  if (!state) throw new Error("UiState not loaded yet");
  return state;
}
