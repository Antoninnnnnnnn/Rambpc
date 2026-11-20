// Style: how recognized words become text. Naturel (light rules) or Fidèle
// (Rambler as is), the three light rules, and a live preview through
// `preview_render`. Each change is saved at once and applies to the next take.

import { useEffect, useId, useState } from "react";
import { Card, ChoiceGroup, FieldError, Switch } from "../components/controls";
import { PREVIEW_SAMPLES, RenderPreview } from "../components/RenderPreview";
import { settingSaveErrorText } from "../labels/fr";
import { useSettings } from "../state/settings";
import { ScreenHeader } from "./ScreenHeader";
import "./StyleScreen.css";

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
      <div className="screen">
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
    <div className="screen">
      <ScreenHeader title="Style">
        Choisissez comment vos mots deviennent du texte. Sambler ne change jamais vos mots : il retouche au plus la
        ponctuation, les espaces et les majuscules. S’applique à la prochaine dictée.
      </ScreenHeader>

      <Card title="Votre style">
        <ChoiceGroup
          legend="Style"
          hideLegend
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
          onChange={(profile) => void update("profile", (s) => ({ ...s, profile }))}
          error={errors.profile ? settingSaveErrorText(errors.profile) : undefined}
        />
      </Card>

      <Card
        title="Règles du style Naturel"
        description={faithful ? "Ces règles sont ignorées en Fidèle." : "Trois retouches légères, chacune réversible."}
      >
        <Switch
          label="Ponctuation légère"
          description="Complète la ponctuation simple, comme le point final."
          checked={settings.text.lightPunctuation}
          disabled={faithful}
          onChange={(lightPunctuation) => void update("text", (s) => ({ ...s, text: { ...s.text, lightPunctuation } }))}
        />
        <Switch
          label="Espaces typographiques"
          description="Espaces insécables du français avant « : ; ! ? » et dans les guillemets."
          checked={settings.text.typographicSpaces}
          disabled={faithful}
          onChange={(typographicSpaces) =>
            void update("text", (s) => ({ ...s, text: { ...s.text, typographicSpaces } }))
          }
        />
        <Switch
          label="Majuscule de phrase"
          description="Met une majuscule au début de chaque phrase."
          checked={settings.text.sentenceCase}
          disabled={faithful}
          onChange={(sentenceCase) => void update("text", (s) => ({ ...s, text: { ...s.text, sentenceCase } }))}
        />
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
    </div>
  );
}
