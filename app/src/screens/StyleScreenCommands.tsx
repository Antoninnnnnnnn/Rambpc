// Style > Commandes de dictée: the switch (`text.voiceCommands`), the fixed
// list from `get_voice_commands` (what is said, what is written), the reminder
// that line breaks need the paste insertion, and a preview. Applied natively
// in Naturel only, for a French dictation. Degrades when the native side
// predates the commands (no `voiceCommands` field, or the command fails).

import { useCallback, useEffect, useState } from "react";
import { Card, FieldError, Switch } from "../components/controls";
import { Glyph } from "../components/Icon";
import { RenderPreview } from "../components/RenderPreview";
import { useBridge } from "../ipc/bridge";
import type { VoiceCommand } from "../ipc/contract";
import { settingSaveErrorText } from "../labels/fr";
import {
  lineBreakReminder,
  nb,
  VOICE_COMMANDS_INTRO,
  VOICE_COMMANDS_LIST_FAILED,
  VOICE_COMMANDS_SAMPLE,
  VOICE_COMMANDS_UNSUPPORTED,
  voiceCommandsSwitchText,
} from "../labels/style";
import { useNavigation } from "../state/navigation";
import { useSettings } from "../state/settings";

export function VoiceCommandsCard() {
  const bridge = useBridge();
  const nav = useNavigation();
  const { settings, errors, update } = useSettings();
  const [commands, setCommands] = useState<VoiceCommand[] | null>(null);
  const [failed, setFailed] = useState(false);

  const load = useCallback(() => {
    setFailed(false);
    bridge
      .invoke("get_voice_commands")
      .then(setCommands)
      .catch(() => {
        setCommands(null);
        setFailed(true);
      });
  }, [bridge]);

  useEffect(load, [load]);

  if (!settings) return null;
  const enabled = settings.text.voiceCommands;

  if (typeof enabled !== "boolean") {
    return (
      <Card title="Commandes de dictée" description={VOICE_COMMANDS_INTRO} className="voice-card" testId="voice-commands">
        <p className="style-note">
          <Glyph name="return" />
          <span>{VOICE_COMMANDS_UNSUPPORTED}</span>
        </p>
      </Card>
    );
  }

  const faithful = settings.profile === "faithful";
  const typing = settings.insertion === "typing";

  return (
    <Card title="Commandes de dictée" description={VOICE_COMMANDS_INTRO} className="voice-card" testId="voice-commands">
      <Switch
        label="Utiliser les commandes de dictée"
        description={voiceCommandsSwitchText(settings.profile, settings.language === "english")}
        checked={enabled}
        disabled={faithful}
        onChange={(voiceCommands) => void update("text", (s) => ({ ...s, text: { ...s.text, voiceCommands } }))}
        error={errors.text ? settingSaveErrorText(errors.text) : undefined}
      />

      {commands && commands.length > 0 ? (
        <div className={faithful ? "voice-commands voice-commands--off" : "voice-commands"}>
          <p className="voice-commands__head" aria-hidden="true">
            <span>Vous dites</span>
            <span>Sambler écrit</span>
          </p>
          <dl className="voice-commands__list">
            {commands.map((command) => (
              <div key={command.spoken} className="voice-command">
                <dt className="voice-command__spoken">{nb(`« ${command.spoken} »`)}</dt>
                <dd className="voice-command__effect">
                  <span className="voice-command__result" aria-hidden="true">
                    {command.result}
                  </span>
                  <span className="voice-command__desc">{command.description}</span>
                </dd>
              </div>
            ))}
          </dl>
        </div>
      ) : failed ? (
        <div className="btn-row">
          <FieldError>{VOICE_COMMANDS_LIST_FAILED}</FieldError>
          <button type="button" className="btn btn--sm" onClick={load}>
            Réessayer
          </button>
        </div>
      ) : null}

      <div className={typing ? "style-note style-note--warning" : "style-note"} data-testid="line-break-reminder">
        <Glyph name="return" />
        <p>
          {lineBreakReminder(typing)}{" "}
          <button type="button" className="link-btn" onClick={() => nav.go("settings", "insertion")}>
            {typing ? "Choisir l’insertion" : "Voir l’insertion"}
          </button>
        </p>
      </div>

      <RenderPreview
        settings={settings}
        sample={VOICE_COMMANDS_SAMPLE}
        caption="Aperçu"
        labels={{ raw: "Vous dites", result: "Sambler écrit" }}
        testId="voice-commands-preview"
      />
    </Card>
  );
}
