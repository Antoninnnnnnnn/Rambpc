// The floating bar's DOM, driven by a fake event source: states and levels
// only, no command, no dictated text.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UiState } from "../ipc/contract";
import { uiState } from "../test/fakeBridge";
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
