// Carnet (milestone 5): notes listed with list_notes, read with get_note,
// saved automatically with save_note; dictation into the open note (button
// and global shortcut) lands at the cursor; audio file transcriptions with
// their progress, end and failures; the window-wide drop banner. Note text
// is shown, never announced.
import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  EVENT_FILE_DROPPED,
  EVENT_TRANSCRIPTION,
  type FileTranscription,
  type Note,
  type NoteMeta,
  type UiState,
} from "../ipc/contract";
import { completeDraft, createFakeBridge, type FakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, liveRegion, recordAnnouncements, renderApp } from "../test/render";

const NOW = Date.now();
const MINUTE = 60_000;

function makeNote(id: string, text: string, ageMinutes: number): Note {
  const at = NOW - ageMinutes * MINUTE;
  const title = text.split("\n").find((line) => line.trim())?.trim().slice(0, 80) ?? "";
  return { id, title, createdAt: at, updatedAt: at, words: text.split(/\s+/).filter(Boolean).length, text };
}

const IDEAS = makeNote("ideas", "Idées pour lundi\n\nCommencer par la démo, puis les chiffres.", 5);
const SHOPPING = makeNote("shopping", "Courses\n\nLait, citrons.", 2 * 24 * 60);
const HELLO = makeNote("hello", "Bonjour monde", 30);

function metaOf(note: Note): NoteMeta {
  const { text: _text, ...meta } = note;
  return meta;
}

/** A fake native side with a real little note store. */
function carnetBridge(notes: Note[] = [IDEAS, SHOPPING], state: Partial<UiState> = {}) {
  const bridge = createFakeBridge({ state });
  const store = new Map(notes.map((note) => [note.id, { ...note }]));
  let created = 0;
  bridge.handlers.list_notes = () => [...store.values()].map(metaOf).sort((a, b) => b.updatedAt - a.updatedAt);
  bridge.handlers.get_note = ({ id }: { id: string }) => {
    const note = store.get(id);
    if (!note) throw { code: "note-not-found", message: "Cette note est introuvable." };
    return { ...note };
  };
  bridge.handlers.save_note = ({ note: input }: { note: { id: string | null; text: string } }) => {
    const id = input.id ?? `new-${++created}`;
    const note = makeNote(id, input.text, 0);
    store.set(id, note);
    return metaOf(note);
  };
  bridge.handlers.delete_note = ({ id }: { id: string }) => {
    store.delete(id);
  };
  return { bridge, store };
}

async function openCarnet(bridge: FakeBridge) {
  const view = renderApp(bridge);
  await appReady();
  await view.user.click(screen.getByRole("button", { name: "Carnet" }));
  await screen.findByRole("heading", { level: 1, name: "Carnet" });
  await screen.findByTestId("note-list");
  return view;
}

const rows = () => screen.queryAllByTestId("note-row");
const field = () => screen.getByTestId("note-field") as HTMLTextAreaElement;
const saveCalls = (bridge: FakeBridge) => bridge.invoke.mock.calls.filter(([command]) => command === "save_note");

function job(patch: Partial<FileTranscription> = {}): FileTranscription {
  return {
    id: "job-1",
    fileName: "entretien.m4a",
    durationMs: 7 * MINUTE + 42_000,
    status: "running",
    progress: 0,
    words: 0,
    noteId: null,
    error: null,
    startedAt: NOW,
    ...patch,
  };
}

const KEPT = { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } } as const;

describe("Carnet: notes", () => {
  it("lists the notes with their title, relative date and word count, and searches the titles", async () => {
    const { bridge } = carnetBridge();
    const { user } = await openCarnet(bridge);
    expect(bridge.invoke).toHaveBeenCalledWith("list_notes");
    expect(rows()).toHaveLength(2);
    expect(rows()[0]).toHaveTextContent("Idées pour lundi");
    expect(rows()[0]).toHaveTextContent("Il y a 5 min");
    expect(rows()[0]).toHaveTextContent("10 mots");
    expect(rows()[1]).toHaveTextContent("Courses");
    expect(screen.getByText("2 notes")).toBeInTheDocument();

    const search = screen.getByRole("searchbox", { name: "Rechercher dans les titres des notes" });
    await user.type(search, "idees");
    expect(rows()).toHaveLength(1);
    expect(rows()[0]).toHaveTextContent("Idées pour lundi");
    await waitFor(() => expect(liveRegion()).toHaveTextContent("1 note correspond."), { timeout: 2000 });

    await user.clear(search);
    await user.type(search, "zzz");
    expect(screen.getByTestId("notes-no-match")).toHaveTextContent("Aucun titre ne correspond à cette recherche.");
  });

  it("opens a note in the editor, with the keyboard in the text; the arrows move in the list", async () => {
    const { bridge } = carnetBridge();
    const { user } = await openCarnet(bridge);
    expect(screen.getByTestId("editor-empty")).toHaveTextContent("Choisissez une note, ou commencez-en une");

    await user.click(rows()[1]);
    expect(bridge.invoke).toHaveBeenCalledWith("get_note", { id: "shopping" });
    await waitFor(() => expect(field()).toHaveValue(SHOPPING.text));
    await waitFor(() => expect(field()).toHaveFocus());
    expect(rows()[1]).toHaveAttribute("aria-current", "true");
    expect(screen.getByTestId("save-state")).toHaveTextContent("Enregistré");

    // One Tab stop for the list: the open note; the arrows move from there.
    expect(rows()[1]).toHaveAttribute("tabindex", "0");
    expect(rows()[0]).toHaveAttribute("tabindex", "-1");
    rows()[1].focus();
    await user.keyboard("{ArrowUp}");
    expect(rows()[0]).toHaveFocus();
    await user.keyboard("{End}");
    expect(rows()[1]).toHaveFocus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(field()).toHaveFocus());
  });

  it("saves automatically after a pause and says « Enregistré »", async () => {
    const { bridge } = carnetBridge();
    const { user } = await openCarnet(bridge);
    await user.click(rows()[0]);
    await waitFor(() => expect(field()).toHaveValue(IDEAS.text));

    await user.type(field(), " Et le budget.");
    expect(screen.getByTestId("save-state")).toHaveTextContent("Modifié");
    expect(saveCalls(bridge)).toHaveLength(0);
    await waitFor(() => expect(screen.getByTestId("save-state")).toHaveTextContent("Enregistré"), { timeout: 3000 });
    // One save for the whole burst of typing.
    expect(saveCalls(bridge)).toEqual([["save_note", { note: { id: "ideas", text: `${IDEAS.text} Et le budget.` } }]]);
    expect(rows()[0]).toHaveTextContent("13 mots");
    expect(rows()[0]).toHaveTextContent("À l’instant");
  });

  it("creates a new note with its first words, not before", async () => {
    const { bridge } = carnetBridge();
    const { user } = await openCarnet(bridge);
    await user.click(screen.getAllByRole("button", { name: "Nouvelle note" })[0]);
    await waitFor(() => expect(field()).toHaveFocus());
    expect(field()).toHaveValue("");
    expect(screen.getByTestId("save-state")).toHaveTextContent("Enregistrée dès les premiers mots");

    await user.type(field(), "Penser au pain");
    await waitFor(() => expect(saveCalls(bridge)).toHaveLength(1), { timeout: 3000 });
    expect(saveCalls(bridge)[0]).toEqual(["save_note", { note: { id: null, text: "Penser au pain" } }]);
    await waitFor(() => expect(rows()[0]).toHaveTextContent("Penser au pain"));

    // The next change updates the same note.
    await user.type(field(), " complet");
    await waitFor(() => expect(saveCalls(bridge)).toHaveLength(2), { timeout: 3000 });
    expect(saveCalls(bridge)[1]).toEqual(["save_note", { note: { id: "new-1", text: "Penser au pain complet" } }]);
  });

  it("keeps the text and offers to retry when a save fails", async () => {
    const { bridge } = carnetBridge();
    const { user } = await openCarnet(bridge);
    await user.click(rows()[0]);
    await waitFor(() => expect(field()).toHaveValue(IDEAS.text));
    const save = bridge.handlers.save_note!;
    bridge.handlers.save_note = () => {
      throw { code: "notes-full", message: "Le Carnet est limité à 1 000 notes." };
    };
    await user.type(field(), "!");
    expect(await screen.findByTestId("save-error", {}, { timeout: 3000 })).toHaveTextContent(
      "Le Carnet est limité à 1 000 notes. Votre texte reste affiché ici.",
    );
    expect(screen.getByTestId("save-state")).toHaveTextContent("Non enregistré");
    expect(field()).toHaveValue(`${IDEAS.text}!`);
    await waitFor(() => expect(liveRegion()).toHaveTextContent("La note n’a pas pu être enregistrée."));

    // The unsaved text is never dropped for another note.
    await user.click(rows()[1]);
    await waitFor(() => expect(liveRegion()).toHaveTextContent("La note ouverte n’est pas enregistrée."));
    expect(bridge.invoke).not.toHaveBeenCalledWith("get_note", { id: "shopping" });
    expect(field()).toHaveValue(`${IDEAS.text}!`);

    bridge.handlers.save_note = save;
    await user.click(within(screen.getByTestId("save-error")).getByRole("button", { name: "Réessayer" }));
    await waitFor(() => expect(screen.getByTestId("save-state")).toHaveTextContent("Enregistré"));
    expect(screen.queryByTestId("save-error")).not.toBeInTheDocument();
  });

  it("copies the note", async () => {
    const { bridge } = carnetBridge();
    const { user } = await openCarnet(bridge);
    await user.click(rows()[0]);
    await waitFor(() => expect(field()).toHaveValue(IDEAS.text));
    await user.click(screen.getByRole("button", { name: "Copier la note" }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Note copiée dans le presse-papiers."));
    expect(await navigator.clipboard.readText()).toBe(IDEAS.text);
    expect(screen.getByRole("button", { name: "Copiée la note" })).toBeInTheDocument();
  });

  it("deletes a note only after the in-page confirmation", async () => {
    const { bridge } = carnetBridge();
    const { user } = await openCarnet(bridge);
    await user.click(rows()[0]);
    await waitFor(() => expect(field()).toHaveValue(IDEAS.text));

    await user.click(screen.getByRole("button", { name: "Supprimer la note" }));
    const question = screen.getByRole("group", { name: "Supprimer définitivement cette note ?" });
    expect(within(question).getByRole("button", { name: "Supprimer" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("group", { name: "Supprimer définitivement cette note ?" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Supprimer la note" })).toHaveFocus();
    expect(bridge.invoke).not.toHaveBeenCalledWith("delete_note", expect.anything());

    await user.click(screen.getByRole("button", { name: "Supprimer la note" }));
    await user.click(screen.getByRole("button", { name: "Supprimer" }));
    expect(bridge.invoke).toHaveBeenCalledWith("delete_note", { id: "ideas" });
    await waitFor(() => expect(rows()).toHaveLength(1));
    expect(screen.getByTestId("editor-empty")).toBeInTheDocument();
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Note supprimée."));
    await waitFor(() => expect(rows()[0]).toHaveFocus());
  });

  it("shows a sober state when this Sambler predates the Carnet", async () => {
    const bridge = createFakeBridge();
    bridge.handlers.list_notes = () => {
      throw "Command list_notes not found";
    };
    bridge.handlers.list_transcriptions = () => {
      throw "Command list_transcriptions not found";
    };
    renderApp(bridge);
    await appReady();
    await screen.findByRole("button", { name: "Carnet" }).then((button) => button.click());
    expect(await screen.findByTestId("carnet-unsupported")).toHaveTextContent(
      "Le Carnet n’est pas disponible dans cette version de Sambler.",
    );
    expect(screen.queryByRole("button", { name: "Nouvelle note" })).not.toBeInTheDocument();
  });
});

describe("Carnet: dictation into the note", () => {
  async function openHello() {
    const { bridge, store } = carnetBridge([HELLO]);
    const view = await openCarnet(bridge);
    await view.user.click(rows()[0]);
    await waitFor(() => expect(field()).toHaveValue("Bonjour monde"));
    return { bridge, store, ...view };
  }

  function endTake(bridge: FakeBridge, take: number, text: string) {
    const draft = completeDraft(text, Date.now(), take);
    bridge.recents = [draft];
    act(() => {
      bridge.pushState({
        phase: { kind: "idle" },
        take: null,
        destination: null,
        listeningMs: null,
        lastOutcome: KEPT,
        recentCount: 1,
        draft: draft.meta,
      });
    });
  }

  it("inserts the result of « Dicter dans la note » at the cursor, then saves the note", async () => {
    const { bridge, user } = await openHello();
    const spoken = recordAnnouncements();
    field().setSelectionRange(7, 7);
    bridge.handlers.dictate_in_sambler = () =>
      bridge.pushState({ phase: { kind: "listening" }, take: 41, destination: "sambler", listeningMs: 0 });

    await user.click(screen.getByRole("button", { name: "Dicter dans la note" }));
    const strip = await screen.findByTestId("note-dictation");
    expect(strip).toHaveTextContent("Écoute");
    expect(screen.getByTestId("note-editor")).toHaveAttribute("data-voice", "listening");
    expect(field()).toHaveAttribute("readonly");

    endTake(bridge, 41, "cher");
    await waitFor(() => expect(field()).toHaveValue("Bonjour cher monde"));
    await waitFor(() =>
      expect(bridge.invoke).toHaveBeenCalledWith("save_note", { note: { id: "hello", text: "Bonjour cher monde" } }),
    );
    expect(field().selectionStart).toBe(12);
    await waitFor(() => expect(field()).toHaveFocus());
    expect(await screen.findByTestId("dictation-inserted")).toHaveTextContent("Dictée insérée et enregistrée");
    // Now in the note, the result leaves « Récents ».
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("clear_draft", { take: 41 }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Dictée insérée dans la note et enregistrée."));
    expect(screen.getByTestId("note-editor")).not.toHaveAttribute("data-voice");
    spoken.stop();
    for (const text of spoken.texts) {
      expect(text).not.toContain("cher");
      expect(text).not.toContain("Bonjour");
    }
  });

  it("does the same with the global shortcut while the note's field has the focus", async () => {
    const { bridge, user } = await openHello();
    await user.click(field());
    field().setSelectionRange(13, 13);
    act(() => {
      bridge.pushState({ phase: { kind: "listening" }, take: 42, destination: "sambler", listeningMs: 0 });
    });
    expect(await screen.findByTestId("note-dictation")).toHaveTextContent("Écoute");

    endTake(bridge, 42, "Voici la liste :\n\n*   pain\n*   lait");
    await waitFor(() => expect(field()).toHaveValue("Bonjour monde Voici la liste :\n\n• pain\n• lait"));
  });

  it("leaves a take for another application alone", async () => {
    const { bridge, user } = await openHello();
    await user.click(field());
    act(() => {
      bridge.pushState({ phase: { kind: "listening" }, take: 43, destination: "field", listeningMs: 0 });
    });
    expect(screen.queryByTestId("note-dictation")).not.toBeInTheDocument();
    endTake(bridge, 43, "ailleurs");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(bridge.invoke).not.toHaveBeenCalledWith("get_recent_drafts");
    expect(field()).toHaveValue("Bonjour monde");
  });

  it("explains a refusal, a missing access and an unavailable service", async () => {
    const { bridge, user } = await openHello();
    bridge.handlers.dictate_in_sambler = () =>
      bridge.pushState({ lastOutcome: { kind: "refused", reason: { kind: "microphone", error: { kind: "no-input" } } } });
    await user.click(screen.getByRole("button", { name: "Dicter dans la note" }));
    expect(await screen.findByTestId("dictation-notice")).toHaveTextContent(
      "Le micro n’a pas pu s’ouvrir Aucun micro n’est branché sur ce PC.",
    );

    act(() => {
      bridge.pushState({ availability: { kind: "needs-access" } });
    });
    const button = screen.getByRole("button", { name: "Dicter dans la note" });
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAccessibleDescription("L’accès à Rambler n’est pas configuré.");
    expect(screen.getByRole("button", { name: "Configurer l’accès" })).toBeInTheDocument();

    act(() => {
      bridge.pushState({ availability: { kind: "unavailable", reason: "network" } });
    });
    expect(screen.getByTestId("dictation-block")).toHaveTextContent(
      "Dictée indisponible. Le service est injoignable. Vérifiez la connexion Internet de ce PC.",
    );
    await user.click(screen.getByRole("button", { name: "Réessayer" }));
    expect(bridge.invoke).toHaveBeenCalledWith("retry_connection");
  });

  it("says when the take was cancelled or gave no text", async () => {
    const { bridge, user } = await openHello();
    bridge.handlers.dictate_in_sambler = () =>
      bridge.pushState({ phase: { kind: "listening" }, take: 44, destination: "sambler", listeningMs: 0 });
    await user.click(screen.getByRole("button", { name: "Dicter dans la note" }));
    await screen.findByTestId("note-dictation");
    await user.keyboard("{Escape}");
    expect(bridge.invoke).toHaveBeenCalledWith("cancel_take", { take: 44 });
    act(() => {
      bridge.pushState({ phase: { kind: "idle" }, take: null, destination: null, lastOutcome: { kind: "cancelled" } });
    });
    expect(await screen.findByTestId("dictation-notice")).toHaveTextContent("Dictée annulée Rien n’a été ajouté à la note.");
    expect(field()).toHaveValue("Bonjour monde");
  });
});

describe("Carnet: audio file transcription", () => {
  async function startTranscription() {
    const { bridge, store } = carnetBridge();
    bridge.handlers.pick_audio_file = () => "C:\\Audio\\entretien.m4a";
    bridge.handlers.transcribe_file = ({ path }: { path: string }) => {
      expect(path).toBe("C:\\Audio\\entretien.m4a");
      return job();
    };
    const view = await openCarnet(bridge);
    expect(screen.getByTestId("dropzone")).toHaveTextContent(
      "WAV, Ogg Opus, MP3, M4A/AAC, WMA et FLAC, jusqu’à 2 heures et 2 Go.",
    );
    await view.user.click(screen.getByRole("button", { name: "Choisir un fichier" }));
    const card = await screen.findByTestId("transcription-card");
    return { bridge, store, card, ...view };
  }

  it("shows the progress, then opens the new note at the end", async () => {
    const { bridge, store, card } = await startTranscription();
    expect(card).toHaveTextContent("Transcription en cours");
    expect(card).toHaveTextContent("entretien.m4a");
    expect(card).toHaveTextContent("7 min 42 s");
    // One at a time: no second file while this one runs.
    expect(screen.queryByTestId("dropzone")).not.toBeInTheDocument();
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Transcription lancée."));

    act(() => bridge.emit(EVENT_TRANSCRIPTION, job({ progress: 0.45, words: 520 })));
    const bar = within(screen.getByTestId("transcription-card")).getByRole("progressbar", {
      name: "Transcription en cours",
    });
    expect(bar).toHaveAttribute("aria-valuenow", "45");
    expect(bar).toHaveAttribute("aria-valuetext", "45 %, 520 mots");

    store.set("t1", makeNote("t1", "Transcription · entretien.m4a\n\nMerci de nous recevoir.", 0));
    act(() => bridge.emit(EVENT_TRANSCRIPTION, job({ status: "done", progress: 1, words: 1180, noteId: "t1" })));
    const done = await screen.findByText("Transcription terminée");
    expect(done.closest("article")).toHaveTextContent("1 180 mots, enregistrés dans une nouvelle note du Carnet.");
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Transcription terminée : 1 180 mots"));
    // No note was open: the new one opens, and appears in the list.
    await waitFor(() => expect(field()).toHaveValue("Transcription · entretien.m4a\n\nMerci de nous recevoir."));
    expect(rows()[0]).toHaveTextContent("Transcription · entretien.m4a");
  });

  it("offers the partial note when a transcription stops before the end", async () => {
    const { bridge, store, user } = await startTranscription();
    await user.click(rows()[1]);
    await waitFor(() => expect(field()).toHaveValue(SHOPPING.text));
    store.set("t2", makeNote("t2", "Transcription · entretien.m4a\n\nBonjour. [Passage non transcrit]", 0));
    act(() =>
      bridge.emit(
        EVENT_TRANSCRIPTION,
        job({ status: "failed", progress: 0.55, words: 640, noteId: "t2", error: "La connexion au service a été perdue." }),
      ),
    );
    const card = await screen.findByTestId("transcription-card");
    await waitFor(() => expect(card).toHaveAttribute("data-status", "failed"));
    expect(card).toHaveTextContent("Transcription interrompue");
    expect(card).toHaveTextContent("La connexion au service a été perdue.");
    expect(card).toHaveTextContent("« [Passage non transcrit] »");
    // A note was open: it stays, the partial one is offered.
    expect(field()).toHaveValue(SHOPPING.text);
    await user.click(within(card).getByRole("button", { name: "Ouvrir la note partielle" }));
    await waitFor(() => expect(field()).toHaveValue("Transcription · entretien.m4a\n\nBonjour. [Passage non transcrit]"));
    expect(screen.queryByTestId("transcription-card")).not.toBeInTheDocument();
  });

  it("cancels a transcription", async () => {
    const { bridge, card, user } = await startTranscription();
    bridge.handlers.cancel_transcription = ({ id }: { id: string }) => job({ id, status: "cancelled", progress: 0.2 });
    await user.click(within(card).getByRole("button", { name: "Annuler la transcription" }));
    expect(bridge.invoke).toHaveBeenCalledWith("cancel_transcription", { id: "job-1" });
    await waitFor(() => expect(screen.getByTestId("transcription-card")).toHaveAttribute("data-status", "cancelled"));
    expect(screen.getByTestId("transcription-card")).toHaveTextContent("Rien n’a été enregistré.");
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Transcription annulée."));
    await user.click(screen.getByRole("button", { name: "Fermer ce message" }));
    expect(screen.queryByTestId("transcription-card")).not.toBeInTheDocument();
    expect(screen.getByTestId("dropzone")).toBeInTheDocument();
  });

  it("explains a refused file in French", async () => {
    const { bridge } = carnetBridge();
    bridge.handlers.pick_audio_file = () => "C:\\Audio\\entretien.m4a";
    bridge.handlers.transcribe_file = () => {
      throw { code: "transcription-busy", message: "Une transcription est déjà en cours ou Sambler est en train de se fermer." };
    };
    const { user } = await openCarnet(bridge);
    await user.click(screen.getByRole("button", { name: "Choisir un fichier" }));
    expect(
      await screen.findByText("Une transcription est déjà en cours ou Sambler est en train de se fermer."),
    ).toBeInTheDocument();
  });
});

describe("Drop banner", () => {
  async function dropOnHome(setup?: (bridge: FakeBridge) => void) {
    const { bridge } = carnetBridge();
    setup?.(bridge);
    const view = renderApp(bridge);
    await appReady();
    await screen.findByTestId("recents");
    act(() => bridge.emit(EVENT_FILE_DROPPED, "C:\\Users\\Antonin\\Documents\\réunion.m4a"));
    const banner = await screen.findByTestId("drop-offer");
    return { bridge, banner, ...view };
  }

  it("asks before sending anything, then transcribes and opens the Carnet", async () => {
    const { bridge, banner, user } = await dropOnHome((fake) => {
      fake.handlers.transcribe_file = () => job({ fileName: "réunion.m4a" });
    });
    const title = within(banner).getByRole("heading", { name: "Transcrire « réunion.m4a » ?" });
    await waitFor(() => expect(title).toHaveFocus());
    expect(banner).toHaveTextContent("Le son est envoyé à Rambler pour être transcrit");
    expect(banner).not.toHaveTextContent("C:\\Users");
    expect(bridge.invoke).not.toHaveBeenCalledWith("transcribe_file", expect.anything());

    await user.click(within(banner).getByRole("button", { name: "Transcrire" }));
    expect(bridge.invoke).toHaveBeenCalledWith("transcribe_file", { path: "C:\\Users\\Antonin\\Documents\\réunion.m4a" });
    expect(await screen.findByRole("heading", { level: 1, name: "Carnet" })).toBeInTheDocument();
    expect(await screen.findByTestId("transcription-card")).toHaveTextContent("réunion.m4a");
    expect(screen.queryByTestId("drop-offer")).not.toBeInTheDocument();
  });

  it("goes away with « Ignorer » or Échap and gives the keyboard back", async () => {
    const { bridge, banner, user } = await dropOnHome();
    const before = screen.getByRole("button", { name: "Dicter dans Sambler" });
    before.focus();
    act(() => bridge.emit(EVENT_FILE_DROPPED, "C:\\Users\\Antonin\\Documents\\réunion.m4a"));
    await waitFor(() => expect(within(screen.getByTestId("drop-offer")).getByRole("heading")).toHaveFocus());
    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("drop-offer")).not.toBeInTheDocument();
    expect(before).toHaveFocus();

    act(() => bridge.emit(EVENT_FILE_DROPPED, "C:\\Users\\Antonin\\Documents\\réunion.m4a"));
    await user.click(within(await screen.findByTestId("drop-offer")).getByRole("button", { name: "Ignorer" }));
    expect(screen.queryByTestId("drop-offer")).not.toBeInTheDocument();
    expect(bridge.invoke).not.toHaveBeenCalledWith("transcribe_file", expect.anything());
    void banner;
  });

  it("says one transcription runs already", async () => {
    const { banner } = await dropOnHome((fake) => {
      fake.handlers.list_transcriptions = () => [job()];
    });
    await waitFor(() => expect(banner).toHaveTextContent("Une transcription est déjà en cours : une seule à la fois."));
    expect(within(banner).getByRole("button", { name: "Voir dans le Carnet" })).toBeInTheDocument();
  });

  it("says when this Sambler cannot transcribe files", async () => {
    const { banner, user } = await dropOnHome((fake) => {
      fake.handlers.transcribe_file = () => {
        throw "Command transcribe_file not found";
      };
    });
    await user.click(within(banner).getByRole("button", { name: "Transcrire" }));
    expect(await within(banner).findByText("La transcription de fichiers n’est pas disponible dans cette version de Sambler.")).toBeInTheDocument();
  });

  it("offers the note of a transcription that ends on another screen", async () => {
    const { bridge, store } = carnetBridge();
    bridge.handlers.list_transcriptions = () => [job()];
    const { user } = renderApp(bridge);
    await appReady();
    await screen.findByTestId("recents");
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("list_transcriptions"));
    await act(async () => {
      await Promise.resolve();
    });
    store.set("t3", makeNote("t3", "Transcription · entretien.m4a\n\nTexte.", 0));
    act(() => bridge.emit(EVENT_TRANSCRIPTION, job({ status: "done", progress: 1, words: 12, noteId: "t3" })));
    const ended = await screen.findByTestId("transcription-ended");
    expect(ended).toHaveTextContent("Transcription terminée");
    await user.click(within(ended).getByRole("button", { name: "Ouvrir la note" }));
    await screen.findByRole("heading", { level: 1, name: "Carnet" });
    await waitFor(() => expect(field()).toHaveValue("Transcription · entretien.m4a\n\nTexte."));
  });
});

describe("Carnet: accessibility and privacy", () => {
  it("never announces the text of a note", async () => {
    const { bridge } = carnetBridge([HELLO, IDEAS]);
    const { user } = await openCarnet(bridge);
    const spoken = recordAnnouncements();
    await user.click(rows()[0]);
    await waitFor(() => expect(field()).toHaveValue(IDEAS.text));
    await user.type(field(), " secret");
    await waitFor(() => expect(saveCalls(bridge)).toHaveLength(1), { timeout: 3000 });
    await user.click(screen.getByRole("button", { name: "Copier la note" }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Note copiée"));
    await user.click(screen.getByRole("button", { name: "Supprimer la note" }));
    await user.click(screen.getByRole("button", { name: "Supprimer" }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Note supprimée."));
    spoken.stop();
    expect(spoken.texts.length).toBeGreaterThan(0);
    for (const text of spoken.texts) {
      expect(text).not.toMatch(/Idées|démo|secret|Bonjour/);
    }
  });

  it("has no detectable accessibility violations, with a note, a dictation and a transcription", async () => {
    const { bridge } = carnetBridge();
    bridge.handlers.list_transcriptions = () => [job({ progress: 0.3, words: 300 })];
    bridge.handlers.dictate_in_sambler = () =>
      bridge.pushState({ phase: { kind: "listening" }, take: 50, destination: "sambler", listeningMs: 0 });
    const { user } = await openCarnet(bridge);
    await screen.findByTestId("transcription-card");
    await user.click(rows()[0]);
    await waitFor(() => expect(field()).toHaveValue(IDEAS.text));
    await user.click(screen.getByRole("button", { name: "Dicter dans la note" }));
    await screen.findByTestId("note-dictation");
    expect(await axeViolations()).toEqual([]);
  });

  it("has no detectable accessibility violations, with the drop banner and the delete question", async () => {
    const { bridge } = carnetBridge();
    const { user } = await openCarnet(bridge);
    await user.click(rows()[0]);
    await waitFor(() => expect(field()).toHaveValue(IDEAS.text));
    await user.click(screen.getByRole("button", { name: "Supprimer la note" }));
    act(() => bridge.emit(EVENT_FILE_DROPPED, "C:\\Audio\\réunion.m4a"));
    await screen.findByTestId("drop-offer");
    expect(await axeViolations()).toEqual([]);
  });
});
