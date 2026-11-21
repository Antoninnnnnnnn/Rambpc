// « Transcrire un fichier audio »: the drop zone with the file button, then a
// card per transcription of this session (progress, duration, words,
// Annuler; at the end, the note or the error). One transcription at a time.
// The end of a transcription is announced by the window-wide FileDropOffer.

import { useId, useState, type CSSProperties } from "react";
import { announcement, useAnnounce } from "../../a11y/announcer";
import type { FileTranscription } from "../../ipc/contract";
import { useTranscriptions } from "../../state/transcriptions";
import { useAction } from "../../state/useAction";
import { FieldError } from "../controls";
import { Glyph, ToneIcon } from "../Icon";
import {
  AUDIO_FORMATS,
  AUDIO_LIMITS,
  formatDuration,
  percentText,
  remainingText,
  transcriptionErrorText,
  UNSUPPORTED_TRANSCRIPTION,
  UNTRANSCRIBED_MARK,
  wordsText,
} from "./labels";
import "./carnet.css";

/** At most the running transcription and the two latest finished ones. */
const FINISHED_SHOWN = 2;

interface Props {
  onOpenNote: (id: string, job: FileTranscription) => void;
}

export function TranscriptionPanel({ onOpenNote }: Props) {
  const transcriptions = useTranscriptions();
  const announce = useAnnounce();
  const pick = useAction();
  const headingId = useId();
  const hintId = useId();
  const running = transcriptions.running;
  const finished = transcriptions.jobs
    .filter((job) => job.status !== "running" && !transcriptions.dismissed.has(job.id))
    .slice(0, FINISHED_SHOWN);
  const jobs = running ? [running, ...finished] : finished;

  const choose = async () => {
    if (running || pick.busy) return;
    const result = await pick.run(() => transcriptions.pickAndStart());
    if (result.ok && result.value) announce(announcement("Transcription lancée."));
  };

  if (transcriptions.unsupported) {
    return (
      <section className="transcribe" aria-labelledby={headingId} data-testid="transcribe">
        <h2 id={headingId} className="visually-hidden">
          Transcrire un fichier audio
        </h2>
        <p className="transcribe__unsupported hint" data-testid="transcribe-unsupported">
          <ToneIcon tone="info" />
          {UNSUPPORTED_TRANSCRIPTION}
        </p>
      </section>
    );
  }

  return (
    <section className="transcribe" aria-labelledby={headingId} data-testid="transcribe">
      <h2 id={headingId} className="visually-hidden">
        Transcrire un fichier audio
      </h2>
      {jobs.map((job) => (
        <TranscriptionCard
          key={job.id}
          job={job}
          onOpenNote={onOpenNote}
          onCancel={() => transcriptions.cancel(job.id)}
          onDismiss={() => transcriptions.dismiss(job.id)}
        />
      ))}
      {running ? null : (
        <div className="dropzone" data-testid="dropzone">
          <span className="dropzone__icon">
            <Glyph name="file-audio" />
          </span>
          <p className="dropzone__title">Transcrire un fichier audio</p>
          <p id={hintId} className="dropzone__hint">
            Déposez-le sur la fenêtre ou choisissez-le : son texte devient une note.{" "}
            <span className="dropzone__formats">
              {AUDIO_FORMATS}, {AUDIO_LIMITS}.
            </span>
          </p>
          <button
            type="button"
            className="btn dropzone__button"
            aria-describedby={hintId}
            aria-disabled={pick.busy || undefined}
            onClick={() => void choose()}
          >
            <Glyph name="add" />
            Choisir un fichier
          </button>
        </div>
      )}
      {pick.error ? <FieldError>{transcriptionErrorText(pick.error)}</FieldError> : null}
    </section>
  );
}

interface CardProps {
  job: FileTranscription;
  onOpenNote: (id: string, job: FileTranscription) => void;
  onCancel: () => Promise<FileTranscription>;
  onDismiss: () => void;
}

const TITLES: Record<FileTranscription["status"], string> = {
  running: "Transcription en cours",
  done: "Transcription terminée",
  failed: "Transcription interrompue",
  cancelled: "Transcription annulée",
};

function TranscriptionCard({ job, onOpenNote, onCancel, onDismiss }: CardProps) {
  const cancel = useAction();
  const [asked, setAsked] = useState(false);
  const titleId = useId();
  const progress = Math.min(1, Math.max(0, job.progress));
  const remaining = remainingText(job);
  const percent = percentText(progress);
  const partial = job.status === "failed" && job.noteId !== null;

  const runCancel = async () => {
    setAsked(true);
    const result = await cancel.run(onCancel);
    if (!result.ok) setAsked(false);
  };

  return (
    <article
      className={`job job--${job.status}`}
      aria-labelledby={titleId}
      data-testid="transcription-card"
      data-status={job.status}
    >
      <span className="job__icon">
        {job.status === "running" ? (
          <Glyph name="file-audio" />
        ) : (
          <ToneIcon tone={job.status === "done" ? "success" : job.status === "failed" ? "warning" : "info"} />
        )}
      </span>
      <div className="job__body">
        <h3 id={titleId} className="job__title" tabIndex={-1} data-job-title>
          {TITLES[job.status]}
        </h3>
        <p className="job__file">
          <span className="job__name">{job.fileName}</span>
          {job.durationMs !== null ? (
            <>
              <span aria-hidden="true"> · </span>
              <span>
                <span className="visually-hidden">durée </span>
                {formatDuration(job.durationMs)}
              </span>
            </>
          ) : null}
        </p>

        {job.status === "running" ? (
          <>
            <div
              className="job__track"
              role="progressbar"
              aria-labelledby={titleId}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(progress * 100)}
              aria-valuetext={`${percent}, ${wordsText(job.words)}`}
              style={{ "--progress": progress } as CSSProperties}
            >
              <span className="job__fill" />
            </div>
            <p className="job__stats">
              <span className="job__percent">{percent}</span>
              <span aria-hidden="true"> · </span>
              <span>{wordsText(job.words)}</span>
              {remaining ? (
                <>
                  <span aria-hidden="true"> · </span>
                  <span>{remaining}</span>
                </>
              ) : null}
            </p>
            <p className="job__note">Vous pouvez continuer à dicter et à écrire pendant la transcription.</p>
          </>
        ) : job.status === "done" ? (
          <p className="job__note">{wordsText(job.words)}, enregistrés dans une nouvelle note du Carnet.</p>
        ) : job.status === "failed" ? (
          <>
            <FieldError>{job.error ?? "La transcription a échoué."}</FieldError>
            {partial ? (
              <p className="job__note">
                Le début a été transcrit ({wordsText(job.words)}) : la note marque « {UNTRANSCRIBED_MARK} » là où le texte
                manque.
              </p>
            ) : null}
          </>
        ) : (
          <p className="job__note">Rien n’a été enregistré.</p>
        )}
        {cancel.error ? <FieldError>{transcriptionErrorText(cancel.error)}</FieldError> : null}
      </div>

      <div className="job__actions btn-row">
        {job.status === "running" ? (
          <button
            type="button"
            className="btn btn--sm"
            aria-disabled={asked || undefined}
            onClick={() => {
              if (!asked) void runCancel();
            }}
          >
            {asked ? "Annulation…" : "Annuler"}
            <span className="visually-hidden"> la transcription</span>
          </button>
        ) : (
          <>
            {job.noteId !== null && (job.status === "done" || job.status === "failed") ? (
              <button type="button" className="btn btn--primary btn--sm" onClick={() => onOpenNote(job.noteId!, job)}>
                {partial ? "Ouvrir la note partielle" : "Ouvrir la note"}
              </button>
            ) : null}
            <button type="button" className="btn btn--ghost btn--sm" onClick={onDismiss}>
              Fermer<span className="visually-hidden"> ce message</span>
            </button>
          </>
        )}
      </div>
    </article>
  );
}
