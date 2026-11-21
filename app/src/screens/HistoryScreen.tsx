// Historique: optional, off by default. Only this view asks for history text
// (`get_history`), keeps it in component state and shows it as plain text,
// grouped by day. Announcements are fixed French copy and counts, never entry
// or search text.

import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { Card, ChoiceGroup, ConfirmButton, FieldError, Switch } from "../components/controls";
import { Glyph } from "../components/Icon";
import { Notice } from "../components/Notice";
import { toCommandError, useBridge } from "../ipc/bridge";
import type { CommandError, HistoryEntry, HistoryStatus } from "../ipc/contract";
import { clipboardErrorText, commandErrorText, settingSaveErrorText } from "../labels/fr";
import {
  dayCountText,
  daysLabel,
  entryCountText,
  formatEntryDate,
  formatEntryTime,
  formatExpiry,
  groupByDay,
  HISTORY_DAY_CHOICES,
  LANGUAGE_LABELS,
  PROFILE_LABELS,
  resultCountText,
  wordCount,
  wordsText,
} from "../labels/history";
import { useSettings } from "../state/settings";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import { ScreenHeader } from "./ScreenHeader";
import "./HistoryScreen.css";

export const HISTORY_PAGE_SIZE = 50;

export function HistoryScreen() {
  const bridge = useBridge();
  const announce = useAnnounce();
  const { settings, update, applyLocal, errors } = useSettings();
  const ui = useUiState();
  const [status, setStatus] = useState<HistoryStatus | null>(null);
  const [statusError, setStatusError] = useState(false);
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [activeSearch, setActiveSearch] = useState("");
  const [listError, setListError] = useState<CommandError | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const actions = useAction();
  const listHeading = useRef<HTMLHeadingElement>(null);
  const ids = { list: useId(), search: useId(), count: useId(), copy: useId() };

  const refreshStatus = useCallback(() => {
    setStatusError(false);
    return bridge
      .invoke("get_history_status")
      .then((next) => {
        setStatus(next);
        return next;
      })
      .catch(() => {
        setStatusError(true);
        return null;
      });
  }, [bridge]);

  const loadFirstPage = useCallback(
    async (query: string) => {
      setListError(null);
      try {
        const page = await bridge.invoke("get_history", {
          query: { search: query || null, limit: HISTORY_PAGE_SIZE, cursor: null },
        });
        setEntries(page.entries);
        setNextCursor(page.nextCursor);
        return page;
      } catch (error) {
        // Never leave another query's entries under this one.
        setEntries(null);
        setNextCursor(null);
        setListError(toCommandError(error));
        return null;
      }
    },
    [bridge],
  );

  // Status first; entries only when there can be some.
  const reload = useCallback(
    async (query: string) => {
      const next = await refreshStatus();
      if (next && next.available && (next.enabled || next.entryCount > 0)) await loadFirstPage(query);
      else setEntries([]);
    },
    [refreshStatus, loadFirstPage],
  );

  useEffect(() => {
    void reload("");
  }, [reload]);

  // A finished take may have added an entry or consumed « sans historique »:
  // reload when the outcome changes, keeping the active search.
  const outcomeKey = ui.lastOutcome ? JSON.stringify([ui.lastOutcome, ui.draft?.take ?? null]) : "";
  const seenOutcome = useRef(outcomeKey);
  const searchRef = useRef(activeSearch);
  useEffect(() => {
    searchRef.current = activeSearch;
  }, [activeSearch]);
  useEffect(() => {
    if (outcomeKey === seenOutcome.current) return;
    seenOutcome.current = outcomeKey;
    void reload(searchRef.current);
  }, [outcomeKey, reload]);

  const runSearch = async (event: FormEvent) => {
    event.preventDefault();
    const query = search.trim();
    setActiveSearch(query);
    const page = await loadFirstPage(query);
    if (page) {
      announce(announcement(resultCountText(page.entries.length, page.nextCursor !== null)));
    }
  };

  const clearSearch = async () => {
    setSearch("");
    setActiveSearch("");
    await loadFirstPage("");
    announce(announcement("Recherche effacée."));
  };

  const loadMore = async () => {
    if (!nextCursor || loadingMore) return;
    setLoadingMore(true);
    setListError(null);
    try {
      const page = await bridge.invoke("get_history", {
        query: { search: activeSearch || null, limit: HISTORY_PAGE_SIZE, cursor: nextCursor },
      });
      setEntries((current) => [...(current ?? []), ...page.entries]);
      setNextCursor(page.nextCursor);
      announce(
        announcement(
          page.entries.length === 1 ? "1 dictée de plus affichée." : `${page.entries.length} dictées de plus affichées.`,
        ),
      );
    } catch (error) {
      setListError(toCommandError(error));
    } finally {
      setLoadingMore(false);
    }
  };

  const setEnabled = async (enabled: boolean) => {
    const ok = await update("history", (s) => ({ ...s, history: { ...s.history, enabled } }));
    const next = await refreshStatus();
    if (ok) {
      announce(announcement(enabled ? "Historique activé." : "Historique désactivé."));
      if (enabled && next?.available) await loadFirstPage(activeSearch);
    }
  };

  const setRetention = async (retentionDays: number) => {
    const ok = await update("history", (s) => ({ ...s, history: { ...s.history, retentionDays } }));
    await refreshStatus();
    if (ok && status && (status.enabled || status.entryCount > 0)) await loadFirstPage(activeSearch);
  };

  const setSkipNext = async (skip: boolean) => {
    const result = await actions.run(() => bridge.invoke("skip_history_next_take", { skip }));
    if (result.ok) {
      setStatus(result.value);
      announce(
        announcement(skip ? "La prochaine dictée ne sera pas enregistrée." : "La prochaine dictée sera enregistrée."),
      );
    }
  };

  const clearAll = async (disable: boolean) => {
    const result = await actions.run(() => bridge.invoke("clear_history", { disable }));
    if (!result.ok) return;
    setStatus(result.value);
    setEntries([]);
    setNextCursor(null);
    if (disable) {
      applyLocal((s) => ({ ...s, history: { ...s.history, enabled: false } }));
      // Its button disappears with the list: keep the keyboard on the page title.
      window.setTimeout(() => document.querySelector<HTMLElement>("[data-screen-title]")?.focus(), 0);
    }
    announce(announcement(disable ? "Historique désactivé et effacé." : "Historique effacé."));
  };

  const removeEntry = async (id: string, index: number) => {
    const result = await actions.run(() => bridge.invoke("delete_history_entry", { id }));
    if (!result.ok) return;
    setStatus(result.value);
    setEntries((current) => (current ?? []).filter((entry) => entry.id !== id));
    announce(announcement("Dictée supprimée."));
    // Keep the keyboard position: next entry, else previous, else the list title.
    window.setTimeout(() => {
      const items = document.querySelectorAll<HTMLElement>("[data-history-entry-title]");
      const target = items[Math.min(index, items.length - 1)] ?? listHeading.current;
      target?.focus();
    }, 0);
  };

  if (!settings || (!status && !statusError)) {
    return (
      <div className="screen">
        <ScreenHeader title="Historique" />
        <p className="muted">Chargement de l’historique…</p>
      </div>
    );
  }

  const enabled = settings.history.enabled;
  const available = status?.available ?? true;
  const count = status?.entryCount ?? 0;
  const showList = available && (enabled || count > 0 || (entries?.length ?? 0) > 0);
  const historyError = errors.history;
  const days = entries ? groupByDay(entries) : [];
  const positions = new Map(entries?.map((entry, index) => [entry.id, index]));

  return (
    <div className="screen history-screen">
      <ScreenHeader title="Historique">
        Retrouvez le texte de vos dernières dictées. Seul le texte est conservé, sur ce PC, protégé par votre compte
        Windows. Aucun son n’est enregistré.
      </ScreenHeader>

      {statusError ? (
        <Notice
          tone="error"
          title="L’état de l’historique n’a pas pu être lu."
          testId="history-status-error"
          actions={
            <button type="button" className="btn btn--sm" onClick={() => void reload(activeSearch)}>
              Réessayer
            </button>
          }
        />
      ) : null}

      {!available ? (
        <Notice tone="info" title="Historique suspendu pour cette session" testId="history-ephemeral">
          <p>
            Sambler fonctionne en session éphémère : son dossier de données n’est pas inscriptible. Les réglages et les
            textes récents restent en mémoire et rien n’est archivé jusqu’à la fermeture.
          </p>
          <p>Pour retrouver l’historique, extrayez Sambler dans un dossier où vous pouvez écrire, puis relancez-le.</p>
        </Notice>
      ) : null}

      {/* The dictations first: on a wide sheet, Conservation sits beside them. */}
      {showList ? (
        <section className="history" aria-labelledby={ids.list}>
          <h2 id={ids.list} ref={listHeading} tabIndex={-1} className="section-title">
            Dictées enregistrées
          </h2>
          <form role="search" className="history__search" onSubmit={(event) => void runSearch(event)}>
            <label htmlFor={ids.search} className="visually-hidden">
              Rechercher dans l’historique
            </label>
            <div className="history__search-row">
              <span className="history__search-field">
                <Glyph name="search" />
                <input
                  id={ids.search}
                  className="input"
                  type="search"
                  value={search}
                  placeholder="Rechercher dans l’historique"
                  onChange={(event) => setSearch(event.currentTarget.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
              </span>
              <button type="submit" className="btn">
                Rechercher
              </button>
              {activeSearch ? (
                <button type="button" className="btn btn--ghost" onClick={() => void clearSearch()}>
                  Effacer la recherche
                </button>
              ) : null}
            </div>
          </form>

          {entries === null ? (
            listError ? (
              <Notice
                tone="error"
                headingLevel={3}
                title={commandErrorText(listError, "L’historique n’a pas pu être lu.")}
                testId="history-list-error"
                actions={
                  <button type="button" className="btn btn--sm" onClick={() => void loadFirstPage(activeSearch)}>
                    Réessayer
                  </button>
                }
              />
            ) : (
              <p className="muted">Chargement…</p>
            )
          ) : entries.length === 0 ? (
            <div className="empty history__empty" data-testid="history-empty">
              <span className="empty__icon">
                <Glyph name={activeSearch ? "search" : "inbox"} />
              </span>
              {activeSearch ? (
                <>
                  <p className="empty__title">Rien trouvé</p>
                  <p className="hint">Aucune dictée ne correspond à cette recherche.</p>
                </>
              ) : (
                <>
                  <p className="empty__title">Aucune dictée enregistrée pour le moment.</p>
                  <p className="hint">
                    Vos prochaines dictées apparaîtront ici, jour par jour, pendant{" "}
                    {daysLabel(settings.history.retentionDays)}.
                  </p>
                </>
              )}
            </div>
          ) : (
            <>
              <p id={ids.copy} className="hint history__copy-note" data-testid="history-copy-note">
                Copier remplace le contenu actuel du presse-papiers. La copie est exclue de l’historique du
                presse-papiers de Windows (Win+V).
              </p>
              <ol className="history__days" aria-describedby={ids.count} data-testid="history-list">
                {days.map((day, index) => {
                  const dayId = `${ids.list}-day-${day.key}`;
                  const words = day.entries.reduce((sum, entry) => sum + wordCount(entry.text), 0);
                  // The last day may continue on the next page.
                  const partial = index === days.length - 1 && nextCursor !== null;
                  return (
                    <li key={day.key} className="history-day">
                      <p id={dayId} className="history-day__title">
                        <span className="history-day__name">{day.label}</span>{" "}
                        <span className="history-day__count">{dayCountText(day.entries.length, words, partial)}</span>
                      </p>
                      <ul className="history__list" aria-labelledby={dayId}>
                        {day.entries.map((entry) => (
                          <HistoryItem
                            key={entry.id}
                            entry={entry}
                            copyNoteId={ids.copy}
                            onDelete={() => void removeEntry(entry.id, positions.get(entry.id) ?? 0)}
                          />
                        ))}
                      </ul>
                    </li>
                  );
                })}
              </ol>
            </>
          )}
          <p id={ids.count} className="visually-hidden">
            {entries ? `${entries.length} dictées affichées.` : ""}
          </p>

          {nextCursor ? (
            <div className="btn-row history__more">
              <button
                type="button"
                className="btn"
                aria-disabled={loadingMore || undefined}
                onClick={() => void loadMore()}
              >
                {loadingMore ? "Chargement…" : "Afficher plus"}
              </button>
            </div>
          ) : null}
          {listError && entries !== null ? (
            <FieldError>{commandErrorText(listError, "L’historique n’a pas pu être lu.")}</FieldError>
          ) : null}
        </section>
      ) : null}

      <Card title="Conservation" className="history-keep">
        <Switch
          label="Conserver mes dictées sur ce PC"
          description={
            <>
              Texte seulement, jamais le son. Les dictées restent sur ce PC, chiffrées pour votre compte Windows, puis
              sont effacées après la durée choisie. La reconnaissance se fait chez Google, que l’historique soit activé
              ou non.
            </>
          }
          checked={enabled}
          disabled={!available}
          onChange={(next) => void setEnabled(next)}
          error={historyError ? settingSaveErrorText(historyError) : undefined}
        />
        <ChoiceGroup
          legend="Durée de conservation"
          name="history-retention"
          variant="segmented"
          value={settings.history.retentionDays}
          options={HISTORY_DAY_CHOICES.map((days) => ({ value: days, label: daysLabel(days) }))}
          onChange={(days) => void setRetention(days)}
          description="7 jours par défaut. Une durée plus courte s’applique aussitôt aux dictées déjà enregistrées."
        />
        {enabled && available ? (
          <Switch
            label="Sans historique pour la prochaine dictée"
            description="La prochaine dictée ne sera pas enregistrée, puis l’historique reprend normalement."
            checked={status?.skipNext ?? false}
            onChange={(next) => void setSkipNext(next)}
          />
        ) : null}
        <div className="stack stack--tight">
          <p className="hint" data-testid="history-count">
            {entryCountText(count)}
            {!enabled && count > 0
              ? " Elles restent jusqu’à leur expiration ; « Tout effacer » les supprime maintenant."
              : null}
          </p>
          <div className="btn-row">
            <ConfirmButton
              small
              label="Tout effacer"
              question={
                count === 1
                  ? "Effacer définitivement la dictée enregistrée ?"
                  : `Effacer définitivement les ${count} dictées enregistrées ?`
              }
              confirmLabel="Effacer définitivement"
              disabled={count === 0 && (entries?.length ?? 0) === 0}
              onConfirm={() => clearAll(false)}
            />
            {enabled ? (
              <ConfirmButton
                small
                label="Désactiver et effacer"
                question="Désactiver l’historique et effacer définitivement toutes les dictées enregistrées ?"
                confirmLabel="Désactiver et effacer"
                onConfirm={() => clearAll(true)}
              />
            ) : null}
          </div>
          {actions.error ? <FieldError>{commandErrorText(actions.error)}</FieldError> : null}
        </div>
      </Card>

      {!showList && available && !enabled ? (
        <div className="empty history-off" data-testid="history-off">
          <span className="empty__icon">
            <Glyph name="history" />
          </span>
          <h2 className="section-title">L’historique est désactivé</h2>
          <p className="hint">
            Activez « Conserver mes dictées sur ce PC » pour retrouver vos prochaines dictées ici, pendant{" "}
            {daysLabel(settings.history.retentionDays)}.
          </p>
        </div>
      ) : null}
    </div>
  );
}

interface HistoryItemProps {
  entry: HistoryEntry;
  /** The shared « Copier remplace… » note. */
  copyNoteId: string;
  onDelete: () => void;
}

function HistoryItem({ entry, copyNoteId, onDelete }: HistoryItemProps) {
  const bridge = useBridge();
  const announce = useAnnounce();
  const copy = useAction();
  const [copied, setCopied] = useState(false);
  const titleId = useId();
  const date = formatEntryDate(entry.createdAtMs);
  const words = wordCount(entry.text);
  const lang = entry.language === "english" ? "en" : "fr";

  const onCopy = async () => {
    setCopied(false);
    const result = await copy.run(() => bridge.invoke("copy_history_entry", { id: entry.id }));
    if (result.ok) {
      setCopied(true);
      announce(announcement("Dictée copiée dans le presse-papiers."));
    } else {
      announce(announcement(clipboardErrorText(result.error)));
    }
  };

  return (
    <li className="history-item" data-testid="history-entry">
      <article aria-labelledby={titleId}>
        {/* The day is the group's label: the time is shown, the full date is read. */}
        <h3 id={titleId} className="history-item__date" tabIndex={-1} data-history-entry-title>
          <time dateTime={new Date(entry.createdAtMs).toISOString()}>
            <span className="history-item__time" aria-hidden="true">
              {formatEntryTime(entry.createdAtMs)}
            </span>
            <span className="visually-hidden">{date}</span>
          </time>
        </h3>
        {entry.text !== null ? (
          <p className="history-item__text" lang={lang} data-testid="history-text">
            {entry.text}
          </p>
        ) : (
          <p className="history-item__unreadable">
            Texte illisible sous ce compte Windows : la dictée est conservée mais ne peut pas être affichée ici.
          </p>
        )}
        <p className="history-item__meta">
          {LANGUAGE_LABELS[entry.language]} · {PROFILE_LABELS[entry.profile]} · {formatExpiry(entry.expiresAtMs)}
        </p>
        {words > 0 ? <p className="history-item__words">{wordsText(words)}</p> : null}
        <div className="history-item__actions">
          <button
            type="button"
            className="btn btn--sm history-item__copy"
            disabled={entry.text === null}
            aria-describedby={copyNoteId}
            onClick={() => void onCopy()}
          >
            <Glyph name={copied ? "check" : "copy"} />
            <span className="btn__label">Copier</span>
            <span className="visually-hidden"> la dictée du {date}</span>
          </button>
          <button type="button" className="btn btn--sm btn--ghost history-item__delete" onClick={onDelete}>
            <Glyph name="trash" />
            <span className="btn__label">Supprimer</span>
            <span className="visually-hidden"> la dictée du {date}</span>
          </button>
        </div>
        {copied ? (
          <p className="history-item__feedback">
            <Glyph name="check" />
            Copiée.
          </p>
        ) : null}
        {copy.error ? <FieldError>{clipboardErrorText(copy.error)}</FieldError> : null}
      </article>
    </li>
  );
}
