// What the floating dictation bar shows for a `sambler://state` change. Pure
// and text-free: built from phases and outcome kinds only, never from
// dictated text (the bar calls no command and never receives any).

import type { TakeId, TakeOutcome, UiState } from "../ipc/contract";

/** How long an outcome stays on screen before fading out (the fade adds ~0.3 s). */
export const FLASH_MS = 2500;
/** Warn in the last 30 seconds of the 5-minute limit. */
export const LIMIT_WARNING_MS = 4.5 * 60_000;

export type FlashTone = "success" | "info" | "neutral" | "warning" | "error";
export type FlashIcon = "check" | "inbox" | "empty" | "cancel" | "warning" | "error" | "lock" | "mic-off";

export interface Flash {
  tone: FlashTone;
  icon: FlashIcon;
  /** A few French words, never dictated text. */
  label: string;
}

export type OverlayView =
  | { kind: "hidden" }
  | { kind: "preparing" }
  | { kind: "listening"; take: TakeId | null; startedAt: number; toSambler: boolean }
  | { kind: "finalizing" }
  | { kind: "outcome"; flash: Flash; until: number };

export const HIDDEN: OverlayView = { kind: "hidden" };

export function flashFor(outcome: TakeOutcome): Flash {
  switch (outcome.kind) {
    case "inserted":
      return { tone: "success", icon: "check", label: "Inséré" };
    case "inserted-partially":
      return { tone: "warning", icon: "warning", label: "Vérifiez le champ" };
    case "kept-in-sambler":
      return { tone: "info", icon: "inbox", label: "Texte dans Sambler" };
    case "no-text":
      return { tone: "neutral", icon: "empty", label: "Aucun texte" };
    case "cancelled":
      return { tone: "neutral", icon: "cancel", label: "Annulé" };
    case "interrupted":
      if (outcome.reason.kind === "duration-limit") {
        return { tone: "info", icon: outcome.recovered ? "inbox" : "warning", label: "Durée maximale atteinte" };
      }
      if (outcome.reason.kind === "microphone" && outcome.reason.error.kind === "access-denied") {
        return { tone: "error", icon: "mic-off", label: "Micro bloqué par Windows" };
      }
      return outcome.recovered
        ? { tone: "warning", icon: "inbox", label: "Interrompue, texte gardé" }
        : { tone: "error", icon: "error", label: "Dictée interrompue" };
    case "refused": {
      const reason = outcome.reason;
      switch (reason.kind) {
        case "needs-consent":
          return { tone: "warning", icon: "lock", label: "Dictée non activée" };
        case "needs-access":
          return { tone: "warning", icon: "lock", label: "Accès à Rambler requis" };
        case "unavailable":
          return { tone: "error", icon: "error", label: "Indisponible" };
        case "protected-field":
          return { tone: "warning", icon: "lock", label: "Champ protégé" };
        case "uncertain-target":
          return { tone: "warning", icon: "warning", label: "Champ non pris en charge" };
        case "pending-draft":
          return { tone: "info", icon: "inbox", label: "Un texte attend" };
        case "microphone":
          switch (reason.error.kind) {
            case "access-denied":
              return { tone: "error", icon: "mic-off", label: "Micro bloqué par Windows" };
            case "not-found":
              return { tone: "error", icon: "mic-off", label: "Micro introuvable" };
            case "no-input":
              return { tone: "error", icon: "mic-off", label: "Aucun micro" };
            default:
              return { tone: "error", icon: "mic-off", label: "Micro indisponible" };
          }
      }
    }
  }
}

/**
 * What an outcome means for the dictated text, for themes that colour the bar
 * by meaning rather than by severity (Verre et aurore: jade, saffron, red).
 * Always shown with the flash's icon and words, never by colour alone.
 */
export type OutcomeMeaning = "inserted" | "kept" | "neutral" | "refused" | "error";

export function outcomeMeaning(outcome: TakeOutcome): OutcomeMeaning {
  switch (outcome.kind) {
    case "inserted":
      return "inserted";
    case "inserted-partially":
    case "kept-in-sambler":
      return "kept";
    case "no-text":
    case "cancelled":
      return "neutral";
    case "interrupted":
      if (outcome.reason.kind === "microphone" && outcome.reason.error.kind === "access-denied") return "error";
      return outcome.recovered ? "kept" : "error";
    case "refused":
      return outcome.reason.kind === "microphone" || outcome.reason.kind === "unavailable" ? "error" : "refused";
  }
}

function outcomeKey(state: UiState): string {
  return state.lastOutcome ? JSON.stringify(state.lastOutcome) : "";
}

/**
 * The next view after a state event. The first event after the page loads
 * (`previous` null) never flashes an old outcome; a take that ends, or a new
 * outcome while idle (a take refused before it started), flashes it.
 */
export function nextView(current: OverlayView, previous: UiState | null, next: UiState, now: number): OverlayView {
  switch (next.phase.kind) {
    case "preparing":
      return { kind: "preparing" };
    case "listening": {
      const sameTake = current.kind === "listening" && current.take === next.take;
      return {
        kind: "listening",
        take: next.take,
        startedAt: sameTake ? current.startedAt : now - Math.max(0, next.listeningMs ?? 0),
        toSambler: next.destination === "sambler",
      };
    }
    case "finalizing":
      return { kind: "finalizing" };
    case "idle": {
      if (previous && next.lastOutcome) {
        const ended = previous.phase.kind !== "idle";
        if (ended || outcomeKey(previous) !== outcomeKey(next)) {
          return { kind: "outcome", flash: flashFor(next.lastOutcome), until: now + FLASH_MS };
        }
      }
      return expire(current, now);
    }
  }
}

/** An outcome past its time becomes hidden; anything else is kept. */
export function expire(view: OverlayView, now: number): OverlayView {
  return view.kind === "outcome" && now >= view.until ? HIDDEN : view;
}

/** "0:07", "4:31". */
export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}
