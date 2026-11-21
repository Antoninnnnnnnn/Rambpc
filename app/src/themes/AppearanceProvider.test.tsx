// Appearance in the app: applied from the cache, then from Settings.appearance;
// chosen in Réglages > Apparence and saved with save_settings; kept in the
// cache when the native side predates themes; « Système » followed live; the
// theme's decor slots stay out of the accessibility tree.

import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EVENT_LEVEL, type Settings } from "../ipc/contract";
import { createFakeBridge, type FakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, renderApp } from "../test/render";
import { APPEARANCE_CACHE_KEY, cacheAppearance } from "./appearance";

const root = document.documentElement;
const cached = () => JSON.parse(localStorage.getItem(APPEARANCE_CACHE_KEY) ?? "null");

const savedSettings = (bridge: FakeBridge) =>
  bridge.invoke.mock.calls
    .filter(([command]) => command === "save_settings")
    .map(([, args]) => (args as { settings: Settings }).settings);

const lastSaved = (bridge: FakeBridge): Settings | undefined => {
  const saved = savedSettings(bridge);
  return saved[saved.length - 1];
};

function mockMedia(initial: { dark?: boolean; reducedMotion?: boolean } = {}) {
  const state = { dark: initial.dark ?? false, reducedMotion: initial.reducedMotion ?? false };
  const listeners = new Set<() => void>();
  window.matchMedia = vi.fn((query: string) => ({
    get matches() {
      return query.includes("prefers-color-scheme: dark")
        ? state.dark
        : query.includes("prefers-reduced-motion")
          ? state.reducedMotion
          : false;
    },
    media: query,
    addEventListener: (_: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_: string, listener: () => void) => listeners.delete(listener),
  })) as unknown as typeof window.matchMedia;
  return {
    setDark(dark: boolean) {
      state.dark = dark;
      listeners.forEach((listener) => listener());
    },
  };
}

async function openAppearance(bridge: FakeBridge) {
  const view = renderApp(bridge);
  await appReady();
  await view.user.click(screen.getByRole("button", { name: "Réglages" }));
  await screen.findByRole("heading", { level: 1, name: "Réglages" });
  return { ...view, card: screen.getByRole("region", { name: "Apparence" }) };
}

beforeEach(() => {
  localStorage.clear();
  delete root.dataset.theme;
  delete root.dataset.scheme;
  root.style.colorScheme = "";
});

afterEach(() => {
  delete (window as { matchMedia?: unknown }).matchMedia;
});

describe("appearance: source of truth and cache", () => {
  it("paints from the cache, then applies and caches the native choice", async () => {
    cacheAppearance({ theme: "verre-aurore", colorScheme: "light" });
    let resolveSettings: (settings: Settings) => void = () => {};
    const bridge = createFakeBridge({ settings: { appearance: { theme: "sable", colorScheme: "dark" } } });
    const native = bridge.settings;
    bridge.handlers.get_settings = () => new Promise<Settings>((resolve) => (resolveSettings = resolve));
    renderApp(bridge);
    await appReady();
    expect(root.dataset.theme).toBe("verre-aurore");
    expect(root.dataset.scheme).toBe("light");

    await act(async () => resolveSettings(native));
    await waitFor(() => expect(root.dataset.theme).toBe("sable"));
    expect(root.dataset.scheme).toBe("dark");
    expect(root.style.colorScheme).toBe("dark");
    expect(cached()).toEqual({ theme: "sable", colorScheme: "dark" });
    expect(savedSettings(bridge)).toHaveLength(0);
  });

  it("saves the theme and the mode chosen in Réglages, and updates the cache", async () => {
    const bridge = createFakeBridge();
    const { user, card } = await openAppearance(bridge);
    expect(within(card).getByRole("radio", { name: "Verre et aurore" })).toBeChecked();
    expect(within(card).getByRole("radio", { name: "Système" })).toBeChecked();

    await user.click(within(card).getByRole("radio", { name: "Sable" }));
    await waitFor(() => expect(lastSaved(bridge)?.appearance).toEqual({ theme: "sable", colorScheme: "system" }));
    expect(root.dataset.theme).toBe("sable");
    expect(cached()).toEqual({ theme: "sable", colorScheme: "system" });

    await user.click(within(card).getByRole("radio", { name: "Sombre" }));
    await waitFor(() => expect(lastSaved(bridge)?.appearance).toEqual({ theme: "sable", colorScheme: "dark" }));
    expect(root.dataset.scheme).toBe("dark");
    expect(cached()).toEqual({ theme: "sable", colorScheme: "dark" });
    // The rest of the settings is saved unchanged.
    expect(lastSaved(bridge)).toMatchObject({ language: "french", shortcut: "Ctrl+Shift+F2" });
  });

  it("rolls back a refused change and says why", async () => {
    const bridge = createFakeBridge();
    const { user, card } = await openAppearance(bridge);
    bridge.handlers.save_settings = () => {
      throw { code: "invalid-settings", message: "Valeur refusée." };
    };
    await user.click(within(card).getByRole("radio", { name: "Sable" }));
    expect(await within(card).findByText("Ce réglage n’a pas pu être enregistré. Valeur refusée.")).toBeInTheDocument();
    expect(within(card).getByRole("radio", { name: "Verre et aurore" })).toBeChecked();
    expect(root.dataset.theme).toBe("verre-aurore");
    expect(cached()).toEqual({ theme: "verre-aurore", colorScheme: "system" });
  });

  it("keeps the choice on this PC when the native side predates themes", async () => {
    const bridge = createFakeBridge({ settings: { appearance: undefined } });
    const { user, card } = await openAppearance(bridge);
    await user.click(within(card).getByRole("radio", { name: "Sable" }));
    await user.click(within(card).getByRole("radio", { name: "Clair" }));
    expect(root.dataset.theme).toBe("sable");
    expect(root.dataset.scheme).toBe("light");
    expect(cached()).toEqual({ theme: "sable", colorScheme: "light" });
    // Nothing the older native side would not understand is sent.
    expect(savedSettings(bridge)).toHaveLength(0);

    // Next start: still Sable, from the cache.
    cleanup();
    delete root.dataset.theme;
    renderApp(createFakeBridge({ settings: { appearance: undefined } }));
    await appReady();
    await waitFor(() => expect(root.dataset.theme).toBe("sable"));
    expect(root.dataset.scheme).toBe("light");
  });

  it("follows the system scheme live while « Système » is chosen", async () => {
    const system = mockMedia({ dark: false });
    const bridge = createFakeBridge();
    const { user, card } = await openAppearance(bridge);
    expect(root.dataset.scheme).toBe("light");
    expect(card).toHaveTextContent("Suit le mode de votre ordinateur : clair en ce moment.");

    act(() => system.setDark(true));
    expect(root.dataset.scheme).toBe("dark");
    expect(card).toHaveTextContent("Suit le mode de votre ordinateur : sombre en ce moment.");

    await user.click(within(card).getByRole("radio", { name: "Clair" }));
    await waitFor(() => expect(root.dataset.scheme).toBe("light"));
    act(() => system.setDark(false));
    act(() => system.setDark(true));
    expect(root.dataset.scheme).toBe("light");
  });
});

describe("Réglages > Apparence", () => {
  it("comes first and offers both themes as an accessible radio group", async () => {
    const { card } = await openAppearance(createFakeBridge());
    const toc = screen.getByRole("navigation", { name: "Sections des réglages" });
    expect(within(toc).getAllByRole("link")[0]).toHaveTextContent("Apparence");

    const themes = within(card).getByRole("group", { name: "Thème" });
    const options = within(themes).getAllByRole("radio");
    expect(options.map((option) => option.getAttribute("value"))).toEqual(["verre-aurore", "sable"]);
    expect(within(themes).getByRole("radio", { name: "Verre et aurore" })).toHaveAccessibleDescription(
      "Du verre dépoli sur une aube lente, qui s’éclaire quand vous parlez.",
    );
    expect(within(themes).getByRole("radio", { name: "Sable" })).toHaveAccessibleDescription(
      /^Sobre et clair\s: tons sable, encre et un seul bleu\.$/,
    );

    const modes = within(card).getByRole("group", { name: "Mode" });
    expect(within(modes).getAllByRole("radio").map((radio) => radio.getAttribute("value"))).toEqual([
      "system",
      "light",
      "dark",
    ]);
    // The previews are decorative.
    for (const preview of card.querySelectorAll(".theme-preview")) {
      expect(preview).toHaveAttribute("aria-hidden", "true");
    }
  });

  it("has no detectable accessibility violations", async () => {
    const { card } = await openAppearance(createFakeBridge());
    expect(await axeViolations(card)).toEqual([]);
  });
});

describe("theme slots", () => {
  it("draws the aurora only in Verre et aurore, outside the accessibility tree", async () => {
    renderApp(createFakeBridge());
    await appReady();
    const backdrop = document.querySelector(".aurora--backdrop");
    expect(backdrop).toHaveAttribute("aria-hidden", "true");
    const art = screen.getByTestId("status-card").querySelector(".status-art");
    expect(art).toHaveAttribute("aria-hidden", "true");
    // Keys and legend are drawn from attributes: no text duplicated in the card.
    expect(art?.textContent).toBe("");
    expect(art?.querySelectorAll("[data-key]")).toHaveLength(3);

    cleanup();
    renderApp(createFakeBridge({ settings: { appearance: { theme: "sable", colorScheme: "light" } } }));
    await appReady();
    await waitFor(() => expect(root.dataset.theme).toBe("sable"));
    expect(document.querySelector(".aurora")).toBeNull();
    expect(document.querySelector(".status-art")).toBeNull();
  });

  it("breathes with the voice level through the one shared listener", async () => {
    const bridge = createFakeBridge({ state: { phase: { kind: "listening" }, take: 3, listeningMs: 0 } });
    renderApp(bridge);
    await appReady();
    await waitFor(() => expect(bridge.listenerCount(EVENT_LEVEL)).toBe(1));
    act(() => bridge.emit(EVENT_LEVEL, 0.7));
    const backdrop = document.querySelector<HTMLElement>(".aurora--backdrop")!;
    await waitFor(() => expect(Number(backdrop.style.getPropertyValue("--voix"))).toBeGreaterThan(0));
    expect(backdrop.dataset.phase).toBe("listening");

    act(() => {
      bridge.pushState({ phase: { kind: "idle" }, take: null, listeningMs: null });
    });
    await waitFor(() => expect(backdrop.style.getPropertyValue("--voix")).toBe(""));
    await waitFor(() => expect(bridge.listenerCount(EVENT_LEVEL)).toBe(0));
  });

  it("stays still with reduced motion", async () => {
    mockMedia({ reducedMotion: true });
    const bridge = createFakeBridge({ state: { phase: { kind: "listening" }, take: 3, listeningMs: 0 } });
    renderApp(bridge);
    await appReady();
    act(() => bridge.emit(EVENT_LEVEL, 0.9));
    const backdrop = document.querySelector<HTMLElement>(".aurora--backdrop")!;
    expect(backdrop.style.getPropertyValue("--voix")).toBe("");
  });

  it("pauses while the window is hidden", async () => {
    renderApp(createFakeBridge());
    await appReady();
    const backdrop = document.querySelector<HTMLElement>(".aurora--backdrop")!;
    expect(backdrop).not.toHaveAttribute("data-paused");
    const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(backdrop).toHaveAttribute("data-paused");
    visibility.mockReturnValue("visible");
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(backdrop).not.toHaveAttribute("data-paused");
  });
});
