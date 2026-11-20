// Réglages: every change is saved at once (save_settings) and applies to the
// next dictation; withdrawing consent applies immediately.

import { useEffect, useId, useState } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { AccessForm, AccessSummary } from "../components/AccessForm";
import { ChoiceGroup, FieldError, Panel, Switch } from "../components/controls";
import { MicPrivacyLink, MicrophonePicker } from "../components/Microphone";
import { RenderPreview } from "../components/RenderPreview";
import { PersonalRulesPanel } from "../components/rules/PersonalRules";
import { ShortcutField } from "../components/ShortcutField";
import { useBridge } from "../ipc/bridge";
import type { CommandError, IndicatorPosition, Settings, TakeOutcome } from "../ipc/contract";
import { commandErrorText, hasConsent, settingSaveErrorText } from "../labels/fr";
import { useAccess } from "../state/access";
import { useUiCommands } from "../state/commands";
import { useNavigation, type SettingsSection } from "../state/navigation";
import { useSettings, type SettingsField } from "../state/settings";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import { ScreenHeader } from "./ScreenHeader";
import "./screens.css";

const SECTIONS: { id: SettingsSection; title: string }[] = [
  { id: "dictation", title: "Dictée" },
  { id: "microphone", title: "Micro" },
  { id: "render", title: "Rendu du texte" },
  { id: "rules", title: "Mots et expressions" },
  { id: "destination", title: "Destination" },
  { id: "indicator", title: "Indicateur" },
  { id: "draft", title: "Brouillon" },
  { id: "startup", title: "Démarrage" },
  { id: "access", title: "Accès à Rambler" },
  { id: "privacy", title: "Confidentialité" },
];

const POSITIONS: { value: IndicatorPosition; label: string }[] = [
  { value: "bottom-center", label: "En bas de l’écran, au centre" },
  { value: "top-center", label: "En haut de l’écran, au centre" },
  { value: "near-cursor", label: "Près du curseur" },
];

export const sectionAnchor = (section: SettingsSection) => `settings-${section}`;

function saveErrorText(error: CommandError | undefined): string | undefined {
  return error ? settingSaveErrorText(error) : undefined;
}

function micAccessDenied(outcome: TakeOutcome | null): boolean {
  if (!outcome) return false;
  if (outcome.kind === "refused" && outcome.reason.kind === "microphone") {
    return outcome.reason.error.kind === "access-denied";
  }
  if (outcome.kind === "interrupted" && outcome.reason.kind === "microphone") {
    return outcome.reason.error.kind === "access-denied";
  }
  return false;
}

export function SettingsScreen({ section }: { section?: SettingsSection }) {
  const { settings, loadFailed, errors, update, reload } = useSettings();
  const state = useUiState();
  const loaded = settings !== null;

  useEffect(() => {
    if (!section || !loaded) return;
    const target = document.getElementById(sectionAnchor(section));
    target?.scrollIntoView?.({ block: "start" });
    target?.focus({ preventScroll: true });
  }, [section, loaded]);

  if (!settings) {
    return (
      <div className="screen">
        <ScreenHeader title="Réglages" />
        {loadFailed ? (
          <div className="stack">
            <FieldError>Les réglages n’ont pas pu être chargés.</FieldError>
            <div className="button-row">
              <button type="button" className="button" onClick={reload}>
                Réessayer
              </button>
            </div>
          </div>
        ) : (
          <p className="muted">Chargement des réglages…</p>
        )}
      </div>
    );
  }

  const set = (field: SettingsField, patch: (current: Settings) => Settings) => update(field, patch);
  const error = (field: SettingsField) => saveErrorText(errors[field]);

  return (
    <div className="screen screen--settings">
      <ScreenHeader title="Réglages">
        Chaque modification est enregistrée aussitôt et s’applique à la prochaine dictée. Désactiver la dictée avec
        Rambler prend effet immédiatement.
      </ScreenHeader>

      <nav className="settings-toc" aria-label="Sections des réglages">
        <ul>
          {SECTIONS.map((item) => (
            <li key={item.id}>
              <a
                href={`#${sectionAnchor(item.id)}`}
                onClick={(event) => {
                  event.preventDefault();
                  const target = document.getElementById(sectionAnchor(item.id));
                  target?.scrollIntoView?.({ block: "start" });
                  target?.focus({ preventScroll: true });
                }}
              >
                {item.title}
              </a>
            </li>
          ))}
        </ul>
      </nav>

      <Panel id={sectionAnchor("dictation")} title="Dictée">
        <ChoiceGroup
          legend="Langue"
          name="language"
          layout="row"
          value={settings.language}
          options={[
            { value: "french", label: "Français" },
            { value: "english", label: "Anglais" },
          ]}
          onChange={(language) => void set("language", (s) => ({ ...s, language }))}
          error={error("language")}
        />
        <ChoiceGroup
          legend="Geste"
          name="gesture"
          value={settings.gesture}
          options={[
            {
              value: "hold",
              label: "Maintenir pour parler",
              description: "Gardez le raccourci enfoncé pendant que vous parlez, relâchez pour terminer.",
            },
            {
              value: "toggle",
              label: "Bascule",
              description: "Appuyez une fois pour commencer, une seconde fois pour terminer.",
            },
          ]}
          onChange={(gesture) => void set("gesture", (s) => ({ ...s, gesture }))}
          error={error("gesture")}
        />
        <ShortcutField
          value={settings.shortcut}
          onChange={(shortcut) => set("shortcut", (s) => ({ ...s, shortcut }))}
          error={errors.shortcut}
        />
        <Switch
          label="Annuler avec Échap pendant la dictée"
          description="Échap n’est intercepté que pendant une dictée ; rien n’est inséré."
          checked={settings.cancelWithEscape}
          onChange={(cancelWithEscape) => void set("cancelWithEscape", (s) => ({ ...s, cancelWithEscape }))}
          error={error("cancelWithEscape")}
        />
      </Panel>

      <Panel id={sectionAnchor("microphone")} title="Micro">
        <MicrophonePicker />
        {micAccessDenied(state.lastOutcome) ? (
          <div className="stack">
            <FieldError>Windows bloque l’accès au micro pour les applications de bureau.</FieldError>
            <MicPrivacyLink />
          </div>
        ) : null}
      </Panel>

      <Panel id={sectionAnchor("render")} title="Rendu du texte">
        <ChoiceGroup
          legend="Profil"
          name="profile"
          value={settings.profile}
          options={[
            {
              value: "natural",
              label: "Naturel",
              description: "Ponctuation, espaces et majuscules légèrement corrigés, sans changer vos mots.",
            },
            {
              value: "faithful",
              label: "Fidèle",
              description: "La transcription de Rambler telle quelle, ponctuation comprise.",
            },
          ]}
          onChange={(profile) => void set("profile", (s) => ({ ...s, profile }))}
          error={error("profile")}
        />
        <fieldset className="subgroup">
          <legend className="subgroup__legend">Règles du profil Naturel</legend>
          {settings.profile === "faithful" ? (
            <p className="field-hint">Ces règles sont ignorées en Fidèle.</p>
          ) : null}
          <Switch
            label="Ponctuation légère"
            description="Complète la ponctuation simple, comme le point final."
            checked={settings.text.lightPunctuation}
            disabled={settings.profile === "faithful"}
            onChange={(lightPunctuation) =>
              void set("text", (s) => ({ ...s, text: { ...s.text, lightPunctuation } }))
            }
          />
          <Switch
            label="Espaces typographiques"
            description="Espaces insécables du français avant « : ; ! ? » et dans les guillemets."
            checked={settings.text.typographicSpaces}
            disabled={settings.profile === "faithful"}
            onChange={(typographicSpaces) =>
              void set("text", (s) => ({ ...s, text: { ...s.text, typographicSpaces } }))
            }
          />
          <Switch
            label="Majuscule de phrase"
            description="Met une majuscule au début de chaque phrase."
            checked={settings.text.sentenceCase}
            disabled={settings.profile === "faithful"}
            onChange={(sentenceCase) => void set("text", (s) => ({ ...s, text: { ...s.text, sentenceCase } }))}
          />
          {error("text") ? <FieldError>{error("text")}</FieldError> : null}
        </fieldset>
        <RenderPreview settings={settings} />
      </Panel>

      <PersonalRulesPanel id={sectionAnchor("rules")} settings={settings} />

      <Panel id={sectionAnchor("destination")} title="Destination">
        <Switch
          label="Toujours garder le texte dans Sambler"
          description="Le texte n’est jamais inséré dans le champ : il attend dans le brouillon, prêt à être copié."
          checked={settings.alwaysKeepInSambler}
          onChange={(alwaysKeepInSambler) =>
            void set("alwaysKeepInSambler", (s) => ({ ...s, alwaysKeepInSambler }))
          }
          error={error("alwaysKeepInSambler")}
        />
      </Panel>

      <Panel id={sectionAnchor("indicator")} title="Indicateur">
        <Switch
          label="Son de début et de fin"
          description="Un son discret signale le début de l’écoute et la fin de la dictée."
          checked={settings.indicator.sound}
          onChange={(sound) => void set("indicator", (s) => ({ ...s, indicator: { ...s.indicator, sound } }))}
        />
        <IndicatorPositionField
          value={settings.indicator.position}
          onChange={(position) =>
            void set("indicator", (s) => ({ ...s, indicator: { ...s.indicator, position } }))
          }
        />
        {error("indicator") ? <FieldError>{error("indicator")}</FieldError> : null}
      </Panel>

      <Panel id={sectionAnchor("draft")} title="Brouillon">
        <ChoiceGroup
          legend="Conserver le brouillon en mémoire pendant"
          name="draftMinutes"
          layout="row"
          value={settings.draftMinutes}
          options={[
            { value: 1, label: "1 minute" },
            { value: 5, label: "5 minutes" },
            { value: 10, label: "10 minutes" },
          ]}
          onChange={(draftMinutes) => void set("draftMinutes", (s) => ({ ...s, draftMinutes }))}
          description="Aucun audio n’est enregistré. Verrouiller Windows ou quitter Sambler efface le brouillon."
          error={error("draftMinutes")}
        />
        <ClearDraftNow />
      </Panel>

      <StartupPanel />

      <AccessPanel startOpen={section === "access"} />

      <PrivacyPanel />
    </div>
  );
}

function IndicatorPositionField({
  value,
  onChange,
}: {
  value: IndicatorPosition;
  onChange: (position: IndicatorPosition) => void;
}) {
  const id = useId();
  return (
    <div className="field">
      <label htmlFor={id} className="field__label">
        Position de l’indicateur
      </label>
      <select
        id={id}
        className="select select--auto"
        value={value}
        onChange={(event) => onChange(event.currentTarget.value as IndicatorPosition)}
      >
        {POSITIONS.map((position) => (
          <option key={position.value} value={position.value}>
            {position.label}
          </option>
        ))}
      </select>
    </div>
  );
}

function ClearDraftNow() {
  const state = useUiState();
  const commands = useUiCommands();
  const announce = useAnnounce();
  const action = useAction();
  return (
    <div className="stack stack--tight">
      <div className="button-row">
        <button
          type="button"
          className="button"
          disabled={!state.draft}
          onClick={async () => {
            const result = await action.run(commands.clearDraft);
            if (result.ok) announce(announcement("Brouillon effacé."));
          }}
        >
          Effacer maintenant
        </button>
        <span className="field-hint">{state.draft ? "Un texte est en attente." : "Aucun texte en attente."}</span>
      </div>
      {action.error ? <FieldError>{commandErrorText(action.error)}</FieldError> : null}
    </div>
  );
}

function AccessPanel({ startOpen }: { startOpen: boolean }) {
  const { status, loadFailed, clear, refresh } = useAccess();
  const announce = useAnnounce();
  const [editing, setEditing] = useState(false);
  const action = useAction();

  useEffect(() => {
    if (startOpen && status && !status.configured) setEditing(true);
  }, [startOpen, status]);

  return (
    <Panel
      id={sectionAnchor("access")}
      title="Accès à Rambler"
      description="Les données d’accès ne sont jamais réaffichées. Seule l’adresse du service est visible."
    >
      {status ? (
        <AccessSummary status={status} />
      ) : loadFailed ? (
        <div className="button-row">
          <FieldError>L’état de l’accès n’a pas pu être lu.</FieldError>
          <button type="button" className="button" onClick={refresh}>
            Réessayer
          </button>
        </div>
      ) : (
        <p className="muted">Chargement…</p>
      )}
      {status?.source === "environment" ? (
        <p className="field-hint">
          Ces valeurs viennent des variables d’environnement de Windows. Saisir un autre accès le remplace pour Sambler.
        </p>
      ) : null}
      {editing ? (
        <AccessForm
          onSaved={() => setEditing(false)}
          onCancel={status?.configured ? () => setEditing(false) : undefined}
          submitLabel={status?.configured ? "Remplacer l’accès" : "Enregistrer l’accès"}
        />
      ) : status ? (
        <div className="button-row">
          <button type="button" className="button" onClick={() => setEditing(true)}>
            {status.configured ? "Modifier l’accès" : "Configurer l’accès"}
          </button>
          {status.configured && status.source !== "environment" ? (
            <button
              type="button"
              className="button"
              onClick={async () => {
                const result = await action.run(clear);
                if (result.ok) announce(announcement("Accès à Rambler retiré."));
              }}
            >
              Retirer l’accès
            </button>
          ) : null}
        </div>
      ) : null}
      {action.error ? <FieldError>{commandErrorText(action.error)}</FieldError> : null}
    </Panel>
  );
}

function PrivacyPanel() {
  const state = useUiState();
  const { settings } = useSettings();
  const commands = useUiCommands();
  const nav = useNavigation();
  const announce = useAnnounce();
  const action = useAction();
  const consent = hasConsent(state.availability);

  return (
    <Panel id={sectionAnchor("privacy")} title="Confidentialité">
      <Switch
        label="Dictée avec Rambler (Google)"
        description={
          <>
            Votre voix est transmise à Google pendant que vous parlez, avec la langue et les données d’accès de la
            session. Le texte de vos documents, le titre des fenêtres et le presse-papiers restent sur ce PC. Désactiver
            arrête immédiatement toute dictée en cours.
          </>
        }
        checked={consent}
        onChange={async (enabled) => {
          const result = await action.run(() => commands.setConsent(enabled));
          if (result.ok) {
            announce(
              announcement(
                enabled ? "Dictée avec Rambler activée." : "Dictée avec Rambler désactivée. Plus rien n’est envoyé.",
              ),
            );
          }
        }}
        error={action.error ? commandErrorText(action.error) : undefined}
      />
      <p className="field-hint">
        Historique des dictées : {settings?.history.enabled ? "activé" : "désactivé"}.{" "}
        <button type="button" className="link-button" onClick={() => nav.go("history")}>
          Gérer l’historique
        </button>
      </p>
      <p>
        <button type="button" className="link-button" onClick={() => nav.openFirstRun("voice")}>
          Revoir la présentation et l’essai guidé
        </button>
      </p>
    </Panel>
  );
}

/** « Démarrer avec Windows » goes through `set_launch_at_startup` (installed edition only). */
function StartupPanel() {
  const bridge = useBridge();
  const { settings, applyLocal } = useSettings();
  const announce = useAnnounce();
  const action = useAction();
  const [portable, setPortable] = useState(false);
  if (!settings) return null;

  const onChange = async (enabled: boolean) => {
    const result = await action.run(() => bridge.invoke("set_launch_at_startup", { enabled }));
    if (result.ok) {
      applyLocal((s) => ({ ...s, launchAtStartup: enabled }));
      announce(announcement(enabled ? "Démarrage avec Windows activé." : "Démarrage avec Windows désactivé."));
    } else if (result.error.code === "portable") {
      setPortable(true);
    }
  };

  const error = portable
    ? "L’édition portable ne s’inscrit jamais au démarrage de ce PC : lancez Sambler depuis son dossier quand vous en avez besoin."
    : action.error
      ? commandErrorText(action.error, "Le démarrage avec Windows n’a pas pu être modifié.")
      : undefined;

  return (
    <Panel id={sectionAnchor("startup")} title="Démarrage">
      <Switch
        label="Démarrer avec Windows"
        description="Sambler s’ouvre à l’ouverture de session, discrètement dans la zone de notification. Désactivé par défaut."
        checked={settings.launchAtStartup}
        disabled={portable}
        onChange={(enabled) => void onChange(enabled)}
        error={error}
      />
    </Panel>
  );
}
