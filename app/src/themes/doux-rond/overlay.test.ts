// @vitest-environment node
// The Doux et rond capsule skin, read as text (Vitest serves CSS imports
// empty): custom properties only, so the structure's reduced-motion and
// forced-colours rules always win; a tint for every state the bar shows;
// a capsule and its shadow inside the 360 x 72 window; both schemes.

// @ts-expect-error type error without @types/node package (as in vite.config.ts)
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const skin = read("./overlay.css").replace(/\/\*[\s\S]*?\*\//g, "");
const structure = read("../../overlay/overlay.css");

/** `selector { body }` pairs outside @keyframes. */
function rules(css: string): { selector: string; body: string }[] {
  const withoutKeyframes = css.replace(/@keyframes[^{]+\{(?:[^{}]*\{[^{}]*\})*[^{}]*\}/g, "");
  return [...withoutKeyframes.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((match) => ({
    selector: match[1].trim(),
    body: match[2],
  }));
}

const declarations = (body: string) =>
  body
    .split(";")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => part.slice(0, part.indexOf(":")).trim());

describe("Doux et rond capsule skin", () => {
  const all = rules(skin);

  it("only sets custom properties, under the theme", () => {
    expect(all.length).toBeGreaterThan(5);
    for (const { selector, body } of all) {
      expect(selector).toMatch(/^:root\[data-theme="doux-rond"\]/);
      expect(declarations(body).filter((name) => !name.startsWith("--"))).toEqual([]);
    }
  });

  it("gives every state its colour: pêche listening, lilas processing, menthe, beurre, rose, brume", () => {
    const tinted = (selector: string) => all.find((rule) => rule.selector.includes(selector))?.body ?? "";
    expect(tinted('.bar[data-kind="finalizing"]')).toMatch(/--cap-p1:\s*var\(--cap-lilas-1\)/);
    expect(tinted('.bar[data-meaning="inserted"]')).toMatch(/--cap-p1:\s*var\(--cap-menthe-1\)/);
    expect(tinted('.bar[data-meaning="kept"]')).toMatch(/--cap-p1:\s*var\(--cap-beurre-1\)/);
    expect(tinted('.bar[data-meaning="refused"]')).toMatch(/--cap-p1:\s*var\(--cap-rose-1\)/);
    expect(all.find((rule) => rule.selector.includes('.bar[data-meaning="error"]'))?.body).toMatch(/--cap-rose-1/);
    expect(tinted('.bar[data-meaning="neutral"]')).toMatch(/--cap-p1:\s*var\(--cap-brume-1\)/);
    // Listening is the default tint, set on the bar itself.
    expect(tinted(':root[data-theme="doux-rond"] .bar')).toMatch(/--cap-p1:\s*var\(--cap-voix-1\)/);
  });

  it("redefines its palette for the dark scheme", () => {
    const dark = all.find((rule) => rule.selector === ':root[data-theme="doux-rond"][data-scheme="dark"]');
    expect(dark?.body).toMatch(/--cap-fond:/);
    expect(dark?.body).toMatch(/--cap-encre:/);
  });

  it("fits the capsule and its shadow in the 360 x 72 window", () => {
    const root = all.find((rule) => rule.selector === ':root[data-theme="doux-rond"]')!.body;
    const [top, sides, bottom] = /--bar-inset:\s*([\d.]+)(?:px)?\s+([\d.]+)px\s+([\d.]+)px/.exec(root)!.slice(1).map(Number);
    expect(top).toBe(0);
    // The pill is 44 px high (overlay.css).
    expect(structure).toMatch(/height: 44px;/);
    expect(44 + bottom).toBeLessThanOrEqual(72);
    // The farthest shadow (offset y, blur, spread) stays inside the window's bottom and sides.
    const shadows = /--bar-shadow:([^;]+);/.exec(root)![1];
    const drops = [...shadows.matchAll(/(?:^|,)\s*0 ([\d.-]+)px ([\d.]+)px(?: ([\d.-]+)px)?/g)];
    expect(drops.length).toBeGreaterThan(0);
    for (const [, y, blur, spread = "0"] of drops) {
      expect(Number(y) + Number(blur) + Number(spread)).toBeLessThanOrEqual(bottom);
      expect(Number(blur) + Number(spread)).toBeLessThanOrEqual(sides);
    }
  });

  it("lets the structure stop the pebbles with reduced motion and hide them in forced colours", () => {
    const reduced = structure.slice(structure.indexOf("@media (prefers-reduced-motion: reduce)"));
    expect(reduced).toMatch(/\.bar__icon::before,\s*\n\s*\.bar__icon::after,/);
    const forced = structure.slice(structure.indexOf("@media (forced-colors: active)"));
    expect(forced).toMatch(/\.bar__icon::after \{\s*display: none;/);
  });
});
