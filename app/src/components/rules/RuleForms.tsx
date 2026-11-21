// Add / edit forms for one lexicon entry or one expression. Values stay in
// this form until « Enregistrer »; the screen then saves the whole set.

import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import type { Expression, LexiconEntry } from "../../ipc/contract";
import { FieldError } from "../controls";

export function newRuleId(): string {
  const random = globalThis.crypto?.randomUUID?.();
  return random ?? `rule-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

interface FormProps<T> {
  initial: T | null;
  onSubmit: (value: T) => void;
  onCancel: () => void;
}

export function LexiconForm({ initial, onSubmit, onCancel }: FormProps<LexiconEntry>) {
  const [heard, setHeard] = useState(initial?.heard ?? "");
  const [written, setWritten] = useState(initial?.written ?? "");
  const [applyInFaithful, setApplyInFaithful] = useState(initial?.applyInFaithful ?? false);
  const [missing, setMissing] = useState<("heard" | "written")[]>([]);
  const first = useRef<HTMLInputElement>(null);
  const id = useId();
  const title = initial ? "Modifier le mot" : "Nouveau mot";

  useEffect(() => first.current?.focus(), []);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const empty = [!heard.trim() ? "heard" : "", !written.trim() ? "written" : ""].filter(Boolean) as (
      | "heard"
      | "written"
    )[];
    setMissing(empty);
    if (empty.length > 0) {
      document.getElementById(`${id}-${empty[0]}`)?.focus();
      return;
    }
    onSubmit({
      id: initial?.id ?? newRuleId(),
      heard: heard.trim(),
      written: written.trim(),
      enabled: initial?.enabled ?? true,
      applyInFaithful,
    });
  };

  return (
    <form
      className="rule-form"
      onSubmit={submit}
      noValidate
      aria-label={title}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          onCancel();
        }
      }}
    >
      <p className="rule-form__title" aria-hidden="true">
        {title}
      </p>
      <div className="rule-form__pair">
        <div className="field">
          <label htmlFor={`${id}-heard`} className="field__label">
            Rambler écrit
          </label>
          <input
            ref={first}
            id={`${id}-heard`}
            className="input rule-form__heard"
            value={heard}
            onChange={(event) => setHeard(event.currentTarget.value)}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={missing.includes("heard") || undefined}
            aria-describedby={`${id}-heard-hint${missing.includes("heard") ? ` ${id}-heard-err` : ""}`}
          />
          <p id={`${id}-heard-hint`} className="hint">
            Tel que Rambler l’écrit aujourd’hui, en mots entiers.
          </p>
          {missing.includes("heard") ? <FieldError id={`${id}-heard-err`}>Ce champ est obligatoire.</FieldError> : null}
        </div>
        <div className="field">
          <label htmlFor={`${id}-written`} className="field__label">
            Je veux
          </label>
          <input
            id={`${id}-written`}
            className="input"
            value={written}
            onChange={(event) => setWritten(event.currentTarget.value)}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={missing.includes("written") || undefined}
            aria-describedby={`${id}-written-hint${missing.includes("written") ? ` ${id}-written-err` : ""}`}
          />
          <p id={`${id}-written-hint`} className="hint">
            La graphie à insérer à la place.
          </p>
          {missing.includes("written") ? (
            <FieldError id={`${id}-written-err`}>Ce champ est obligatoire.</FieldError>
          ) : null}
        </div>
      </div>
      <div className="check-row">
        <input
          id={`${id}-faithful`}
          type="checkbox"
          checked={applyInFaithful}
          onChange={(event) => setApplyInFaithful(event.currentTarget.checked)}
          aria-describedby={`${id}-faithful-hint`}
        />
        <div>
          <label htmlFor={`${id}-faithful`} className="field__label">
            Appliquer aussi en Fidèle
          </label>
          <p id={`${id}-faithful-hint`} className="hint">
            En style Fidèle, seuls les mots cochés ici sont remplacés.
          </p>
        </div>
      </div>
      <div className="btn-row">
        <button type="submit" className="btn btn--primary">
          Enregistrer
        </button>
        <button type="button" className="btn btn--ghost" onClick={onCancel}>
          Annuler
        </button>
      </div>
    </form>
  );
}

export function ExpressionForm({ initial, onSubmit, onCancel }: FormProps<Expression>) {
  const [trigger, setTrigger] = useState(initial?.trigger ?? "");
  const [text, setText] = useState(initial?.text ?? "");
  const [missing, setMissing] = useState<("trigger" | "text")[]>([]);
  const first = useRef<HTMLInputElement>(null);
  const id = useId();
  const title = initial ? "Modifier l’expression" : "Nouvelle expression";

  useEffect(() => first.current?.focus(), []);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const empty = [!trigger.trim() ? "trigger" : "", !text.trim() ? "text" : ""].filter(Boolean) as (
      | "trigger"
      | "text"
    )[];
    setMissing(empty);
    if (empty.length > 0) {
      document.getElementById(`${id}-${empty[0]}`)?.focus();
      return;
    }
    onSubmit({
      id: initial?.id ?? newRuleId(),
      trigger: trigger.trim(),
      // The payload is literal: keep its inner line breaks and spacing.
      text,
      enabled: initial?.enabled ?? true,
    });
  };

  return (
    <form
      className="rule-form"
      onSubmit={submit}
      noValidate
      aria-label={title}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          onCancel();
        }
      }}
    >
      <p className="rule-form__title" aria-hidden="true">
        {title}
      </p>
      <div className="field">
        <label htmlFor={`${id}-trigger`} className="field__label">
          Déclencheur
        </label>
        <input
          ref={first}
          id={`${id}-trigger`}
          className="input rule-form__trigger"
          value={trigger}
          onChange={(event) => setTrigger(event.currentTarget.value)}
          autoComplete="off"
          spellCheck={false}
          aria-invalid={missing.includes("trigger") || undefined}
          aria-describedby={`${id}-trigger-hint${missing.includes("trigger") ? ` ${id}-trigger-err` : ""}`}
        />
        <p id={`${id}-trigger-hint`} className="hint">
          La phrase à dire, par exemple « insère ma formule courte ».
        </p>
        {missing.includes("trigger") ? (
          <FieldError id={`${id}-trigger-err`}>Ce champ est obligatoire.</FieldError>
        ) : null}
      </div>
      <div className="field">
        <label htmlFor={`${id}-text`} className="field__label">
          Texte à insérer
        </label>
        <textarea
          id={`${id}-text`}
          className="input"
          value={text}
          rows={4}
          onChange={(event) => setText(event.currentTarget.value)}
          spellCheck={false}
          aria-invalid={missing.includes("text") || undefined}
          aria-describedby={`${id}-text-hint${missing.includes("text") ? ` ${id}-text-err` : ""}`}
        />
        <p id={`${id}-text-hint`} className="hint">
          Plusieurs lignes possibles. Entrée ajoute une ligne ; le bouton Enregistrer valide.
        </p>
        {missing.includes("text") ? <FieldError id={`${id}-text-err`}>Ce champ est obligatoire.</FieldError> : null}
      </div>
      <section className="rule-preview" aria-labelledby={`${id}-preview`}>
        <h3 id={`${id}-preview`} className="rule-preview__title">
          Aperçu de l’insertion
        </h3>
        {text.trim() ? (
          <p className="rule-preview__text" data-testid="expression-preview">
            {text}
          </p>
        ) : (
          <p className="muted">Le texte apparaîtra ici, tel qu’il sera inséré.</p>
        )}
        <p className="hint">Inséré tel quel, retours à la ligne compris : la ponctuation automatique ne le modifie pas.</p>
      </section>
      <div className="btn-row">
        <button type="submit" className="btn btn--primary">
          Enregistrer
        </button>
        <button type="button" className="btn btn--ghost" onClick={onCancel}>
          Annuler
        </button>
      </div>
    </form>
  );
}
