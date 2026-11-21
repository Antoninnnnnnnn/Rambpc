// The notes of the Carnet: a title search and the list, most recently
// updated first (title, relative date, word count). One Tab stop for the
// whole list; the arrow keys, Début and Fin move within it; Entrée opens.

import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { announcement, useAnnounce } from "../../a11y/announcer";
import type { NoteMeta } from "../../ipc/contract";
import { Glyph } from "../Icon";
import {
  isTranscriptionTitle,
  longDate,
  notesCountText,
  relativeDate,
  searchKey,
  searchResultText,
  wordsText,
} from "./labels";
import "./carnet.css";

interface Props {
  notes: NoteMeta[];
  /** The open note (null: none, or a new note not saved yet). */
  activeId: string | null;
  /** A new note is open and not saved yet: shown first in the list. */
  draft: boolean;
  now: number;
  query: string;
  onQuery: (query: string) => void;
  onOpen: (id: string) => void;
  /** Back to the new, unsaved note. */
  onOpenDraft: () => void;
}

export function NoteList({ notes, activeId, draft, now, query, onQuery, onOpen, onOpenDraft }: Props) {
  const announce = useAnnounce();
  const ids = { heading: useId(), search: useId(), count: useId() };
  const listRef = useRef<HTMLUListElement>(null);
  const [focusIndex, setFocusIndex] = useState<number | null>(null);

  const shown = useMemo(() => {
    const key = searchKey(query.trim());
    if (!key) return notes;
    return notes.filter((note) => searchKey(note.title).includes(key));
  }, [notes, query]);

  // A count after a pause in typing (never the query itself).
  const searching = query.trim() !== "";
  useEffect(() => {
    if (!searching) return;
    const timer = window.setTimeout(() => announce(announcement(searchResultText(shown.length))), 700);
    return () => window.clearTimeout(timer);
  }, [searching, query, shown.length, announce]);

  const rows = draft ? shown.length + 1 : shown.length;
  const activeIndex = draft ? 0 : Math.max(0, shown.findIndex((note) => note.id === activeId));
  const tabIndex = focusIndex !== null && focusIndex < rows ? focusIndex : activeIndex;

  const focusRow = (index: number) => {
    const buttons = listRef.current?.querySelectorAll<HTMLButtonElement>("[data-note-row]");
    if (!buttons || buttons.length === 0) return;
    const target = Math.max(0, Math.min(index, buttons.length - 1));
    setFocusIndex(target);
    buttons[target].focus();
  };

  const onRowKey = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        focusRow(index + 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        if (index === 0) document.getElementById(ids.search)?.focus();
        else focusRow(index - 1);
        break;
      case "Home":
        event.preventDefault();
        focusRow(0);
        break;
      case "End":
        event.preventDefault();
        focusRow(rows - 1);
        break;
    }
  };

  return (
    <section className="note-list" aria-labelledby={ids.heading} data-testid="note-list">
      <div className="note-list__head">
        <h2 id={ids.heading} className="note-list__title" tabIndex={-1} data-note-list-title>
          Notes
        </h2>
        <span className="note-list__count" id={ids.count}>
          {searching ? `${shown.length} sur ${notes.length}` : notesCountText(notes.length)}
        </span>
      </div>

      <div className="note-list__search" role="search">
        <label htmlFor={ids.search} className="visually-hidden">
          Rechercher dans les titres des notes
        </label>
        <Glyph name="search" className="note-list__search-icon" />
        <input
          id={ids.search}
          className="input note-list__search-input"
          type="search"
          value={query}
          placeholder="Rechercher un titre"
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => onQuery(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              focusRow(0);
            }
          }}
          data-testid="note-search"
        />
      </div>

      {notes.length === 0 && !draft ? (
        <p className="note-list__empty" data-testid="notes-empty">
          Aucune note pour l’instant. Créez-en une, dictez, ou transcrivez un fichier audio.
        </p>
      ) : shown.length === 0 && !draft ? (
        <p className="note-list__empty" data-testid="notes-no-match">
          Aucun titre ne correspond à cette recherche.
        </p>
      ) : (
        <ul ref={listRef} className="note-list__items" aria-labelledby={ids.heading} aria-describedby={ids.count}>
          {draft ? (
            <li>
              <button
                type="button"
                className="note-row note-row--draft"
                aria-current="true"
                tabIndex={tabIndex === 0 ? 0 : -1}
                data-note-row
                onFocus={() => setFocusIndex(0)}
                onKeyDown={(event) => onRowKey(event, 0)}
                onClick={onOpenDraft}
              >
                <span className="note-row__title">Nouvelle note</span>
                <span className="note-row__meta">Pas encore enregistrée</span>
              </button>
            </li>
          ) : null}
          {shown.map((note, position) => {
            const index = draft ? position + 1 : position;
            const active = !draft && note.id === activeId;
            const transcription = isTranscriptionTitle(note.title);
            return (
              <li key={note.id}>
                <button
                  type="button"
                  className={active ? "note-row note-row--active" : "note-row"}
                  aria-current={active ? "true" : undefined}
                  tabIndex={tabIndex === index ? 0 : -1}
                  data-note-row
                  data-note-id={note.id}
                  data-testid="note-row"
                  onFocus={() => setFocusIndex(index)}
                  onKeyDown={(event) => onRowKey(event, index)}
                  onClick={() => onOpen(note.id)}
                >
                  <span className="note-row__title">
                    {transcription ? <Glyph name="file-audio" className="note-row__kind" /> : null}
                    <span className="note-row__title-text">{note.title || "Note sans titre"}</span>
                  </span>
                  <span className="note-row__meta">
                    <time dateTime={new Date(note.updatedAt).toISOString()} title={longDate(note.updatedAt)}>
                      {relativeDate(note.updatedAt, now)}
                    </time>
                    <span aria-hidden="true"> · </span>
                    <span>{wordsText(note.words)}</span>
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
