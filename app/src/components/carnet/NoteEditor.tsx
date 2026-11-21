// The open note: its text in a plain field, saved automatically, with
// « Dicter dans la note », « Copier » and « Supprimer » (confirmed in place).
// While a take for this note runs, the theme's aurora lives around the plate
// (`data-voice`, `--voix` follows the voice level) and the text arrives at
// the cursor when the take ends.

import { useEffect, useId, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { announcement, useAnnounce } from "../../a11y/announcer";
import { useBridge } from "../../ipc/bridge";
import { EVENT_LEVEL } from "../../ipc/contract";
import { listenShared } from "../../ipc/shared";
import { commandErrorText, PHASE_LABELS } from "../../labels/fr";
import { formatClock } from "../../lib/time";
import { prefersReducedMotion, speechLevel } from "../../lib/waveform";
import { useUiCommands } from "../../state/commands";
import { useNavigation } from "../../state/navigation";
import type { NoteEditor as NoteEditorStore, SaveStatus } from "../../state/notes";
import { useSettings } from "../../state/settings";
import { useUiState } from "../../state/ui";
import { FieldError, Keys } from "../controls";
import { Glyph, StatusIcon, ToneIcon } from "../Icon";
import { Waveform } from "../Waveform";
import { countWords, editedText, isUnsupported, longDate, titleOf, UNSUPPORTED_NOTES, wordsText } from "./labels";
import type { ActivePhase, DictationBlock, DictationNotice, NoteDictation } from "./useNoteDictation";
import "./carnet.css";

export const NOTE_MAX_CHARS = 200_000;

interface Props {
  editor: NoteEditorStore;
  /** Owned by the Carnet, so that a take outlives a change of note. */
  dictation: NoteDictation;
  field: RefObject<HTMLTextAreaElement | null>;
  now: number;
  /** Small windows: back to the list of notes. */
  onBack: () => void;
  /** The note was deleted (its id, null for a new note never saved). */
  onDeleted: (id: string | null) => void;
}

/** Eases `--voix` (0..1) on the plate toward the voice level while listening. */
function useVoiceLevel(target: RefObject<HTMLElement | null>, listening: boolean) {
  const bridge = useBridge();
  useEffect(() => {
    const element = target.current;
    if (!element || !listening || prefersReducedMotion()) return;
    let wanted = 0;
    let shown = 0;
    let frame = 0;
    const tick = () => {
      frame = 0;
      shown += (wanted - shown) * (wanted > shown ? 0.35 : 0.12);
      if (Math.abs(wanted - shown) < 0.004) shown = wanted;
      element.style.setProperty("--voix", shown.toFixed(3));
      if (shown !== wanted) frame = window.requestAnimationFrame(tick);
    };
    const stop = listenShared(bridge, EVENT_LEVEL, (level) => {
      if (document.visibilityState === "hidden") return;
      wanted = speechLevel(level);
      if (typeof window.requestAnimationFrame !== "function") {
        element.style.setProperty("--voix", wanted.toFixed(3));
      } else if (!frame) {
        frame = window.requestAnimationFrame(tick);
      }
    });
    return () => {
      stop();
      if (frame) window.cancelAnimationFrame(frame);
      element.style.removeProperty("--voix");
    };
  }, [bridge, listening, target]);
}

async function writeClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Fall back to the selection copy below.
  }
  const scratch = document.createElement("textarea");
  scratch.value = text;
  scratch.setAttribute("readonly", "");
  scratch.style.position = "fixed";
  scratch.style.opacity = "0";
  document.body.appendChild(scratch);
  scratch.select();
  let copied = false;
  try {
    copied = document.execCommand("copy");
  } catch {
    copied = false;
  }
  scratch.remove();
  return copied;
}

export function NoteEditor({ editor, dictation, field, now, onBack, onDeleted }: Props) {
  const note = editor.note!;
  const announce = useAnnounce();
  const plate = useRef<HTMLElement>(null);
  const ids = { title: useId(), field: useId(), hint: useId(), block: useId(), confirm: useId(), count: useId() };
  const [asking, setAsking] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [copy, setCopy] = useState<"idle" | "done" | "failed">("idle");
  const copyTimer = useRef<number | undefined>(undefined);
  const deleteButton = useRef<HTMLButtonElement>(null);
  const confirmButton = useRef<HTMLButtonElement>(null);
  const returnToDelete = useRef(false);

  useEffect(() => () => window.clearTimeout(copyTimer.current), []);

  const voice = dictation.view.kind === "active" ? dictation.view.phase : null;
  useVoiceLevel(plate, voice === "listening");

  // A failed save is spoken once (the message itself is French and text-free).
  const failed = note.status.kind === "error";
  useEffect(() => {
    if (failed) announce(announcement("La note n’a pas pu être enregistrée."));
  }, [failed, announce]);

  useEffect(() => {
    if (asking) confirmButton.current?.focus();
    else if (returnToDelete.current) {
      returnToDelete.current = false;
      deleteButton.current?.focus();
    }
  }, [asking]);

  const runCopy = async () => {
    window.clearTimeout(copyTimer.current);
    const ok = note.text.length > 0 && (await writeClipboard(note.text));
    setCopy(ok ? "done" : "failed");
    announce(announcement(ok ? "Note copiée dans le presse-papiers." : "La copie a échoué."));
    if (ok) copyTimer.current = window.setTimeout(() => setCopy("idle"), 3000);
  };

  const runDelete = async () => {
    setDeleteError(null);
    const id = note.id;
    const result = await editor.remove();
    if (!result.ok) {
      setDeleteError(commandErrorText(result.error, "La note n’a pas pu être supprimée."));
      return;
    }
    announce(announcement("Note supprimée."));
    onDeleted(id);
  };

  const onFieldKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // Ctrl+S: save now rather than after the delay.
    if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === "s") {
      event.preventDefault();
      void editor.flush();
    }
  };

  const onPlateKey = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    if (dictation.view.kind === "active") {
      event.preventDefault();
      dictation.cancel();
    }
  };

  const text = note.text;
  const title = titleOf(text);
  const words = countWords(text);
  const meta = note.meta;
  const isNew = note.id === null;
  const listening = voice === "listening";
  const block = dictation.view.kind === "off" ? dictation.block : null;

  return (
    <section
      ref={plate}
      className="note-editor"
      aria-labelledby={ids.title}
      data-voice={voice ?? undefined}
      data-testid="note-editor"
      onKeyDown={onPlateKey}
    >
      <span className="note-editor__aura" aria-hidden="true" />
      <h2 id={ids.title} className="visually-hidden">
        {isNew && !title ? "Nouvelle note" : title || "Note sans titre"}
      </h2>

      <div className="note-editor__bar">
        <button type="button" className="btn btn--ghost btn--sm note-editor__back" onClick={onBack}>
          <Glyph name="chevron-down" className="note-editor__back-icon" />
          Toutes les notes
        </button>
        <DictateButton dictation={dictation} describedBy={block ? ids.block : ids.hint} />
        <span className="note-editor__spacer" />
        {dictation.inserted && note.status.kind !== "error" ? (
          <p className="note-save note-save--inserted" data-testid="dictation-inserted">
            <Glyph name="check" />
            Dictée insérée et enregistrée
          </p>
        ) : (
          <SaveState status={note.status} />
        )}
        <div className="note-editor__tools">
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            onClick={() => void runCopy()}
            aria-disabled={text.length === 0 || undefined}
          >
            <Glyph name={copy === "done" ? "check" : "copy"} />
            <span className="btn__label">{copy === "done" ? "Copiée" : "Copier"}</span>
            <span className="visually-hidden"> la note</span>
          </button>
          {asking ? null : (
            <button
              ref={deleteButton}
              type="button"
              className="btn btn--ghost btn--sm note-editor__delete"
              onClick={() => {
                setDeleteError(null);
                setAsking(true);
              }}
            >
              <Glyph name="trash" />
              <span className="btn__label">Supprimer</span>
              <span className="visually-hidden"> la note</span>
            </button>
          )}
        </div>
      </div>

      {block ? <BlockReason id={ids.block} block={block} /> : null}

      {asking ? (
        <div
          className="confirm note-editor__confirm"
          role="group"
          aria-labelledby={ids.confirm}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              returnToDelete.current = true;
              setAsking(false);
            }
          }}
        >
          <p id={ids.confirm} className="confirm__question">
            Supprimer définitivement cette note ?
          </p>
          <p className="hint">Elle disparaît du Carnet. Cette action ne peut pas être annulée.</p>
          <div className="btn-row">
            <button ref={confirmButton} type="button" className="btn btn--danger btn--sm" onClick={() => void runDelete()}>
              Supprimer
            </button>
            <button
              type="button"
              className="btn btn--sm"
              onClick={() => {
                returnToDelete.current = true;
                setAsking(false);
              }}
            >
              Annuler
            </button>
          </div>
          {deleteError ? <FieldError>{deleteError}</FieldError> : null}
        </div>
      ) : null}

      {note.status.kind === "error" ? (
        <div className="note-editor__save-error" data-testid="save-error">
          <FieldError>
            {isUnsupported(note.status.error)
              ? UNSUPPORTED_NOTES
              : commandErrorText(note.status.error, "La note n’a pas pu être enregistrée.")}{" "}
            Votre texte reste affiché ici.
          </FieldError>
          <button type="button" className="btn btn--sm" onClick={() => void editor.flush()}>
            Réessayer
          </button>
        </div>
      ) : null}

      {copy === "failed" ? (
        <FieldError>La copie a échoué. Sélectionnez le texte, puis appuyez sur Ctrl+C.</FieldError>
      ) : null}

      {dictation.view.kind === "active" ? (
        <LiveStrip phase={dictation.view.phase} listening={listening} dictation={dictation} />
      ) : dictation.view.kind === "inserting" ? (
        <p className="note-dictation note-dictation--quiet" data-testid="note-dictation">
          <StatusIcon shape="finalizing" className="note-dictation__icon" />
          <span>Insertion du texte dicté…</span>
        </p>
      ) : null}

      {dictation.notice ? <DictationNoticeView notice={dictation.notice} onClose={dictation.dismissNotice} /> : null}

      <label htmlFor={ids.field} className="visually-hidden">
        Texte de la note
      </label>
      <textarea
        ref={field}
        id={ids.field}
        className="note-editor__field"
        value={text}
        lang="fr"
        spellCheck
        maxLength={NOTE_MAX_CHARS}
        readOnly={dictation.view.kind !== "off"}
        placeholder="Écrivez ou dictez votre note. Sa première ligne lui sert de titre."
        aria-describedby={ids.hint}
        onChange={(event) => editor.setText(event.currentTarget.value)}
        onKeyDown={onFieldKey}
        data-testid="note-field"
      />

      <div className="note-editor__foot">
        <p className="note-editor__meta">
          {meta ? (
            <>
              <time dateTime={new Date(meta.updatedAt).toISOString()} title={longDate(meta.updatedAt)}>
                {editedText(meta.updatedAt, now)}
              </time>
              <span aria-hidden="true"> · </span>
            </>
          ) : null}
          <span>{wordsText(words)}</span>
          {text.length > NOTE_MAX_CHARS * 0.9 ? (
            <span id={ids.count}>
              <span aria-hidden="true"> · </span>
              {text.length.toLocaleString("fr-FR")} caractères sur 200 000
            </span>
          ) : null}
        </p>
        <p id={ids.hint} className="note-editor__hint">
          <ShortcutTip />
        </p>
      </div>
    </section>
  );
}

/** Why a dictation did not reach the note, or what to check. */
export function DictationNoticeView({ notice, onClose }: { notice: DictationNotice; onClose: () => void }) {
  return (
    <div className={`note-notice note-notice--${notice.tone}`} data-testid="dictation-notice">
      <ToneIcon tone={notice.tone} />
      <p className="note-notice__text">
        <strong>{notice.title}</strong>
        {notice.detail ? <> {notice.detail}</> : null}
      </p>
      <button type="button" className="btn btn--ghost btn--sm" onClick={onClose}>
        Fermer<span className="visually-hidden"> le message</span>
      </button>
    </div>
  );
}

function ShortcutTip() {
  const state = useUiState();
  return state.gesture === "hold" ? (
    <>
      Le curseur dans la note, maintenez <Keys shortcut={state.shortcut} /> pour dicter à cet endroit.
    </>
  ) : (
    <>
      Le curseur dans la note, appuyez sur <Keys shortcut={state.shortcut} /> pour dicter à cet endroit, puis de nouveau
      pour terminer.
    </>
  );
}

function DictateButton({ dictation, describedBy }: { dictation: NoteDictation; describedBy: string }) {
  const block = dictation.view.kind === "off" ? dictation.block : null;
  const busy = dictation.view.kind !== "off";
  return (
    <button
      type="button"
      className="btn btn--soft btn--sm note-editor__dictate-btn"
      aria-disabled={block !== null || busy || undefined}
      aria-describedby={describedBy}
      onClick={() => {
        if (block === null && !busy) void dictation.start();
      }}
    >
      <Glyph name="mic" />
      Dicter dans la note
    </button>
  );
}

/** Dictation cannot start now: why, and the way out when there is one. */
function BlockReason({ id, block }: { id: string; block: DictationBlock }) {
  const { openFirstRun, go } = useNavigation();
  const commands = useUiCommands();
  return (
    <p className="note-editor__block" data-testid="dictation-block">
      <ToneIcon tone="info" />
      <span id={id}>{block.reason}</span>
      {block.action ? (
        <button
          type="button"
          className="link-btn"
          onClick={() => {
            if (block.action === "enable") openFirstRun("consent");
            else if (block.action === "access") go("settings", "access");
            else void commands.retryConnection().catch(() => undefined);
          }}
        >
          {block.action === "enable" ? "Activer la dictée" : block.action === "access" ? "Configurer l’accès" : "Réessayer"}
        </button>
      ) : null}
    </p>
  );
}

function SaveState({ status }: { status: SaveStatus }) {
  switch (status.kind) {
    case "new":
      return (
        <p className="note-save note-save--new" data-testid="save-state">
          Enregistrée dès les premiers mots
        </p>
      );
    case "pending":
      return (
        <p className="note-save note-save--pending" data-testid="save-state">
          Modifiée
        </p>
      );
    case "saving":
      return (
        <p className="note-save note-save--saving" data-testid="save-state">
          Enregistrement…
        </p>
      );
    case "clean":
    case "saved":
      return (
        <p className="note-save note-save--saved" data-testid="save-state">
          <Glyph name="check" />
          Enregistré
        </p>
      );
    case "error":
      return (
        <p className="note-save note-save--error" data-testid="save-state">
          <ToneIcon tone="error" />
          Non enregistré
        </p>
      );
  }
}

function LiveStrip({ phase, listening, dictation }: { phase: ActivePhase; listening: boolean; dictation: NoteDictation }) {
  const state = useUiState();
  const { settings } = useSettings();
  const escapeCancels = settings?.cancelWithEscape ?? true;
  const [start, setStart] = useState(() => Date.now() - (state.listeningMs ?? 0));
  const [now, setNow] = useState(() => Date.now());
  const listeningMs = state.listeningMs;

  useEffect(() => {
    if (!listening) return;
    setStart(Date.now() - (listeningMs ?? 0));
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, [listening, listeningMs]);

  return (
    <div className="note-dictation" data-phase={phase} data-testid="note-dictation">
      <span className="note-dictation__badge">
        <StatusIcon shape={phase} />
      </span>
      <div className="note-dictation__text">
        <p className="note-dictation__label">
          {PHASE_LABELS[phase]}
          {listening ? (
            <span className="note-dictation__clock">
              <span className="visually-hidden">, durée </span> {formatClock(now - start)}
            </span>
          ) : null}
        </p>
        <p className="note-dictation__hint">
          {phase === "listening"
            ? "Parlez, puis choisissez « Terminer » : le texte s’insérera au curseur."
            : phase === "finalizing"
              ? "Rambler termine la transcription. Le texte arrive au curseur."
              : "Ouverture du micro. Attendez « Écoute » avant de parler."}
        </p>
      </div>
      {listening ? (
        <div className="note-dictation__wave">
          <Waveform event={EVENT_LEVEL} active bars={19} />
        </div>
      ) : (
        <span className="note-dictation__dots" aria-hidden="true">
          <span />
          <span />
          <span />
        </span>
      )}
      <div className="btn-row note-dictation__actions">
        {phase !== "finalizing" ? (
          <button type="button" className="btn btn--primary btn--sm" onClick={dictation.finish}>
            <Glyph name="stop" />
            Terminer
          </button>
        ) : null}
        <button type="button" className="btn btn--sm" onClick={dictation.cancel}>
          Annuler
        </button>
      </div>
      {escapeCancels ? (
        <p className="note-dictation__note">
          <Keys shortcut="Escape" /> annule aussi la dictée.
        </p>
      ) : null}
    </div>
  );
}
