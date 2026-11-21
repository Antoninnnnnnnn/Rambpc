// Carnet: dictated notes and audio file transcriptions (milestone 5).
// Two panes when there is room (the list, the open note), one at a time in a
// small window. Only this screen reads note text (`get_note`) and keeps it in
// its own state; announcements carry counts and fixed copy, never the text.

import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { FieldError, Keys } from "../components/controls";
import { Glyph } from "../components/Icon";
import { insertAtSelection } from "../components/carnet/insertText";
import { isUnsupported, UNSUPPORTED_NOTES } from "../components/carnet/labels";
import { DictationNoticeView, NOTE_MAX_CHARS, NoteEditor } from "../components/carnet/NoteEditor";
import { NoteList } from "../components/carnet/NoteList";
import { TranscriptionPanel } from "../components/carnet/TranscriptionPanel";
import { useNoteDictation } from "../components/carnet/useNoteDictation";
import type { FileTranscription } from "../ipc/contract";
import { commandErrorText } from "../labels/fr";
import { useNoteEditor, useNoteList, useOpenNoteRequests } from "../state/notes";
import { useTranscriptions } from "../state/transcriptions";
import { useUiState } from "../state/ui";
import "./CarnetScreen.css";

type Pane = "list" | "editor";

/** Where the keyboard goes once the screen has re-rendered. */
type FocusTarget = { target: "field" } | { target: "row"; index: number } | { target: "active-row" };
type FocusRequest = FocusTarget & { n: number };

export function CarnetScreen() {
  const list = useNoteList();
  const editor = useNoteEditor({ onSaved: list.upsert });
  const transcriptions = useTranscriptions();
  const state = useUiState();
  const announce = useAnnounce();
  const field = useRef<HTMLTextAreaElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const newButton = useRef<HTMLButtonElement>(null);
  const [query, setQuery] = useState("");
  const [pane, setPane] = useState<Pane>("list");
  const [now, setNow] = useState(() => Date.now());
  const [focus, setFocus] = useState<FocusRequest | null>(null);
  const requests = useRef(0);
  const emptyHint = useId();
  const { open, create, close, setText, flush } = editor;
  const { reload, remove } = list;

  // Dictation into the open note, owned here so that a take outlives a change
  // of note (its text then stays in Récents).
  const currentNote = useRef(editor.note);
  useEffect(() => {
    currentNote.current = editor.note;
  });
  const caret = useRef<{ position: number; focus: boolean } | null>(null);
  const dictation = useNoteDictation({
    noteKey: editor.note?.key ?? null,
    field,
    insert: async (text) => {
      const note = currentNote.current;
      if (!note) return false;
      const element = field.current;
      const start = element ? element.selectionStart : note.text.length;
      const end = element ? element.selectionEnd : start;
      const result = insertAtSelection(note.text, start, end, text);
      if (result.text === note.text) return true;
      if (result.text.length > NOTE_MAX_CHARS) return false;
      // Back to the field when the keyboard was in the note, or nowhere.
      const active = document.activeElement;
      const inNote = active instanceof Element && active.closest("[data-testid='note-editor']") !== null;
      caret.current = { position: result.caret, focus: !active || active === document.body || inNote };
      setText(result.text);
      return flush();
    },
  });

  // A dictation landed: the caret goes right after it.
  const noteText = editor.note?.text;
  useLayoutEffect(() => {
    const pending = caret.current;
    const element = field.current;
    if (!pending || !element) return;
    caret.current = null;
    if (pending.focus) element.focus({ preventScroll: true });
    element.setSelectionRange(pending.position, pending.position);
  }, [noteText]);

  // Relative dates stay true while the screen is open.
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);

  const requestFocus = useCallback((request: FocusTarget) => {
    requests.current += 1;
    setFocus({ ...request, n: requests.current });
  }, []);

  const noteKey = editor.note?.key ?? null;
  useEffect(() => {
    if (!focus) return;
    if (focus.target === "field") {
      if (!noteKey || !field.current) return;
      field.current.focus();
    } else {
      const rows = root.current?.querySelectorAll<HTMLElement>("[data-note-row]") ?? [];
      const active = root.current?.querySelector<HTMLElement>('[data-note-row][aria-current="true"]');
      const target =
        focus.target === "active-row" ? (active ?? rows[0]) : rows[Math.min(focus.index, rows.length - 1)];
      (target ?? root.current?.querySelector<HTMLElement>("[data-note-list-title]") ?? newButton.current)?.focus();
    }
    setFocus(null);
  }, [focus, noteKey, pane]);

  // The open note keeps its text until it is stored: another note waits.
  const blocked = useCallback(() => {
    announce(announcement("La note ouverte n’est pas enregistrée. Réessayez de l’enregistrer avant d’en changer."));
  }, [announce]);

  /** `show`: the person asked for it (the note comes forward with the keyboard). */
  const openNote = useCallback(
    async (id: string, show = true) => {
      const ok = await open(id);
      if (ok && show) {
        setPane("editor");
        requestFocus({ target: "field" });
      } else if (!ok && show && currentNote.current?.status.kind === "error") {
        blocked();
      }
      return ok;
    },
    [open, requestFocus, blocked],
  );

  const newNote = async () => {
    if (await create()) {
      setPane("editor");
      requestFocus({ target: "field" });
    } else if (currentNote.current?.status.kind === "error") {
      blocked();
    }
  };

  const backToList = () => {
    setPane("list");
    requestFocus({ target: "active-row" });
  };

  const onDeleted = (id: string | null) => {
    const rows = Array.from(root.current?.querySelectorAll<HTMLElement>("[data-note-row]") ?? []);
    const index = Math.max(0, rows.findIndex((row) => row.dataset.noteId === id));
    if (id !== null) remove(id);
    setPane("list");
    requestFocus({ target: "row", index });
  };

  const openFromTranscription = async (id: string, job: FileTranscription) => {
    await reload();
    transcriptions.dismiss(job.id);
    await openNote(id);
  };

  // « Ouvrir la note » from the end-of-transcription banner on another screen.
  useOpenNoteRequests((id) => {
    void reload().then(() => openNote(id));
  });

  // A transcription that ends brings its note: the list refreshes and, if no
  // note is open, the new one opens (without taking the keyboard).
  const statuses = useRef<Map<string, FileTranscription["status"]> | null>(null);
  const idleRef = useRef(true);
  useEffect(() => {
    idleRef.current = editor.note === null && editor.loading === null;
  });
  useEffect(() => {
    if (!transcriptions.loaded) return;
    const before = statuses.current;
    statuses.current = new Map(transcriptions.jobs.map((job) => [job.id, job.status]));
    if (!before) return;
    const ended = transcriptions.jobs.find((job) => before.get(job.id) === "running" && job.status !== "running");
    if (!ended) return;
    void reload();
    if (ended.noteId && idleRef.current) void openNote(ended.noteId, false);
  }, [transcriptions.jobs, transcriptions.loaded, reload, openNote]);

  const header = (
    <header className="screen__header carnet__header">
      <h1 tabIndex={-1} data-screen-title>
        Carnet
      </h1>
      <p className="screen__lead">Vos notes dictées et les transcriptions de fichiers audio, gardées sur ce PC.</p>
      {list.status === "ready" ? (
        <div className="carnet__header-actions">
          <button ref={newButton} type="button" className="btn btn--primary" onClick={() => void newNote()}>
            <Glyph name="add" />
            Nouvelle note
          </button>
        </div>
      ) : null}
    </header>
  );

  if (list.status === "unsupported") {
    return (
      <div className="screen carnet" data-testid="carnet">
        {header}
        <div className="empty carnet__unavailable" data-testid="carnet-unsupported">
          <span className="empty__icon">
            <Glyph name="notebook" />
          </span>
          <h2 className="section-title">{UNSUPPORTED_NOTES}</h2>
          <p className="hint">Mettez Sambler à jour pour dicter des notes et transcrire des fichiers audio.</p>
        </div>
      </div>
    );
  }

  if (list.status !== "ready") {
    return (
      <div className="screen carnet" data-testid="carnet">
        {header}
        {list.status === "loading" ? (
          <p className="muted">Chargement du Carnet…</p>
        ) : (
          <div className="stack stack--tight" data-testid="carnet-failed">
            <FieldError>{commandErrorText(list.error ?? { code: "", message: "" }, "Les notes n’ont pas pu être lues.")}</FieldError>
            <div className="btn-row">
              <button type="button" className="btn btn--sm" onClick={() => void reload()}>
                Réessayer
              </button>
            </div>
          </div>
        )}
      </div>
    );
  }

  const draft = editor.note !== null && editor.note.id === null;
  const loadError = editor.loadError;

  return (
    <div ref={root} className="screen carnet" data-pane={pane} data-testid="carnet">
      {header}

      <TranscriptionPanel onOpenNote={(id, job) => void openFromTranscription(id, job)} />

      <div className="carnet__panes">
        <div className="carnet__pane carnet__pane--list">
          <NoteList
            notes={list.notes}
            activeId={editor.note?.id ?? editor.loading}
            draft={draft}
            now={now}
            query={query}
            onQuery={setQuery}
            onOpen={(id) => void openNote(id)}
            onOpenDraft={() => {
              setPane("editor");
              requestFocus({ target: "field" });
            }}
          />
          {list.error ? (
            <FieldError>{commandErrorText(list.error, "La liste des notes n’a pas pu être actualisée.")}</FieldError>
          ) : null}
        </div>

        <div className="carnet__pane carnet__pane--editor">
          {editor.note ? (
            <NoteEditor
              key={editor.note.key}
              editor={editor}
              dictation={dictation}
              field={field}
              now={now}
              onBack={backToList}
              onDeleted={onDeleted}
            />
          ) : editor.loading ? (
            <p className="carnet__placeholder muted">Ouverture de la note…</p>
          ) : loadError ? (
            <div className="carnet__placeholder stack stack--tight" data-testid="note-load-error">
              <FieldError>
                {isUnsupported(loadError) ? UNSUPPORTED_NOTES : commandErrorText(loadError, "Cette note n’a pas pu être ouverte.")}
              </FieldError>
              <div className="btn-row">
                <button type="button" className="btn btn--sm" onClick={() => void close().then(() => reload()).then(backToList)}>
                  Revenir à la liste
                </button>
              </div>
            </div>
          ) : (
            <div className="carnet__empty-editor" data-testid="editor-empty">
              {dictation.notice ? <DictationNoticeView notice={dictation.notice} onClose={dictation.dismissNotice} /> : null}
              <span className="carnet__empty-icon">
                <Glyph name="notebook" />
              </span>
              <p className="carnet__empty-title">Choisissez une note, ou commencez-en une</p>
              <p id={emptyHint} className="hint">
                Dans une note, le curseur à l’endroit voulu, « Dicter dans la note » ou <Keys shortcut={state.shortcut} />{" "}
                y écrit ce que vous dites.
              </p>
              <button type="button" className="btn" aria-describedby={emptyHint} onClick={() => void newNote()}>
                <Glyph name="add" />
                Nouvelle note
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
