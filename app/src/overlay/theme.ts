// Which theme and colour scheme the floating bar wears. The cached copy paints
// the first frame; then every `sambler://state` that carries `appearance`
// (natives from milestone 4 on) wins. An older native sends none: the bar keeps
// what it has. An unknown theme id falls back to the default theme.

import type { UiState } from "../ipc/contract";
import {
  applyAppearance,
  normalizeAppearance,
  readCachedAppearance,
  watchSystemScheme,
  type Appearance,
} from "../themes/appearance";

/** Applies the cached appearance now; returns the handler for each state event. */
export function followAppearance(page: HTMLElement = document.documentElement): (state: UiState) => void {
  let current: Appearance = readCachedAppearance();
  applyAppearance(current, page);
  // "Système" follows Windows live; a no-op for an explicit light or dark.
  watchSystemScheme(() => applyAppearance(current, page));
  return (state) => {
    if (!state.appearance) return;
    const next = normalizeAppearance(state.appearance);
    if (next.theme === current.theme && next.colorScheme === current.colorScheme) return;
    current = next;
    applyAppearance(current, page);
  };
}
