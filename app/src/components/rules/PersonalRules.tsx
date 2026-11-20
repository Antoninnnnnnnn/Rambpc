// Réglages → « Mots et expressions ». Rule text is fetched by this view only
// (`get_personal_rules`), edited locally and saved as a whole set with
// `save_personal_rules`. Conflicts are shown next to the rules involved;
// announcements never contain rule text.

import { useCallback, useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from "react";
import { announcement, useAnnounce } from "../../a11y/announcer";
import { toCommandError, useBridge } from "../../ipc/bridge";
import type {
  CommandError,
  Expression,
  LexiconEntry,
  PersonalRules as StoredRules,
  RuleConflict,
  RuleKind,
  Settings,
} from "../../ipc/contract";
import { commandErrorText } from "../../labels/fr";
import { conflictSummary, conflictsFor, type DescribeRule } from "../../labels/rules";
import { FieldError, Panel } from "../controls";
import { Glyph } from "../Icon";
import { InfoBar } from "../InfoBar";
import { ExpressionForm, LexiconForm } from "./RuleForms";
import "./rules.css";

interface WorkingRules {
  lexicon: LexiconEntry[];
  expressions: Expression[];
}

type Editing = { kind: RuleKind; id: string | null } | null;

export function PersonalRulesPanel({ id, settings }: { id: string; settings: Settings }) {
  const bridge = useBridge();
  const announce = useAnnounce();
  const [stored, setStored] = useState<StoredRules | null>(null);
  const [working, setWorking] = useState<WorkingRules | null>(null);
  const [conflicts, setConflicts] = useState<RuleConflict[]>([]);
  const [loadError, setLoadError] = useState(false);
  const [saveError, setSaveError] = useState<CommandError | null>(null);
  const [editing, setEditing] = useState<Editing>(null);
  const workingRef = useRef<WorkingRules | null>(null);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const focusTargets = useRef(new Map<string, HTMLElement | null>());
  const pendingFocus = useRef<string | null>(null);
  const faithful = settings.profile === "faithful";

  const load = useCallback(() => {
    setLoadError(false);
    bridge
      .invoke("get_personal_rules")
      .then((rules) => {
        setStored(rules);
        workingRef.current = { lexicon: rules.lexicon, expressions: rules.expressions };
        setWorking(workingRef.current);
      })
      .catch(() => setLoadError(true));
  }, [bridge]);

  useEffect(load, [load]);

  // Move focus after the list re-rendered (closing a form, deleting a rule).
  useEffect(() => {
    if (!pendingFocus.current) return;
    focusTargets.current.get(pendingFocus.current)?.focus();
    pendingFocus.current = null;
  });

  /** Apply a change to the working set, then save the whole set. */
  const commit = useCallback(
    (change: (rules: WorkingRules) => WorkingRules, done: string) => {
      const job = queue.current.then(async () => {
        if (!workingRef.current) return false;
        const next = change(workingRef.current);
        workingRef.current = next;
        setWorking(next);
        setSaveError(null);
        try {
          const result = await bridge.invoke("save_personal_rules", { rules: next });
          if (result.saved) {
            setStored(result.rules);
            workingRef.current = { lexicon: result.rules.lexicon, expressions: result.rules.expressions };
            setWorking(workingRef.current);
            setConflicts([]);
            announce(announcement(done));
          } else {
            // Nothing was written: keep the edited set so it can be fixed.
            setConflicts(result.conflicts);
            announce(announcement(conflictSummary(result.conflicts.length)));
          }
          return result.saved;
        } catch (error) {
          setSaveError(toCommandError(error));
          announce(announcement("Les mots et expressions n’ont pas pu être enregistrés."));
          return false;
        }
      });
      queue.current = job;
      return job;
    },
    [bridge, announce],
  );

  const revert = () => {
    if (!stored) return;
    workingRef.current = { lexicon: stored.lexicon, expressions: stored.expressions };
    setWorking(workingRef.current);
    setConflicts([]);
    setSaveError(null);
    announce(announcement("Règles enregistrées rétablies."));
  };

  const describe: DescribeRule = (ref) => {
    if (!working) return "une autre règle";
    if (ref.kind === "lexicon") {
      const entry = working.lexicon.find((item) => item.id === ref.id);
      return entry ? `« ${entry.heard} »` : "une autre règle";
    }
    const expression = working.expressions.find((item) => item.id === ref.id);
    return expression ? `« ${expression.trigger} »` : "une autre expression";
  };

  const closeForm = (focusKey: string) => {
    pendingFocus.current = focusKey;
    setEditing(null);
  };

  const unsaved = conflicts.length > 0 || saveError !== null;
  const readOnly = stored?.unreadable ?? false;

  return (
    <Panel
      id={id}
      title="Mots et expressions"
      description="Vos mots et expressions restent sur ce PC, chiffrés pour votre compte Windows. Ils s’appliquent à la prochaine dictée."
    >
      {loadError ? (
        <div className="stack stack--tight">
          <FieldError>Les mots et expressions n’ont pas pu être lus.</FieldError>
          <div className="button-row">
            <button type="button" className="button" onClick={load}>
              Réessayer
            </button>
          </div>
        </div>
      ) : !working || !stored ? (
        <p className="muted">Chargement…</p>
      ) : (
        <>
          {readOnly ? (
            <InfoBar tone="info" title="Mots et expressions illisibles ici" headingLevel={3} testId="rules-unreadable">
              <p>
                Vos mots et expressions ont été créés sous un autre compte Windows ou sur un autre PC. Ils sont
                conservés, mais ne peuvent pas être lus ni modifiés ici.
              </p>
            </InfoBar>
          ) : null}

          {unsaved ? (
            <InfoBar
              tone="error"
              title={
                conflicts.length > 0
                  ? conflictSummary(conflicts.length)
                  : "Les mots et expressions n’ont pas pu être enregistrés."
              }
              headingLevel={3}
              testId="rules-conflicts"
              actions={
                <button type="button" className="button" onClick={revert}>
                  Rétablir les règles enregistrées
                </button>
              }
            >
              <p>Les dernières modifications ne sont pas enregistrées : la dictée utilise encore les règles précédentes.</p>
              {saveError ? <p>{commandErrorText(saveError)}</p> : null}
            </InfoBar>
          ) : null}

          <LexiconSection
            entries={working.lexicon}
            conflicts={conflicts}
            describe={describe}
            faithful={faithful}
            readOnly={readOnly}
            editing={editing}
            setEditing={setEditing}
            closeForm={closeForm}
            focusTargets={focusTargets.current}
            commit={commit}
          />

          <ExpressionSection
            entries={working.expressions}
            conflicts={conflicts}
            describe={describe}
            faithful={faithful}
            readOnly={readOnly}
            editing={editing}
            setEditing={setEditing}
            closeForm={closeForm}
            focusTargets={focusTargets.current}
            commit={commit}
          />

          <TryRules settings={settings} />
        </>
      )}
    </Panel>
  );
}

interface SectionProps<T> {
  entries: T[];
  conflicts: RuleConflict[];
  describe: DescribeRule;
  faithful: boolean;
  readOnly: boolean;
  editing: Editing;
  setEditing: (editing: Editing) => void;
  closeForm: (focusKey: string) => void;
  focusTargets: Map<string, HTMLElement | null>;
  commit: (change: (rules: WorkingRules) => WorkingRules, done: string) => Promise<boolean>;
}

const keyFor = (kind: RuleKind, id: string) => `${kind}:${id}`;
const addKey = (kind: RuleKind) => `${kind}:add`;

function LexiconSection(props: SectionProps<LexiconEntry>) {
  const { entries, conflicts, describe, faithful, readOnly, editing, setEditing, closeForm, focusTargets, commit } =
    props;
  const headingId = useId();
  const adding = editing?.kind === "lexicon" && editing.id === null;

  const save = (entry: LexiconEntry, isNew: boolean) => {
    void commit(
      (rules) => ({
        ...rules,
        lexicon: isNew ? [...rules.lexicon, entry] : rules.lexicon.map((item) => (item.id === entry.id ? entry : item)),
      }),
      isNew ? "Mot ajouté." : "Mot enregistré.",
    );
    closeForm(isNew ? addKey("lexicon") : keyFor("lexicon", entry.id));
  };

  return (
    <section className="rules-section" aria-labelledby={headingId} data-testid="lexicon-section">
      <h3 id={headingId} className="rules-section__title">
        Mots personnels
      </h3>
      <p className="field-hint">
        Remplace un mot entier écrit par Rambler. Exemple : « projet mélusine » devient « projet Mélusine ».
        {faithful ? " Profil Fidèle actif : seuls les mots marqués « Aussi en Fidèle » sont remplacés." : null}
      </p>
      {entries.length === 0 && !adding ? <p className="muted">Aucun mot personnel.</p> : null}
      {entries.length > 0 ? (
        <ul className="rule-list">
          {entries.map((entry) =>
            editing?.kind === "lexicon" && editing.id === entry.id ? (
              <li key={entry.id} className="rule rule--editing">
                <LexiconForm
                  initial={entry}
                  onSubmit={(value) => save(value, false)}
                  onCancel={() => closeForm(keyFor("lexicon", entry.id))}
                />
              </li>
            ) : (
              <RuleItem
                key={entry.id}
                kind="lexicon"
                entry={entry}
                messages={conflictsFor({ kind: "lexicon", id: entry.id }, conflicts, describe)}
                readOnly={readOnly}
                focusTargets={focusTargets}
                onEdit={() => setEditing({ kind: "lexicon", id: entry.id })}
                onToggle={(enabled) =>
                  void commit(
                    (rules) => ({
                      ...rules,
                      lexicon: rules.lexicon.map((item) => (item.id === entry.id ? { ...item, enabled } : item)),
                    }),
                    enabled ? "Mot activé." : "Mot désactivé.",
                  )
                }
                onDelete={() => {
                  void commit(
                    (rules) => ({ ...rules, lexicon: rules.lexicon.filter((item) => item.id !== entry.id) }),
                    "Mot supprimé.",
                  );
                  closeForm(addKey("lexicon"));
                }}
              >
                <p className="rule__pair">
                  <span className="rule__from">{entry.heard}</span>
                  <span className="rule__arrow" aria-hidden="true">
                    →
                  </span>
                  <span className="visually-hidden"> devient </span>
                  <span className="rule__to">{entry.written}</span>
                </p>
                <p className="rule__meta">
                  {entry.enabled ? "Actif" : "Désactivé"}
                  {entry.applyInFaithful ? " · Aussi en Fidèle" : ""}
                </p>
              </RuleItem>
            ),
          )}
        </ul>
      ) : null}
      {adding ? (
        <LexiconForm initial={null} onSubmit={(value) => save(value, true)} onCancel={() => closeForm(addKey("lexicon"))} />
      ) : !readOnly ? (
        <div className="button-row">
          <button
            ref={(element) => {
              focusTargets.set(addKey("lexicon"), element);
            }}
            type="button"
            className="button"
            onClick={() => setEditing({ kind: "lexicon", id: null })}
          >
            <Glyph name="add" />
            Ajouter un mot
          </button>
        </div>
      ) : null}
    </section>
  );
}

function ExpressionSection(props: SectionProps<Expression>) {
  const { entries, conflicts, describe, faithful, readOnly, editing, setEditing, closeForm, focusTargets, commit } =
    props;
  const headingId = useId();
  const adding = editing?.kind === "expression" && editing.id === null;

  const save = (expression: Expression, isNew: boolean) => {
    void commit(
      (rules) => ({
        ...rules,
        expressions: isNew
          ? [...rules.expressions, expression]
          : rules.expressions.map((item) => (item.id === expression.id ? expression : item)),
      }),
      isNew ? "Expression ajoutée." : "Expression enregistrée.",
    );
    closeForm(isNew ? addKey("expression") : keyFor("expression", expression.id));
  };

  return (
    <section className="rules-section" aria-labelledby={headingId} data-testid="expression-section">
      <h3 id={headingId} className="rules-section__title">
        Expressions
      </h3>
      <p className="field-hint">
        Dites un déclencheur pour insérer le texte choisi, tel quel, même sur plusieurs lignes.
      </p>
      <p className={faithful ? "rules-note rules-note--active" : "rules-note"} data-testid="faithful-note">
        {faithful
          ? "Profil Fidèle actif : vos expressions ne s’appliquent pas. Elles reprennent en profil Naturel."
          : "Les expressions sont inactives en profil Fidèle."}
      </p>
      {entries.length === 0 && !adding ? <p className="muted">Aucune expression.</p> : null}
      {entries.length > 0 ? (
        <ul className="rule-list">
          {entries.map((expression) =>
            editing?.kind === "expression" && editing.id === expression.id ? (
              <li key={expression.id} className="rule rule--editing">
                <ExpressionForm
                  initial={expression}
                  onSubmit={(value) => save(value, false)}
                  onCancel={() => closeForm(keyFor("expression", expression.id))}
                />
              </li>
            ) : (
              <RuleItem
                key={expression.id}
                kind="expression"
                entry={expression}
                messages={conflictsFor({ kind: "expression", id: expression.id }, conflicts, describe)}
                readOnly={readOnly}
                focusTargets={focusTargets}
                onEdit={() => setEditing({ kind: "expression", id: expression.id })}
                onToggle={(enabled) =>
                  void commit(
                    (rules) => ({
                      ...rules,
                      expressions: rules.expressions.map((item) =>
                        item.id === expression.id ? { ...item, enabled } : item,
                      ),
                    }),
                    enabled ? "Expression activée." : "Expression désactivée.",
                  )
                }
                onDelete={() => {
                  void commit(
                    (rules) => ({
                      ...rules,
                      expressions: rules.expressions.filter((item) => item.id !== expression.id),
                    }),
                    "Expression supprimée.",
                  );
                  closeForm(addKey("expression"));
                }}
              >
                <p className="rule__trigger">
                  <span className="visually-hidden">Déclencheur : </span>« {expression.trigger} »
                </p>
                <p className="rule__payload">
                  <span className="visually-hidden">Texte inséré : </span>
                  {expression.text}
                </p>
                <p className="rule__meta">{expression.enabled ? "Active" : "Désactivée"}</p>
              </RuleItem>
            ),
          )}
        </ul>
      ) : null}
      {adding ? (
        <ExpressionForm
          initial={null}
          onSubmit={(value) => save(value, true)}
          onCancel={() => closeForm(addKey("expression"))}
        />
      ) : !readOnly ? (
        <div className="button-row">
          <button
            ref={(element) => {
              focusTargets.set(addKey("expression"), element);
            }}
            type="button"
            className="button"
            onClick={() => setEditing({ kind: "expression", id: null })}
          >
            <Glyph name="add" />
            Ajouter une expression
          </button>
        </div>
      ) : null}
    </section>
  );
}

interface RuleItemProps {
  kind: RuleKind;
  entry: LexiconEntry | Expression;
  messages: string[];
  readOnly: boolean;
  focusTargets: Map<string, HTMLElement | null>;
  onEdit: () => void;
  onToggle: (enabled: boolean) => void;
  onDelete: () => void;
  children: ReactNode;
}

function RuleItem({ kind, entry, messages, readOnly, focusTargets, onEdit, onToggle, onDelete, children }: RuleItemProps) {
  const id = useId();
  const name = kind === "lexicon" ? (entry as LexiconEntry).heard : (entry as Expression).trigger;
  return (
    <li
      className={messages.length > 0 ? "rule rule--conflict" : entry.enabled ? "rule" : "rule rule--disabled"}
      data-testid={`rule-${kind}`}
      data-rule-id={entry.id}
    >
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
          <span className="check-row check-row--inline">
            <input
              id={`${id}-enabled`}
              type="checkbox"
              checked={entry.enabled}
              onChange={(event) => onToggle(event.currentTarget.checked)}
            />
            <label htmlFor={`${id}-enabled`}>
              {kind === "lexicon" ? "Actif" : "Active"}
              <span className="visually-hidden"> : « {name} »</span>
            </label>
          </span>
          <button
            ref={(element) => {
              focusTargets.set(keyFor(kind, entry.id), element);
            }}
            type="button"
            className="button"
            onClick={onEdit}
          >
            <Glyph name="edit" />
            Modifier<span className="visually-hidden"> « {name} »</span>
          </button>
          <button type="button" className="button" onClick={onDelete}>
            <Glyph name="trash" />
            Supprimer<span className="visually-hidden"> « {name} »</span>
          </button>
        </div>
      ) : null}
    </li>
  );
}

/** Try a typed sentence with `preview_render` and the current settings. */
function TryRules({ settings }: { settings: Settings }) {
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
    <section className="rules-section" aria-labelledby={`${id}-title`}>
      <h3 id={`${id}-title`} className="rules-section__title">
        Essayer une phrase
      </h3>
      <form className="rules-try" onSubmit={(event) => void run(event)}>
        <label htmlFor={`${id}-sample`} className="field__label">
          Phrase telle que Rambler l’écrirait
        </label>
        <div className="rules-try__row">
          <input
            id={`${id}-sample`}
            className="text-input"
            value={sample}
            maxLength={500}
            onChange={(event) => setSample(event.currentTarget.value)}
            autoComplete="off"
            spellCheck={false}
            aria-describedby={`${id}-hint`}
          />
          <button type="submit" className="button">
            Essayer
          </button>
        </div>
        <p id={`${id}-hint`} className="field-hint">
          Sans ouvrir le micro, avec vos mots et expressions enregistrés et vos réglages de rendu actuels.
        </p>
      </form>
      {result !== null ? (
        <div className="render-preview" data-testid="rules-try-result">
          <p className="render-preview__title">Texte inséré</p>
          <p className="rules-try__result">{result}</p>
        </div>
      ) : null}
      {error ? <FieldError>{commandErrorText(error, "L’essai n’a pas pu être effectué.")}</FieldError> : null}
    </section>
  );
}
