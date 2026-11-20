// Appearance = which theme and which colour scheme, applied as `data-theme`
// and `data-scheme` on <html>. No framework: the floating bar uses it
// directly; React uses it through the appearance provider.
//
// Source of truth: `Settings.appearance` (native settings file, so it follows
// the profile and the portable edition). The floating bar gets the same value
// in `UiState.appearance`. A copy is cached in localStorage only to paint the
// right theme before the settings arrive, and as the fallback when the native
// side predates themes (field absent).

import type { AppearanceSettings, ColorSchemeChoice } from "../ipc/contract";
import { DEFAULT_THEME, isThemeId, type ThemeId } from "./registry";

export type Scheme = "light" | "dark";

export interface Appearance {
  theme: ThemeId;
  colorScheme: ColorSchemeChoice;
}

export const APPEARANCE_CACHE_KEY = "sambler.appearance.v1";
export const DEFAULT_APPEARANCE: Appearance = { theme: DEFAULT_THEME, colorScheme: "system" };

const SCHEMES: readonly ColorSchemeChoice[] = ["system", "light", "dark"];

/** Validates a native or cached value; unknown theme ids fall back to the default. */
export function normalizeAppearance(value: Partial<AppearanceSettings> | null | undefined): Appearance {
  return {
    theme: isThemeId(value?.theme) ? value.theme : DEFAULT_APPEARANCE.theme,
    colorScheme: SCHEMES.includes(value?.colorScheme as ColorSchemeChoice)
      ? (value!.colorScheme as ColorSchemeChoice)
      : DEFAULT_APPEARANCE.colorScheme,
  };
}

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

/** Cached copy, for the first paint. Never throws. */
export function readCachedAppearance(): Appearance {
  try {
    const raw = storage()?.getItem(APPEARANCE_CACHE_KEY);
    return normalizeAppearance(raw ? (JSON.parse(raw) as Partial<AppearanceSettings>) : null);
  } catch {
    return { ...DEFAULT_APPEARANCE };
  }
}

/** Best effort; the native settings stay the source of truth. */
export function cacheAppearance(appearance: Appearance): void {
  try {
    storage()?.setItem(APPEARANCE_CACHE_KEY, JSON.stringify(appearance));
  } catch {
    // Storage unavailable: nothing to do.
  }
}

function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)").matches
    : false;
}

export function resolveScheme(choice: ColorSchemeChoice): Scheme {
  if (choice === "system") return systemPrefersDark() ? "dark" : "light";
  return choice;
}

/** Writes `data-theme`, `data-scheme` and `color-scheme` on the root element. */
export function applyAppearance(
  appearance: Appearance,
  root: HTMLElement = document.documentElement,
): Scheme {
  const scheme = resolveScheme(appearance.colorScheme);
  root.dataset.theme = appearance.theme;
  root.dataset.scheme = scheme;
  root.style.colorScheme = scheme;
  return scheme;
}

/** Calls `onChange` when the system scheme flips. Returns an unsubscribe function. */
export function watchSystemScheme(onChange: () => void): () => void {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return () => {};
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  media.addEventListener?.("change", onChange);
  return () => media.removeEventListener?.("change", onChange);
}
