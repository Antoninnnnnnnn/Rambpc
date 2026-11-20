// Live dictation state: shape icon + label + hint, voice bars and timer while
// listening, « Terminer » / « Annuler » for the active take (by take id), and
// « Dicter dans Sambler » when idle.

import { useEffect, useId, useRef, useState } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { EVENT_LEVEL, type UiState } from "../ipc/contract";
import { canStartTake, commandErrorText, describeStatus, isTakeActive } from "../labels/fr";
import { formatClock, TAKE_LIMIT_MS, TAKE_LIMIT_WARNING_MS } from "../lib/time";
import { useUiCommands } from "../state/commands";
import { useSettings } from "../state/settings";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import { FieldError, Keys } from "./controls";
import { Glyph, StatusIcon } from "./Icon";
import { Waveform } from "./Waveform";
import "./StatusCard.css";

interface Props {
  headingLevel?: 2 | 3;
  /** Offer « Dicter dans Sambler » while idle. */
  dictateLabel?: string;
}

/** How to start a take, with the shortcut drawn as keys. */
export function ShortcutHint({ gesture, shortcut }: { gesture: UiState["gesture"]; shortcut: string }) {
  return gesture === "hold" ? (
    <>
      Dans n’importe quelle application, maintenez <Keys shortcut={shortcut} />, parlez, puis relâchez.
    </>
  ) : (
    <>
      Dans n’importe quelle application, appuyez sur <Keys shortcut={shortcut} /> pour commencer, puis de nouveau pour
      terminer.
    </>
  );
}

export function StatusCard({ headingLevel = 2, dictateLabel = "Dicter dans Sambler" }: Props) {
  const state = useUiState();
  const { settings } = useSettings();
  const commands = useUiCommands();
  const { run, error } = useAction();
  const dictate = useAction();
  const headingId = useId();
  const dictateHint = useId();
  const status = describeStatus(state);
  const active = isTakeActive(state);
  const phase = state.phase.kind;
  const take = state.take;
  const escapeCancels = settings?.cancelWithEscape ?? true;
  const startable = canStartTake(state.availability);
  const Heading = headingLevel === 2 ? "h2" : "h3";

  // While a take runs, Escape in this window cancels it too (the global
  // Escape hotkey is native and only covers other windows' focus).
  useEffect(() => {
    if (!active || take === null || !escapeCancels) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) {
        event.preventDefault();
        void run(() => commands.cancelTake(take));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, take, escapeCancels, commands, run]);

  return (
    <section
      className={`live live--${status.shape}`}
      aria-labelledby={headingId}
      data-testid="status-card"
      data-phase={phase}
    >
      <Heading id={headingId} className="visually-hidden">
        État de la dictée
      </Heading>
      <div className="live__main">
        <span className="live__badge">
          <StatusIcon shape={status.shape} />
        </span>
        <div className="live__text">
          <p className="live__label">{status.label}</p>
          <p className="live__hint">
            {status.shape === "available" ? <ShortcutHint gesture={state.gesture} shortcut={state.shortcut} /> : status.hint}
          </p>
        </div>
        {phase === "listening" ? <ListeningTimer state={state} /> : null}
      </div>

      {phase === "listening" ? (
        <div className="live__wave">
          <Waveform event={EVENT_LEVEL} active bars={57} />
        </div>
      ) : phase === "finalizing" || phase === "preparing" ? (
        <div className="live__wave">
          <span className="live__dots" aria-hidden="true">
            <span />
            <span />
            <span />
          </span>
        </div>
      ) : null}

      {active && state.destination === "sambler" ? (
        <p className="live__note">Le texte de cette dictée restera dans Sambler.</p>
      ) : null}

      {active && take !== null ? (
        <div className="live__actions">
          <div className="btn-row">
            {phase !== "finalizing" ? (
              <button type="button" className="btn btn--primary" onClick={() => void run(() => commands.finishTake(take))}>
                <Glyph name="stop" />
                Terminer
              </button>
            ) : null}
            <button type="button" className="btn" onClick={() => void run(() => commands.cancelTake(take))}>
              Annuler
            </button>
          </div>
          {escapeCancels ? (
            <p className="live__note">
              <Keys shortcut="Escape" /> annule aussi la dictée.
            </p>
          ) : null}
        </div>
      ) : null}

      {!active && dictateLabel ? (
        <div className="live__actions">
          <button
            type="button"
            className="btn"
            onClick={() => void dictate.run(commands.dictateInSambler)}
            disabled={!startable}
            aria-describedby={dictateHint}
          >
            <Glyph name="mic" />
            {dictateLabel}
          </button>
          <p id={dictateHint} className="live__note">
            {startable
              ? "Sans raccourci : le texte reste dans Sambler, prêt à être copié."
              : "Disponible quand la dictée est activée et que Rambler répond."}
          </p>
        </div>
      ) : null}
      {error ? <FieldError>{commandErrorText(error)}</FieldError> : null}
      {dictate.error ? <FieldError>{commandErrorText(dictate.error)}</FieldError> : null}
    </section>
  );
}

function ListeningTimer({ state }: { state: UiState }) {
  const announce = useAnnounce();
  const base = useRef({ ms: state.listeningMs ?? 0, at: Date.now() });
  const warned = useRef(false);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    base.current = { ms: state.listeningMs ?? 0, at: Date.now() };
  }, [state.listeningMs]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, []);

  const elapsed = base.current.ms + Math.max(0, now - base.current.at);
  const nearLimit = elapsed >= TAKE_LIMIT_WARNING_MS;

  useEffect(() => {
    if (nearLimit && !warned.current) {
      warned.current = true;
      announce(announcement("La dictée s’arrêtera dans moins de 30 secondes."));
    }
  }, [nearLimit, announce]);

  return (
    <p className={nearLimit ? "live__timer live__timer--warning" : "live__timer"}>
      <span className="visually-hidden">Durée : </span>
      <span className="live__clock">{formatClock(Math.min(elapsed, TAKE_LIMIT_MS))}</span>
      <span className="live__limit"> sur 5:00</span>
      {nearLimit ? <span className="live__limit-note"> — la dictée s’arrêtera bientôt (durée maximale atteinte).</span> : null}
    </p>
  );
}
