// Dictation into the open note: the « Dicter dans la note » button, or the
// global shortcut while the note's field has the focus (the take then goes to
// Sambler). When the take ends with a new recent result, its text goes in at
// the cursor and the note is saved; the result then leaves « Récents ».
//
// The dictated text only passes through here on its way into the note: it is
// never announced, logged or kept outside the note's own state.

import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { announcement, useAnnounce } from "../../a11y/announcer";
import { toCommandError, useBridge } from "../../ipc/bridge";
import type { PhaseKind, TakeId, UiState } from "../../ipc/contract";
import {
  commandErrorText,
  describeOutcome,
  isMicAccessDenied,
  micErrorText,
  unavailableReasonText,
  type Tone,
} from "../../labels/fr";
import { useUiCommands } from "../../state/commands";
import { useUiState } from "../../state/ui";
import { dictatedText } from "./insertText";

export type ActivePhase = Exclude<PhaseKind, "idle">;

export type DictationView =
  | { kind: "off" }
  | { kind: "active"; phase: ActivePhase; take: TakeId | null }
  /** The take ended: its text is being read and inserted. */
  | { kind: "inserting" };

export interface DictationNotice {
  tone: Tone;
  title: string;
  detail?: string;
}

/** Why the button cannot start a take right now, and what helps. */
export interface DictationBlock {
  reason: string;
  action: "enable" | "access" | "retry" | null;
}

interface Armed {
  /** The note the text is for. */
  noteKey: string;
  take: TakeId | null;
  /** The take was seen running (the button's command may answer after the first event). */
  active: boolean;
}

export function dictationBlock(state: UiState, ours: boolean): DictationBlock | null {
  if (state.phase.kind !== "idle" && !ours) {
    return { reason: "Une autre dictée est en cours. Attendez sa fin pour dicter dans la note.", action: null };
  }
  const availability = state.availability;
  switch (availability.kind) {
    case "ready":
    case "connecting":
      return null;
    case "needs-consent":
      return { reason: "La dictée n’est pas activée : activez-la d’abord avec Rambler.", action: "enable" };
    case "needs-access":
      return { reason: "L’accès à Rambler n’est pas configuré.", action: "access" };
    case "unavailable":
      return { reason: `Dictée indisponible. ${unavailableReasonText(availability.reason)}`, action: "retry" };
  }
}

interface Options {
  /** Key of the open note (null: no note open). */
  noteKey: string | null;
  field: RefObject<HTMLTextAreaElement | null>;
  /** Put the text at the field's cursor and save; resolves true once stored. */
  insert: (text: string) => Promise<boolean>;
}

export interface NoteDictation {
  view: DictationView;
  notice: DictationNotice | null;
  /** A dictation was just inserted (short visual confirmation). */
  inserted: boolean;
  block: DictationBlock | null;
  start: () => Promise<void>;
  finish: () => void;
  cancel: () => void;
  dismissNotice: () => void;
}

export function useNoteDictation({ noteKey, field, insert }: Options): NoteDictation {
  const bridge = useBridge();
  const state = useUiState();
  const commands = useUiCommands();
  const announce = useAnnounce();
  const armed = useRef<Armed | null>(null);
  const previous = useRef<UiState>(state);
  const noteKeyRef = useRef(noteKey);
  const insertRef = useRef(insert);
  const [view, setView] = useState<DictationView>({ kind: "off" });
  const [notice, setNotice] = useState<DictationNotice | null>(null);
  const [inserted, setInserted] = useState(false);
  const insertedTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    noteKeyRef.current = noteKey;
    insertRef.current = insert;
  });

  useEffect(() => () => window.clearTimeout(insertedTimer.current), []);

  const flashInserted = useCallback(() => {
    setInserted(true);
    window.clearTimeout(insertedTimer.current);
    insertedTimer.current = window.setTimeout(() => setInserted(false), 4000);
  }, []);

  /** The take ended: read its result and put it in the note. */
  const complete = useCallback(
    async (take: Armed, ended: UiState) => {
      const outcome = ended.lastOutcome;
      if (take.noteKey !== noteKeyRef.current) {
        setView({ kind: "off" });
        if (outcome?.kind === "kept-in-sambler") {
          setNotice({
            tone: "info",
            title: "Texte gardé dans Récents",
            detail: "La note a changé pendant la dictée : le texte vous attend dans Récents, sur l’Accueil.",
          });
        }
        return;
      }
      const recovered =
        outcome?.kind === "kept-in-sambler" ||
        outcome?.kind === "inserted-partially" ||
        (outcome?.kind === "interrupted" && outcome.recovered);
      if (!outcome || !recovered) {
        setView({ kind: "off" });
        if (outcome?.kind === "cancelled") {
          setNotice({ tone: "info", title: "Dictée annulée", detail: "Rien n’a été ajouté à la note." });
        } else if (outcome) {
          const described = describeOutcome(outcome, false);
          setNotice({ tone: described.tone, title: described.title, detail: described.detail });
        }
        return;
      }

      setView({ kind: "inserting" });
      try {
        const views = await bridge.invoke("get_recent_drafts");
        const result =
          take.take !== null ? views.find((each) => each.meta.take === take.take) : views[0];
        const dictated = result ? dictatedText(result) : null;
        if (!result || !dictated || !dictated.text.trim()) {
          setNotice({ tone: "info", title: "Aucun texte obtenu", detail: "Rien n’a été ajouté à la note." });
          return;
        }
        if (take.noteKey !== noteKeyRef.current) {
          setNotice({
            tone: "info",
            title: "Texte gardé dans Récents",
            detail: "La note a changé pendant la dictée : le texte vous attend dans Récents, sur l’Accueil.",
          });
          return;
        }
        const stored = await insertRef.current(dictated.text);
        if (!stored) {
          setNotice({
            tone: "warning",
            title: "Texte inséré, mais pas encore enregistré",
            detail: "Il reste aussi dans Récents, sur l’Accueil, jusqu’à ce que la note soit enregistrée.",
          });
          announce(announcement("Dictée insérée dans la note, mais la note n’a pas pu être enregistrée."));
          return;
        }
        flashInserted();
        if (dictated.gaps) {
          setNotice({
            tone: "warning",
            title: "Dictée insérée, avec des passages manquants",
            detail: "Les passages perdus sont marqués « [Passage non transcrit] » dans la note.",
          });
          announce(announcement("Dictée insérée dans la note. Des passages manquent : ils sont marqués dans le texte."));
        } else {
          setNotice(null);
          announce(announcement("Dictée insérée dans la note et enregistrée."));
        }
        // The text is now in the note: it no longer needs to wait in Récents.
        void commands.clearDraft(result.meta.take).catch(() => undefined);
      } catch {
        setNotice({
          tone: "error",
          title: "Le texte dicté n’a pas pu être lu",
          detail: "Il vous attend dans Récents, sur l’Accueil.",
        });
      } finally {
        setView({ kind: "off" });
      }
    },
    [announce, bridge, commands, flashInserted],
  );

  // Follow the takes: the ones this note started, and the global shortcut
  // pressed while the note's field has the focus.
  useEffect(() => {
    const before = previous.current;
    previous.current = state;
    const phase = state.phase.kind;
    let current = armed.current;

    if (
      !current &&
      before.phase.kind === "idle" &&
      phase !== "idle" &&
      state.destination !== "field" &&
      noteKeyRef.current !== null &&
      field.current !== null &&
      document.activeElement === field.current &&
      document.hasFocus()
    ) {
      current = { noteKey: noteKeyRef.current, take: state.take, active: false };
      armed.current = current;
      setNotice(null);
    }
    if (!current) return;

    if (phase !== "idle") {
      // A take that goes to another application is not for the note.
      if (state.destination === "field") {
        armed.current = null;
        setView({ kind: "off" });
        return;
      }
      current.active = true;
      if (current.take === null) current.take = state.take;
      setView({ kind: "active", phase, take: current.take });
      return;
    }
    if (!current.active) return;
    armed.current = null;
    void complete(current, state);
  }, [state, field, complete]);

  const start = useCallback(async () => {
    const key = noteKeyRef.current;
    if (armed.current || key === null) return;
    setNotice(null);
    armed.current = { noteKey: key, take: null, active: false };
    try {
      const next = await commands.dictateInSambler();
      const current = armed.current;
      if (!current || current.active) return;
      if (next.phase.kind !== "idle" && next.take !== null) {
        current.active = true;
        current.take = next.take;
        setView({ kind: "active", phase: next.phase.kind, take: next.take });
        return;
      }
      armed.current = null;
      setView({ kind: "off" });
      if (next.lastOutcome?.kind === "refused") {
        const described = describeOutcome(next.lastOutcome, false);
        setNotice({ tone: described.tone, title: described.title, detail: described.detail });
      } else {
        const block = dictationBlock(next, false);
        setNotice({ tone: "info", title: "La dictée n’a pas démarré", detail: block?.reason });
      }
    } catch (raw) {
      armed.current = null;
      setView({ kind: "off" });
      const error = toCommandError(raw);
      const detail = isMicAccessDenied(error) ? micErrorText({ kind: "access-denied" }) : commandErrorText(error);
      setNotice({ tone: "error", title: "La dictée n’a pas démarré", detail });
    }
  }, [commands]);

  const take = view.kind === "active" ? view.take : null;

  const finish = useCallback(() => {
    if (take !== null) void commands.finishTake(take).catch(() => undefined);
  }, [commands, take]);

  const cancel = useCallback(() => {
    if (take !== null) void commands.cancelTake(take).catch(() => undefined);
  }, [commands, take]);

  const ours = view.kind !== "off";
  return {
    view,
    notice,
    inserted,
    block: dictationBlock(state, ours),
    start,
    finish,
    cancel,
    dismissNotice: () => setNotice(null),
  };
}
