// Window-wide, mounted once by the main layout. An audio file dropped on the
// window (`sambler://file-dropped`) brings a non-modal banner « Transcrire
// <fichier> ? »: nothing leaves the PC until the person confirms. Confirmed,
// the transcription starts and the Carnet opens on its progress.
//
// It also announces the end of every transcription (count only, never the
// text) and, away from the Carnet, offers to open the new note.

import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { announcement, useAnnounce } from "../../a11y/announcer";
import { useBridge } from "../../ipc/bridge";
import { EVENT_FILE_DROPPED, type FileTranscription } from "../../ipc/contract";
import { listenShared } from "../../ipc/shared";
import { useNavigation } from "../../state/navigation";
import { requestOpenNote } from "../../state/notes";
import { useTranscriptions } from "../../state/transcriptions";
import { useAction } from "../../state/useAction";
import { FieldError } from "../controls";
import { Glyph, ToneIcon } from "../Icon";
import { baseName, transcriptionEndAnnouncement, transcriptionErrorText, wordsText } from "./labels";
import "./FileDropOffer.css";

interface Offer {
  path: string;
  name: string;
  /** Each drop is a new offer, even for the same file. */
  nonce: number;
}

export function FileDropOffer() {
  const bridge = useBridge();
  const announce = useAnnounce();
  const { view, go } = useNavigation();
  const transcriptions = useTranscriptions();
  const start = useAction();
  const [offer, setOffer] = useState<Offer | null>(null);
  const [ended, setEnded] = useState<FileTranscription | null>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);
  const drops = useRef(0);
  const ids = { offer: useId(), offerDesc: useId(), ended: useId() };
  const clearStartError = start.clearError;

  useEffect(
    () =>
      listenShared(bridge, EVENT_FILE_DROPPED, (path) => {
        if (typeof path !== "string" || !path) return;
        const active = document.activeElement;
        if (active instanceof HTMLElement && !active.closest("[data-drop-offer]")) returnFocus.current = active;
        clearStartError();
        drops.current += 1;
        setOffer({ path, name: baseName(path), nonce: drops.current });
      }),
    [bridge, clearStartError],
  );

  // The person just dropped the file: the question takes the keyboard.
  const nonce = offer?.nonce;
  useEffect(() => {
    if (nonce !== undefined) titleRef.current?.focus();
  }, [nonce]);

  const restoreFocus = useCallback(() => {
    const target = returnFocus.current;
    returnFocus.current = null;
    if (target && target.isConnected) target.focus();
  }, []);

  // Announce each end of transcription once; away from the Carnet, offer the note.
  const seen = useRef<Map<string, FileTranscription["status"]> | null>(null);
  useEffect(() => {
    if (!transcriptions.loaded) return;
    const before = seen.current;
    seen.current = new Map(transcriptions.jobs.map((job) => [job.id, job.status]));
    if (!before) return;
    for (const job of transcriptions.jobs) {
      if (before.get(job.id) !== "running" || job.status === "running") continue;
      const sentence = transcriptionEndAnnouncement(job);
      if (sentence) announce(announcement(sentence));
      if (view !== "carnet" && job.status !== "cancelled") setEnded(job);
    }
  }, [transcriptions.jobs, transcriptions.loaded, announce, view]);

  // On the Carnet, its own card shows the result.
  useEffect(() => {
    if (view === "carnet") setEnded(null);
  }, [view]);

  const dismiss = () => {
    setOffer(null);
    clearStartError();
    restoreFocus();
  };

  const accept = async () => {
    if (!offer || start.busy) return;
    const result = await start.run(() => transcriptions.start(offer.path));
    if (!result.ok) return;
    announce(announcement("Transcription lancée."));
    setOffer(null);
    if (view !== "carnet") {
      returnFocus.current = null;
      go("carnet");
    } else {
      restoreFocus();
    }
  };

  const openCarnet = () => {
    setOffer(null);
    returnFocus.current = null;
    go("carnet");
  };

  const openEnded = (job: FileTranscription) => {
    setEnded(null);
    if (job.noteId) {
      transcriptions.dismiss(job.id);
      requestOpenNote(bridge, job.noteId);
    }
    go("carnet");
  };

  const onKey = (event: KeyboardEvent<HTMLElement>, close: () => void) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  };

  if (offer) {
    const busy = transcriptions.running !== null;
    return (
      <section
        className="drop-offer"
        aria-labelledby={ids.offer}
        aria-describedby={ids.offerDesc}
        data-drop-offer
        data-testid="drop-offer"
        onKeyDown={(event) => onKey(event, dismiss)}
      >
        <span className="drop-offer__icon">
          <Glyph name="file-audio" />
        </span>
        <div className="drop-offer__body">
          <h2 id={ids.offer} ref={titleRef} tabIndex={-1} className="drop-offer__title">
            Transcrire « <span className="drop-offer__name">{offer.name}</span> » ?
          </h2>
          <p id={ids.offerDesc} className="drop-offer__desc">
            {busy
              ? "Une transcription est déjà en cours : une seule à la fois. Déposez ce fichier de nouveau quand elle sera finie."
              : "Le son est envoyé à Rambler pour être transcrit ; le texte devient une note du Carnet."}
          </p>
          {start.error ? <FieldError>{transcriptionErrorText(start.error)}</FieldError> : null}
        </div>
        <div className="drop-offer__actions btn-row">
          {busy ? (
            view === "carnet" ? null : (
              <button type="button" className="btn btn--sm" onClick={openCarnet}>
                Voir dans le Carnet
              </button>
            )
          ) : (
            <button
              type="button"
              className="btn btn--primary btn--sm"
              aria-disabled={start.busy || undefined}
              onClick={() => void accept()}
            >
              {start.busy ? "Lancement…" : "Transcrire"}
            </button>
          )}
          <button type="button" className="btn btn--ghost btn--sm" onClick={dismiss}>
            {start.error || busy ? "Fermer" : "Ignorer"}
          </button>
        </div>
      </section>
    );
  }

  if (ended) {
    const done = ended.status === "done";
    return (
      <section
        className="drop-offer drop-offer--ended"
        aria-labelledby={ids.ended}
        data-drop-offer
        data-testid="transcription-ended"
        onKeyDown={(event) => onKey(event, () => setEnded(null))}
      >
        <span className="drop-offer__icon">
          <ToneIcon tone={done ? "success" : "warning"} />
        </span>
        <div className="drop-offer__body">
          <h2 id={ids.ended} className="drop-offer__title">
            {done ? "Transcription terminée" : "Transcription interrompue"}
          </h2>
          <p className="drop-offer__desc">
            <span className="drop-offer__name">{ended.fileName}</span>
            {" · "}
            {done
              ? `${wordsText(ended.words)}, dans une nouvelle note.`
              : ended.noteId
                ? "Une note partielle est dans le Carnet."
                : (ended.error ?? "La transcription a échoué.")}
          </p>
        </div>
        <div className="drop-offer__actions btn-row">
          {ended.noteId ? (
            <button type="button" className="btn btn--primary btn--sm" onClick={() => openEnded(ended)}>
              Ouvrir la note
            </button>
          ) : null}
          <button type="button" className="btn btn--ghost btn--sm" onClick={() => setEnded(null)}>
            Fermer
          </button>
        </div>
      </section>
    );
  }

  return null;
}
