// The floating bar's theme: cached copy first, then the appearance carried by
// each state event; fallbacks for an older native or an unknown theme.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { previewAppearance } from "../dev/overlayPreview";
import { uiState } from "../test/fakeBridge";
import { APPEARANCE_CACHE_KEY } from "../themes/appearance";
import { DEFAULT_THEME } from "../themes/registry";
import { followAppearance } from "./theme";

/** A controllable `prefers-color-scheme: dark`; every other query is false. */
function fakeSystemScheme(dark: boolean) {
  const listeners = new Set<() => void>();
  const media = {
    get matches() {
      return dark;
    },
    addEventListener: (_type: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_type: string, listener: () => void) => listeners.delete(listener),
  };
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) =>
      query === "(prefers-color-scheme: dark)" ? media : { matches: false, addEventListener() {}, removeEventListener() {} },
    ),
  );
  return {
    flip(next: boolean) {
      dark = next;
      for (const listener of listeners) listener();
    },
  };
}

describe("floating bar appearance", () => {
  let page: HTMLElement;

  beforeEach(() => {
    localStorage.clear();
    page = document.createElement("div");
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    localStorage.clear();
  });

  it("paints the cached appearance first, then the one each state carries", () => {
    localStorage.setItem(APPEARANCE_CACHE_KEY, JSON.stringify({ theme: "sable", colorScheme: "dark" }));
    const follow = followAppearance(page);
    expect(page.dataset).toMatchObject({ theme: "sable", scheme: "dark" });
    follow(uiState({ appearance: { theme: "verre-aurore", colorScheme: "light" } }));
    expect(page.dataset).toMatchObject({ theme: "verre-aurore", scheme: "light" });
    expect(page.style.colorScheme).toBe("light");
  });

  it("keeps what it has when a state carries no appearance (older native)", () => {
    localStorage.setItem(APPEARANCE_CACHE_KEY, JSON.stringify({ theme: "sable", colorScheme: "dark" }));
    const follow = followAppearance(page);
    follow(uiState());
    expect(page.dataset).toMatchObject({ theme: "sable", scheme: "dark" });
    follow(uiState({ appearance: { theme: "verre-aurore", colorScheme: "light" } }));
    follow(uiState({ phase: { kind: "listening" }, take: 2 }));
    expect(page.dataset).toMatchObject({ theme: "verre-aurore", scheme: "light" });
  });

  it("starts with the default theme without a usable cache", () => {
    localStorage.setItem(APPEARANCE_CACHE_KEY, "{not json");
    followAppearance(page);
    expect(page.dataset).toMatchObject({ theme: DEFAULT_THEME, scheme: "light" });
  });

  it("falls back to the default theme for an unknown id, and to Système for an unknown scheme", () => {
    fakeSystemScheme(true);
    localStorage.setItem(APPEARANCE_CACHE_KEY, JSON.stringify({ theme: "sable", colorScheme: "light" }));
    const follow = followAppearance(page);
    follow(uiState({ appearance: { theme: "neon", colorScheme: "sepia" as never } }));
    expect(page.dataset).toMatchObject({ theme: DEFAULT_THEME, scheme: "dark" });
  });

  it("follows Windows live in Système, not with an explicit scheme", () => {
    const system = fakeSystemScheme(false);
    const follow = followAppearance(page);
    follow(uiState({ appearance: { theme: "sable", colorScheme: "system" } }));
    expect(page.dataset.scheme).toBe("light");
    system.flip(true);
    expect(page.dataset.scheme).toBe("dark");
    follow(uiState({ appearance: { theme: "sable", colorScheme: "light" } }));
    system.flip(false);
    system.flip(true);
    expect(page.dataset.scheme).toBe("light");
  });

  it("never writes the cache: the main window owns it", () => {
    const follow = followAppearance(page);
    follow(uiState({ appearance: { theme: "sable", colorScheme: "dark" } }));
    expect(localStorage.getItem(APPEARANCE_CACHE_KEY)).toBeNull();
  });
});

describe("overlay preview appearance parameters", () => {
  const parse = (query: string) => previewAppearance(new URLSearchParams(query));

  it("sends nothing without parameters, like an older native", () => {
    expect(parse("state=listening&wall=dark")).toBeUndefined();
  });

  it("forces the theme and the scheme", () => {
    expect(parse("theme=verre-aurore&scheme=dark")).toEqual({ theme: "verre-aurore", colorScheme: "dark" });
    expect(parse("theme=sable")).toEqual({ theme: "sable", colorScheme: "system" });
    expect(parse("scheme=light")).toEqual({ theme: DEFAULT_THEME, colorScheme: "light" });
  });

  it("passes an unknown theme on, so the bar's fallback shows", () => {
    expect(parse("theme=neon&scheme=sepia")).toEqual({ theme: "neon", colorScheme: "system" });
  });
});
