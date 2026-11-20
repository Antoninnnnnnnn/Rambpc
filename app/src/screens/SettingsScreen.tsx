// Réglages: every change is saved at once (save_settings) and applies to the
// next dictation; withdrawing consent applies immediately.

import { useEffect, useId, useState } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { AccessForm, AccessSummary } from "../components/AccessForm";
import { Card, ChoiceGroup, FieldError, Row, Switch } from "../components/controls";
import { Glyph } from "../components/Icon";
import { MicPrivacyLink, MicrophonePicker } from "../components/Microphone";
import { ShortcutField } from "../components/ShortcutField";
import { useBridge } from "../ipc/bridge";
import type { CommandError, IndicatorPosition, Settings, TakeOutcome } from "../ipc/contract";
import {
  commandErrorText,
  hasConsent,
  INSERTION_DESCRIPTIONS,
  INSERTION_LABELS,
  INSERTION_TARGET_DESCRIPTIONS,
  INSERTION_TARGET_LABELS,
  PASSWORD_NEVER_TEXT,
  settingSaveErrorText,
} from "../labels/fr";
import { useAccess } from "../state/access";
import { useUiCommands } from "../state/commands";
import { useNavigation, type SettingsSection } from "../state/navigation";
import { useSettings, type SettingsField } from "../state/settings";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import { ScreenHeader } from "./ScreenHeader";
import "./SettingsScreen.css";

const SECTIONS: { id: SettingsSection; title: string }[] = [
  { id: "microphone", title: "Micro" },
  { id: "shortcut", title: "Raccourci" },
  { id: "language", title: "Langue" },
  { id: "insertion", title: "Insertion" },
  { id: "destination", title: "Destination" },
  { id: "indicator", title: "Barre de dictée" },
  { id: "recents", title: "Récents" },
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

function jumpTo(section: SettingsSection) {
  const target = document.getElementById(sectionAnchor(section));
  target?.scrollIntoView?.({ block: "start" });
  target?.focus({ preventScroll: true });
}

export function SettingsScreen({ section }: { section?: SettingsSection }) {
  const { settings, loadFailed, errors, update, reload } = useSettings();
  const state = useUiState();
  const loaded = settings !== null;

  useEffect(() => {
    if (section && loaded) jumpTo(section);
  }, [section, loaded]);

  if (!settings) {
    return (
      <div className="screen">
        <ScreenHeader title="Réglages" />
        {loadFailed ? (
          <div className="stack stack--tight">
            <FieldError>Les réglages n’ont pas pu être chargés.</FieldError>
            <div className="btn-row">
              <button type="button" className="btn" onClick={reload}>
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
    <div className="screen settings">
      <ScreenHeader title="Réglages">
        Chaque modification est enregistrée aussitôt et s’applique à la prochaine dictée. Désactiver la dictée avec
        Rambler prend effet immédiatement.
      </ScreenHeader>

      <nav className="settings__toc" aria-label="Sections des réglages">
        <ul>
          {SECTIONS.map((item) => (
            <li key={item.id}>
              <a
                href={`#${sectionAnchor(item.id)}`}
                onClick={(event) => {
                  event.preventDefault();
                  jumpTo(item.id);
                }}
              >
                {item.title}
              </a>
            </li>
          ))}
        </ul>
      </nav>

      <Card id={sectionAnchor("microphone")} title="Micro">
        <MicrophonePicker />
        {micAccessDenied(state.lastOutcome) ? (
          <div className="stack stack--tight">
            <FieldError>Windows bloque l’accès au micro pour les applications de bureau.</FieldError>
            <MicPrivacyLink />
          </div>
        ) : null}
      </Card>

      <Card id={sectionAnchor("shortcut")} title="Raccourci et geste">
        <ShortcutField
          value={settings.shortcut}
          onChange={(shortcut) => set("shortcut", (s) => ({ ...s, shortcut }))}
          error={errors.shortcut}
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
        <Switch
          label="Annuler avec Échap pendant la dictée"
          description="Échap n’est intercepté que pendant une dictée ; rien n’est inséré."
          checked={settings.cancelWithEscape}
          onChange={(cancelWithEscape) => void set("cancelWithEscape", (s) => ({ ...s, cancelWithEscape }))}
          error={error("cancelWithEscape")}
        />
      </Card>

      <Card id={sectionAnchor("language")} title="Langue" description="La langue que vous parlez, figée au début de chaque dictée.">
        <ChoiceGroup
          legend="Langue de dictée"
          hideLegend
          name="language"
          variant="segmented"
          value={settings.language}
          options={[
            { value: "french", label: "Français" },
            { value: "english", label: "Anglais" },
          ]}
          onChange={(language) => void set("language", (s) => ({ ...s, language }))}
          error={error("language")}
        />
      </Card>

      <Card
        id={sectionAnchor("insertion")}
        title="Insertion"
        description="Comment le texte arrive dans les navigateurs, Outlook et les applications web. Le Bloc-notes, WordPad et Word le reçoivent toujours directement."
      >
        <ChoiceGroup
          legend="Méthode d’insertion"
          hideLegend
          name="insertion"
          value={settings.insertion}
          options={[
            { value: "paste", label: INSERTION_LABELS.paste, description: INSERTION_DESCRIPTIONS.paste },
            { value: "typing", label: INSERTION_LABELS.typing, description: INSERTION_DESCRIPTIONS.typing },
          ]}
          onChange={(insertion) => void set("insertion", (s) => ({ ...s, insertion }))}
          error={error("insertion")}
        />
      </Card>

      <Card id={sectionAnchor("destination")} title="Destination">
        <ChoiceGroup
          legend="Où insérer le texte"
          name="insertionTarget"
          value={settings.insertionTarget}
          disabled={settings.alwaysKeepInSambler}
          description={
            settings.alwaysKeepInSambler
              ? `Sans effet tant que le texte est toujours gardé dans Sambler. ${PASSWORD_NEVER_TEXT}`
              : PASSWORD_NEVER_TEXT
          }
          options={[
            {
              value: "original-field",
              label: INSERTION_TARGET_LABELS["original-field"],
              description: INSERTION_TARGET_DESCRIPTIONS["original-field"],
            },
            {
              value: "cursor-at-end",
              label: INSERTION_TARGET_LABELS["cursor-at-end"],
              description: INSERTION_TARGET_DESCRIPTIONS["cursor-at-end"],
            },
          ]}
          onChange={(insertionTarget) => void set("insertionTarget", (s) => ({ ...s, insertionTarget }))}
          error={error("insertionTarget")}
        />
        <Switch
          label="Toujours garder le texte dans Sambler"
          description="Le texte n’est jamais inséré dans le champ : il vous attend dans Récents, prêt à être copié."
          checked={settings.alwaysKeepInSambler}
          onChange={(alwaysKeepInSambler) => void set("alwaysKeepInSambler", (s) => ({ ...s, alwaysKeepInSambler }))}
          error={error("alwaysKeepInSambler")}
        />
      </Card>

      <Card id={sectionAnchor("indicator")} title="Barre de dictée" description="La petite barre flottante qui montre l’écoute.">
        <Switch
          label="Son de début et de fin"
          description="Un son discret signale le début de l’écoute et la fin de la dictée."
          checked={settings.indicator.sound}
          onChange={(sound) => void set("indicator", (s) => ({ ...s, indicator: { ...s.indicator, sound } }))}
        />
        <IndicatorPositionField
          value={settings.indicator.position}
          onChange={(position) => void set("indicator", (s) => ({ ...s, indicator: { ...s.indicator, position } }))}
        />
        {error("indicator") ? <FieldError>{error("indicator")}</FieldError> : null}
      </Card>

      <Card
        id={sectionAnchor("recents")}
        title="Récents"
        description="Aucun audio n’est enregistré. Les textes récents restent en mémoire seulement ; verrouiller Windows ou quitter Sambler les efface."
      >
        <ChoiceGroup
          legend="Garder les textes récents pendant"
          name="draftMinutes"
          variant="segmented"
          value={settings.draftMinutes}
          options={[
            { value: 1, label: "1 minute" },
            { value: 5, label: "5 minutes" },
            { value: 10, label: "10 minutes" },
          ]}
          onChange={(draftMinutes) => void set("draftMinutes", (s) => ({ ...s, draftMinutes }))}
          error={error("draftMinutes")}
        />
        <ClearRecentsNow />
      </Card>

      <StartupCard />

      <AccessCard startOpen={section === "access"} />

      <PrivacyCard />
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
    <Row label="Position de la barre" htmlFor={id}>
      <select
        id={id}
        className="select settings__select"
        value={value}
        onChange={(event) => onChange(event.currentTarget.value as IndicatorPosition)}
      >
        {POSITIONS.map((position) => (
          <option key={position.value} value={position.value}>
            {position.label}
          </option>
        ))}
      </select>
    </Row>
  );
}

function ClearRecentsNow() {
  const state = useUiState();
  const commands = useUiCommands();
  const announce = useAnnounce();
  const action = useAction();
  const any = state.recentCount > 0 || state.draft !== null;
  return (
    <div className="stack stack--tight">
      <Row
        label={any ? "Des textes récents sont en mémoire." : "Aucun texte récent en mémoire."}
        description="Les effacer tout de suite, sans attendre le délai."
      >
        <button
          type="button"
          className="btn btn--sm"
          disabled={!any}
          onClick={async () => {
            const result = await action.run(() => commands.clearDraft());
            if (result.ok) announce(announcement("Textes récents effacés."));
          }}
        >
          <Glyph name="trash" />
          Effacer maintenant
        </button>
      </Row>
      {action.error ? <FieldError>{commandErrorText(action.error)}</FieldError> : null}
    </div>
  );
}

function AccessCard({ startOpen }: { startOpen: boolean }) {
  const { status, loadFailed, clear, refresh } = useAccess();
  const announce = useAnnounce();
  const [editing, setEditing] = useState(false);
  const action = useAction();

  useEffect(() => {
    if (startOpen && status && !status.configured) setEditing(true);
  }, [startOpen, status]);

  return (
    <Card
      id={sectionAnchor("access")}
      title="Accès à Rambler"
      description="Les données d’accès ne sont jamais réaffichées. Seule l’adresse du service est visible."
    >
      {status ? (
        <AccessSummary status={status} />
      ) : loadFailed ? (
        <div className="btn-row">
          <FieldError>L’état de l’accès n’a pas pu être lu.</FieldError>
          <button type="button" className="btn btn--sm" onClick={refresh}>
            Réessayer
          </button>
        </div>
      ) : (
        <p className="muted">Chargement…</p>
      )}
      {status?.source === "environment" ? (
        <p className="hint">
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
        <div className="btn-row">
          <button type="button" className="btn btn--sm" onClick={() => setEditing(true)}>
            <Glyph name="key" />
            {status.configured ? "Modifier l’accès" : "Configurer l’accès"}
          </button>
          {status.configured && status.source !== "environment" ? (
            <button
              type="button"
              className="btn btn--sm btn--ghost"
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
    </Card>
  );
}

function PrivacyCard() {
  const state = useUiState();
  const { settings } = useSettings();
  const commands = useUiCommands();
  const nav = useNavigation();
  const announce = useAnnounce();
  const action = useAction();
  const consent = hasConsent(state.availability);

  return (
    <Card id={sectionAnchor("privacy")} title="Confidentialité">
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
              announcement(enabled ? "Dictée avec Rambler activée." : "Dictée avec Rambler désactivée. Plus rien n’est envoyé."),
            );
          }
        }}
        error={action.error ? commandErrorText(action.error) : undefined}
      />
      <Row
        label="Historique des dictées"
        description={settings?.history.enabled ? "Activé : le texte est gardé sur ce PC, chiffré." : "Désactivé."}
      >
        <button type="button" className="btn btn--sm" onClick={() => nav.go("history")}>
          Gérer l’historique
        </button>
      </Row>
      <Row label="Présentation" description="Ce que reçoit Google, le micro, l’accès et un essai guidé.">
        <button type="button" className="btn btn--sm" onClick={() => nav.openFirstRun("consent")}>
          Revoir la prise en main
        </button>
      </Row>
    </Card>
  );
}

/** « Démarrer avec Windows » goes through `set_launch_at_startup` (installed edition only). */
function StartupCard() {
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
    <Card id={sectionAnchor("startup")} title="Démarrage">
      <Switch
        label="Démarrer avec Windows"
        description="Sambler s’ouvre à l’ouverture de session, discrètement dans la zone de notification. Désactivé par défaut."
        checked={settings.launchAtStartup}
        disabled={portable}
        onChange={(enabled) => void onChange(enabled)}
        error={error}
      />
    </Card>
  );
}
