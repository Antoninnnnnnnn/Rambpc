// @vitest-environment node
// Every theme defines every semantic token (Sable is the reference: it has
// nothing private), keyed on data-theme / data-scheme, without media queries
// on the colour scheme.

// @ts-expect-error type error without @types/node package (as in vite.config.ts)
import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { THEME_IDS } from "./registry";

// Read from disk: Vitest serves CSS imports empty, even with `?raw`.
const sources: Record<string, string> = {};
for (const id of THEME_IDS) {
  const url = new URL(`./${id}/theme.css`, import.meta.url);
  if (existsSync(url)) sources[`./${id}/theme.css`] = readFileSync(url, "utf8").replace(/\r\n/g, "\n");
}

function block(css: string, selector: string): string {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) return "";
  return css.slice(start, css.indexOf("\n}", start));
}

const tokens = (css: string) => new Set([...css.matchAll(/^\s*(--[a-z0-9-]+)\s*:/gm)].map((match) => match[1]));

describe("theme contract", () => {
  const reference = tokens(block(sources["./sable/theme.css"], ':root[data-theme="sable"]'));

  it("has a theme.css for every registered theme", () => {
    for (const id of THEME_IDS) expect(sources[`./${id}/theme.css`], id).toBeTypeOf("string");
    expect(reference.size).toBeGreaterThan(80);
  });

  it.each(THEME_IDS)("%s defines every token, light and dark, without scheme media queries", (id) => {
    const css = sources[`./${id}/theme.css`];
    const light = tokens(block(css, `:root[data-theme="${id}"]`));
    const dark = tokens(block(css, `:root[data-theme="${id}"][data-scheme="dark"]`));
    const missing = [...reference].filter((token) => !light.has(token));
    expect(missing).toEqual([]);
    expect(dark.size).toBeGreaterThan(10);
    // The dark block only redefines tokens the light block declares.
    expect([...dark].filter((token) => !light.has(token))).toEqual([]);
    expect(css).not.toMatch(/prefers-color-scheme/);
    expect(css).toMatch(/color-scheme: light;/);
    expect(css).toMatch(/color-scheme: dark;/);
  });
});
