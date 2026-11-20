// French copy for the Récents list (no dictated text here: only part
// statuses, times and counts).

import type { PartStatus } from "../ipc/contract";
import { SAFETY_BLOCK_TEXT } from "./fr";

export type GapKind = Exclude<PartStatus, "admitted">;

/** Inline marker, read with brackets by screen readers: « [passage manquant] ». */
export const GAP_LABELS: Record<GapKind, string> = {
  missing: "passage manquant",
  "needs-review": "à revoir",
  blocked: "bloqué",
  "no-text": "aucun texte obtenu",
};

export const GAP_HELP: Record<GapKind, string> = {
  missing: "perdu lors d’une coupure ; il faut le redicter.",
  "needs-review": "reconnaissance incertaine, exclue de l’insertion automatique.",
  blocked: SAFETY_BLOCK_TEXT,
  "no-text": "aucune parole reconnue sur ce passage.",
};

const clock = new Intl.DateTimeFormat("fr-FR", { hour: "2-digit", minute: "2-digit" });

/** « À l’instant », « Il y a 3 min », then the clock time « 14:32 ». */
export function recentTime(createdAtMs: number, now = Date.now()): string {
  const age = now - createdAtMs;
  if (age < 60_000) return "À l’instant";
  if (age < 60 * 60_000) return `Il y a ${Math.floor(age / 60_000)} min`;
  return clock.format(new Date(createdAtMs));
}

/** Coarse countdown for a list row: « Effacé dans 9 min », « Effacé dans 40 s ». */
export function recentExpiry(expiresAtMs: number, now = Date.now()): string {
  const left = Math.max(0, expiresAtMs - now);
  if (left <= 0) return "Expiré";
  if (left < 60_000) return `Effacé dans ${Math.ceil(left / 1000)} s`;
  return `Effacé dans ${Math.ceil(left / 60_000)} min`;
}

export function recentCountText(count: number): string {
  if (count === 0) return "Aucun texte récent.";
  return count === 1 ? "1 texte récent." : `${count} textes récents.`;
}

export function minutesText(minutes: number): string {
  return minutes === 1 ? "1 minute" : `${minutes} minutes`;
}
