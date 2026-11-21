// Optional, theme-owned pieces of React, mapped by theme id. Components place
// a slot (<ThemeBackdrop />, <ThemeStatusArt />) and the active theme fills
// it, or not: a theme without an entry renders nothing there. Slots are
// decorative only (aria-hidden, no focusable content, no text that the
// accessible content does not already carry), so every theme keeps the same
// structure, ARIA and tests.

import type { ComponentType } from "react";
import type { Gesture, PhaseKind } from "../ipc/contract";
import { useActiveTheme } from "./AppearanceProvider";
import type { ThemeId } from "./registry";
import { AuroraBackdrop, AuroraStatusArt } from "./verre-aurore/Aurora";

export interface StatusArtProps {
  phase: PhaseKind;
  /** Idle and able to start a take (Rambler ready or connecting). */
  ready: boolean;
  gesture: Gesture;
  /** Accelerator syntax, e.g. "Ctrl+Shift+F2". */
  shortcut: string;
}

export interface ThemeSlots {
  /** Decor behind the whole window, title bar included (`position: fixed; z-index: -1`). */
  Backdrop?: ComponentType;
  /** Decor in the Accueil status card, beside the live state. */
  StatusArt?: ComponentType<StatusArtProps>;
}

export const THEME_SLOTS: Record<ThemeId, ThemeSlots> = {
  "verre-aurore": { Backdrop: AuroraBackdrop, StatusArt: AuroraStatusArt },
  sable: {},
};

export function ThemeBackdrop() {
  const theme = useActiveTheme();
  const Backdrop = theme ? THEME_SLOTS[theme].Backdrop : undefined;
  return Backdrop ? <Backdrop /> : null;
}

export function ThemeStatusArt(props: StatusArtProps) {
  const theme = useActiveTheme();
  const StatusArt = theme ? THEME_SLOTS[theme].StatusArt : undefined;
  return StatusArt ? <StatusArt {...props} /> : null;
}
