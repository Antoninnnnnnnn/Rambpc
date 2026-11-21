// Pure helpers that put a dictated result into a note at the cursor. They
// handle text but never log, announce or store it.

import type { DraftView } from "../../ipc/contract";
import { UNTRANSCRIBED_MARK } from "./labels";

export interface DictatedText {
  text: string;
  /** Some passages could not be recovered and are marked in the text. */
  gaps: boolean;
}

/**
 * The text of a recent result: the rendered full text when complete,
 * otherwise the parts that arrived, with the native marker where a passage
 * is missing or was refused.
 */
export function dictatedText(view: DraftView): DictatedText {
  if (view.fullText !== null) return { text: view.fullText, gaps: false };
  let gaps = false;
  const pieces = view.parts.flatMap((part) => {
    if (part.text !== null && (part.status === "admitted" || part.status === "needs-review")) return [part.text];
    if (part.status === "no-text") return [];
    gaps = true;
    return [UNTRANSCRIBED_MARK];
  });
  return { text: pieces.join(" "), gaps };
}

/**
 * Plain-text tidying: Rambler formats some enumerations as Markdown bullets
 * ("\n\n*   élément"); a note shows them as « • élément ». Trailing spaces and
 * runs of blank lines go; leading line breaks are kept for the caller.
 */
export function tidyDictation(raw: string): string {
  return raw
    .replace(/\r\n?/g, "\n")
    .replace(/^[ \t]*[*•][ \t]+/gm, "• ")
    .replace(/[ \t]+$/gm, "")
    .replace(/\n{3,}/g, "\n\n")
    .replace(/\s+$/, "");
}

export interface Insertion {
  text: string;
  /** Caret position right after the inserted text. */
  caret: number;
}

const NO_SPACE_BEFORE = /^[\s.,;:!?…)\]}»”’]/;

/**
 * Insert `raw` in `value` in place of the selection [start, end), with the
 * spaces a person would type: none at the start of a line, one between words.
 * A result that opens with line breaks (a list after a sentence) keeps them.
 */
export function insertAtSelection(value: string, start: number, end: number, raw: string): Insertion {
  const from = Math.max(0, Math.min(start, value.length));
  const to = Math.max(from, Math.min(end, value.length));
  const before = value.slice(0, from);
  const after = value.slice(to);
  let piece = tidyDictation(raw);
  if (!piece.trim()) return { text: value, caret: from };

  const lead = /^\s*/.exec(piece)?.[0] ?? "";
  const body = piece.slice(lead.length);
  if (before === "" || before.endsWith("\n")) {
    piece = body;
  } else if (lead.includes("\n")) {
    piece = lead.replace(/[^\n]/g, "") + body;
  } else {
    piece = (/\s$/.test(before) ? "" : " ") + body;
  }
  if (after !== "" && !NO_SPACE_BEFORE.test(after)) piece += " ";
  return { text: before + piece + after, caret: before.length + piece.length };
}
