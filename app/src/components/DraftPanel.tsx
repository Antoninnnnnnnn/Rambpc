// The draft: the only place where dictated text is shown. It is fetched with
// `get_draft` while this view is mounted, rendered as plain React text (no
// HTML), kept in component state only, and dropped when the draft goes away.

import { useEffect, useId, useState } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { useBridge } from "../ipc/bridge";
import type { DraftMeta, DraftPart, DraftView, PartStatus } from "../ipc/contract";
import { clipboardErrorText, commandErrorText } from "../labels/fr";
import { formatRemaining, isoDuration } from "../lib/time";
import { useUiCommands } from "../state/commands";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import { FieldError } from "./controls";
import { Glyph } from "./Icon";
import "./DraftPanel.css";

const GAP_LABELS: Record<Exclude<PartStatus, "admitted">, string> = {
  missing: "passage manquant",
  "needs-review": "à revoir",
  blocked: "bloqué",
  "no-text": "aucun texte obtenu",
};

const GAP_HELP: Record<Exclude<PartStatus, "admitted">, string> = {
  missing: "perdu lors d’une coupure ; il faut le redicter.",
  "needs-review": "reconnaissance incertaine, exclue de l’insertion automatique.",
  blocked: "masqué par le filtre de sécurité du service.",
  "no-text": "aucune parole reconnue sur ce passage.",
};

interface Props {
  headingLevel?: 2 | 3;
  title?: string;
}

export function DraftPanel({ headingLevel = 2, title = "Texte dicté" }: Props) {
  const state = useUiState();
  const meta = state.draft;
  const bridge = useBridge();
  const commands = useUiCommands();
  const announce = useAnnounce();
  const [view, setView] = useState<DraftView | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [copied, setCopied] = useState(false);
  const copy = useAction();
  const clear = useAction();
  const headingId = useId();
  const textId = useId();
  const Heading = headingLevel === 2 ? "h2" : "h3";
  const draftKey = meta ? `${meta.take}:${meta.createdAtMs}:${meta.complete}` : null;

  useEffect(() => {
    setCopied(false);
    copy.clearError();
    if (!draftKey) {
      setView(null);
      return;
    }
    let cancelled = false;
    setLoadFailed(false);
    bridge
      .invoke("get_draft")
      .then((draft) => {
        if (!cancelled) setView(draft);
      })
      .catch(() => {
        if (!cancelled) setLoadFailed(true);
      });
    return () => {
      cancelled = true;
    };
    // Refetch once per draft identity (take, creation, completeness).
  }, [bridge, draftKey, copy.clearError]);

  const remaining = useRemaining(meta);
  const expired = remaining !== null && remaining <= 0;

  // Drop the text from memory as soon as the draft expires or disappears.
  useEffect(() => {
    if (expired) setView(null);
  }, [expired]);

  if (!meta) return null;

  const onCopy = async () => {
    setCopied(false);
    const result = await copy.run(() => bridge.invoke("copy_draft"));
    if (result.ok) {
      setCopied(true);
      announce(announcement("Texte copié dans le presse-papiers."));
    } else {
      announce(announcement(clipboardErrorText(result.error)));
    }
  };

  const onClear = async () => {
    const result = await clear.run(commands.clearDraft);
    if (result.ok) {
      setView(null);
      announce(announcement("Brouillon effacé."));
    }
  };

  const current = view && view.meta.take === meta.take ? view : null;
  const gaps = current ? gapKinds(current.parts) : [];

  return (
    <section className="draft-panel" aria-labelledby={headingId} data-testid="draft-panel">
      <div className="draft-panel__header">
        <Heading id={headingId} className="draft-panel__title">
          {title}
        </Heading>
        <p className="draft-panel__expiry">
          {expired ? (
            "Brouillon expiré."
          ) : remaining !== null ? (
            <>
              Effacé automatiquement dans{" "}
              <time dateTime={isoDuration(remaining)}>{formatRemaining(remaining)}</time>
            </>
          ) : null}
        </p>
      </div>

      <p className={meta.complete ? "draft-panel__status" : "draft-panel__status draft-panel__status--incomplete"}>
        {meta.complete
          ? "Texte complet."
          : "Texte incomplet : certains passages manquent ou sont à revoir. Seules les parties reçues sont récupérables."}
      </p>

      {loadFailed ? (
        <FieldError>Le texte n’a pas pu être affiché. Copier reste possible.</FieldError>
      ) : current ? (
        <div
          id={textId}
          className="draft-text"
          role="region"
          aria-label="Texte du brouillon"
          tabIndex={0}
          data-testid="draft-text"
        >
          {current.fullText !== null ? current.fullText : <DraftParts parts={current.parts} />}
        </div>
      ) : expired ? null : (
        <p className="muted">Chargement du texte…</p>
      )}

      {gaps.length > 0 ? (
        <ul className="draft-panel__legend" aria-label="Signification des repères">
          {gaps.map((kind) => (
            <li key={kind}>
              <span className={`gap gap--${kind}`}>{GAP_LABELS[kind]}</span> : {GAP_HELP[kind]}
            </li>
          ))}
        </ul>
      ) : null}

      <div className="button-row">
        <button
          type="button"
          className="button button--primary"
          onClick={() => void onCopy()}
          disabled={expired}
          aria-describedby={`${textId}-copy-help`}
        >
          <Glyph name="copy" />
          {copy.busy ? "Copie…" : "Copier"}
        </button>
        <button type="button" className="button" onClick={() => void onClear()} disabled={expired}>
          <Glyph name="trash" />
          Effacer
        </button>
      </div>
      <p id={`${textId}-copy-help`} className="field-hint">
        La copie est exclue de l’historique du presse-papiers de Windows (Win+V).
        {meta.complete ? null : " Les passages manquants ne sont pas copiés."}
      </p>
      {copied ? (
        <p className="draft-panel__feedback" data-testid="copy-feedback">
          <Glyph name="copy" /> Texte copié. Collez-le avec Ctrl+V là où vous le souhaitez.
        </p>
      ) : null}
      {copy.error ? <FieldError>{clipboardErrorText(copy.error)}</FieldError> : null}
      {clear.error ? <FieldError>{commandErrorText(clear.error)}</FieldError> : null}
    </section>
  );
}

function DraftParts({ parts }: { parts: DraftPart[] }) {
  return (
    <>
      {parts.map((part, index) => {
        const separator = index > 0 ? " " : "";
        if (part.status === "admitted") {
          return (
            <span key={index}>
              {separator}
              {part.text ?? ""}
            </span>
          );
        }
        const label = GAP_LABELS[part.status];
        return (
          <span key={index}>
            {separator}
            <span className={`gap gap--${part.status}`} data-gap={part.status}>
              <span className="visually-hidden">[</span>
              {label}
              <span className="visually-hidden">]</span>
            </span>
            {part.status === "needs-review" && part.text ? (
              <>
                {" "}
                <mark className="review-text">{part.text}</mark>
              </>
            ) : null}
          </span>
        );
      })}
    </>
  );
}

function gapKinds(parts: DraftPart[]): Exclude<PartStatus, "admitted">[] {
  const kinds = new Set<Exclude<PartStatus, "admitted">>();
  for (const part of parts) if (part.status !== "admitted") kinds.add(part.status);
  return [...kinds];
}

/** Milliseconds until expiry, ticking every second; null without a draft. */
function useRemaining(meta: DraftMeta | null): number | null {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!meta) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [meta]);
  return meta ? meta.expiresAtMs - now : null;
}
