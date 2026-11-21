// French copy and formatters for the Carnet. Inputs are dates, counts,
// statuses and file names: never the text of a note.

import type { CommandError, FileTranscription } from "../../ipc/contract";
import { commandErrorText, formatCount, wordsUnit } from "../../labels/fr";

/** What the native side reads (transcribe/decode.rs), as shown to the person. */
export const AUDIO_FORMATS = "WAV, Ogg Opus, MP3, M4A/AAC, WMA et FLAC";
export const AUDIO_LIMITS = "jusqu’à 2 heures et 2 Go";

export const UNSUPPORTED_NOTES = "Le Carnet n’est pas disponible dans cette version de Sambler.";
export const UNSUPPORTED_TRANSCRIPTION = "La transcription de fichiers n’est pas disponible dans cette version de Sambler.";

/** The native marker of a passage Rambler could not transcribe. */
export const UNTRANSCRIBED_MARK = "[Passage non transcrit]";

/**
 * A command this native side does not know (an older Sambler): Tauri rejects
 * with a plain string, which `toCommandError` turns into the `unknown` code.
 */
export function isUnsupported(error: CommandError): boolean {
  return error.code === "unknown";
}

export function transcriptionErrorText(error: CommandError): string {
  return isUnsupported(error) ? UNSUPPORTED_TRANSCRIPTION : commandErrorText(error);
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

const clock = new Intl.DateTimeFormat("fr-FR", { hour: "2-digit", minute: "2-digit" });
const weekday = new Intl.DateTimeFormat("fr-FR", { weekday: "long" });
const dayMonth = new Intl.DateTimeFormat("fr-FR", { day: "numeric", month: "short" });
const dayMonthYear = new Intl.DateTimeFormat("fr-FR", { day: "numeric", month: "short", year: "numeric" });
const fullDate = new Intl.DateTimeFormat("fr-FR", { dateStyle: "long", timeStyle: "short" });

function startOfDay(ms: number): number {
  const date = new Date(ms);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** « À l’instant », « Il y a 5 min », « Il y a 3 h », « Hier, 14:02 », « Mardi », « 12 nov. », « 12 nov. 2024 ». */
export function relativeDate(ms: number, now = Date.now()): string {
  const age = now - ms;
  if (age < MINUTE) return "À l’instant";
  if (age < HOUR) return `Il y a ${Math.floor(age / MINUTE)} min`;
  const days = Math.round((startOfDay(now) - startOfDay(ms)) / DAY);
  if (days <= 0) return `Il y a ${Math.floor(age / HOUR)} h`;
  if (days === 1) return `Hier, ${clock.format(ms)}`;
  if (days < 7) return capitalize(weekday.format(ms));
  return new Date(ms).getFullYear() === new Date(now).getFullYear() ? dayMonth.format(ms) : dayMonthYear.format(ms);
}

/** « Modifiée à l’instant », « Modifiée il y a 5 min », « Modifiée hier à 14:02 », « Modifiée le 12 nov. ». */
export function editedText(ms: number, now = Date.now()): string {
  const age = now - ms;
  if (age < MINUTE) return "Modifiée à l’instant";
  if (age < HOUR) return `Modifiée il y a ${Math.floor(age / MINUTE)} min`;
  const days = Math.round((startOfDay(now) - startOfDay(ms)) / DAY);
  if (days <= 0) return `Modifiée il y a ${Math.floor(age / HOUR)} h`;
  if (days === 1) return `Modifiée hier à ${clock.format(ms)}`;
  if (days < 7) return `Modifiée ${weekday.format(ms)}`;
  const date = new Date(ms).getFullYear() === new Date(now).getFullYear() ? dayMonth.format(ms) : dayMonthYear.format(ms);
  return `Modifiée le ${date}`;
}

/** « 21 novembre 2026 à 14:02 », for a tooltip. */
export function longDate(ms: number): string {
  return fullDate.format(ms);
}

export function wordsText(count: number): string {
  return `${formatCount(count)} ${wordsUnit(count)}`;
}

export function notesCountText(count: number): string {
  if (count === 0) return "Aucune note";
  return count === 1 ? "1 note" : `${formatCount(count)} notes`;
}

/** Spoken after a search (a count only, never the query). */
export function searchResultText(count: number): string {
  if (count === 0) return "Aucune note ne correspond.";
  return count === 1 ? "1 note correspond." : `${formatCount(count)} notes correspondent.`;
}

/** Lowercase and without accents, for a forgiving title search. */
export function searchKey(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/** « 42 s », « 7 min 42 s », « 1 h 05 min ». */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  if (hours > 0) return `${hours} h ${String(minutes).padStart(2, "0")} min`;
  if (minutes > 0) return seconds > 0 ? `${minutes} min ${String(seconds).padStart(2, "0")} s` : `${minutes} min`;
  return `${seconds} s`;
}

/** « 45 % », with the French no-break space. */
export function percentText(progress: number): string {
  return `${Math.round(Math.min(1, Math.max(0, progress)) * 100)} %`;
}

/** Remaining time, once enough audio went through to estimate it. */
export function remainingText(job: FileTranscription, now = Date.now()): string | null {
  if (job.status !== "running" || job.progress < 0.08 || job.progress >= 1) return null;
  const elapsed = now - job.startedAt;
  if (elapsed < 5000) return null;
  const left = (elapsed * (1 - job.progress)) / job.progress;
  if (left < MINUTE) return "Moins d’une minute restante";
  const minutes = Math.round(left / MINUTE);
  return minutes === 1 ? "Environ 1 minute restante" : `Environ ${minutes} minutes restantes`;
}

/** « Transcription · réunion.m4a » notes, made by the native transcription. */
export function isTranscriptionTitle(title: string): boolean {
  return title.startsWith("Transcription · ");
}

/** First non-empty line, at most 80 characters (like the native title). */
export function titleOf(text: string): string {
  const line = text.split("\n").find((each) => each.trim()) ?? "";
  return line.trim().slice(0, 80);
}

export function countWords(text: string): number {
  return text.split(/\s+/).filter(Boolean).length;
}

/** The base name of a dropped or chosen path (the full path is never shown). */
export function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Spoken when a transcription ends (no file name, no text). */
export function transcriptionEndAnnouncement(job: FileTranscription): string | null {
  switch (job.status) {
    case "done":
      return `Transcription terminée : ${wordsText(job.words)}, enregistrés dans une nouvelle note du Carnet.`;
    case "failed":
      return job.noteId
        ? "La transcription s’est arrêtée avant la fin. Une note partielle est dans le Carnet."
        : "La transcription a échoué.";
    case "cancelled":
      return "Transcription annulée.";
    case "running":
      return null;
  }
}
