// Style: how recognized words become text. Naturel (light rules) or Fidèle
// (Rambler as is), the three light rules, a live preview through
// `preview_render`, the dictation commands (StyleScreenCommands) and the
// per-application styles (StyleScreenApps). Each change is saved at once and
// applies to the next take.

import { useEffect, useId, useState } from "react";
import { Card, ChoiceGroup, FieldError, Switch } from "../components/controls";
import { PREVIEW_SAMPLES, RenderPreview } from "../components/RenderPreview";
import { settingSaveErrorText } from "../labels/fr";
import { nb, PROFILE_DESCRIPTIONS, PROFILE_LABELS, TEXT_OPTION_DESCRIPTIONS, TEXT_OPTION_LABELS } from "../labels/style";
import { useSettings } from "../state/settings";
import { ScreenHeader } from "./ScreenHeader";
import { AppStylesCard } from "./StyleScreenApps";
import { VoiceCommandsCard } from "./StyleScreenCommands";
import "./StyleScreen.css";

const LIGHT_RULES = ["lightPunctuation", "typographicSpaces", "sentenceCase"] as const;

export function StyleScreen() {
  const { settings, errors, update, loadFailed, reload } = useSettings();
  const sampleId = useId();
  const language = settings?.language ?? "french";
  const [sample, setSample] = useState<string>(PREVIEW_SAMPLES[language]);
  const [edited, setEdited] = useState(false);

  // Follow the dictation language until the person writes their own sample.
  useEffect(() => {
    if (!edited) setSample(PREVIEW_SAMPLES[language]);
  }, [language, edited]);

  if (!settings) {
    return (
      <div className="screen style">
        <ScreenHeader title="Style" />
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
          <p className="muted">Chargement…</p>
        )}
      </div>
    );
  }

  const faithful = settings.profile === "faithful";
  const textError = errors.text ? settingSaveErrorText(errors.text) : undefined;

  return (
    <div className="screen style">
      <ScreenHeader title="Style">
        {nb(
          "Choisissez comment vos mots deviennent du texte. Sambler ne change jamais vos mots : il retouche au plus la ponctuation, les espaces et les majuscules. S’applique à la prochaine dictée.",
        )}
      </ScreenHeader>

      <Card title="Votre style">
        <ChoiceGroup
          legend="Style"
          hideLegend
          name="profile"
          value={settings.profile}
          options={[
            { value: "natural", label: PROFILE_LABELS.natural, description: PROFILE_DESCRIPTIONS.natural },
            { value: "faithful", label: PROFILE_LABELS.faithful, description: PROFILE_DESCRIPTIONS.faithful },
          ]}
          onChange={(profile) => void update("profile", (s) => ({ ...s, profile }))}
          error={errors.profile ? settingSaveErrorText(errors.profile) : undefined}
        />
      </Card>

      <Card
        title="Règles du style Naturel"
        description={faithful ? "Ces règles sont ignorées en Fidèle." : "Trois retouches légères, chacune réversible."}
      >
        {LIGHT_RULES.map((key) => (
          <Switch
            key={key}
            label={TEXT_OPTION_LABELS[key]}
            description={TEXT_OPTION_DESCRIPTIONS[key]}
            checked={settings.text[key]}
            disabled={faithful}
            onChange={(value) => void update("text", (s) => ({ ...s, text: { ...s.text, [key]: value } }))}
          />
        ))}
        {textError ? <FieldError>{textError}</FieldError> : null}
      </Card>

      <Card title="Aperçu" description="Sans ouvrir le micro, avec vos réglages actuels, vos mots et vos expressions.">
        <div className="field">
          <label htmlFor={sampleId} className="field__label">
            Phrase telle que Rambler l’écrirait
          </label>
          <textarea
            id={sampleId}
            className="input style__sample"
            rows={2}
            maxLength={500}
            value={sample}
            spellCheck={false}
            lang={language === "english" ? "en" : "fr"}
            onChange={(event) => {
              setEdited(true);
              setSample(event.currentTarget.value);
            }}
          />
          {edited ? (
            <div>
              <button
                type="button"
                className="link-btn"
                onClick={() => {
                  setEdited(false);
                  setSample(PREVIEW_SAMPLES[language]);
                }}
              >
                Revenir à la phrase d’exemple
              </button>
            </div>
          ) : null}
        </div>
        <RenderPreview settings={settings} sample={sample} />
      </Card>

      <VoiceCommandsCard />

      <AppStylesCard sample={sample} />
    </div>
  );
}
