// DEV PREVIEW ONLY: simulated per-application identities and spoken layout
// commands (milestone 5). Never part of a production build.

import type { AppIdentity, Settings, VoiceCommand } from "../ipc/contract";

export const PREVIEW_APPS: AppIdentity[] = [
  { id: "winword", name: "Word" },
  { id: "brave", name: "Brave" },
  { id: "discord", name: "Discord" },
  { id: "code", name: "Visual Studio Code" },
  { id: "outlook", name: "Outlook" },
  { id: "slack", name: "Slack" },
  { id: "notepad", name: "Bloc-notes" },
];

/** Same list as the native catalog (src-tauri/src/text/voice.rs). */
export const PREVIEW_VOICE_COMMANDS: VoiceCommand[] = [
  { spoken: "à la ligne", result: "↵", description: "Saut de ligne, avec une pause ou en début de phrase." },
  { spoken: "nouvelle ligne", result: "↵", description: "Saut de ligne, avec une pause ou en début de phrase." },
  { spoken: "nouveau paragraphe", result: "↵↵", description: "Deux sauts de ligne, avec une pause ou en début de phrase." },
  { spoken: "ouvrez les guillemets", result: "«", description: "Ouvre les guillemets français." },
  { spoken: "fermez les guillemets", result: "»", description: "Ferme les guillemets français." },
  { spoken: "ouvrez la parenthèse", result: "(", description: "Ouvre une parenthèse." },
  { spoken: "fermez la parenthèse", result: ")", description: "Ferme une parenthèse." },
];

const LAYOUT: [RegExp, string][] = [
  [/,?\s*(?:à la ligne|nouvelle ligne)\b,?\s*/gi, "\n"],
  [/,?\s*\bnouveau paragraphe\b,?\s*/gi, "\n\n"],
  [/\s*\bouvrez les guillemets\b\s*/gi, " « "],
  [/\s*\bfermez les guillemets\b/gi, " »"],
  [/\s*\bouvrez la parenthèse\b\s*/gi, " ("],
  [/\s*\bfermez la parenthèse\b/gi, ")"],
];

/**
 * The language a take is rendered in. In automatic mode, a rough stand-in for
 * the native guess: English function words against French ones.
 */
export function previewLanguage(sample: string, settings: Settings): "french" | "english" {
  if (settings.language !== "auto") return settings.language;
  const words = sample.toLowerCase().match(/[\p{L}']+/gu) ?? [];
  const count = (list: string[]) => words.filter((word) => list.includes(word)).length;
  const english = count(["the", "and", "is", "to", "you", "i", "it", "on", "at", "for", "will", "does", "be", "not"]);
  const french = count(["le", "la", "les", "de", "et", "je", "vous", "pas", "ne", "à", "est", "que", "un", "une"]);
  return english > french ? "english" : "french";
}

/**
 * A rough stand-in for the native commands, applied to the sample before the
 * preview's light rules: Naturel and French only, like the native side.
 */
export function previewVoiceCommands(sample: string, settings: Settings): string {
  if (settings.profile !== "natural" || !settings.text.voiceCommands || previewLanguage(sample, settings) !== "french") return sample;
  let out = sample;
  for (const [pattern, text] of LAYOUT) out = out.replace(pattern, text);
  out = out.replace(/^ +| +$/gm, "");
  return settings.text.sentenceCase ? out.replace(/(^|\n)(\p{L})/gu, (_m, p: string, c: string) => p + c.toUpperCase()) : out;
}

export function createPreviewApps() {
  return {
    get_known_apps: () => structuredClone(PREVIEW_APPS),
    get_voice_commands: () => structuredClone(PREVIEW_VOICE_COMMANDS),
  };
}
