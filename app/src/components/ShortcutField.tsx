// Key-capture field for the global shortcut. Produces Tauri accelerator
// syntax; rejects bare modifiers, Windows-key combos, F12 and Ctrl+Alt.
// Tab still leaves the field (no keyboard trap) and Escape cancels.

import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { DEFAULT_SHORTCUT, type CommandError } from "../ipc/contract";
import { commandErrorText, shortcutRejectionText } from "../labels/fr";
import { captureShortcut, formatShortcut, sameShortcut, type ShortcutRejection } from "../lib/shortcut";
import { FieldError, Keys } from "./controls";
import "./ShortcutField.css";

interface Props {
  value: string;
  /** Save the new accelerator; resolves false when the native side refused it. */
  onChange: (accelerator: string) => Promise<boolean>;
  /** Error of the last refused save (the old shortcut stays active). */
  error?: CommandError;
  label?: string;
}

export function ShortcutField({ value, onChange, error, label = "Raccourci" }: Props) {
  const announce = useAnnounce();
  const [capturing, setCapturing] = useState(false);
  const [held, setHeld] = useState<string[]>([]);
  const [rejection, setRejection] = useState<ShortcutRejection | null>(null);
  const [saving, setSaving] = useState(false);
  const captureRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const refocusButton = useRef(false);
  const id = useId();

  useEffect(() => {
    if (capturing) {
      captureRef.current?.focus();
    } else if (refocusButton.current) {
      refocusButton.current = false;
      buttonRef.current?.focus();
    }
  }, [capturing]);

  const stopCapture = (refocus: boolean) => {
    refocusButton.current = refocus;
    setCapturing(false);
    setHeld([]);
  };

  const reject = (reason: ShortcutRejection) => {
    setHeld([]);
    setRejection(reason);
    announce(announcement(shortcutRejectionText(reason)));
  };

  const save = async (accelerator: string) => {
    if (sameShortcut(accelerator, value)) return;
    setSaving(true);
    const ok = await onChange(accelerator);
    setSaving(false);
    announce(
      announcement(
        ok
          ? `Raccourci enregistré : ${formatShortcut(accelerator)}.`
          : "Le raccourci n’a pas pu être enregistré. L’ancien raccourci reste actif.",
      ),
    );
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const plain = !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
    if (event.key === "Tab" && !event.ctrlKey && !event.altKey && !event.metaKey) {
      stopCapture(false);
      return; // let the focus move on
    }
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape" && plain) {
      setRejection(null);
      stopCapture(true);
      return;
    }
    if (event.repeat) return;
    const result = captureShortcut({
      key: event.key,
      code: event.code,
      ctrlKey: event.ctrlKey,
      altKey: event.altKey,
      shiftKey: event.shiftKey,
      metaKey: event.metaKey,
      altGraph: event.getModifierState?.("AltGraph") ?? false,
    });
    if (result.status === "pending") {
      setRejection(null);
      setHeld(result.modifiers);
    } else if (result.status === "rejected") {
      reject(result.reason);
    } else {
      setRejection(null);
      stopCapture(true);
      void save(result.accelerator);
    }
  };

  const onKeyUp = (event: KeyboardEvent<HTMLDivElement>) => {
    event.preventDefault();
    const released = !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
    if (held.length > 0 && released) reject("modifier-only");
  };

  const describedBy = [
    `${id}-hint`,
    rejection ? `${id}-rejection` : "",
    error ? `${id}-error` : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className="shortcut-field" role="group" aria-labelledby={`${id}-label`} data-testid="shortcut-field">
      <span id={`${id}-label`} className="field__label">
        {label}
      </span>
      {capturing ? (
        <div
          ref={captureRef}
          className="shortcut-field__capture"
          role="textbox"
          aria-label="Nouvelle combinaison"
          aria-describedby={describedBy}
          aria-invalid={rejection ? true : undefined}
          tabIndex={0}
          onKeyDown={onKeyDown}
          onKeyUp={onKeyUp}
          onBlur={() => stopCapture(false)}
          data-testid="shortcut-capture"
        >
          {held.length > 0 ? <Keys shortcut={[...held, "…"].join("+")} /> : "Appuyez sur la nouvelle combinaison…"}
        </div>
      ) : (
        <div className="shortcut-field__current">
          <span className="shortcut-field__value" data-testid="shortcut-value">
            <Keys shortcut={value} />
          </span>
          <button
            ref={buttonRef}
            type="button"
            className="button"
            onClick={() => {
              if (saving) return;
              setRejection(null);
              setCapturing(true);
            }}
            aria-describedby={describedBy}
            aria-disabled={saving || undefined}
          >
            Modifier le raccourci
          </button>
          {saving ? <span className="field-hint">Enregistrement…</span> : null}
          {!sameShortcut(value, DEFAULT_SHORTCUT) ? (
            <button type="button" className="button button--subtle" onClick={() => void save(DEFAULT_SHORTCUT)}>
              Rétablir {formatShortcut(DEFAULT_SHORTCUT)}
            </button>
          ) : null}
        </div>
      )}
      <p id={`${id}-hint`} className="field-hint">
        {capturing
          ? "Maintenez Ctrl, Alt ou Maj puis appuyez sur une touche, par exemple Ctrl+Maj+F2. Échap pour annuler."
          : "Les combinaisons avec la touche Windows, F12, Ctrl+Alt et les modificateurs seuls sont refusés."}
      </p>
      {rejection ? <FieldError id={`${id}-rejection`}>{shortcutRejectionText(rejection)}</FieldError> : null}
      {error ? (
        <FieldError id={`${id}-error`}>
          Le raccourci n’a pas pu être enregistré. {commandErrorText(error, "")} L’ancien raccourci reste actif :{" "}
          {formatShortcut(value)}.
        </FieldError>
      ) : null}
    </div>
  );
}
