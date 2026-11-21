// The floating bar's DOM, driven by a fake event source: states and levels
// only, no command, no dictated text.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiState } from "../ipc/contract";
import { uiState } from "../test/fakeBridge";
import { APPEARANCE_CACHE_KEY } from "../themes/appearance";
import { DEFAULT_THEME } from "../themes/registry";
import { mountOverlay, type OverlaySource } from "./bar";
import { FLASH_MS } from "./model";

function fakeSource() {
  let state: (value: UiState) => void = () => {};
  let level: (value: number) => void = () => {};
  const source: OverlaySource = {
    onState: (handler) => {
      state = handler;
    },
    onLevel: (handler) => {
      level = handler;
    },
  };
  return { source, state: (value: UiState) => state(value), level: (value: number) => level(value) };
}

describe("floating bar", () => {
  let root: HTMLElement;

  beforeEach(() => {
    vi.useFakeTimers();
    root = document.createElement("div");
    root.id = "bar";
    document.body.appendChild(root);
  });

  afterEach(() => {
    vi.useRealTimers();
    root.remove();
  });

  it("stays hidden until a take starts", () => {
    const events = fakeSource();
    mountOverlay(root, events.source);
    expect(root.dataset.kind).toBe("hidden");
    events.state(uiState({ lastOutcome: { kind: "inserted" } }));
    expect(root.dataset.kind).toBe("hidden");
  });

  it("listens with voice bars and a running timer", () => {
    const events = fakeSource();
    mountOverlay(root, events.source);
    events.state(uiState());
    events.state(uiState({ phase: { kind: "listening" }, take: 3, listeningMs: 2000 }));
    expect(root.dataset.kind).toBe("listening");
    expect(root.querySelectorAll(".wave__bar")).toHaveLength(17);
    expect(root.querySelector(".bar__timer")).toHaveTextContent("0:02");
    vi.advanceTimersByTime(5000);
    expect(root.querySelector(".bar__timer")).toHaveTextContent("0:07");
    events.level(0.6);
    expect(root.querySelector<HTMLElement>(".bar__wave")!.dataset.level).toBe("0.60");
  });

  it("warns near the 5-minute limit", () => {
    const events = fakeSource();
    mountOverlay(root, events.source);
    events.state(uiState({ phase: { kind: "listening" }, take: 3, listeningMs: 4.6 * 60_000 }));
    expect(root).toHaveAttribute("data-warn");
  });

  it("shows processing, then flashes the outcome and fades out", () => {
    const events = fakeSource();
    mountOverlay(root, events.source);
    events.state(uiState({ phase: { kind: "listening" }, take: 3, listeningMs: 0 }));
    events.state(uiState({ phase: { kind: "finalizing" }, take: 3 }));
    expect(root.dataset.kind).toBe("finalizing");
    events.state(uiState({ lastOutcome: { kind: "kept-in-sambler", reason: { kind: "uncertain-target", reason: "terminal" } } }));
    expect(root.dataset.kind).toBe("outcome");
    expect(root.dataset.tone).toBe("info");
    expect(root.querySelector(".bar__label")).toHaveTextContent("Texte dans Sambler");
    vi.advanceTimersByTime(FLASH_MS - 10);
    expect(root.dataset.kind).toBe("outcome");
    vi.advanceTimersByTime(20);
    expect(root.dataset.kind).toBe("hidden");
    // The last words stay while the pill fades out.
    expect(root.querySelector(".bar__label")).toHaveTextContent("Texte dans Sambler");
  });

  it("never shows anything but fixed words", () => {
    const events = fakeSource();
    mountOverlay(root, events.source);
    events.state(uiState({ phase: { kind: "listening" }, take: 1, listeningMs: 0 }));
    events.state(uiState({ lastOutcome: { kind: "no-text" } }));
    expect(root.textContent).toBe("Aucun texte");
  });
});

describe("floating bar theming", () => {
  let root: HTMLElement;
  let page: HTMLElement;

  beforeEach(() => {
    vi.useFakeTimers();
    localStorage.clear();
    root = document.createElement("div");
    page = document.createElement("div");
    document.body.appendChild(root);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    localStorage.clear();
    root.remove();
  });

  it("wears the cached theme from the start, then the one from the state", () => {
    localStorage.setItem(APPEARANCE_CACHE_KEY, JSON.stringify({ theme: "sable", colorScheme: "dark" }));
    const events = fakeSource();
    mountOverlay(root, events.source, page);
    expect(page.dataset).toMatchObject({ theme: "sable", scheme: "dark" });
    events.state(uiState({ phase: { kind: "listening" }, take: 1, appearance: { theme: "verre-aurore", colorScheme: "light" } }));
    expect(page.dataset).toMatchObject({ theme: "verre-aurore", scheme: "light" });
    expect(root.dataset.kind).toBe("listening");
  });

  it("keeps its theme without appearance, and falls back for an unknown one", () => {
    localStorage.setItem(APPEARANCE_CACHE_KEY, JSON.stringify({ theme: "sable", colorScheme: "light" }));
    const events = fakeSource();
    mountOverlay(root, events.source, page);
    events.state(uiState({ phase: { kind: "listening" }, take: 1 }));
    expect(page.dataset.theme).toBe("sable");
    events.state(uiState({ phase: { kind: "finalizing" }, take: 1, appearance: { theme: "neon", colorScheme: "dark" } }));
    expect(page.dataset).toMatchObject({ theme: DEFAULT_THEME, scheme: "dark" });
  });

  it("marks what an outcome means, beside its severity", () => {
    const events = fakeSource();
    mountOverlay(root, events.source, page);
    events.state(uiState({ phase: { kind: "listening" }, take: 1, listeningMs: 0 }));
    events.state(uiState({ lastOutcome: { kind: "inserted" } }));
    expect(root.dataset).toMatchObject({ kind: "outcome", tone: "success", meaning: "inserted" });
    // While it fades out, the outcome keeps its colours.
    vi.advanceTimersByTime(FLASH_MS);
    expect(root.dataset).toMatchObject({ kind: "hidden", meaning: "inserted" });
    // A refusal while idle: a warning by severity, red by meaning.
    events.state(uiState({ lastOutcome: { kind: "refused", reason: { kind: "protected-field" } } }));
    expect(root.dataset).toMatchObject({ kind: "outcome", tone: "warning", meaning: "refused" });
    events.state(uiState({ phase: { kind: "listening" }, take: 2, listeningMs: 0 }));
    expect(root.dataset.meaning).toBeUndefined();
    events.state(uiState({ lastOutcome: { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } } }));
    expect(root.dataset.meaning).toBe("kept");
  });

  it("hands the voice level to the theme while listening only", () => {
    const events = fakeSource();
    mountOverlay(root, events.source, page);
    events.level(0.9);
    expect(root.style.getPropertyValue("--level")).toBe("");
    events.state(uiState({ phase: { kind: "listening" }, take: 1, listeningMs: 0 }));
    events.level(0.76);
    expect(root.style.getPropertyValue("--level")).toBe("1.00");
    events.level(0.1);
    expect(root.style.getPropertyValue("--level")).toBe("0.00");
    events.level(0.52);
    expect(root.style.getPropertyValue("--level")).toBe("0.50");
    events.state(uiState({ phase: { kind: "finalizing" }, take: 1 }));
    expect(root.style.getPropertyValue("--level")).toBe("");
    expect(root.style.getPropertyValue("--bar-flash-ms")).toBe(`${FLASH_MS}ms`);
  });

  it("keeps the horizon still with reduced motion", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((query: string) => ({
        matches: query === "(prefers-reduced-motion: reduce)",
        addEventListener() {},
        removeEventListener() {},
      })),
    );
    const events = fakeSource();
    mountOverlay(root, events.source, page);
    events.state(uiState({ phase: { kind: "listening" }, take: 1, listeningMs: 0 }));
    events.level(0.76);
    expect(root.style.getPropertyValue("--level")).toBe("");
  });

  it("builds the same parts for every theme, decorations included", () => {
    const events = fakeSource();
    mountOverlay(root, events.source, page);
    expect(root.querySelector(".bar__aurora")).not.toBeNull();
    expect(root.querySelectorAll(".bar__dots i")).toHaveLength(3);
    const bars = root.querySelectorAll<HTMLElement>(".wave__bar");
    expect(bars[16].style.getPropertyValue("--k")).toBe("16");
    expect(root.querySelector<HTMLElement>(".bar__wave")!.style.getPropertyValue("--n")).toBe("17");
  });
});
