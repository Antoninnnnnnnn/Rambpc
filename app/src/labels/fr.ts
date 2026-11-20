// French copy for every text-free value of the contract. Nothing here ever
// receives dictated text: inputs are UiState fields, enums and settings.

import type {
  AccessSource,
  Availability,
  ClipboardError,
  CommandError,
  InterruptReason,
  KeptReason,
  MicError,
  NotWrittenReason,
  RefusalReason,
  TakeOutcome,
  TargetChange,
  UiState,
  UnavailableReason,
  UncertainReason,
} from "../ipc/contract";
import { formatShortcut, type ShortcutRejection } from "../lib/shortcut";

// ------------------------------------------------------------------ status

/** Icon shape of a status: the state is never conveyed by color alone. */
export type StatusShape =
  | "available"
  | "connecting"
  | "preparing"
  | "listening"
  | "finalizing"
  | "unavailable"
  | "setup";

export interface StatusView {
  shape: StatusShape;
  label: string;
  hint: string;
}

export const PHASE_LABELS = {
  idle: "Disponible",
  preparing: "Préparation",
  listening: "Écoute",
  finalizing: "Traitement",
} as const;

export function unavailableReasonText(reason: UnavailableReason): string {
  switch (reason) {
    case "network":
      return "Le service est injoignable. Vérifiez la connexion Internet de ce PC.";
    case "access-refused":
      return "Le service a refusé l’accès. Vérifiez les données d’accès à Rambler.";
    case "throttled":
      return "Le service limite temporairement les demandes. Réessayez dans quelques instants.";
    case "invalid-access":
      return "Les données d’accès à Rambler sont incomplètes ou mal formées.";
    case "service":
      return "Le service Rambler ne répond pas correctement pour le moment.";
  }
}

export function startHint(gesture: UiState["gesture"], shortcut: string): string {
  const keys = formatShortcut(shortcut);
  return gesture === "hold"
    ? `Dans n’importe quelle application, maintenez ${keys}, parlez, puis relâchez.`
    : `Dans n’importe quelle application, appuyez sur ${keys} pour commencer, puis de nouveau pour terminer.`;
}

export function stopHint(gesture: UiState["gesture"], shortcut: string): string {
  const keys = formatShortcut(shortcut);
  return gesture === "hold"
    ? `Parlez maintenant, puis relâchez ${keys}.`
    : `Parlez maintenant, puis appuyez de nouveau sur ${keys}.`;
}

export function describeStatus(state: UiState): StatusView {
  switch (state.phase.kind) {
    case "preparing":
      return {
        shape: "preparing",
        label: PHASE_LABELS.preparing,
        hint: "Ouverture du micro. Attendez le signal « Écoute » avant de parler.",
      };
    case "listening":
      return {
        shape: "listening",
        label: PHASE_LABELS.listening,
        // A take kept in Sambler may come from the button: « Terminer » always works.
        hint:
          state.destination === "sambler"
            ? "Parlez maintenant, puis choisissez « Terminer »."
            : stopHint(state.gesture, state.shortcut),
      };
    case "finalizing":
      return {
        shape: "finalizing",
        label: PHASE_LABELS.finalizing,
        hint: "Rambler termine la transcription. Le résultat arrive dans un instant.",
      };
    case "idle":
      return describeAvailability(state);
  }
}

function describeAvailability(state: UiState): StatusView {
  const availability = state.availability;
  switch (availability.kind) {
    case "ready":
      return {
        shape: "available",
        label: PHASE_LABELS.idle,
        hint: startHint(state.gesture, state.shortcut),
      };
    case "connecting":
      return {
        shape: "connecting",
        label: "Connexion à Rambler",
        hint: "La dictée sera disponible dans un instant.",
      };
    // The availability banner carries the reason and the actions; the status
    // card says what it means for the microphone.
    case "needs-consent":
      return {
        shape: "setup",
        label: "Dictée non activée",
        hint: "Rien n’est envoyé à Google tant que la dictée n’est pas activée.",
      };
    case "needs-access":
      return {
        shape: "setup",
        label: "Accès à Rambler requis",
        hint: "Le micro de dictée reste fermé. Le test du micro et les réglages restent utilisables.",
      };
    case "unavailable":
      return {
        shape: "unavailable",
        label: "Dictée indisponible",
        hint: "Le micro de dictée reste fermé jusqu’au retour du service.",
      };
  }
}

/** Is a take in progress (Terminer / Annuler apply)? */
export function isTakeActive(state: UiState): boolean {
  return state.phase.kind !== "idle" && state.take !== null;
}

export function canStartTake(availability: Availability): boolean {
  return availability.kind === "ready" || availability.kind === "connecting";
}

/** Consent is the first availability check of the controller. */
export function hasConsent(availability: Availability): boolean {
  return availability.kind !== "needs-consent";
}

// ------------------------------------------------------------------ outcomes

export type Tone = "success" | "info" | "warning" | "error";

export type OutcomeAction =
  | "open-draft"
  | "dictate-in-sambler"
  | "open-mic-privacy"
  | "enable-dictation"
  | "configure-access"
  | "retry";

export interface OutcomeView {
  tone: Tone;
  title: string;
  detail?: string;
  actions: OutcomeAction[];
}

export function micErrorText(error: MicError): string {
  switch (error.kind) {
    case "no-input":
      return "Aucun micro n’est branché sur ce PC.";
    case "not-found":
      return "Le micro choisi est introuvable. Choisissez-en un autre dans les réglages.";
    case "access-denied":
      return "Windows bloque l’accès au micro pour les applications de bureau.";
    case "unsupported-format":
      return `Le format de ce micro n’est pas pris en charge (${error.format}). Choisissez un autre micro.`;
    case "open-timeout":
      return "Le micro n’a pas démarré à temps.";
    case "disconnected":
      return "Le micro a été débranché ou désactivé.";
    case "other":
      return "Le micro a rencontré une erreur.";
  }
}

export function targetChangeText(change: TargetChange): string {
  switch (change) {
    case "focus-moved":
      return "Le curseur a quitté le champ pendant la dictée.";
    case "selection-changed":
      return "La sélection a changé dans le champ pendant la dictée.";
    case "window-gone":
      return "La fenêtre de départ a été fermée.";
    case "protected":
      return "Un champ protégé (mot de passe) a pris le focus : le micro a été coupé.";
    case "unknown":
      return "Le champ de départ a changé.";
  }
}

export function notWrittenText(reason: NotWrittenReason): string {
  switch (reason) {
    case "target-changed":
      return "Le champ a changé juste avant l’écriture : rien n’a été saisi.";
    case "modifiers-held":
      return "Des touches Ctrl, Maj ou Alt étaient encore enfoncées : rien n’a été saisi.";
    case "unsupported":
      return "Ce champ n’accepte pas ce texte (plusieurs lignes ou longueur) : rien n’a été saisi.";
    case "refused":
      return "Windows a refusé la saisie dans ce champ : rien n’a été saisi.";
  }
}

export function keptReasonText(reason: KeptReason): string {
  switch (reason.kind) {
    case "sambler-destination":
      return "Vous avez choisi de garder le texte dans Sambler.";
    case "target-changed":
      return targetChangeText(reason.change);
    case "not-written":
      return notWrittenText(reason.reason);
    case "incomplete":
      return "Des passages manquent ou sont à revoir : rien n’a été inséré automatiquement.";
  }
}

export function uncertainReasonText(reason: UncertainReason): string {
  switch (reason) {
    case "no-focused-field":
      return "Aucun champ de saisie n’était actif.";
    case "read-only":
      return "Le champ est en lecture seule.";
    case "selection":
      return "Du texte était sélectionné : il aurait été remplacé.";
    case "elevated":
      return "L’application fonctionne avec des droits d’administrateur.";
    case "terminal":
      return "Un terminal ne reçoit pas la dictée directement.";
    case "remote-desktop":
      return "Le champ d’un Bureau à distance ne peut pas être vérifié.";
    case "unresponsive":
      return "L’application ne répondait pas.";
    case "unsupported":
      return "Ce type de champ n’est pas encore pris en charge.";
  }
}

export function interruptReasonText(reason: InterruptReason): string {
  switch (reason.kind) {
    case "microphone-stalled":
      return "Le micro n’envoyait plus de son.";
    case "microphone":
      return micErrorText(reason.error);
    case "capture-overflow":
      return "La capture audio a pris du retard : l’enregistrement est incomplet.";
    case "network":
      return "La connexion au service a été perdue.";
    case "access-refused":
      return "Le service a refusé l’accès.";
    case "throttled":
      return "Le service limite temporairement les demandes.";
    case "timeout":
      return "Le service n’a pas répondu à temps.";
    case "service":
      return "Le service a rencontré une erreur.";
    case "system-interrupted":
      return "Windows a été verrouillé ou mis en veille.";
    case "consent-withdrawn":
      return "Vous avez désactivé la dictée avec Rambler.";
    case "duration-limit":
      return "La durée maximale d’une dictée (5 minutes) est atteinte.";
  }
}

function refusalView(reason: RefusalReason): OutcomeView {
  const title = "La dictée n’a pas démarré";
  switch (reason.kind) {
    case "needs-consent":
      return {
        tone: "info",
        title,
        detail: "Activez d’abord la dictée avec Rambler.",
        actions: ["enable-dictation"],
      };
    case "needs-access":
      return {
        tone: "info",
        title,
        detail: "Configurez d’abord l’accès à Rambler.",
        actions: ["configure-access"],
      };
    case "unavailable":
      return {
        tone: "error",
        title: "Dictée indisponible",
        detail: unavailableReasonText(reason.reason),
        actions: ["retry"],
      };
    case "protected-field":
      return {
        tone: "warning",
        title: "Champ protégé",
        detail: "Sambler n’ouvre pas le micro dans un champ de mot de passe ni sur un écran sécurisé.",
        actions: [],
      };
    case "uncertain-target":
      return {
        tone: "warning",
        title: "Sambler ne peut pas écrire sûrement dans ce champ",
        detail: `${uncertainReasonText(reason.reason)} Vous pouvez dicter dans Sambler, puis copier le texte.`,
        actions: ["dictate-in-sambler"],
      };
    case "pending-draft":
      return {
        tone: "warning",
        title:
          "Un texte attend dans Sambler : copiez-le ou rappuyez sur le raccourci pour le remplacer",
        actions: ["open-draft"],
      };
    case "microphone":
      return {
        tone: "error",
        title: "Le micro n’a pas pu s’ouvrir",
        detail: micErrorText(reason.error),
        actions: reason.error.kind === "access-denied" ? ["open-mic-privacy"] : [],
      };
  }
}

export function describeOutcome(outcome: TakeOutcome, hasDraft: boolean): OutcomeView {
  const draftAction: OutcomeAction[] = hasDraft ? ["open-draft"] : [];
  switch (outcome.kind) {
    case "inserted":
      return { tone: "success", title: "Texte transmis au champ", actions: [] };
    case "inserted-partially":
      return {
        tone: "warning",
        title: "Vérifiez le champ, une partie du texte a pu être saisie",
        detail: hasDraft ? "Le texte complet reste disponible dans le brouillon." : undefined,
        actions: draftAction,
      };
    case "kept-in-sambler":
      return {
        tone: "info",
        title: "Texte disponible",
        detail: keptReasonText(outcome.reason),
        actions: draftAction,
      };
    case "no-text":
      return {
        tone: "info",
        title: "Aucun texte obtenu",
        detail: "Rambler n’a reconnu aucune parole. Vérifiez le micro puis réessayez.",
        actions: [],
      };
    case "cancelled":
      return { tone: "info", title: "Dictée annulée", detail: "Rien n’a été inséré.", actions: [] };
    case "interrupted": {
      const limit = outcome.reason.kind === "duration-limit";
      const recovery = outcome.recovered
        ? "Les passages déjà reçus sont dans le brouillon."
        : "Aucun texte récupéré.";
      const actions: OutcomeAction[] = outcome.recovered ? draftAction : [];
      if (outcome.reason.kind === "microphone" && outcome.reason.error.kind === "access-denied") {
        actions.push("open-mic-privacy");
      }
      return {
        tone: limit ? "info" : "error",
        title: limit ? "Dictée terminée à la durée maximale" : "Dictée interrompue",
        detail: `${interruptReasonText(outcome.reason)} ${recovery}`,
        actions,
      };
    }
    case "refused":
      return refusalView(outcome.reason);
  }
}

// ------------------------------------------------------------------ errors

export const GENERIC_ERROR = "L’opération n’a pas abouti. Réessayez.";

/** Command errors: `message` is French and text-free by contract. */
export function commandErrorText(error: CommandError, fallback = GENERIC_ERROR): string {
  return error.message.trim() || fallback;
}

/** `copy_draft` / `copy_history_entry` codes (shell/commands.rs). */
const CLIPBOARD_CODES: Record<string, ClipboardError | "no-draft"> = {
  "clipboard-busy": "busy",
  "clipboard-exclusion-failed": "exclusion-failed",
  "clipboard-other": "other",
  "no-draft": "no-draft",
};

export function clipboardErrorText(error: CommandError): string {
  switch (CLIPBOARD_CODES[error.code]) {
    case "busy":
      return "Le presse-papiers est occupé par une autre application. Réessayez dans un instant.";
    case "exclusion-failed":
      return "Copie annulée : Windows n’a pas pu exclure ce texte de l’historique du presse-papiers (Win+V). Le texte reste dans Sambler.";
    case "no-draft":
      return "Il n’y a plus de texte à copier.";
    case "other":
      return "La copie a échoué. Le texte reste dans Sambler.";
    default:
      return commandErrorText(error, "La copie a échoué. Le texte reste dans Sambler.");
  }
}

/** Does this command error mean Windows blocks the microphone? (`mic-<kind>` codes) */
export function isMicAccessDenied(error: CommandError): boolean {
  return error.code === "mic-access-denied";
}

// ------------------------------------------------------------------ shortcut

export function shortcutRejectionText(reason: ShortcutRejection): string {
  switch (reason) {
    case "modifier-only":
      return "Ajoutez une touche principale : une lettre, un chiffre, Espace ou une touche de fonction.";
    case "windows-key":
      return "Les combinaisons avec la touche Windows sont réservées à Windows.";
    case "f12":
      return "F12 est réservée par Windows : choisissez une autre touche.";
    case "ctrl-alt":
      return "Ctrl+Alt sert à écrire des caractères (AltGr) : choisissez une autre combinaison.";
    case "needs-modifier":
      return "Ajoutez Ctrl, Alt ou Maj pour ne pas bloquer cette touche dans vos applications.";
    case "needs-two-modifiers":
      return "Avec une lettre, un chiffre ou Espace, combinez deux touches : Ctrl+Maj ou Alt+Maj.";
    case "reserved":
      return "Alt+F4 ferme les fenêtres : choisissez une autre combinaison.";
    case "unsupported-key":
      return "Cette touche ne peut pas servir de raccourci. Choisissez une lettre, un chiffre, Espace ou une touche de fonction.";
  }
}

/** A refused shortcut keeps the previous one (save_settings code `shortcut`). */
export function shortcutSaveErrorText(error: CommandError, current: string): string {
  if (error.code === "save-failed") return commandErrorText(error);
  const detail =
    error.code === "shortcut"
      ? "Cette combinaison est peut-être déjà utilisée par Windows ou par une autre application."
      : commandErrorText(error, "");
  return `Le raccourci n’a pas pu être enregistré. ${detail} L’ancien raccourci reste actif : ${formatShortcut(current)}.`
    .replace(/\s+\./g, ".")
    .replace(/ {2,}/g, " ");
}

/**
 * A refused setting is rolled back; `save-failed` means it applies for this
 * session but could not be written to disk, so it is kept.
 */
export function settingSaveErrorText(error: CommandError): string {
  if (error.code === "save-failed") return commandErrorText(error);
  return `Ce réglage n’a pas pu être enregistré. ${commandErrorText(error, "")}`.trim();
}

// ------------------------------------------------------------------ access

export function accessSourceText(source: AccessSource): string {
  switch (source) {
    case "none":
      return "Aucun accès configuré";
    case "environment":
      return "Variables d’environnement RAMBLER_* de Windows";
    case "saved":
      return "Mémorisé sur ce PC, chiffré pour votre compte Windows";
    case "session":
      return "Pour cette session seulement (oublié à la fermeture)";
  }
}
