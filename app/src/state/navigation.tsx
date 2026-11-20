import { createContext, useContext } from "react";

export type View = "home" | "draft" | "settings";

export type SettingsSection =
  | "dictation"
  | "microphone"
  | "render"
  | "destination"
  | "indicator"
  | "draft"
  | "access"
  | "privacy";

export type FirstRunStep = "voice" | "microphone" | "access" | "consent" | "try";

export interface Navigation {
  view: View;
  /** Go to a screen; for Réglages, optionally scroll to a section. */
  go: (view: View, section?: SettingsSection) => void;
  /** Open the guided first-run flow (reachable again after consent). */
  openFirstRun: (step?: FirstRunStep) => void;
}

export const NavigationContext = createContext<Navigation>({
  view: "home",
  go: () => {},
  openFirstRun: () => {},
});

export function useNavigation(): Navigation {
  return useContext(NavigationContext);
}
