// Réglages > Apparence: the theme (cards with a small preview drawn from the
// theme's manifest) and the scheme (Système, Clair, Sombre). Both are native
// radio groups: arrow keys move between options. Changes apply at once.

import { useId, type CSSProperties } from "react";
import { settingSaveErrorText } from "../labels/fr";
import type { Scheme } from "../themes/appearance";
import { useAppearance } from "../themes/AppearanceProvider";
import { THEME_IDS, THEMES, type ThemeId, type ThemeManifest } from "../themes/registry";
import { Card, ChoiceGroup, FieldError } from "./controls";
import { Glyph } from "./Icon";
import "./AppearanceSettings.css";

const SCHEMES = [
  { value: "system", label: "Système" },
  { value: "light", label: "Clair" },
  { value: "dark", label: "Sombre" },
] as const;

/** Where each ambient colour of a preview glows from. */
const GLOW_SPOTS = ["8% 100%", "58% 112%", "100% 0%", "26% -8%"];

export function AppearanceSettings({ id }: { id: string }) {
  const { appearance, scheme, error, setAppearance } = useAppearance();
  return (
    <Card id={id} title="Apparence" description="Le thème et le mode s’appliquent tout de suite, à toute l’application.">
      <ThemePicker value={appearance.theme} scheme={scheme} onChange={(theme) => void setAppearance({ theme })} />
      <ChoiceGroup
        legend="Mode"
        name="colorScheme"
        variant="segmented"
        value={appearance.colorScheme}
        options={SCHEMES.map((option) => ({ ...option }))}
        description={
          appearance.colorScheme === "system"
            ? `Suit le mode de votre ordinateur : ${scheme === "dark" ? "sombre" : "clair"} en ce moment.`
            : undefined
        }
        onChange={(colorScheme) => void setAppearance({ colorScheme })}
      />
      {error ? <FieldError>{settingSaveErrorText(error)}</FieldError> : null}
    </Card>
  );
}

function ThemePicker({
  value,
  scheme,
  onChange,
}: {
  value: ThemeId;
  scheme: Scheme;
  onChange: (theme: ThemeId) => void;
}) {
  const id = useId();
  return (
    <fieldset className="choices theme-picker">
      <legend className="choices__legend">Thème</legend>
      <div className="theme-picker__options">
        {THEME_IDS.map((themeId) => {
          const theme = THEMES[themeId];
          const optionId = `${id}-${themeId}`;
          return (
            <label key={themeId} className="theme-option" htmlFor={optionId}>
              <input
                id={optionId}
                className="theme-option__input"
                type="radio"
                name="theme"
                value={themeId}
                checked={value === themeId}
                aria-labelledby={`${optionId}-label`}
                aria-describedby={`${optionId}-desc`}
                onChange={() => onChange(themeId)}
              />
              <span className="theme-option__body">
                <ThemePreview theme={theme} scheme={scheme} />
                <span className="theme-option__text">
                  <span id={`${optionId}-label`} className="theme-option__label">
                    {theme.label}
                  </span>
                  <span id={`${optionId}-desc`} className="theme-option__desc">
                    {theme.description}
                  </span>
                </span>
                <span className="theme-option__check" aria-hidden="true">
                  <Glyph name="check" />
                </span>
              </span>
            </label>
          );
        })}
      </div>
    </fieldset>
  );
}

/**
 * A tiny window in the theme's own colours (from its manifest, whatever the
 * active theme): rail, sheet, a title, two lines and a button, over the
 * theme's ambient glow if it has one. Decorative.
 */
function ThemePreview({ theme, scheme }: { theme: ThemeManifest; scheme: Scheme }) {
  const colors = theme.swatches[scheme];
  const [background, surface = background, text = surface, accent = text, extra = accent] = colors;
  const glow = theme.glow?.[scheme] ?? [];
  const style = {
    "--preview-bg": background,
    "--preview-surface": surface,
    "--preview-text": text,
    "--preview-accent": accent,
    "--preview-extra": extra,
    backgroundImage: glow.length
      ? glow
          .map((color, index) => `radial-gradient(70% 90% at ${GLOW_SPOTS[index % GLOW_SPOTS.length]}, ${color}, transparent)`)
          .join(", ")
      : undefined,
  } as CSSProperties;
  return (
    <span className="theme-preview" style={style} data-glass={glow.length ? "" : undefined} aria-hidden="true">
      <span className="theme-preview__window">
        <span className="theme-preview__rail">
          <i />
          <i />
          <i />
        </span>
        <span className="theme-preview__sheet">
          <i className="theme-preview__title" />
          <i className="theme-preview__line" />
          <i className="theme-preview__line theme-preview__line--short" />
          <i className="theme-preview__button" />
        </span>
      </span>
      <span className="theme-preview__swatches">
        {colors.map((color, index) => (
          <i key={`${color}-${index}`} style={{ background: color }} />
        ))}
      </span>
    </span>
  );
}
