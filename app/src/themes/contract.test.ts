// @vitest-environment node
// Every theme defines every semantic token (Verre et aurore is the reference,
// minus its private `--va-*` variables), keyed on data-theme / data-scheme,
// without media queries on the colour scheme. Component CSS uses tokens only.

// @ts-expect-error type error without @types/node package (as in vite.config.ts)
import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { THEME_IDS } from "./registry";

// Read from disk: Vitest serves CSS imports empty, even with `?raw`.
function read(path: string): string | undefined {
  const url = new URL(path, import.meta.url);
  return existsSync(url) ? readFileSync(url, "utf8").replace(/\r\n/g, "\n") : undefined;
}

const sources: Record<string, string> = {};
for (const id of THEME_IDS) {
  const css = read(`./${id}/theme.css`);
  if (css !== undefined) sources[id] = css;
}

function block(css: string, selector: string): string {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) return "";
  return css.slice(start, css.indexOf("\n}", start));
}

const tokens = (css: string) => new Set([...css.matchAll(/^\s*(--[a-z0-9-]+)\s*:/gm)].map((match) => match[1]));

/** Private variables of a theme: only its own CSS uses them. */
const PRIVATE = /^--(va|dr)-/;

describe("theme contract", () => {
  const reference = new Set(
    [...tokens(block(sources["verre-aurore"] ?? "", ':root[data-theme="verre-aurore"]'))].filter(
      (token) => !PRIVATE.test(token),
    ),
  );

  it("has a theme.css for every registered theme", () => {
    for (const id of THEME_IDS) expect(sources[id], id).toBeTypeOf("string");
    expect(reference.size).toBeGreaterThan(80);
  });

  it.each(THEME_IDS)("%s defines every token, light and dark, without scheme media queries", (id) => {
    const css = sources[id];
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

  // Colours live in theme.css (and a bar skin's palette); a theme's
  // component CSS, its slots and its preview use tokens only.
  const styling: [string, string][] = [];
  for (const id of THEME_IDS) {
    for (const file of ["components.css", "galets.css", "preview.css"]) {
      const css = read(`./${id}/${file}`);
      if (css !== undefined) styling.push([`${id}/${file}`, css]);
    }
  }

  it.each(styling)("%s has no literal colour", (_file, css) => {
    const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
    expect(withoutComments).not.toMatch(/#[0-9a-f]{3,8}\b|\b(rgb|rgba|hsl|hsla|oklch|lab)\(/i);
  });
});
