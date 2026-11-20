// Rambler access entry. Values go once to `set_access` and are then dropped:
// they are never displayed back, logged or stored by the WebView.

import { useId, useState, type FormEvent } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import type { AccessStatus } from "../ipc/contract";
import { accessSourceText, commandErrorText } from "../labels/fr";
import { useAccess } from "../state/access";
import { useAction } from "../state/useAction";
import { FieldError } from "./controls";
import "./AccessForm.css";

/** Default service address of the SDK, used when the field is left empty. */
export const DEFAULT_AUTHORITY = "speechs3proto2-pa.googleapis.com:443";

type FieldName = "apiKey" | "androidPackage" | "androidCert";

const EMPTY = { authority: "", apiKey: "", androidPackage: "", androidCert: "", remember: false };

const REQUIRED: { name: FieldName; label: string; hint: string; secret: boolean }[] = [
  { name: "apiKey", label: "Clé d’accès", hint: "Valeur de RAMBLER_API_KEY.", secret: true },
  {
    name: "androidPackage",
    label: "Package Android",
    hint: "Valeur de RAMBLER_ANDROID_PACKAGE.",
    secret: false,
  },
  {
    name: "androidCert",
    label: "Certificat",
    hint: "Empreinte du certificat, valeur de RAMBLER_ANDROID_CERT.",
    secret: true,
  },
];

interface Props {
  onSaved?: (status: AccessStatus) => void;
  onCancel?: () => void;
  submitLabel?: string;
}

export function AccessForm({ onSaved, onCancel, submitLabel = "Enregistrer l’accès" }: Props) {
  const { status, save } = useAccess();
  const announce = useAnnounce();
  const [values, setValues] = useState(EMPTY);
  const [missing, setMissing] = useState<FieldName[]>([]);
  const action = useAction();
  const id = useId();
  const canRemember = status?.canRemember ?? false;

  const onSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (action.busy) return;
    const empty = REQUIRED.filter((field) => !values[field.name].trim()).map((field) => field.name);
    setMissing(empty);
    if (empty.length > 0) {
      document.getElementById(`${id}-${empty[0]}`)?.focus();
      return;
    }
    const result = await action.run(() =>
      save({
        authority: values.authority.trim(),
        apiKey: values.apiKey.trim(),
        androidPackage: values.androidPackage.trim(),
        androidCert: values.androidCert.trim(),
        remember: canRemember && values.remember,
      }),
    );
    if (result.ok) {
      setValues(EMPTY);
      announce(announcement("Accès à Rambler enregistré."));
      onSaved?.(result.value);
    } else {
      announce(announcement("L’accès n’a pas pu être enregistré."));
    }
  };

  const set = (name: keyof typeof EMPTY, value: string | boolean) =>
    setValues((current) => ({ ...current, [name]: value }));

  return (
    <form className="access-form" onSubmit={(event) => void onSubmit(event)} noValidate aria-describedby={`${id}-intro`}>
      <p id={`${id}-intro`} className="field-hint">
        Ces valeurs restent dans la partie native de Sambler et ne sont jamais réaffichées.
      </p>
      {REQUIRED.map((field) => {
        const invalid = missing.includes(field.name);
        return (
          <div className="field" key={field.name}>
            <label htmlFor={`${id}-${field.name}`} className="field__label">
              {field.label}
            </label>
            <input
              id={`${id}-${field.name}`}
              className="text-input"
              type={field.secret ? "password" : "text"}
              value={values[field.name]}
              onChange={(event) => set(field.name, event.currentTarget.value)}
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
              required
              aria-required="true"
              aria-invalid={invalid || undefined}
              aria-describedby={`${id}-${field.name}-hint${invalid ? ` ${id}-${field.name}-err` : ""}`}
            />
            <p id={`${id}-${field.name}-hint`} className="field-hint">
              {field.hint}
            </p>
            {invalid ? <FieldError id={`${id}-${field.name}-err`}>Ce champ est obligatoire.</FieldError> : null}
          </div>
        );
      })}

      <details className="access-form__advanced">
        <summary>Options avancées</summary>
        <div className="field">
          <label htmlFor={`${id}-authority`} className="field__label">
            Adresse du service (facultatif)
          </label>
          <input
            id={`${id}-authority`}
            className="text-input"
            type="text"
            inputMode="url"
            value={values.authority}
            placeholder={DEFAULT_AUTHORITY}
            onChange={(event) => set("authority", event.currentTarget.value)}
            autoComplete="off"
            autoCapitalize="off"
            spellCheck={false}
            aria-describedby={`${id}-authority-hint`}
          />
          <p id={`${id}-authority-hint`} className="field-hint">
            Laissez vide pour utiliser l’adresse par défaut ({DEFAULT_AUTHORITY}).
          </p>
        </div>
      </details>

      {canRemember ? (
        <div className="access-form__remember">
          <input
            id={`${id}-remember`}
            type="checkbox"
            checked={values.remember}
            onChange={(event) => set("remember", event.currentTarget.checked)}
            aria-describedby={`${id}-remember-hint`}
          />
          <div>
            <label htmlFor={`${id}-remember`}>Mémoriser sur ce PC (chiffré pour votre compte Windows)</label>
            <p id={`${id}-remember-hint`} className="field-hint">
              Sinon, l’accès est oublié à la fermeture de Sambler.
            </p>
          </div>
        </div>
      ) : (
        <p className="field-hint">
          Édition portable : l’accès reste en mémoire pour cette session seulement et n’est jamais écrit sur le disque.
        </p>
      )}

      {action.error ? (
        <FieldError>{commandErrorText(action.error, "Ces données d’accès n’ont pas pu être enregistrées.")}</FieldError>
      ) : null}

      <div className="button-row">
        <button type="submit" className="button button--primary" aria-disabled={action.busy || undefined}>
          {action.busy ? "Enregistrement…" : submitLabel}
        </button>
        {onCancel ? (
          <button type="button" className="button" onClick={onCancel}>
            Annuler
          </button>
        ) : null}
      </div>
    </form>
  );
}

/** Configured / source / address, never a secret. */
export function AccessSummary({ status }: { status: AccessStatus }) {
  return (
    <dl className="access-summary" data-testid="access-summary">
      <div>
        <dt>État</dt>
        <dd>{status.configured ? "Accès configuré" : "Aucun accès configuré"}</dd>
      </div>
      {status.configured ? (
        <>
          <div>
            <dt>Source</dt>
            <dd>{accessSourceText(status.source)}</dd>
          </div>
          <div>
            <dt>Adresse du service</dt>
            <dd>{status.authority || `${DEFAULT_AUTHORITY} (par défaut)`}</dd>
          </div>
        </>
      ) : null}
    </dl>
  );
}
