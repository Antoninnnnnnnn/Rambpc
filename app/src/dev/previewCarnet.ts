// DEV PREVIEW ONLY: simulated Carnet notes and audio file transcriptions
// (milestone 5). Never part of a production build.
//
// A file dropped on the window is simulated from the console:
//   window.__samblerDropFile("C:\\Users\\Antonin\\Documents\\réunion.m4a")
// A file name containing « coupure » stops halfway with a partial note.

import { EVENT_FILE_DROPPED, EVENT_TRANSCRIPTION, type FileTranscription, type Note, type NoteMeta } from "../ipc/contract";

declare global {
  interface Window {
    /** Dev preview only: what the native side emits when a file is dropped on the window. */
    __samblerDropFile?: (path: string) => void;
  }
}

const MINUTE = 60_000;

function meta(note: Note): NoteMeta {
  const { text: _text, ...rest } = note;
  return rest;
}

function titleOf(text: string): string {
  const line = text.split("\n").find((l) => l.trim()) ?? "";
  return line.trim().slice(0, 80) || "Note sans titre";
}

function countWords(text: string): number {
  return text.split(/\s+/).filter(Boolean).length;
}

function sampleNotes(): Note[] {
  const now = Date.now();
  const texts = [
    "Idées pour la présentation de lundi\n\nCommencer par la démo en direct, puis les chiffres du trimestre. Garder la partie budget pour la fin et prévoir dix minutes de questions.",
    "Transcription · réunion-equipe.m4a\n\nBonjour à tous, on commence par le point sur le recrutement. Deux candidats sont retenus pour le poste de développeur, les entretiens finaux ont lieu jeudi.",
    "Courses\n\nLait d’avoine, citrons, pâtes fraîches, piles AA.",
    "Ordre du jour du comité\n\n• Budget du trimestre\n• Recrutement des deux postes\n• Questions diverses",
    "Lettre à Mme Garnier\n\nMadame, je vous remercie pour votre retour sur le devis. Comme convenu par téléphone, je vous propose de commencer les travaux le 2 décembre.",
  ];
  return texts.map((text, index) => {
    const created = now - (index * index * 22 + 3) * 60 * MINUTE;
    return {
      id: `note-${index + 1}`,
      title: titleOf(text),
      createdAt: created,
      updatedAt: created + 12 * MINUTE,
      words: countWords(text),
      text,
    };
  });
}

type Emit = (event: string, payload: unknown) => void;
type Fail = (code: string, message: string) => never;

export function createPreviewCarnet(emit: Emit, fail: Fail) {
  let notes: Note[] = sampleNotes();
  let transcriptions: FileTranscription[] = [];
  let nextNote = notes.length + 1;
  let nextJob = 1;
  let timer = 0;

  const saveNote = (id: string | null, text: string): NoteMeta => {
    const now = Date.now();
    if (id === null) {
      const note: Note = { id: `note-${nextNote++}`, title: titleOf(text), createdAt: now, updatedAt: now, words: countWords(text), text };
      notes = [note, ...notes];
      return meta(note);
    }
    const existing = notes.find((note) => note.id === id);
    if (!existing) fail("note-not-found", "Cette note n’existe plus.");
    const updated: Note = { ...existing!, text, title: titleOf(text), updatedAt: now, words: countWords(text) };
    notes = [updated, ...notes.filter((note) => note.id !== id)];
    return meta(updated);
  };

  const publish = (job: FileTranscription) => {
    transcriptions = [job, ...transcriptions.filter((item) => item.id !== job.id)];
    emit(EVENT_TRANSCRIPTION, structuredClone(job));
    return structuredClone(job);
  };

  if (import.meta.env.DEV) {
    window.__samblerDropFile = (path: string) => emit(EVENT_FILE_DROPPED, path);
  }

  return {
    list_notes: () => [...notes].sort((a, b) => b.updatedAt - a.updatedAt).map(meta),
    get_note: (args?: Record<string, unknown>) => {
      const note = notes.find((item) => item.id === args!.id);
      if (!note) fail("note-not-found", "Cette note n’existe plus.");
      return structuredClone(note!);
    },
    save_note: (args?: Record<string, unknown>) => {
      const input = args!.note as { id: string | null; text: string };
      return saveNote(input.id, input.text);
    },
    copy_note: (args?: Record<string, unknown>) => {
      if (!notes.some((note) => note.id === args!.id)) fail("note-not-found", "Cette note n’existe plus.");
    },
    delete_note: (args?: Record<string, unknown>) => {
      notes = notes.filter((note) => note.id !== args!.id);
    },
    pick_audio_file: () => "C:\\Users\\Antonin\\Documents\\Enregistrements\\entretien-client.m4a",
    transcribe_file: (args?: Record<string, unknown>) => {
      if (transcriptions.some((item) => item.status === "running")) {
        fail("transcription-busy", "Une transcription est déjà en cours.");
      }
      const path = String(args!.path);
      const fileName = path.split(/[\\/]/).pop() ?? path;
      const durationMs = 7 * MINUTE + 42_000;
      let job: FileTranscription = {
        id: `job-${nextJob++}`,
        fileName,
        durationMs,
        status: "running",
        progress: 0,
        words: 0,
        noteId: null,
        error: null,
        startedAt: Date.now(),
      };
      const breaks = /coupure/i.test(fileName);
      window.clearInterval(timer);
      timer = window.setInterval(() => {
        if (job.status !== "running") return window.clearInterval(timer);
        const progress = Math.min(1, job.progress + 0.02);
        job = { ...job, progress, words: Math.round(progress * 1_180) };
        if (breaks && progress >= 0.55) {
          window.clearInterval(timer);
          const text = `Transcription · ${fileName}\n\nBonjour à tous, on commence par le point sur le budget. [Passage non transcrit]`;
          const saved = saveNote(null, text);
          job = {
            ...job,
            status: "failed",
            noteId: saved.id,
            error: "La connexion au service a été perdue pendant la transcription.",
          };
        } else if (progress >= 1) {
          window.clearInterval(timer);
          const text = `Transcription · ${fileName}\n\nMerci d’avoir pris le temps de nous recevoir. Nous avons repris les trois points du devis : le calendrier, le budget et la maintenance.`;
          const saved = saveNote(null, text);
          job = { ...job, status: "done", noteId: saved.id };
        }
        publish(job);
      }, 250);
      return publish(job);
    },
    cancel_transcription: (args?: Record<string, unknown>) => {
      const job = transcriptions.find((item) => item.id === args!.id);
      if (!job) fail("transcription-not-found", "Cette transcription n’existe plus.");
      window.clearInterval(timer);
      return publish({ ...job!, status: "cancelled" });
    },
    list_transcriptions: () => structuredClone(transcriptions),
  };
}
