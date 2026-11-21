// French copy for the history view (no entry text here).

import type { Language, RenderProfile } from "../ipc/contract";

export const HISTORY_DAY_CHOICES = [1, 7, 30] as const;

export const LANGUAGE_LABELS: Record<Language, string> = { french: "Français", english: "Anglais" };
export const PROFILE_LABELS: Record<RenderProfile, string> = { natural: "Naturel", faithful: "Fidèle" };

const dateFormat = new Intl.DateTimeFormat("fr-FR", {
  weekday: "long",
  day: "numeric",
  month: "long",
  hour: "2-digit",
  minute: "2-digit",
});

const timeFormat = new Intl.DateTimeFormat("fr-FR", { hour: "2-digit", minute: "2-digit" });
const dayFormat = new Intl.DateTimeFormat("fr-FR", { weekday: "long", day: "numeric", month: "long" });
const dayFormatWithYear = new Intl.DateTimeFormat("fr-FR", {
  weekday: "long",
  day: "numeric",
  month: "long",
  year: "numeric",
});
const numberFormat = new Intl.NumberFormat("fr-FR");

const relative = new Intl.RelativeTimeFormat("fr-FR", { numeric: "auto" });

/** "jeudi 20 novembre à 14:32" */
export function formatEntryDate(ms: number): string {
  return dateFormat.format(new Date(ms));
}

/** "14:32", the time shown in a day group. */
export function formatEntryTime(ms: number): string {
  return timeFormat.format(new Date(ms));
}

/** Local calendar day of a timestamp, as a stable key. */
export function dayKey(ms: number): string {
  const date = new Date(ms);
  return `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
}

/** "Aujourd’hui", "Hier", "Mardi 18 novembre" (with the year when it is not this one). */
export function dayLabel(ms: number, now = Date.now()): string {
  const key = dayKey(ms);
  if (key === dayKey(now)) return "Aujourd’hui";
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  if (key === dayKey(yesterday.getTime())) return "Hier";
  const date = new Date(ms);
  const text = (date.getFullYear() === new Date(now).getFullYear() ? dayFormat : dayFormatWithYear).format(date);
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export interface HistoryDay<T> {
  key: string;
  label: string;
  entries: T[];
}

/** Entries of the same local day together, days in order of first appearance (newest first). */
export function groupByDay<T extends { createdAtMs: number }>(entries: T[], now = Date.now()): HistoryDay<T>[] {
  const days = new Map<string, HistoryDay<T>>();
  for (const entry of entries) {
    const key = dayKey(entry.createdAtMs);
    const day = days.get(key);
    if (day) day.entries.push(entry);
    else days.set(key, { key, label: dayLabel(entry.createdAtMs, now), entries: [entry] });
  }
  return [...days.values()];
}

/** Words of a dictation, 0 when its text cannot be read. */
export function wordCount(text: string | null): number {
  return text ? text.split(/\s+/).filter(Boolean).length : 0;
}

export function wordsText(count: number): string {
  return count === 1 ? "1 mot" : `${numberFormat.format(count)} mots`;
}

/**
 * "6 dictées · 81 mots". `partial`: the day may continue on the next page, so
 * the count is of the dictations shown.
 */
export function dayCountText(count: number, words: number, partial = false): string {
  const dictations = count === 1 ? `1 dictée${partial ? " affichée" : ""}` : `${count} dictées${partial ? " affichées" : ""}`;
  return words > 0 ? `${dictations} · ${wordsText(words)}` : dictations;
}

/** "Expire dans 6 jours", "Expire dans 3 heures", "Expire bientôt". */
export function formatExpiry(expiresAtMs: number, now = Date.now()): string {
  const left = expiresAtMs - now;
  if (left <= 60_000) return "Expire bientôt";
  const hours = left / 3_600_000;
  if (hours >= 24) return `Expire ${relative.format(Math.floor(hours / 24), "day")}`;
  if (hours >= 1) return `Expire ${relative.format(Math.floor(hours), "hour")}`;
  return `Expire ${relative.format(Math.ceil(left / 60_000), "minute")}`;
}

export function daysLabel(days: number): string {
  return days === 1 ? "1 jour" : `${days} jours`;
}

export function entryCountText(count: number): string {
  if (count === 0) return "Aucune dictée enregistrée.";
  return count === 1 ? "1 dictée enregistrée." : `${count} dictées enregistrées.`;
}

export function resultCountText(count: number, more: boolean): string {
  if (count === 0) return "Aucune dictée trouvée.";
  const base = count === 1 ? "1 dictée trouvée" : `${count} dictées trouvées`;
  return more ? `${base}, d’autres peuvent être affichées.` : `${base}.`;
}
