import { createContext, useContext } from "react";

/** Sidebar destinations. */
export type View = "home" | "dictionary" | "shortcuts" | "style" | "history" | "settings";

export type SettingsSection =
  | "microphone"
  | "shortcut"
  | "language"
  | "insertion"
  | "destination"
  | "indicator"
  | "recents"
  | "startup"
  | "access"
  | "privacy";

/** A place inside a screen that navigation can bring into view and focus. */
export type Section = SettingsSection | "home-recents";

/** Onboarding: consent → micro → access (only if needed) → try. */
export type FirstRunStep = "consent" | "microphone" | "access" | "try";

export interface Navigation {
  view: View;
  /** Go to a screen; optionally bring one of its sections into view. */
  go: (view: View, section?: Section) => void;
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
