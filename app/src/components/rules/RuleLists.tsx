// Lists of personal words (Dictionnaire) and voice shortcuts (Raccourcis
// vocaux), their conflicts, and a sentence tester through `preview_render`.

import { useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from "react";
import { toCommandError, useBridge } from "../../ipc/bridge";
import type { CommandError, Expression, LexiconEntry, RuleKind, Settings } from "../../ipc/contract";
import { commandErrorText } from "../../labels/fr";
import { conflictSummary, conflictsFor } from "../../labels/rules";
import { FieldError } from "../controls";
import { Glyph, type GlyphName } from "../Icon";
import { Notice } from "../Notice";
import { ExpressionForm, LexiconForm } from "./RuleForms";
import type { RulesStore } from "./useRules";
import "./rules.css";

type Editing = { id: string | null } | null;

const keyFor = (kind: RuleKind, id: string) => `${kind}:${id}`;
const addKey = (kind: RuleKind) => `${kind}:add`;

/** Focus targets after the list re-rendered (closing a form, deleting a rule). */
function useFocusAfterRender() {
  const targets = useRef(new Map<string, HTMLElement | null>());
  const pending = useRef<string | null>(null);
  useEffect(() => {
    if (!pending.current) return;
    targets.current.get(pending.current)?.focus();
    pending.current = null;
  });
  return {
    register: (key: string) => (element: HTMLElement | null) => {
      targets.current.set(key, element);
    },
    request: (key: string) => {
      pending.current = key;
    },
  };
}

/** Load failure, unreadable rules and refused saves, shared by both screens. */
export function RulesStatus({ store }: { store: RulesStore }) {
  if (store.loadError) {
    return (
      <div className="stack stack--tight">
        <FieldError>Les mots et expressions n’ont pas pu être lus.</FieldError>
        <div className="btn-row">
          <button type="button" className="btn btn--sm" onClick={store.load}>
            Réessayer
          </button>
        </div>
      </div>
    );
  }
  const unsaved = store.conflicts.length > 0 || store.saveError !== null;
  return (
    <>
      {store.stored?.unreadable ? (
        <Notice tone="info" title="Mots et expressions illisibles ici" headingLevel={2} testId="rules-unreadable">
          <p>
            Vos mots et expressions ont été créés sous un autre compte Windows ou sur un autre PC. Ils sont conservés,
            mais ne peuvent pas être lus ni modifiés ici.
          </p>
        </Notice>
      ) : null}
      {unsaved ? (
        <Notice
          tone="error"
          title={
            store.conflicts.length > 0
              ? conflictSummary(store.conflicts.length)
              : "Les mots et expressions n’ont pas pu être enregistrés."
          }
          headingLevel={2}
          testId="rules-conflicts"
          actions={
            <button type="button" className="btn btn--sm" onClick={store.revert}>
              Rétablir les règles enregistrées
            </button>
          }
        >
          <p>Les dernières modifications ne sont pas enregistrées : la dictée utilise encore les règles précédentes.</p>
          {store.saveError ? <p>{commandErrorText(store.saveError)}</p> : null}
        </Notice>
      ) : null}
    </>
  );
}

interface ListShellProps {
  title: string;
  addLabel: string;
  count: number;
  readOnly: boolean;
  adding: boolean;
  addRef: (element: HTMLElement | null) => void;
  onAdd: () => void;
  form: ReactNode;
  empty: { glyph: GlyphName; title: string; text: ReactNode };
  children: ReactNode;
  testId: string;
  note?: ReactNode;
}

function ListShell({ title, addLabel, count, readOnly, adding, addRef, onAdd, form, empty, children, testId, note }: ListShellProps) {
  const headingId = useId();
  return (
    <section className="rules" aria-labelledby={headingId} data-testid={testId}>
      <div className="rules__header">
        <h2 id={headingId} className="section-title">
          {title}
          <span className="rules__count">{count}</span>
        </h2>
        {!readOnly && !adding ? (
          <button ref={addRef} type="button" className="btn btn--primary btn--sm" onClick={onAdd}>
            <Glyph name="add" />
            {addLabel}
          </button>
        ) : null}
      </div>
      {note}
      {adding ? <div className="rules__form">{form}</div> : null}
      {count === 0 && !adding ? (
        <div className="empty">
          <span className="empty__icon">
            <Glyph name={empty.glyph} />
          </span>
          <p className="empty__title">{empty.title}</p>
          <p className="hint">{empty.text}</p>
        </div>
      ) : null}
      {count > 0 ? <ul className="rule-list">{children}</ul> : null}
    </section>
  );
}

export function LexiconList({ store, faithful }: { store: RulesStore; faithful: boolean }) {
  const [editing, setEditing] = useState<Editing>(null);
  const focus = useFocusAfterRender();
  const entries = store.working?.lexicon ?? [];
  const readOnly = store.stored?.unreadable ?? false;
  const adding = editing !== null && editing.id === null;

  const close = (key: string) => {
    focus.request(key);
    setEditing(null);
  };

  const save = (entry: LexiconEntry, isNew: boolean) => {
    void store.commit(
      (rules) => ({
        ...rules,
        lexicon: isNew ? [...rules.lexicon, entry] : rules.lexicon.map((item) => (item.id === entry.id ? entry : item)),
      }),
      isNew ? "Mot ajouté." : "Mot enregistré.",
    );
    close(isNew ? addKey("lexicon") : keyFor("lexicon", entry.id));
  };

  return (
    <ListShell
      title="Vos mots"
      addLabel="Ajouter un mot"
      count={entries.length}
      readOnly={readOnly}
      adding={adding}
      addRef={focus.register(addKey("lexicon"))}
      onAdd={() => setEditing({ id: null })}
      testId="lexicon-section"
      note={
        faithful ? (
          <p className="rules-note rules-note--active" data-testid="faithful-note">
            Style Fidèle actif : seuls les mots marqués « Aussi en Fidèle » sont remplacés.
          </p>
        ) : null
      }
      form={<LexiconForm initial={null} onSubmit={(value) => save(value, true)} onCancel={() => close(addKey("lexicon"))} />}
      empty={{
        glyph: "dictionary",
        title: "Aucun mot pour le moment",
        text: <>Ajoutez un nom propre, un sigle ou un terme métier que Rambler écrit mal.</>,
      }}
    >
      {entries.map((entry) =>
        editing?.id === entry.id ? (
          <li key={entry.id} className="rule rule--editing">
            <LexiconForm
              initial={entry}
              onSubmit={(value) => save(value, false)}
              onCancel={() => close(keyFor("lexicon", entry.id))}
            />
          </li>
        ) : (
          <RuleItem
            key={entry.id}
            kind="lexicon"
            entry={entry}
            store={store}
            readOnly={readOnly}
            editRef={focus.register(keyFor("lexicon", entry.id))}
            onEdit={() => setEditing({ id: entry.id })}
            onToggle={(enabled) =>
              void store.commit(
                (rules) => ({
                  ...rules,
                  lexicon: rules.lexicon.map((item) => (item.id === entry.id ? { ...item, enabled } : item)),
                }),
                enabled ? "Mot activé." : "Mot désactivé.",
              )
            }
            onDelete={() => {
              void store.commit(
                (rules) => ({ ...rules, lexicon: rules.lexicon.filter((item) => item.id !== entry.id) }),
                "Mot supprimé.",
              );
              close(addKey("lexicon"));
            }}
          >
            <p className="rule__pair">
              <span className="rule__from">{entry.heard}</span>
              <Glyph name="arrow-right" className="rule__arrow" />
              <span className="visually-hidden"> devient </span>
              <span className="rule__to">{entry.written}</span>
            </p>
            <p className="rule__meta">
              <span className={entry.enabled ? "rule__state" : "rule__state rule__state--off"}>
                {entry.enabled ? "Actif" : "Désactivé"}
              </span>
              {entry.applyInFaithful ? <span className="chip">Aussi en Fidèle</span> : null}
            </p>
          </RuleItem>
        ),
      )}
    </ListShell>
  );
}

export function ExpressionList({ store, faithful }: { store: RulesStore; faithful: boolean }) {
  const [editing, setEditing] = useState<Editing>(null);
  const focus = useFocusAfterRender();
  const entries = store.working?.expressions ?? [];
  const readOnly = store.stored?.unreadable ?? false;
  const adding = editing !== null && editing.id === null;

  const close = (key: string) => {
    focus.request(key);
    setEditing(null);
  };

  const save = (expression: Expression, isNew: boolean) => {
    void store.commit(
      (rules) => ({
        ...rules,
        expressions: isNew
          ? [...rules.expressions, expression]
          : rules.expressions.map((item) => (item.id === expression.id ? expression : item)),
      }),
      isNew ? "Expression ajoutée." : "Expression enregistrée.",
    );
    close(isNew ? addKey("expression") : keyFor("expression", expression.id));
  };

  return (
    <ListShell
      title="Vos raccourcis"
      addLabel="Ajouter une expression"
      count={entries.length}
      readOnly={readOnly}
      adding={adding}
      addRef={focus.register(addKey("expression"))}
      onAdd={() => setEditing({ id: null })}
      testId="expression-section"
      note={
        <p className={faithful ? "rules-note rules-note--active" : "rules-note"} data-testid="faithful-note">
          {faithful
            ? "Style Fidèle actif : vos expressions ne s’appliquent pas. Elles reprennent en style Naturel."
            : "Les expressions sont inactives en style Fidèle."}
        </p>
      }
      form={
        <ExpressionForm
          initial={null}
          onSubmit={(value) => save(value, true)}
          onCancel={() => close(addKey("expression"))}
        />
      }
      empty={{
        glyph: "shortcuts",
        title: "Aucune expression pour le moment",
        text: <>Par exemple, dites « insère ma signature » pour écrire votre formule de politesse complète.</>,
      }}
    >
      {entries.map((expression) =>
        editing?.id === expression.id ? (
          <li key={expression.id} className="rule rule--editing">
            <ExpressionForm
              initial={expression}
              onSubmit={(value) => save(value, false)}
              onCancel={() => close(keyFor("expression", expression.id))}
            />
          </li>
        ) : (
          <RuleItem
            key={expression.id}
            kind="expression"
            entry={expression}
            store={store}
            readOnly={readOnly}
            editRef={focus.register(keyFor("expression", expression.id))}
            onEdit={() => setEditing({ id: expression.id })}
            onToggle={(enabled) =>
              void store.commit(
                (rules) => ({
                  ...rules,
                  expressions: rules.expressions.map((item) => (item.id === expression.id ? { ...item, enabled } : item)),
                }),
                enabled ? "Expression activée." : "Expression désactivée.",
              )
            }
            onDelete={() => {
              void store.commit(
                (rules) => ({ ...rules, expressions: rules.expressions.filter((item) => item.id !== expression.id) }),
                "Expression supprimée.",
              );
              close(addKey("expression"));
            }}
          >
            <p className="rule__trigger">
              <span className="visually-hidden">Déclencheur : </span>« {expression.trigger} »
            </p>
            <p className="rule__payload">
              <span className="visually-hidden">Texte inséré : </span>
              {expression.text}
            </p>
            <p className="rule__meta">
              <span className={expression.enabled ? "rule__state" : "rule__state rule__state--off"}>
                {expression.enabled ? "Active" : "Désactivée"}
              </span>
            </p>
          </RuleItem>
        ),
      )}
    </ListShell>
  );
}

interface RuleItemProps {
  kind: RuleKind;
  entry: LexiconEntry | Expression;
  store: RulesStore;
  readOnly: boolean;
  editRef: (element: HTMLElement | null) => void;
  onEdit: () => void;
  onToggle: (enabled: boolean) => void;
  onDelete: () => void;
  children: ReactNode;
}

function RuleItem({ kind, entry, store, readOnly, editRef, onEdit, onToggle, onDelete, children }: RuleItemProps) {
  const id = useId();
  const name = kind === "lexicon" ? (entry as LexiconEntry).heard : (entry as Expression).trigger;
  const messages = conflictsFor({ kind, id: entry.id }, store.conflicts, store.describe);
  return (
    <li
      className={[
        "rule",
        messages.length > 0 ? "rule--conflict" : entry.enabled ? "" : "rule--disabled",
        readOnly ? "rule--readonly" : "",
      ]
        .filter(Boolean)
        .join(" ")}
      data-testid={`rule-${kind}`}
      data-rule-id={entry.id}
    >
      {!readOnly ? (
        <span className="switch switch--sm rule__toggle">
          <input
            id={`${id}-enabled`}
            type="checkbox"
            role="switch"
            checked={entry.enabled}
            onChange={(event) => onToggle(event.currentTarget.checked)}
            aria-label={`${kind === "lexicon" ? "Actif" : "Active"} : « ${name} »`}
          />
          <span className="switch__track" aria-hidden="true">
            <span className="switch__thumb">
              <svg viewBox="0 0 12 12" width="12" height="12" focusable="false">
                <path d="M3 6.2l2 2 4-4.4" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </span>
          </span>
        </span>
      ) : null}
      <div className="rule__body">
        {children}
        {messages.length > 0 ? (
          <ul className="rule__conflicts" aria-label="Conflits">
            {messages.map((message) => (
              <li key={message}>
                <FieldError>{message}</FieldError>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
      {!readOnly ? (
        <div className="rule__actions">
          <button ref={editRef} type="button" className="btn btn--sm btn--ghost" onClick={onEdit}>
            <Glyph name="edit" />
            Modifier<span className="visually-hidden"> « {name} »</span>
          </button>
          <button type="button" className="btn btn--sm btn--ghost" onClick={onDelete}>
            <Glyph name="trash" />
            Supprimer<span className="visually-hidden"> « {name} »</span>
          </button>
        </div>
      ) : null}
    </li>
  );
}

/** Try a typed sentence with `preview_render`, saved rules and current settings. */
export function TryRules({ settings, placeholder }: { settings: Settings; placeholder?: string }) {
  const bridge = useBridge();
  const [sample, setSample] = useState("");
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<CommandError | null>(null);
  const id = useId();

  const run = async (event: FormEvent) => {
    event.preventDefault();
    setError(null);
    setResult(null);
    if (!sample.trim()) return;
    try {
      setResult(await bridge.invoke("preview_render", { sample, settings }));
    } catch (raw) {
      setError(toCommandError(raw));
    }
  };

  return (
    <section className="card try" aria-labelledby={`${id}-title`}>
      <div className="card__titles">
        <h2 id={`${id}-title`} className="card__title">
          Essayer une phrase
        </h2>
        <p id={`${id}-hint`} className="card__desc">
          Sans ouvrir le micro, avec vos mots et expressions enregistrés et vos réglages de style actuels.
        </p>
      </div>
      <form className="try__form" onSubmit={(event) => void run(event)}>
        <label htmlFor={`${id}-sample`} className="field__label">
          Phrase telle que Rambler l’écrirait
        </label>
        <div className="try__row">
          <input
            id={`${id}-sample`}
            className="input"
            value={sample}
            maxLength={500}
            placeholder={placeholder}
            onChange={(event) => setSample(event.currentTarget.value)}
            autoComplete="off"
            spellCheck={false}
            aria-describedby={`${id}-hint`}
          />
          <button type="submit" className="btn">
            Essayer
          </button>
        </div>
      </form>
      {result !== null ? (
        <div className="try__result" data-testid="rules-try-result">
          <p className="try__result-label">Texte inséré</p>
          <p className="try__result-text">{result}</p>
        </div>
      ) : null}
      {error ? <FieldError>{commandErrorText(error, "L’essai n’a pas pu être effectué.")}</FieldError> : null}
    </section>
  );
}
