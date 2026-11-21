// DEV PREVIEW ONLY: simulated per-application identities and spoken layout
// commands (milestone 5). Never part of a production build.

import type { AppIdentity, VoiceCommand } from "../ipc/contract";

export const PREVIEW_APPS: AppIdentity[] = [
  { id: "winword", name: "Word" },
  { id: "brave", name: "Brave" },
  { id: "discord", name: "Discord" },
  { id: "code", name: "Visual Studio Code" },
  { id: "outlook", name: "Outlook" },
  { id: "slack", name: "Slack" },
  { id: "notepad", name: "Bloc-notes" },
];

export const PREVIEW_VOICE_COMMANDS: VoiceCommand[] = [
  { spoken: "à la ligne", result: "↵", description: "Passe à la ligne suivante." },
  { spoken: "nouvelle ligne", result: "↵", description: "Passe à la ligne suivante." },
  { spoken: "nouveau paragraphe", result: "↵ ↵", description: "Laisse une ligne vide puis commence un paragraphe." },
  { spoken: "ouvrez les guillemets", result: "«", description: "Ouvre des guillemets français, avec l’espace qui suit." },
  { spoken: "fermez les guillemets", result: "»", description: "Ferme les guillemets, avec l’espace qui précède." },
  { spoken: "ouvrez la parenthèse", result: "(", description: "Ouvre une parenthèse." },
  { spoken: "fermez la parenthèse", result: ")", description: "Ferme la parenthèse." },
];

export function createPreviewApps() {
  return {
    get_known_apps: () => structuredClone(PREVIEW_APPS),
    get_voice_commands: () => structuredClone(PREVIEW_VOICE_COMMANDS),
  };
}
