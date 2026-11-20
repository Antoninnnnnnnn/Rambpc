// Current phase: icon shape + label + hint, level meter while listening,
// and « Terminer » / « Annuler » for the active take (by take id).

import { useEffect, useId, useRef, useState } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { EVENT_LEVEL, type UiState } from "../ipc/contract";
import { commandErrorText, describeStatus, isTakeActive } from "../labels/fr";
import { formatClock, TAKE_LIMIT_MS, TAKE_LIMIT_WARNING_MS } from "../lib/time";
import { useUiCommands } from "../state/commands";
import { useSettings } from "../state/settings";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import { FieldError, Keys } from "./controls";
import { StatusIcon } from "./Icon";
import { LevelMeter } from "./LevelMeter";
import "./StatusCard.css";

export function StatusCard({ headingLevel = 2 }: { headingLevel?: 2 | 3 }) {
  const state = useUiState();
  const { settings } = useSettings();
  const commands = useUiCommands();
  const { run, error } = useAction();
  const headingId = useId();
  const status = describeStatus(state);
  const active = isTakeActive(state);
  const phase = state.phase.kind;
  const take = state.take;
  const escapeCancels = settings?.cancelWithEscape ?? true;
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
      className={`status-card status-card--${status.shape}`}
      aria-labelledby={headingId}
      data-testid="status-card"
      data-phase={phase}
    >
      <Heading id={headingId} className="visually-hidden">
        État de la dictée
      </Heading>
      <div className="status-card__main">
        <span className="status-card__badge">
          <StatusIcon shape={status.shape} />
        </span>
        <div className="status-card__text">
          <p className="status-card__label">{status.label}</p>
          <p className="status-card__hint">{status.hint}</p>
        </div>
      </div>

      {phase === "listening" ? (
        <div className="status-card__live">
          <LevelMeter event={EVENT_LEVEL} active label="Niveau du micro" />
          <ListeningTimer state={state} />
        </div>
      ) : null}

      {active && state.destination === "sambler" ? (
        <p className="status-card__note">Le texte de cette dictée restera dans Sambler.</p>
      ) : null}

      {active && take !== null ? (
        <div className="status-card__actions">
          <div className="button-row">
            {phase !== "finalizing" ? (
              <button
                type="button"
                className="button button--primary"
                onClick={() => void run(() => commands.finishTake(take))}
              >
                Terminer
              </button>
            ) : null}
            <button type="button" className="button" onClick={() => void run(() => commands.cancelTake(take))}>
              Annuler
            </button>
          </div>
          {escapeCancels ? (
            <p className="status-card__note">
              <Keys shortcut="Escape" /> annule aussi la dictée.
            </p>
          ) : null}
        </div>
      ) : null}
      {error ? <FieldError>{commandErrorText(error)}</FieldError> : null}
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
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
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
    <p className={nearLimit ? "status-card__timer status-card__timer--warning" : "status-card__timer"}>
      <span>Durée : {formatClock(Math.min(elapsed, TAKE_LIMIT_MS))} sur 5:00</span>
      {nearLimit ? <span> — la dictée s’arrêtera bientôt (durée maximale atteinte).</span> : null}
    </p>
  );
}
