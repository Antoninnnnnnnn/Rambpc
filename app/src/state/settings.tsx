// Ordinary preferences. Each change is saved immediately with `save_settings`
// (serialized, one at a time); a refused change rolls back and its error is
// attached to the control that made it.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { toCommandError, useBridge } from "../ipc/bridge";
import type { CommandError, Settings } from "../ipc/contract";

export type SettingsField =
  | "language"
  | "microphone"
  | "gesture"
  | "shortcut"
  | "cancelWithEscape"
  | "profile"
  | "text"
  | "alwaysKeepInSambler"
  | "indicator"
  | "draftMinutes"
  | "history"
  | "launchAtStartup"
  | "insertion"
  | "insertionTarget"
  | "appearance"
  | "appStyles";

type Patch = (settings: Settings) => Settings;

interface PendingChange {
  id: number;
  field: SettingsField;
  patch: Patch;
}

interface SettingsStore {
  /** Saved settings with not-yet-confirmed changes applied (optimistic). */
  settings: Settings | null;
  loadFailed: boolean;
  errors: Partial<Record<SettingsField, CommandError>>;
  /**
   * Resolves `true` once applied, `false` if refused (rolled back). A
   * `save-failed` error still applies (for this session) and is reported.
   */
  update: (field: SettingsField, patch: Patch) => Promise<boolean>;
  /** Record a change another command already applied natively (no save_settings). */
  applyLocal: (patch: Patch) => void;
  reload: () => void;
}

const SettingsContext = createContext<SettingsStore | null>(null);

export function SettingsProvider({ children }: { children: ReactNode }) {
  const bridge = useBridge();
  const [confirmed, setConfirmed] = useState<Settings | null>(null);
  const [pending, setPending] = useState<PendingChange[]>([]);
  const [errors, setErrors] = useState<SettingsStore["errors"]>({});
  const [loadFailed, setLoadFailed] = useState(false);
  const confirmedRef = useRef<Settings | null>(null);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const nextId = useRef(0);

  const reload = useCallback(() => {
    setLoadFailed(false);
    bridge
      .invoke("get_settings")
      .then((loaded) => {
        confirmedRef.current = loaded;
        setConfirmed(loaded);
      })
      .catch(() => setLoadFailed(true));
  }, [bridge]);

  useEffect(reload, [reload]);

  const update = useCallback(
    (field: SettingsField, patch: Patch) => {
      const id = ++nextId.current;
      setPending((list) => [...list, { id, field, patch }]);
      setErrors(({ [field]: _cleared, ...rest }) => rest);
      const job = queue.current.then(async () => {
        const base = confirmedRef.current;
        try {
          if (!base) throw { code: "not-loaded", message: "" };
          const saved = await bridge.invoke("save_settings", { settings: patch(base) });
          confirmedRef.current = saved;
          setConfirmed(saved);
          return true;
        } catch (raw) {
          const error = toCommandError(raw);
          setErrors((current) => ({ ...current, [field]: error }));
          if (error.code === "save-failed" && base) {
            // Applied natively for this session, only the disk write failed.
            confirmedRef.current = patch(base);
            setConfirmed(confirmedRef.current);
            return true;
          }
          return false;
        } finally {
          setPending((list) => list.filter((change) => change.id !== id));
        }
      });
      queue.current = job;
      return job;
    },
    [bridge],
  );

  const applyLocal = useCallback((patch: Patch) => {
    if (!confirmedRef.current) return;
    confirmedRef.current = patch(confirmedRef.current);
    setConfirmed(confirmedRef.current);
  }, []);

  const settings = useMemo(
    () => (confirmed ? pending.reduce((current, change) => change.patch(current), confirmed) : null),
    [confirmed, pending],
  );

  const store = useMemo(
    () => ({ settings, loadFailed, errors, update, applyLocal, reload }),
    [settings, loadFailed, errors, update, applyLocal, reload],
  );
  return <SettingsContext.Provider value={store}>{children}</SettingsContext.Provider>;
}

export function useSettings(): SettingsStore {
  const store = useContext(SettingsContext);
  if (!store) throw new Error("useSettings must be used inside <SettingsProvider>");
  return store;
}
