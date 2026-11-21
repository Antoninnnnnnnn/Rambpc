// Visual themes. A theme is data (this manifest) plus CSS keyed on
// `:root[data-theme="<id>"]` and `[data-scheme="dark"]`, and optionally a few
// theme-owned pieces (a backdrop, an overlay skin). Pure data: no React, no
// DOM, so the floating bar can import it too. See README.md in this folder.
//
// An id that is no longer here (« sable », removed in milestone 5) may still
// sit in the native settings or the cache: `isThemeId` rejects it and the
// appearance falls back to DEFAULT_THEME.

export const THEME_IDS = ["verre-aurore", "doux-rond"] as const;
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
  "doux-rond": {
    id: "doux-rond",
    label: "Doux et rond",
    description: "Des galets pêche et lilas qui respirent avec votre voix, sur une feuille de lait.",
    // Iris (window), lait (sheet), encre, pêche (voice and action), lilas.
    swatches: {
      light: ["#f1edf8", "#ffffff", "#2a2342", "#ffa584", "#8a76e6"],
      dark: ["#1a1631", "#231d3e", "#f5f1ff", "#ffad8e", "#a795ff"],
    },
    // The soft desktop the pebble window rests on, in the preview only.
    glow: {
      light: ["#cfede2", "#dcd0fa", "#dcd0fa", "#ffdccb"],
      dark: ["#0f3a3c", "#22205a", "#22205a", "#3a1f45"],
    },
  },
};

export function isThemeId(value: unknown): value is ThemeId {
  return typeof value === "string" && (THEME_IDS as readonly string[]).includes(value);
}
