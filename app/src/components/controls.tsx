// Small accessible form primitives built on native controls.

import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { shortcutKeys } from "../lib/shortcut";
import { ToneIcon } from "./Icon";
import "./controls.css";

interface ConfirmButtonProps {
  label: string;
  /** The question shown once the button was pressed. */
  question: string;
  confirmLabel: string;
  onConfirm: () => void | Promise<void>;
  disabled?: boolean;
}

/**
 * Two-step button for permanent deletions: the first press asks, the second
 * confirms. Escape or « Annuler » returns to the first button.
 */
export function ConfirmButton({ label, question, confirmLabel, onConfirm, disabled }: ConfirmButtonProps) {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const returnFocus = useRef(false);
  const id = useId();

  useEffect(() => {
    if (asking) confirmRef.current?.focus();
    else if (returnFocus.current) {
      returnFocus.current = false;
      triggerRef.current?.focus();
    }
  }, [asking]);

  const cancel = () => {
    returnFocus.current = true;
    setAsking(false);
  };

  if (!asking) {
    return (
      <button ref={triggerRef} type="button" className="button" disabled={disabled} onClick={() => setAsking(true)}>
        {label}
      </button>
    );
  }
  return (
    <div
      className="confirm"
      role="group"
      aria-labelledby={`${id}-q`}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          cancel();
        }
      }}
    >
      <p id={`${id}-q`} className="confirm__question">
        {question}
      </p>
      <div className="button-row">
        <button
          ref={confirmRef}
          type="button"
          className="button button--danger"
          aria-disabled={busy || undefined}
          onClick={async () => {
            if (busy) return;
            setBusy(true);
            try {
              await onConfirm();
            } finally {
              setBusy(false);
              returnFocus.current = true;
              setAsking(false);
            }
          }}
        >
          {confirmLabel}
        </button>
        <button type="button" className="button" onClick={cancel}>
          Annuler
        </button>
      </div>
    </div>
  );
}

/** A shortcut shown as keyboard keys: Ctrl + Maj + F2. */
export function Keys({ shortcut }: { shortcut: string }) {
  const keys = shortcutKeys(shortcut);
  return (
    <span className="keys">
      {keys.map((key, index) => (
        <span key={`${key}-${index}`}>
          {index > 0 ? <span className="keys__plus">+</span> : null}
          <kbd>{key}</kbd>
        </span>
      ))}
    </span>
  );
}

interface SwitchProps {
  label: string;
  description?: ReactNode;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
  error?: string;
}

/** A checkbox exposed as a switch (on/off), labelled and described. */
export function Switch({ label, description, checked, disabled, onChange, error }: SwitchProps) {
  const id = useId();
  const describedBy = [description ? `${id}-desc` : "", error ? `${id}-err` : ""].filter(Boolean).join(" ");
  return (
    <div className="switch-row">
      <div className="switch-row__text">
        <label htmlFor={id} className="switch-row__label">
          {label}
        </label>
        {description ? (
          <p id={`${id}-desc`} className="switch-row__desc">
            {description}
          </p>
        ) : null}
        {error ? <FieldError id={`${id}-err`}>{error}</FieldError> : null}
      </div>
      <span className="switch">
        <input
          id={id}
          type="checkbox"
          role="switch"
          checked={checked}
          disabled={disabled}
          aria-describedby={describedBy || undefined}
          aria-invalid={error ? true : undefined}
          onChange={(event) => onChange(event.currentTarget.checked)}
        />
        <span className="switch__track" aria-hidden="true">
          <span className="switch__thumb" />
        </span>
        <span className="switch__state" aria-hidden="true">
          {checked ? "Activé" : "Désactivé"}
        </span>
      </span>
    </div>
  );
}

export interface ChoiceOption<T extends string | number> {
  value: T;
  label: string;
  description?: string;
}

interface ChoiceGroupProps<T extends string | number> {
  legend: string;
  name: string;
  value: T;
  options: ChoiceOption<T>[];
  onChange: (value: T) => void;
  description?: ReactNode;
  error?: string;
  layout?: "stack" | "row";
}

/** Native radio group in a fieldset: arrow keys move between options. */
export function ChoiceGroup<T extends string | number>({
  legend,
  name,
  value,
  options,
  onChange,
  description,
  error,
  layout = "stack",
}: ChoiceGroupProps<T>) {
  const id = useId();
  const describedBy = [description ? `${id}-desc` : "", error ? `${id}-err` : ""].filter(Boolean).join(" ");
  return (
    <fieldset className="choice-group" aria-describedby={describedBy || undefined}>
      <legend className="choice-group__legend">{legend}</legend>
      {description ? (
        <p id={`${id}-desc`} className="field-hint">
          {description}
        </p>
      ) : null}
      <div className={`choice-group__options choice-group__options--${layout}`}>
        {options.map((option) => {
          const optionId = `${id}-${option.value}`;
          return (
            <label key={option.value} className="choice" htmlFor={optionId}>
              <input
                id={optionId}
                type="radio"
                name={name}
                value={String(option.value)}
                checked={option.value === value}
                aria-describedby={option.description ? `${optionId}-desc` : undefined}
                onChange={() => onChange(option.value)}
              />
              <span className="choice__text">
                <span className="choice__label">{option.label}</span>
                {option.description ? (
                  <span id={`${optionId}-desc`} className="choice__desc">
                    {option.description}
                  </span>
                ) : null}
              </span>
            </label>
          );
        })}
      </div>
      {error ? <FieldError id={`${id}-err`}>{error}</FieldError> : null}
    </fieldset>
  );
}

export function FieldError({ id, children }: { id?: string; children: ReactNode }) {
  return (
    <p id={id} className="field-error">
      <ToneIcon tone="error" className="field-error__icon" />
      <span>{children}</span>
    </p>
  );
}

/** Section of a settings-like page: <section> labelled by its heading. */
export function Panel({
  id,
  title,
  description,
  children,
  headingLevel = 2,
}: {
  id?: string;
  title: string;
  description?: ReactNode;
  children: ReactNode;
  headingLevel?: 2 | 3;
}) {
  const headingId = useId();
  const Heading = headingLevel === 2 ? "h2" : "h3";
  return (
    <section id={id} className="panel" aria-labelledby={headingId} tabIndex={-1}>
      <Heading id={headingId} className="panel__title">
        {title}
      </Heading>
      {description ? <p className="panel__desc">{description}</p> : null}
      <div className="panel__body">{children}</div>
    </section>
  );
}
