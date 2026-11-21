// Visual themes. A theme is data (this manifest) plus CSS keyed on
// `:root[data-theme="<id>"]` and `[data-scheme="dark"]`, and optionally a few
// theme-owned pieces (a backdrop, an overlay skin). Pure data: no React, no
// DOM, so the floating bar can import it too. See README.md in this folder.

export const THEME_IDS = ["verre-aurore", "sable"] as const;
export type ThemeId = (typeof THEME_IDS)[number];

export const DEFAULT_THEME: ThemeId = "verre-aurore";

export interface ThemeManifest {
  id: ThemeId;
  /** Name shown in Réglages > Apparence. */
  label: string;
  /** One short sentence under the name. */
  description: string;
  /**
   * 3 to 5 colours for the picker preview, per scheme. By position:
   * background, surface (sheet), text, accent, then an optional second accent.
   */
  swatches: { light: readonly string[]; dark: readonly string[] };
  /** Optional ambient colours drawn behind the preview (e.g. the aurora). */
  glow?: { light: readonly string[]; dark: readonly string[] };
}

export const THEMES: Record<ThemeId, ThemeManifest> = {
  "verre-aurore": {
    id: "verre-aurore",
    label: "Verre et aurore",
    description: "Du verre dépoli sur une aube lente, qui s’éclaire quand vous parlez.",
    swatches: {
      light: ["#f4ece6", "#fffaf6", "#2a1c21", "#b4461a", "#3cc4a2"],
      dark: ["#100a10", "#241820", "#f7ede7", "#ff9a6a", "#6edbbf"],
    },
    glow: {
      light: ["#ffb08a", "#ffd27a", "#a6e2cf", "#f6b0c2"],
      dark: ["#c4471e", "#d48a26", "#138470", "#93294f"],
    },
  },
  sable: {
    id: "sable",
    label: "Sable",
    description: "Sobre et clair : tons sable, encre et un seul bleu.",
    swatches: {
      light: ["#f4f2ee", "#ffffff", "#18191d", "#2357e0"],
      dark: ["#0f1013", "#17181c", "#edeef0", "#7da6ff"],
    },
  },
};

export function isThemeId(value: unknown): value is ThemeId {
  return typeof value === "string" && (THEME_IDS as readonly string[]).includes(value);
}
