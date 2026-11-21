// React side of the appearance (appearance.ts does the DOM work). The source
// of truth is `Settings.appearance`, read and saved through the settings
// store (save_settings, one change at a time, rolled back if refused). Every
// applied value is also cached for the next first paint. When the native side
// predates themes (no `appearance` field), the choice lives in that cache
// only and still applies. « Système » follows the system scheme live.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import type { CommandError } from "../ipc/contract";
import { useSettings } from "../state/settings";
import {
  applyAppearance,
  cacheAppearance,
  normalizeAppearance,
  readCachedAppearance,
  resolveScheme,
  watchSystemScheme,
  type Appearance,
  type Scheme,
} from "./appearance";
import { THEMES, type ThemeManifest } from "./registry";

export interface AppearanceStore {
  /** The person's choice: theme and Système / Clair / Sombre. */
  appearance: Appearance;
  /** The scheme actually shown (« Système » resolved). */
  scheme: Scheme;
  manifest: ThemeManifest;
  /** False when the native settings cannot keep it (older native side): this PC's cache only. */
  persisted: boolean;
  /** Why the last change was refused (it was rolled back), if it was. */
  error: CommandError | undefined;
  /** Applies at once; resolves `false` if the native side refused it. */
  setAppearance: (change: Partial<Appearance>) => Promise<boolean>;
}

const AppearanceContext = createContext<AppearanceStore | null>(null);

/** Must sit inside <SettingsProvider>. */
export function AppearanceProvider({ children }: { children: ReactNode }) {
  const { settings, update, errors } = useSettings();
  const [local, setLocal] = useState<Appearance>(readCachedAppearance);
  const native = settings?.appearance;
  const persisted = native != null;
  const nativeTheme = native?.theme;
  const nativeScheme = native?.colorScheme;

  const appearance = useMemo<Appearance>(
    () => (persisted ? normalizeAppearance({ theme: nativeTheme, colorScheme: nativeScheme }) : local),
    [persisted, nativeTheme, nativeScheme, local],
  );
  const [scheme, setScheme] = useState<Scheme>(() => resolveScheme(appearance.colorScheme));

  // Before paint, so a change never shows one frame of the previous theme.
  useLayoutEffect(() => {
    setScheme(applyAppearance(appearance));
    cacheAppearance(appearance);
  }, [appearance]);

  useEffect(() => {
    if (appearance.colorScheme !== "system") return;
    return watchSystemScheme(() => setScheme(applyAppearance(appearance)));
  }, [appearance]);

  const setAppearance = useCallback(
    async (change: Partial<Appearance>) => {
      setLocal((current) => normalizeAppearance({ ...current, ...change }));
      if (!persisted) return true;
      return update("appearance", (s) => ({
        ...s,
        appearance: { ...normalizeAppearance(s.appearance), ...change },
      }));
    },
    [persisted, update],
  );

  const store = useMemo<AppearanceStore>(
    () => ({
      appearance,
      scheme,
      manifest: THEMES[appearance.theme],
      persisted,
      error: errors.appearance,
      setAppearance,
    }),
    [appearance, scheme, persisted, errors.appearance, setAppearance],
  );
  return <AppearanceContext.Provider value={store}>{children}</AppearanceContext.Provider>;
}

export function useAppearance(): AppearanceStore {
  const store = useContext(AppearanceContext);
  if (!store) throw new Error("useAppearance must be used inside <AppearanceProvider>");
  return store;
}

/** The active theme, or null outside the provider (theme slots render nothing then). */
export function useActiveTheme() {
  return useContext(AppearanceContext)?.appearance.theme ?? null;
}
