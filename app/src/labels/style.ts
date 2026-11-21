// French copy of the Style screen: profiles, light rules, dictation commands
// and per-application styles. Inputs are settings and application names only,
// never dictated text.

import type { AppStyle, RenderProfile, TextOptions } from "../ipc/contract";

/**
 * French typography: a no-break space before « : ; ! ? » and inside « ».
 * Lets the copy below be written with ordinary spaces.
 */
export function nb(text: string): string {
  return text.replace(/ ([:;!?»])/g, " $1").replace(/« /g, "« ");
}

export const PROFILE_LABELS: Record<RenderProfile, string> = {
  natural: "Naturel",
  faithful: "Fidèle",
};

export const PROFILE_DESCRIPTIONS: Record<RenderProfile, string> = {
  natural: "Ponctuation, espaces et majuscules légèrement corrigés, sans changer vos mots.",
  faithful: "La transcription de Rambler telle quelle, ponctuation comprise.",
};

/** Every text option, in display order (the last one arrived with milestone 5). */
export type TextOptionKey = keyof TextOptions;
export const TEXT_OPTION_KEYS: TextOptionKey[] = ["lightPunctuation", "typographicSpaces", "sentenceCase", "voiceCommands"];

export const TEXT_OPTION_LABELS: Record<TextOptionKey, string> = {
  lightPunctuation: "Ponctuation légère",
  typographicSpaces: "Espaces typographiques",
  sentenceCase: "Majuscule de phrase",
  voiceCommands: "Commandes de dictée",
};

export const TEXT_OPTION_DESCRIPTIONS: Record<TextOptionKey, string> = {
  lightPunctuation: "Complète la ponctuation simple, comme le point final.",
  typographicSpaces: nb("Espaces fines avant « ; ! ? » et « : », et guillemets français « » à la place des guillemets droits."),
  sentenceCase: "Met une majuscule au début de chaque phrase.",
  voiceCommands: nb("« À la ligne », « ouvrez les guillemets »… deviennent de la mise en page."),
};

/** Lower-case form after « avec » / « sans ». */
const OPTION_PHRASES: Record<TextOptionKey, string> = {
  lightPunctuation: "ponctuation légère",
  typographicSpaces: "espaces typographiques",
  sentenceCase: "majuscule de phrase",
  voiceCommands: "commandes de dictée",
};

// ------------------------------------------------------------------ dictation commands

/**
 * A raw recognizer-like sentence that uses the commands. Break commands come
 * after a pause (a comma in the recognizer output), as the native side expects.
 */
export const VOICE_COMMANDS_SAMPLE =
  "bonjour Claire, à la ligne le client a dit ouvrez les guillemets c'est parfait fermez les guillemets, nouveau paragraphe à bientôt";

export const VOICE_COMMANDS_INTRO = nb(
  "Pendant la dictée, dites « à la ligne » ou « ouvrez les guillemets » : Sambler écrit la mise en page au lieu des mots. En profil Naturel et en français seulement.",
);

export function voiceCommandsSwitchText(profile: RenderProfile, english: boolean): string {
  if (profile === "faithful") return nb("Ignorées en Fidèle : les mots de la commande sont écrits tels quels.");
  if (english) return nb("Ignorées pendant une dictée en anglais : les commandes sont en français.");
  return nb("Marquez une courte pause avant « à la ligne » et « nouveau paragraphe ».");
}

export const VOICE_COMMANDS_UNSUPPORTED = "Cette version de Sambler ne prend pas encore en charge les commandes de dictée.";

export const VOICE_COMMANDS_LIST_FAILED = "La liste des commandes n’a pas pu être lue.";

/** Line breaks need the paste insertion outside the native editors. */
export function lineBreakReminder(typing: boolean): string {
  return typing
    ? nb(
        "Les sauts de ligne demandent l’insertion par collage. Avec la frappe caractère par caractère, votre réglage actuel, un texte de plusieurs lignes reste dans Sambler, sauf dans le Bloc-notes, WordPad et Word.",
      )
    : nb("Les sauts de ligne demandent l’insertion par collage : c’est votre réglage actuel.");
}

// ------------------------------------------------------------------ per application

export type TriState = "inherit" | "on" | "off";

export function triState(value: boolean | undefined): TriState {
  return value === undefined ? "inherit" : value ? "on" : "off";
}

export function triStateValue(state: TriState): boolean | undefined {
  return state === "inherit" ? undefined : state === "on";
}

/** « Comme le général (avec) »: the inherited choice names the current general value. */
export function inheritLabel(current: string): string {
  return `Comme le général (${current})`;
}

export function onOff(value: boolean): string {
  return value ? "avec" : "sans";
}

/** One line for a collapsed application: « Fidèle · sans majuscule de phrase ». */
export function appStyleSummary(style: AppStyle): string {
  const parts: string[] = [];
  if (style.profile) parts.push(PROFILE_LABELS[style.profile]);
  for (const key of TEXT_OPTION_KEYS) {
    const value = style.text?.[key];
    if (value !== undefined) parts.push(`${onOff(value)} ${OPTION_PHRASES[key]}`);
  }
  if (parts.length === 0) return "Comme le réglage général";
  const text = parts.join(" · ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export const APP_STYLES_INTRO = nb(
  "Un style à part pour certaines applications, par exemple Fidèle dans un éditeur de code. Ce qui reste « comme le général » suit les réglages ci-dessus.",
);

export const APP_STYLES_EMPTY = "Toutes les applications suivent le réglage général.";

export const APP_STYLES_UNSUPPORTED = "Cette version de Sambler ne prend pas encore en charge les styles par application.";

export const KNOWN_APPS_EMPTY =
  "Aucune application pour l’instant. Les applications apparaissent ici après vos premières dictées dedans.";

export const KNOWN_APPS_ALL_SET = "Toutes les applications où vous avez dicté ont déjà leur style.";

export const KNOWN_APPS_FAILED = "La liste des applications n’a pas pu être lue.";

/** The native side keeps at most 50 per-application styles. */
export const MAX_APP_STYLES = 50;

export const APP_STYLES_FULL = nb(`${MAX_APP_STYLES} applications au plus : retirez-en une pour en ajouter une autre.`);
