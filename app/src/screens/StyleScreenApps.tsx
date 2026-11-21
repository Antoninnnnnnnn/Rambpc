// Style > Par application: a style of its own for some applications. Each
// field is either left to the general setting (omitted from the `AppStyle`)
// or forced on/off; the native side applies only the given fields when a
// take's target is that application. Saved with the settings store like any
// other setting (`appStyles`, at most 50, one per application id).
//
// Applications are proposed from `get_known_apps` (the ones already dictated
// into). Degrades when the native side predates per-app styles (no
// `appStyles` field) or the list cannot be read.

import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { Card, ChoiceGroup, FieldError } from "../components/controls";
import { Glyph } from "../components/Icon";
import { RenderPreview } from "../components/RenderPreview";
import { useBridge } from "../ipc/bridge";
import type { AppIdentity, AppStyle, RenderProfile, Settings, TextOptions } from "../ipc/contract";
import { settingSaveErrorText } from "../labels/fr";
import {
  APP_STYLES_EMPTY,
  APP_STYLES_FULL,
  APP_STYLES_INTRO,
  APP_STYLES_UNSUPPORTED,
  appStyleSummary,
  inheritLabel,
  KNOWN_APPS_ALL_SET,
  KNOWN_APPS_EMPTY,
  KNOWN_APPS_FAILED,
  MAX_APP_STYLES,
  onOff,
  PROFILE_LABELS,
  TEXT_OPTION_KEYS,
  TEXT_OPTION_LABELS,
  triState,
  triStateValue,
  type TextOptionKey,
  type TriState,
} from "../labels/style";
import { useSettings } from "../state/settings";

// ------------------------------------------------------------------ pure helpers

/** The general settings with one application's overrides applied: what a take there uses. */
export function settingsForApp(settings: Settings, style: AppStyle): Settings {
  const text: TextOptions = { ...settings.text };
  for (const key of TEXT_OPTION_KEYS) {
    const value = style.text?.[key];
    if (value !== undefined) text[key] = value;
  }
  return { ...settings, profile: style.profile ?? settings.profile, text };
}

/** Replace (or drop, with null) the style of one application. */
function changeStyle(settings: Settings, appId: string, change: (style: AppStyle) => AppStyle | null): Settings {
  return {
    ...settings,
    appStyles: (settings.appStyles ?? []).flatMap((style) => {
      if (style.app.id !== appId) return [style];
      const next = change(style);
      return next ? [next] : [];
    }),
  };
}

function withProfile(style: AppStyle, profile: RenderProfile | undefined): AppStyle {
  const { profile: _previous, ...rest } = style;
  return profile ? { ...rest, profile } : rest;
}

function withOption(style: AppStyle, key: TextOptionKey, value: boolean | undefined): AppStyle {
  const text: Partial<TextOptions> = { ...style.text };
  if (value === undefined) delete text[key];
  else text[key] = value;
  const { text: _previous, ...rest } = style;
  return Object.keys(text).length > 0 ? { ...rest, text } : rest;
}

function initial(name: string): string {
  return Array.from(name.trim())[0]?.toUpperCase() ?? "?";
}

// ------------------------------------------------------------------ card

export function AppStylesCard({ sample }: { sample: string }) {
  const bridge = useBridge();
  const announce = useAnnounce();
  const { settings, errors, update } = useSettings();
  const [known, setKnown] = useState<AppIdentity[] | null>(null);
  const [knownFailed, setKnownFailed] = useState(false);
  const [picking, setPicking] = useState(false);
  const [open, setOpen] = useState<ReadonlySet<string>>(() => new Set());
  const toggles = useRef(new Map<string, HTMLButtonElement>());
  const addRef = useRef<HTMLButtonElement>(null);
  const pendingFocus = useRef<string | null>(null);
  const id = useId();

  const loadKnown = useCallback(() => {
    setKnownFailed(false);
    bridge
      .invoke("get_known_apps")
      .then(setKnown)
      .catch(() => setKnownFailed(true));
  }, [bridge]);

  useEffect(loadKnown, [loadKnown]);

  const styles = settings?.appStyles;

  // After an addition or a removal, the focus goes to the application's
  // header, the next one, or « Ajouter une application ».
  useEffect(() => {
    const target = pendingFocus.current;
    if (target === null) return;
    pendingFocus.current = null;
    (target === "" ? addRef.current : toggles.current.get(target))?.focus();
  }, [styles, picking]);

  if (!settings) return null;

  if (!styles) {
    return (
      <Card title="Par application" description={APP_STYLES_INTRO} className="apps-card" testId="app-styles">
        <p className="style-note">
          <Glyph name="apps" />
          <span>{APP_STYLES_UNSUPPORTED}</span>
        </p>
      </Card>
    );
  }

  const full = styles.length >= MAX_APP_STYLES;
  const available = (known ?? []).filter((app) => !styles.some((style) => style.app.id === app.id));
  const pickerId = `${id}-picker`;

  const setOpenFor = (appId: string, value: boolean) =>
    setOpen((current) => {
      const next = new Set(current);
      if (value) next.add(appId);
      else next.delete(appId);
      return next;
    });

  const closePicker = (refocus: boolean) => {
    if (refocus) pendingFocus.current = "";
    setPicking(false);
  };

  const add = async (app: AppIdentity) => {
    setOpenFor(app.id, true);
    pendingFocus.current = app.id;
    setPicking(false);
    const ok = await update("appStyles", (current) => {
      const others = (current.appStyles ?? []).filter((style) => style.app.id !== app.id);
      return others.length >= MAX_APP_STYLES ? current : { ...current, appStyles: [...others, { app }] };
    });
    if (ok) announce(announcement(`${app.name} a maintenant son propre style.`));
  };

  const remove = async (app: AppIdentity, index: number) => {
    const neighbour = styles[index + 1] ?? styles[index - 1];
    pendingFocus.current = neighbour ? neighbour.app.id : "";
    setOpenFor(app.id, false);
    const ok = await update("appStyles", (current) => ({
      ...current,
      appStyles: (current.appStyles ?? []).filter((style) => style.app.id !== app.id),
    }));
    if (ok) announce(announcement(`${app.name} suit de nouveau le réglage général.`));
  };

  return (
    <Card title="Par application" description={APP_STYLES_INTRO} className="apps-card" testId="app-styles">
      {styles.length > 0 ? (
        <ul className="app-styles">
          {styles.map((style, index) => (
            <AppStyleItem
              key={style.app.id}
              style={style}
              settings={settings}
              sample={sample}
              open={open.has(style.app.id)}
              panelId={`${id}-panel-${index}`}
              toggleRef={(element) => {
                if (element) toggles.current.set(style.app.id, element);
                else toggles.current.delete(style.app.id);
              }}
              onToggle={() => setOpenFor(style.app.id, !open.has(style.app.id))}
              onRemove={() => void remove(style.app, index)}
              onChange={(change) => void update("appStyles", (current) => changeStyle(current, style.app.id, change))}
            />
          ))}
        </ul>
      ) : (
        <p className="app-styles__empty">{APP_STYLES_EMPTY}</p>
      )}

      <div className="app-add">
        <div className="btn-row">
          <button
            ref={addRef}
            type="button"
            className="btn btn--sm"
            aria-expanded={picking}
            aria-controls={picking ? pickerId : undefined}
            disabled={full}
            onClick={() => {
              if (!picking) loadKnown();
              setPicking(!picking);
            }}
          >
            <Glyph name="add" />
            Ajouter une application
          </button>
          {full ? <p className="hint">{APP_STYLES_FULL}</p> : null}
        </div>
        {picking ? (
          <AppPicker
            id={pickerId}
            known={known}
            failed={knownFailed && known === null}
            available={available}
            onPick={(app) => void add(app)}
            onRetry={loadKnown}
            onClose={() => closePicker(true)}
          />
        ) : null}
      </div>

      {errors.appStyles ? <FieldError>{settingSaveErrorText(errors.appStyles)}</FieldError> : null}
    </Card>
  );
}

// ------------------------------------------------------------------ picker

interface PickerProps {
  id: string;
  known: AppIdentity[] | null;
  failed: boolean;
  available: AppIdentity[];
  onPick: (app: AppIdentity) => void;
  onRetry: () => void;
  onClose: () => void;
}

function AppPicker({ id, known, failed, available, onPick, onRetry, onClose }: PickerProps) {
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  };

  return (
    <div id={id} className="app-picker" role="group" aria-labelledby={`${id}-title`} onKeyDown={onKeyDown}>
      <p id={`${id}-title`} className="app-picker__title">
        Choisissez une application
      </p>
      {failed ? (
        <div className="btn-row">
          <FieldError>{KNOWN_APPS_FAILED}</FieldError>
          <button type="button" className="btn btn--sm" onClick={onRetry}>
            Réessayer
          </button>
        </div>
      ) : known === null ? (
        <p className="hint">Lecture des applications…</p>
      ) : known.length === 0 ? (
        <p className="hint">{KNOWN_APPS_EMPTY}</p>
      ) : available.length === 0 ? (
        <p className="hint">{KNOWN_APPS_ALL_SET}</p>
      ) : (
        <ul className="app-picker__list">
          {available.map((app) => (
            <li key={app.id}>
              <button type="button" className="btn btn--sm app-picker__app" onClick={() => onPick(app)}>
                <span className="app-mono" aria-hidden="true">
                  {initial(app.name)}
                </span>
                {app.name}
              </button>
            </li>
          ))}
        </ul>
      )}
      <div>
        <button type="button" className="btn btn--sm btn--ghost" onClick={onClose}>
          Annuler
        </button>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ one application

interface ItemProps {
  style: AppStyle;
  settings: Settings;
  sample: string;
  open: boolean;
  panelId: string;
  toggleRef: (element: HTMLButtonElement | null) => void;
  onToggle: () => void;
  onRemove: () => void;
  onChange: (change: (style: AppStyle) => AppStyle) => void;
}

function AppStyleItem({ style, settings, sample, open, panelId, toggleRef, onToggle, onRemove, onChange }: ItemProps) {
  const app = style.app;
  const merged = settingsForApp(settings, style);
  const faithful = merged.profile === "faithful";
  // Older native sides have no dictation commands: nothing to override.
  const keys = TEXT_OPTION_KEYS.filter((key) => typeof settings.text[key] === "boolean");

  return (
    <li className={open ? "app-style app-style--open" : "app-style"} data-testid={`app-style-${app.id}`}>
      <div className="app-style__header">
        <span className="app-mono" aria-hidden="true">
          {initial(app.name)}
        </span>
        <h3 className="app-style__title">
          <button
            ref={toggleRef}
            type="button"
            className="app-style__toggle"
            aria-expanded={open}
            aria-controls={open ? panelId : undefined}
            onClick={onToggle}
          >
            <span className="app-style__name">{app.name}</span>{" "}
            <span className="app-style__summary">{appStyleSummary(style)}</span>
            <Glyph name="chevron-down" />
          </button>
        </h3>
        <button type="button" className="btn btn--sm btn--ghost app-style__remove" onClick={onRemove}>
          Retirer<span className="visually-hidden"> {app.name}</span>
        </button>
      </div>

      {open ? (
        <div id={panelId} className="app-style__panel">
          <ChoiceGroup<RenderProfile | "inherit">
            legend="Profil"
            name={`${panelId}-profile`}
            variant="segmented"
            value={style.profile ?? "inherit"}
            options={[
              { value: "inherit", label: inheritLabel(PROFILE_LABELS[settings.profile]) },
              { value: "natural", label: PROFILE_LABELS.natural },
              { value: "faithful", label: PROFILE_LABELS.faithful },
            ]}
            onChange={(value) => onChange((current) => withProfile(current, value === "inherit" ? undefined : value))}
          />
          {faithful ? <p className="hint app-style__note">En Fidèle, les règles ci-dessous sont ignorées pour {app.name}.</p> : null}
          {keys.map((key) => (
            <ChoiceGroup<TriState>
              key={key}
              legend={TEXT_OPTION_LABELS[key]}
              name={`${panelId}-${key}`}
              variant="segmented"
              value={triState(style.text?.[key])}
              disabled={faithful}
              options={[
                { value: "inherit", label: inheritLabel(onOff(Boolean(settings.text[key]))) },
                { value: "on", label: "Avec" },
                { value: "off", label: "Sans" },
              ]}
              onChange={(value) => onChange((current) => withOption(current, key, triStateValue(value)))}
            />
          ))}
          <RenderPreview
            settings={merged}
            sample={sample}
            compact
            labels={{ raw: "Reconnu", result: `Inséré dans ${app.name}` }}
            testId={`app-preview-${app.id}`}
          />
        </div>
      ) : null}
    </li>
  );
}
