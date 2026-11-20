import { describe, expect, it } from "vitest";
import { createWaveform, speechLevel } from "./waveform";

describe("voice bars", () => {
  it("rests on room noise and fills with speech", () => {
    expect(speechLevel(0)).toBe(0);
    expect(speechLevel(0.2)).toBe(0); // about −48 dBFS
    expect(speechLevel(0.5)).toBeGreaterThan(0.4); // about −30 dBFS
    expect(speechLevel(0.85)).toBe(1);
    expect(speechLevel(Number.NaN)).toBe(0);
  });

  it("creates its bars and records the latest level", () => {
    const root = document.createElement("div");
    const wave = createWaveform(root, 9, false);
    expect(root.children).toHaveLength(9);
    wave.push(0.75);
    expect(root.dataset.level).toBe("0.75");
    wave.reset();
    expect(root.dataset.level).toBe("0.00");
    for (const bar of Array.from(root.children) as HTMLElement[]) expect(bar.style.transform).toBe("scaleY(0.16)");
    wave.destroy();
  });

  it("does not move with reduced motion", () => {
    const root = document.createElement("div");
    const wave = createWaveform(root, 5, true);
    wave.push(0.9);
    for (const bar of Array.from(root.children) as HTMLElement[]) expect(bar.style.transform).toBe("scaleY(0.16)");
    wave.destroy();
  });
});
