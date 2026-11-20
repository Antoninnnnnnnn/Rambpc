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
  small?: boolean;
  /** Visually hidden suffix that makes the trigger's name unique. */
  labelSuffix?: string;
}

/**
 * Two-step button for permanent deletions: the first press asks, the second
 * confirms. Escape or « Annuler » returns to the first button.
 */
export function ConfirmButton({
  label,
  question,
  confirmLabel,
  onConfirm,
  disabled,
  small,
  labelSuffix,
}: ConfirmButtonProps) {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const returnFocus = useRef(false);
  const id = useId();
  const size = small ? " btn--sm" : "";

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
      <button
        ref={triggerRef}
        type="button"
        className={`btn btn--ghost${size}`}
        disabled={disabled}
        onClick={() => setAsking(true)}
      >
        {label}
        {labelSuffix ? <span className="visually-hidden"> {labelSuffix}</span> : null}
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
      <div className="btn-row">
        <button
          ref={confirmRef}
          type="button"
          className={`btn btn--danger${size}`}
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
        <button type="button" className={`btn${size}`} onClick={cancel}>
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
        <span key={`${key}-${index}`} className="keys__item">
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
    <div className={disabled ? "row row--switch row--disabled" : "row row--switch"}>
      <div className="row__text">
        <label htmlFor={id} className="row__label">
          {label}
        </label>
        {description ? (
          <p id={`${id}-desc`} className="row__desc">
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
          <span className="switch__thumb">
            <svg viewBox="0 0 12 12" width="12" height="12" focusable="false">
              <path d="M3 6.2l2 2 4-4.4" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </span>
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
  /** `segmented`: short labels side by side; `cards`: labels with a description. */
  variant?: "segmented" | "cards";
  /** Hide the legend visually (it stays the group's accessible name). */
  hideLegend?: boolean;
  disabled?: boolean;
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
  variant = "cards",
  hideLegend = false,
  disabled = false,
}: ChoiceGroupProps<T>) {
  const id = useId();
  const describedBy = [description ? `${id}-desc` : "", error ? `${id}-err` : ""].filter(Boolean).join(" ");
  return (
    <fieldset
      className={`choices choices--${variant}`}
      aria-describedby={describedBy || undefined}
      disabled={disabled}
    >
      <legend className={hideLegend ? "visually-hidden" : "choices__legend"}>{legend}</legend>
      {description ? (
        <p id={`${id}-desc`} className="hint choices__desc">
          {description}
        </p>
      ) : null}
      <div className="choices__options">
        {options.map((option) => {
          const optionId = `${id}-${option.value}`;
          return (
            <label key={option.value} className="choice" htmlFor={optionId}>
              <input
                id={optionId}
                className="choice__input"
                type="radio"
                name={name}
                value={String(option.value)}
                checked={option.value === value}
                aria-labelledby={`${optionId}-label`}
                aria-describedby={option.description ? `${optionId}-desc` : undefined}
                onChange={() => onChange(option.value)}
              />
              <span className="choice__body">
                {variant === "cards" ? <span className="choice__radio" aria-hidden="true" /> : null}
                <span className="choice__text">
                  <span id={`${optionId}-label`} className="choice__label">
                    {option.label}
                  </span>
                  {option.description ? (
                    <span id={`${optionId}-desc`} className="choice__desc">
                      {option.description}
                    </span>
                  ) : null}
                </span>
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
      <ToneIcon tone="error" />
      <span>{children}</span>
    </p>
  );
}

interface CardProps {
  id?: string;
  title: string;
  description?: ReactNode;
  children: ReactNode;
  headingLevel?: 2 | 3;
  /** Header actions, right of the title. */
  actions?: ReactNode;
  className?: string;
  testId?: string;
}

/** A titled section: <section> labelled by its heading, focusable by navigation. */
export function Card({ id, title, description, children, headingLevel = 2, actions, className, testId }: CardProps) {
  const headingId = useId();
  const Heading = headingLevel === 2 ? "h2" : "h3";
  return (
    <section
      id={id}
      className={className ? `card ${className}` : "card"}
      aria-labelledby={headingId}
      tabIndex={-1}
      data-testid={testId}
    >
      <div className="card__header">
        <div className="card__titles">
          <Heading id={headingId} className="card__title">
            {title}
          </Heading>
          {description ? <p className="card__desc">{description}</p> : null}
        </div>
        {actions ? <div className="card__actions">{actions}</div> : null}
      </div>
      <div className="card__body">{children}</div>
    </section>
  );
}

/** Label and description on the left, a control on the right; stacks when narrow. */
export function Row({
  label,
  htmlFor,
  description,
  descriptionId,
  children,
}: {
  label: ReactNode;
  htmlFor?: string;
  description?: ReactNode;
  descriptionId?: string;
  children: ReactNode;
}) {
  return (
    <div className="row">
      <div className="row__text">
        {htmlFor ? (
          <label htmlFor={htmlFor} className="row__label">
            {label}
          </label>
        ) : (
          <span className="row__label">{label}</span>
        )}
        {description ? (
          <p id={descriptionId} className="row__desc">
            {description}
          </p>
        ) : null}
      </div>
      <div className="row__control">{children}</div>
    </div>
  );
}

/** Small « Copié » confirmation shown in place of a button label for a moment. */
export function useFlag(durationMs = 2200): [boolean, () => void, () => void] {
  const [on, setOn] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const raise = () => {
    setOn(true);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOn(false), durationMs);
  };
  const lower = () => {
    window.clearTimeout(timer.current);
    setOn(false);
  };
  return [on, raise, lower];
}
