// French wording of the Activité screen: dates, durations, counts. Inputs
// are counters and local dates only.

import { formatCount } from "../../labels/fr";
import { addDays, type SavedTime } from "./model";

const monthLong = new Intl.DateTimeFormat("fr-FR", { month: "long" });
const monthShort = new Intl.DateTimeFormat("fr-FR", { month: "short" });
const weekdayLong = new Intl.DateTimeFormat("fr-FR", { weekday: "long" });
const weekdayShort = new Intl.DateTimeFormat("fr-FR", { weekday: "short" });

/** « 1er juillet », « 24 août », « 3 janvier 2026 » (year only when not `reference`'s). */
export function formatDay(date: Date, reference: Date = new Date()): string {
  const day = date.getDate() === 1 ? "1er" : String(date.getDate());
  const year = date.getFullYear() === reference.getFullYear() ? "" : ` ${date.getFullYear()}`;
  return `${day} ${monthLong.format(date)}${year}`;
}

/** « vendredi 20 novembre ». */
export function formatWeekday(date: Date, reference: Date = new Date()): string {
  return `${weekdayLong.format(date)} ${formatDay(date, reference)}`;
}

/** « ven. », « sam. ». */
export function shortWeekday(date: Date): string {
  return weekdayShort.format(date);
}

/** « sam. 21 nov. » */
export function shortDay(date: Date): string {
  return `${weekdayShort.format(date)} ${date.getDate() === 1 ? "1er" : date.getDate()} ${monthShort.format(date)}`;
}

/** « août », « sept. » */
export function shortMonth(date: Date): string {
  return monthShort.format(date);
}

/** « aujourd’hui », « hier » or « vendredi 20 novembre ». */
export function relativeDay(date: Date, today: Date): string {
  if (date.getTime() === today.getTime()) return "aujourd’hui";
  if (date.getTime() === addDays(today, -1).getTime()) return "hier";
  return formatWeekday(date, today);
}

/**
 * A large figure, « 48 312 »: a no-break space as the thousands separator,
 * since the narrow one nearly vanishes in the display face and in small
 * chart labels. Tables keep `formatCount`: their tabular figures align.
 */
export function formatFigure(value: number): string {
  return formatCount(value).replace(/ /g, " ");
}

/** « 0 mot », « 1 mot », « 1 284 mots ». */
export function wordsText(value: number): string {
  return `${formatFigure(value)} ${value < 2 ? "mot" : "mots"}`;
}

/** « 1 jour », « 31 jours ». */
export function daysText(value: number): string {
  return `${formatCount(value)} ${value < 2 ? "jour" : "jours"}`;
}

/** « du 24 août au 23 septembre », or « le 24 août » for a single day. */
export function rangeText(from: Date, to: Date, reference: Date = new Date()): string {
  if (from.getTime() === to.getTime()) return `le ${formatDay(from, reference)}`;
  return `du ${formatDay(from, reference)} au ${formatDay(to, reference)}`;
}

export interface Duration {
  hours: number;
  minutes: number;
}

export function splitMinutes(total: number): Duration {
  const minutes = Math.max(0, Math.floor(total));
  return { hours: Math.floor(minutes / 60), minutes: minutes % 60 };
}

/** « 15 h 42 », « 42 min », « 0 min ». */
export function formatDuration(total: number): string {
  const { hours, minutes } = splitMinutes(total);
  if (hours === 0) return `${minutes} min`;
  return `${formatCount(hours)} h ${String(minutes).padStart(2, "0")}`;
}

/** « 15 heures et 42 minutes », for screen readers. */
export function spokenDuration(total: number): string {
  const { hours, minutes } = splitMinutes(total);
  const minutesText = `${minutes} ${minutes < 2 ? "minute" : "minutes"}`;
  if (hours === 0) return minutesText;
  const hoursText = `${formatCount(hours)} ${hours < 2 ? "heure" : "heures"}`;
  return minutes === 0 ? hoursText : `${hoursText} et ${minutesText}`;
}

/** The sentence under the figures: how the time saved is computed, with its numbers. */
export function savedExplanation(words: number, saved: SavedTime): string {
  const speaking = saved.estimated
    ? `moins environ ${formatDuration(saved.speakingMinutes)} de parole, d’après votre vitesse moyenne`
    : `moins ${formatDuration(saved.speakingMinutes)} de parole réelle`;
  return (
    `Temps gagné : ${wordsText(words)} ÷ 40 mots/min, soit ${formatDuration(saved.typingMinutes)} de frappe au clavier, ` +
    `${speaking}.`
  );
}
