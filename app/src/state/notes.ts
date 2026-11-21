// Carnet notes (milestone 5). The list carries metadata only (`list_notes`);
// the text of a note is asked with `get_note` by the Carnet's editor, kept in
// its state while the note is open and saved back with `save_note`. Note text
// never goes into events, logs or the live region.

import { useCallback, useEffect, useRef, useState } from "react";
import { toCommandError, useBridge, type Bridge } from "../ipc/bridge";
import type { CommandError, NoteMeta } from "../ipc/contract";

/** Autosave delay after the last change. */
export const AUTOSAVE_DELAY_MS = 800;

function byUpdate(a: NoteMeta, b: NoteMeta): number {
  return b.updatedAt - a.updatedAt;
}

// ------------------------------------------------------------------ list

export type NotesStatus = "loading" | "ready" | "failed" | "unsupported";

export interface NoteList {
  /** Most recently updated first. */
  notes: NoteMeta[];
  status: NotesStatus;
  /** The last refresh failed (the list shown may be stale). */
  error: CommandError | null;
  reload: () => Promise<void>;
  /** A note was created or saved. */
  upsert: (meta: NoteMeta) => void;
  remove: (id: string) => void;
}

export function useNoteList(): NoteList {
  const bridge = useBridge();
  const [notes, setNotes] = useState<NoteMeta[]>([]);
  const [status, setStatus] = useState<NotesStatus>("loading");
  const [error, setError] = useState<CommandError | null>(null);

  const reload = useCallback(async () => {
    try {
      const list = await bridge.invoke("list_notes");
      setNotes([...list].sort(byUpdate));
      setStatus("ready");
      setError(null);
    } catch (raw) {
      const failure = toCommandError(raw);
      setError(failure);
      setStatus((current) => (failure.code === "unknown" ? "unsupported" : current === "ready" ? "ready" : "failed"));
    }
  }, [bridge]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const upsert = useCallback((meta: NoteMeta) => {
    setNotes((current) => [meta, ...current.filter((note) => note.id !== meta.id)].sort(byUpdate));
  }, []);

  const remove = useCallback((id: string) => {
    setNotes((current) => current.filter((note) => note.id !== id));
  }, []);

  return { notes, status, error, reload, upsert, remove };
}

// ------------------------------------------------------------------ editor

export type SaveStatus =
  /** Stored as shown. */
  | { kind: "clean" }
  /** A new note: created with its first words. */
  | { kind: "new" }
  /** Changes wait for the autosave delay. */
  | { kind: "pending" }
  | { kind: "saving" }
  /** Saved a moment ago (« Enregistré »). */
  | { kind: "saved" }
  | { kind: "error"; error: CommandError };

export interface OpenNote {
  /** Stable while the note is open: its id, or a local key until a new note is created. */
  key: string;
  /** null until a new note is created by its first save. */
  id: string | null;
  text: string;
  meta: NoteMeta | null;
  status: SaveStatus;
}

interface Session extends OpenNote {
  /** The last stored text. */
  saved: string;
  timer: number | undefined;
  /** Saves run one after the other. */
  queue: Promise<void>;
  closed: boolean;
}

export interface NoteEditor {
  note: OpenNote | null;
  /** Id of the note being read. */
  loading: string | null;
  loadError: CommandError | null;
  /** Open a stored note, after saving the current one. False if it stays on the current note. */
  open: (id: string) => Promise<boolean>;
  /** Start a new, empty note (created on its first words). */
  create: () => Promise<boolean>;
  close: () => Promise<boolean>;
  setText: (text: string) => void;
  /** Save now. Resolves true when the text shown is stored. */
  flush: () => Promise<boolean>;
  /** Id of the open note once stored (null for a new note not saved yet). */
  storedId: () => string | null;
  /** Delete the open note for good. */
  remove: () => Promise<{ ok: true } | { ok: false; error: CommandError }>;
}

interface EditorOptions {
  onSaved: (meta: NoteMeta) => void;
}

function isStored(session: Session): boolean {
  return session.text === session.saved || (session.id === null && !session.text.trim());
}

export function useNoteEditor({ onSaved }: EditorOptions): NoteEditor {
  const bridge = useBridge();
  const session = useRef<Session | null>(null);
  const [note, setNote] = useState<OpenNote | null>(null);
  const [loading, setLoading] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<CommandError | null>(null);
  const onSavedRef = useRef(onSaved);
  const newNotes = useRef(0);
  const loadToken = useRef(0);

  useEffect(() => {
    onSavedRef.current = onSaved;
  });

  const publish = useCallback((s: Session) => {
    if (session.current !== s) return;
    setNote({ key: s.key, id: s.id, text: s.text, meta: s.meta, status: s.status });
  }, []);

  const persist = useCallback(
    (s: Session): Promise<void> => {
      window.clearTimeout(s.timer);
      s.timer = undefined;
      s.queue = s.queue.then(async () => {
        if (s.closed) return;
        if (isStored(s)) {
          if (s.status.kind === "pending" || s.status.kind === "error") {
            s.status = s.id === null ? { kind: "new" } : { kind: "saved" };
            publish(s);
          }
          return;
        }
        const text = s.text;
        s.status = { kind: "saving" };
        publish(s);
        try {
          const meta = await bridge.invoke("save_note", { note: { id: s.id, text } });
          s.id = meta.id;
          s.meta = meta;
          s.saved = text;
          s.status = s.text === text ? { kind: "saved" } : { kind: "pending" };
          onSavedRef.current(meta);
        } catch (raw) {
          s.status = { kind: "error", error: toCommandError(raw) };
        }
        publish(s);
      });
      return s.queue;
    },
    [bridge, publish],
  );

  /** Save the current note before showing another one; false when its text could not be stored. */
  const leave = useCallback(async (): Promise<boolean> => {
    const current = session.current;
    if (!current) return true;
    await persist(current);
    if (session.current !== current) return true;
    if (!isStored(current)) return false;
    current.closed = true;
    return true;
  }, [persist]);

  const open = useCallback(
    async (id: string) => {
      if (session.current?.id === id) return true;
      if (!(await leave())) return false;
      const token = ++loadToken.current;
      session.current = null;
      setNote(null);
      setLoading(id);
      setLoadError(null);
      try {
        const { text, ...meta } = await bridge.invoke("get_note", { id });
        if (token !== loadToken.current) return false;
        const s: Session = {
          key: id,
          id,
          text,
          saved: text,
          meta,
          status: { kind: "clean" },
          timer: undefined,
          queue: Promise.resolve(),
          closed: false,
        };
        session.current = s;
        publish(s);
        return true;
      } catch (raw) {
        if (token === loadToken.current) setLoadError(toCommandError(raw));
        return false;
      } finally {
        if (token === loadToken.current) setLoading(null);
      }
    },
    [bridge, leave, publish],
  );

  const create = useCallback(async () => {
    if (!(await leave())) return false;
    loadToken.current += 1;
    setLoading(null);
    setLoadError(null);
    newNotes.current += 1;
    const s: Session = {
      key: `new-${newNotes.current}`,
      id: null,
      text: "",
      saved: "",
      meta: null,
      status: { kind: "new" },
      timer: undefined,
      queue: Promise.resolve(),
      closed: false,
    };
    session.current = s;
    publish(s);
    return true;
  }, [leave, publish]);

  const close = useCallback(async () => {
    if (!(await leave())) return false;
    loadToken.current += 1;
    session.current = null;
    setNote(null);
    setLoading(null);
    setLoadError(null);
    return true;
  }, [leave]);

  const setText = useCallback(
    (text: string) => {
      const s = session.current;
      if (!s || s.text === text) return;
      s.text = text;
      if (s.status.kind !== "saving") {
        s.status = isStored(s) ? (s.id === null ? { kind: "new" } : { kind: "clean" }) : { kind: "pending" };
      }
      publish(s);
      window.clearTimeout(s.timer);
      s.timer = window.setTimeout(() => void persist(s), AUTOSAVE_DELAY_MS);
    },
    [persist, publish],
  );

  const flush = useCallback(async () => {
    const s = session.current;
    if (!s) return true;
    await persist(s);
    return isStored(s);
  }, [persist]);

  const remove = useCallback(async (): Promise<{ ok: true } | { ok: false; error: CommandError }> => {
    const s = session.current;
    if (!s) return { ok: true };
    window.clearTimeout(s.timer);
    // A first save in flight gives the new note its id.
    await s.queue;
    if (s.id !== null) {
      try {
        await bridge.invoke("delete_note", { id: s.id });
      } catch (raw) {
        return { ok: false, error: toCommandError(raw) };
      }
    }
    s.closed = true;
    if (session.current === s) {
      session.current = null;
      setNote(null);
    }
    return { ok: true };
  }, [bridge]);

  // Save right away when the window is hidden (closing it hides it) and when
  // the Carnet goes away.
  useEffect(() => {
    const onHidden = () => {
      if (document.visibilityState === "hidden" && session.current) void persist(session.current);
    };
    document.addEventListener("visibilitychange", onHidden);
    return () => {
      document.removeEventListener("visibilitychange", onHidden);
      if (session.current) void persist(session.current);
    };
  }, [persist]);

  const storedId = useCallback(() => session.current?.id ?? null, []);

  return { note, loading, loadError, open, create, close, setText, flush, storedId, remove };
}

// ------------------------------------------------------------------ open requests

interface OpenChannel {
  pending: string | null;
  handler: ((id: string) => void) | null;
}

const openChannels = new WeakMap<Bridge, OpenChannel>();

function channelFor(bridge: Bridge): OpenChannel {
  let channel = openChannels.get(bridge);
  if (!channel) {
    channel = { pending: null, handler: null };
    openChannels.set(bridge, channel);
  }
  return channel;
}

/** Ask the Carnet to open a note: now if it is shown, else when it next appears. */
export function requestOpenNote(bridge: Bridge, id: string) {
  const channel = channelFor(bridge);
  if (channel.handler) channel.handler(id);
  else channel.pending = id;
}

/** The Carnet's side of `requestOpenNote`. */
export function useOpenNoteRequests(handler: (id: string) => void) {
  const bridge = useBridge();
  const handlerRef = useRef(handler);
  useEffect(() => {
    handlerRef.current = handler;
  });
  useEffect(() => {
    const channel = channelFor(bridge);
    const forward = (id: string) => handlerRef.current(id);
    channel.handler = forward;
    if (channel.pending !== null) {
      const id = channel.pending;
      channel.pending = null;
      forward(id);
    }
    return () => {
      if (channel.handler === forward) channel.handler = null;
    };
  }, [bridge]);
}
