// « Récents »: the latest results kept in memory, newest first, each with
// Copier, Afficher tout and Effacer. The only place (with the onboarding try)
// where dictated text is shown; announcements never contain it.

import { forwardRef, useEffect, useId, useImperativeHandle, useRef, useState } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import type { DraftView, TakeId } from "../ipc/contract";
import { clipboardErrorText, commandErrorText } from "../labels/fr";
import { minutesText, recentExpiry, recentTime } from "../labels/recents";
import { isoDuration } from "../lib/time";
import type { RecentsStore } from "../state/recents";
import { useAction } from "../state/useAction";
import { ConfirmButton, FieldError } from "./controls";
import { DraftText, GapLegend, gapKinds } from "./DraftText";
import { Glyph } from "./Icon";
import "./RecentList.css";

export interface RecentListHandle {
  /** Bring the list into view and move the keyboard to it. */
  focus: () => void;
}

interface Props {
  store: RecentsStore;
  /** Retention in minutes, for the explanations. */
  minutes: number;
  headingLevel?: 2 | 3;
  title?: string;
  /** Show only the newest result, expanded (onboarding try). */
  newestOnly?: boolean;
  id?: string;
}

export const RecentList = forwardRef<RecentListHandle, Props>(function RecentList(
  { store, minutes, headingLevel = 2, title = "Récents", newestOnly = false, id },
  ref,
) {
  const announce = useAnnounce();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const sectionRef = useRef<HTMLElement>(null);
  const headingId = useId();
  const copyNoteId = useId();
  const clearAll = useAction();
  const Heading = headingLevel === 2 ? "h2" : "h3";
  const items = store.items;
  const shown = items ? (newestOnly ? items.slice(0, 1) : items) : null;
  const pendingFocus = useRef<number | null>(null);

  useImperativeHandle(ref, () => ({
    focus: () => {
      sectionRef.current?.scrollIntoView?.({ block: "start", behavior: "smooth" });
      const first = sectionRef.current?.querySelector<HTMLElement>("[data-recent-title]");
      (first ?? headingRef.current)?.focus({ preventScroll: true });
    },
  }));

  // After a removal, keep the keyboard nearby: same position, else the title.
  useEffect(() => {
    if (pendingFocus.current === null) return;
    const index = pendingFocus.current;
    pendingFocus.current = null;
    const titles = sectionRef.current?.querySelectorAll<HTMLElement>("[data-recent-title]") ?? [];
    const target = titles[Math.min(index, titles.length - 1)] ?? headingRef.current;
    target?.focus();
  }, [items]);

  const anyIncomplete = shown?.some((item) => !item.meta.complete) ?? false;

  return (
    <section ref={sectionRef} id={id} className="recents" aria-labelledby={headingId} data-testid="recents">
      <div className="recents__header">
        <div className="recents__titles">
          <Heading id={headingId} ref={headingRef} tabIndex={-1} className="recents__title">
            {title}
          </Heading>
          {!newestOnly ? (
            <p className="hint">
              Gardés {minutesText(minutes)} en mémoire, jamais sur le disque.
            </p>
          ) : null}
        </div>
        {!newestOnly && shown && shown.length > 1 ? (
          <ConfirmButton
            small
            label="Tout effacer"
            labelSuffix="les textes récents"
            question={`Effacer les ${shown.length} textes récents ?`}
            confirmLabel="Effacer"
            onConfirm={async () => {
              const result = await clearAll.run(() => store.clear());
              if (result.ok) {
                announce(announcement("Textes récents effacés."));
                window.setTimeout(() => headingRef.current?.focus(), 0);
              }
            }}
          />
        ) : null}
      </div>

      {store.failed ? (
        <div className="btn-row">
          <FieldError>Les textes récents n’ont pas pu être affichés.</FieldError>
          <button type="button" className="btn btn--sm" onClick={store.reload}>
            Réessayer
          </button>
        </div>
      ) : null}

      {shown === null ? (
        <p className="muted">Chargement des textes récents…</p>
      ) : shown.length === 0 ? (
        newestOnly ? null : (
          <div className="recents__empty" data-testid="recents-empty">
            <span className="recents__empty-icon">
              <Glyph name="inbox" />
            </span>
            <div>
              <p className="recents__empty-title">Aucun texte récent</p>
              <p className="hint">
                Après chaque dictée, le texte s’affiche ici pendant {minutesText(minutes)}, prêt à être copié.
              </p>
            </div>
          </div>
        )
      ) : (
        <>
          <p id={copyNoteId} className="hint recents__copy-note" data-testid="recents-copy-note">
            Copier remplace le presse-papiers, sans passer par l’historique Win+V.
            {anyIncomplete ? " Les passages manquants ne sont pas copiés." : null}
          </p>
          <ol className="recents__list">
            {shown.map((item, index) => (
              <RecentItem
                key={`${item.meta.take}:${item.meta.createdAtMs}`}
                item={item}
                now={store.now}
                copyNoteId={copyNoteId}
                startExpanded={newestOnly}
                onCopy={store.copy}
                onClear={async (take) => {
                  await store.clear(take);
                  pendingFocus.current = index;
                }}
              />
            ))}
          </ol>
        </>
      )}
      {clearAll.error ? <FieldError>{commandErrorText(clearAll.error)}</FieldError> : null}
    </section>
  );
});

interface ItemProps {
  item: DraftView;
  now: number;
  copyNoteId: string;
  startExpanded: boolean;
  onCopy: (take: TakeId) => Promise<void>;
  onClear: (take: TakeId) => Promise<void>;
}

function RecentItem({ item, now, copyNoteId, startExpanded, onCopy, onClear }: ItemProps) {
  const announce = useAnnounce();
  const copy = useAction();
  const clear = useAction();
  const [expanded, setExpanded] = useState(startExpanded);
  const [copied, setCopied] = useState(false);
  const id = useId();
  const { meta } = item;
  const time = recentTime(meta.createdAtMs, now);
  const gaps = gapKinds(item.parts);
  const remaining = meta.expiresAtMs - now;
  // Short, complete texts are shown whole: no « Afficher tout » for them.
  const plain = item.fullText ?? item.parts.map((part) => part.text ?? "").join(" ");
  const expandable = gaps.length > 0 || plain.length > 160 || plain.includes("\n");

  const runCopy = async () => {
    setCopied(false);
    const result = await copy.run(() => onCopy(meta.take));
    if (result.ok) {
      setCopied(true);
      announce(announcement("Texte copié dans le presse-papiers."));
    } else {
      announce(announcement(clipboardErrorText(result.error)));
    }
  };

  const runClear = async () => {
    const result = await clear.run(() => onClear(meta.take));
    if (result.ok) announce(announcement("Texte effacé."));
  };

  return (
    <li className={meta.complete ? "recent" : "recent recent--incomplete"} data-testid="recent-item" data-take={meta.take}>
      <article aria-labelledby={`${id}-title`}>
        <div className="recent__meta">
          <h3 id={`${id}-title`} className="recent__time" tabIndex={-1} data-recent-title>
            <time dateTime={new Date(meta.createdAtMs).toISOString()}>{time}</time>
          </h3>
          <span className={meta.complete ? "chip" : "chip chip--warning"}>
            {meta.complete ? null : <Glyph name="clock" />}
            {meta.complete ? "Complet" : "Incomplet"}
          </span>
          <span className="recent__expiry">
            <time dateTime={isoDuration(remaining)}>{recentExpiry(meta.expiresAtMs, now)}</time>
          </span>
        </div>

        <div
          id={`${id}-text`}
          className={expanded ? "recent__text recent__text--expanded" : "recent__text"}
          data-testid="recent-text"
        >
          <DraftText view={item} />
        </div>

        {expanded && !meta.complete ? (
          <p className="hint">
            Texte incomplet : certains passages manquent ou sont à revoir. Seules les parties reçues sont récupérables.
          </p>
        ) : null}
        {expanded ? <GapLegend kinds={gaps} idPrefix={id} /> : null}

        <div className="recent__actions">
          <button
            type="button"
            className="btn btn--sm"
            onClick={() => void runCopy()}
            aria-describedby={copyNoteId}
            aria-disabled={copy.busy || undefined}
          >
            <Glyph name={copied ? "check" : "copy"} />
            <span className="btn__label">{copy.busy ? "Copie…" : "Copier"}</span>
            <span className="visually-hidden"> le texte de {time}</span>
          </button>
          {expandable ? (
            <button
              type="button"
              className="btn btn--sm btn--ghost"
              aria-expanded={expanded}
              aria-controls={`${id}-text`}
              onClick={() => setExpanded((value) => !value)}
            >
              <Glyph name="chevron-down" className={expanded ? "recent__chevron recent__chevron--up" : "recent__chevron"} />
              <span className="btn__label">{expanded ? "Réduire" : "Afficher tout"}</span>
              <span className="visually-hidden"> le texte de {time}</span>
            </button>
          ) : null}
          <button type="button" className="btn btn--sm btn--ghost recent__clear" onClick={() => void runClear()}>
            <Glyph name="trash" />
            <span className="btn__label">Effacer</span>
            <span className="visually-hidden"> le texte de {time}</span>
          </button>
        </div>
        {copied ? (
          <p className="recent__feedback" data-testid="copy-feedback">
            <Glyph name="check" /> Texte copié. Collez-le avec Ctrl+V là où vous le souhaitez.
          </p>
        ) : null}
        {copy.error ? <FieldError>{clipboardErrorText(copy.error)}</FieldError> : null}
        {clear.error ? <FieldError>{commandErrorText(clear.error)}</FieldError> : null}
      </article>
    </li>
  );
}
