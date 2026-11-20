import { useId } from "react";
import { AvailabilityBanner } from "../components/AvailabilityBanner";
import { FieldError } from "../components/controls";
import { Glyph } from "../components/Icon";
import { InfoBar } from "../components/InfoBar";
import { OutcomeNotice } from "../components/OutcomeNotice";
import { StatusCard } from "../components/StatusCard";
import { canStartTake, commandErrorText, describeOutcome, isTakeActive } from "../labels/fr";
import { useUiCommands } from "../state/commands";
import { useNavigation } from "../state/navigation";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import { ScreenHeader } from "./ScreenHeader";
import "./screens.css";

export function Home() {
  const state = useUiState();
  const commands = useUiCommands();
  const nav = useNavigation();
  const dictate = useAction();
  const outcomeHeading = useId();
  const active = isTakeActive(state);
  const startable = canStartTake(state.availability);
  const hasDraft = state.draft !== null;
  // The outcome already points to the draft when it is the reason the text waits.
  const outcomeOffersDraft =
    state.lastOutcome !== null && describeOutcome(state.lastOutcome, hasDraft).actions.includes("open-draft");

  return (
    <div className="screen">
      <ScreenHeader title="Accueil">
        Dictez au curseur dans n’importe quelle application. Le texte est inséré une seule fois, dans le champ où
        vous avez commencé.
      </ScreenHeader>

      <AvailabilityBanner />

      <StatusCard />

      {state.lastOutcome && !active ? (
        <section className="screen__section" aria-labelledby={outcomeHeading}>
          <h2 id={outcomeHeading} className="screen__section-title">
            Dernière dictée
          </h2>
          <OutcomeNotice outcome={state.lastOutcome} hasDraft={hasDraft} headingLevel={3} />
        </section>
      ) : null}

      {hasDraft && !active && !outcomeOffersDraft ? (
        <InfoBar
          tone="info"
          title="Un texte attend dans Sambler"
          testId="draft-callout"
          actions={
            <button type="button" className="button button--primary" onClick={() => nav.go("draft")}>
              <Glyph name="draft" />
              Ouvrir le brouillon
            </button>
          }
        >
          <p>Vous pouvez le copier ou l’effacer. Il est effacé automatiquement à l’expiration du délai.</p>
        </InfoBar>
      ) : null}

      {!active ? (
        <div className="screen__actions">
          <button
            type="button"
            className="button button--large"
            onClick={() => void dictate.run(commands.dictateInSambler)}
            disabled={!startable}
            aria-describedby={`${outcomeHeading}-dictate`}
          >
            <Glyph name="mic" />
            Dicter dans Sambler
          </button>
          <p id={`${outcomeHeading}-dictate`} className="field-hint">
            {startable
              ? "Sans raccourci : le texte reste dans Sambler, prêt à être copié."
              : "Disponible quand la dictée est activée et que Rambler répond."}
          </p>
          {dictate.error ? <FieldError>{commandErrorText(dictate.error)}</FieldError> : null}
        </div>
      ) : null}
    </div>
  );
}
