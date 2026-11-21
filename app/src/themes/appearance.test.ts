import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  APPEARANCE_CACHE_KEY,
  applyAppearance,
  cacheAppearance,
  DEFAULT_APPEARANCE,
  normalizeAppearance,
  readCachedAppearance,
  resolveScheme,
  watchSystemScheme,
} from "./appearance";
import { DEFAULT_THEME, isThemeId, THEME_IDS, THEMES } from "./registry";

/** A controllable `prefers-color-scheme: dark` media query. */
function mockSystemScheme(dark: boolean) {
  const listeners = new Set<() => void>();
  const media = {
    get matches() {
      return dark;
    },
    addEventListener: (_: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_: string, listener: () => void) => listeners.delete(listener),
  };
  window.matchMedia = vi.fn(() => media) as unknown as typeof window.matchMedia;
  return {
    set(next: boolean) {
      dark = next;
      listeners.forEach((listener) => listener());
    },
    listeners,
  };
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  delete (window as { matchMedia?: unknown }).matchMedia;
  const root = document.documentElement;
  delete root.dataset.theme;
  delete root.dataset.scheme;
  root.style.colorScheme = "";
});

describe("normalizeAppearance", () => {
  it("keeps a valid choice", () => {
    expect(normalizeAppearance({ theme: "doux-rond", colorScheme: "dark" })).toEqual({
      theme: "doux-rond",
      colorScheme: "dark",
    });
  });

  it("brings a removed theme (Sable) back to the default, keeping the mode", () => {
    expect(isThemeId("sable")).toBe(false);
    expect(normalizeAppearance({ theme: "sable", colorScheme: "dark" })).toEqual({
      theme: DEFAULT_THEME,
      colorScheme: "dark",
    });
  });

  it("falls back to the default theme and to « Système » for unknown values", () => {
    expect(normalizeAppearance({ theme: "papier-chaud", colorScheme: "sepia" as never })).toEqual(DEFAULT_APPEARANCE);
    expect(normalizeAppearance({ theme: 42 as never })).toEqual({ theme: DEFAULT_THEME, colorScheme: "system" });
    expect(normalizeAppearance(null)).toEqual(DEFAULT_APPEARANCE);
    expect(normalizeAppearance(undefined)).toEqual(DEFAULT_APPEARANCE);
  });

  it("completes a partial value", () => {
    expect(normalizeAppearance({ colorScheme: "light" })).toEqual({ theme: DEFAULT_THEME, colorScheme: "light" });
  });
});

describe("the cache for the first paint", () => {
  it("is empty at first", () => {
    expect(readCachedAppearance()).toEqual(DEFAULT_APPEARANCE);
  });

  it("round-trips a choice", () => {
    cacheAppearance({ theme: "doux-rond", colorScheme: "light" });
    expect(JSON.parse(localStorage.getItem(APPEARANCE_CACHE_KEY)!)).toEqual({ theme: "doux-rond", colorScheme: "light" });
    expect(readCachedAppearance()).toEqual({ theme: "doux-rond", colorScheme: "light" });
  });

  it("never throws on a damaged or foreign value", () => {
    localStorage.setItem(APPEARANCE_CACHE_KEY, "{not json");
    expect(readCachedAppearance()).toEqual(DEFAULT_APPEARANCE);
    localStorage.setItem(APPEARANCE_CACHE_KEY, JSON.stringify({ theme: "ancien-theme", colorScheme: "dark" }));
    expect(readCachedAppearance()).toEqual({ theme: DEFAULT_THEME, colorScheme: "dark" });
  });

  it("paints the default theme over a Sable left in the cache", () => {
    localStorage.setItem(APPEARANCE_CACHE_KEY, JSON.stringify({ theme: "sable", colorScheme: "light" }));
    expect(readCachedAppearance()).toEqual({ theme: DEFAULT_THEME, colorScheme: "light" });
    applyAppearance(readCachedAppearance());
    expect(document.documentElement.dataset.theme).toBe(DEFAULT_THEME);
  });

  it("survives a storage that refuses to write", () => {
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    expect(() => cacheAppearance({ theme: "doux-rond", colorScheme: "dark" })).not.toThrow();
    setItem.mockRestore();
  });
});

describe("applying the appearance", () => {
  it("writes the theme, the resolved scheme and color-scheme on <html>", () => {
    const scheme = applyAppearance({ theme: "doux-rond", colorScheme: "dark" });
    const root = document.documentElement;
    expect(scheme).toBe("dark");
    expect(root.dataset.theme).toBe("doux-rond");
    expect(root.dataset.scheme).toBe("dark");
    expect(root.style.colorScheme).toBe("dark");
  });

  it("resolves « Système » from the system preference", () => {
    expect(resolveScheme("system")).toBe("light");
    mockSystemScheme(true);
    expect(resolveScheme("system")).toBe("dark");
    expect(applyAppearance({ theme: "verre-aurore", colorScheme: "system" })).toBe("dark");
    expect(resolveScheme("light")).toBe("light");
  });

  it("follows the system when it changes, until unsubscribed", () => {
    const system = mockSystemScheme(false);
    const onChange = vi.fn();
    const stop = watchSystemScheme(onChange);
    system.set(true);
    expect(onChange).toHaveBeenCalledTimes(1);
    stop();
    system.set(false);
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(system.listeners.size).toBe(0);
  });

  it("does nothing without matchMedia", () => {
    expect(() => watchSystemScheme(() => {})()).not.toThrow();
  });
});

describe("the registry", () => {
  it("offers Verre et aurore (the default) and Doux et rond, and no longer Sable", () => {
    expect(THEME_IDS).toEqual(["verre-aurore", "doux-rond"]);
    expect(DEFAULT_THEME).toBe("verre-aurore");
    expect(THEMES["doux-rond"].label).toBe("Doux et rond");
    expect(Object.keys(THEMES)).not.toContain("sable");
  });

  it("describes every theme with 3 to 5 preview colours per scheme", () => {
    for (const id of THEME_IDS) {
      const theme = THEMES[id];
      expect(theme.id).toBe(id);
      expect(theme.label).not.toBe("");
      for (const scheme of ["light", "dark"] as const) {
        expect(theme.swatches[scheme].length).toBeGreaterThanOrEqual(3);
        expect(theme.swatches[scheme].length).toBeLessThanOrEqual(5);
        for (const color of theme.swatches[scheme]) expect(color).toMatch(/^#[0-9a-f]{6}$/);
      }
    }
  });
});
