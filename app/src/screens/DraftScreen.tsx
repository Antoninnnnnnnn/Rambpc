import { DraftPanel } from "../components/DraftPanel";
import { FieldError } from "../components/controls";
import { Glyph } from "../components/Icon";
import { OutcomeNotice } from "../components/OutcomeNotice";
import { canStartTake, commandErrorText, isTakeActive } from "../labels/fr";
import { useUiCommands } from "../state/commands";
import { useSettings } from "../state/settings";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import { ScreenHeader } from "./ScreenHeader";
import "./screens.css";

export function DraftScreen() {
  const state = useUiState();
  const { settings } = useSettings();
  const commands = useUiCommands();
  const dictate = useAction();
  const minutes = settings?.draftMinutes ?? 10;

  return (
    <div className="screen">
      <ScreenHeader title="Brouillon">
        Un texte qui n’a pas pu être inséré attend ici {minutes} minute{minutes > 1 ? "s" : ""}, en mémoire
        seulement. Verrouiller Windows ou quitter Sambler l’efface.
      </ScreenHeader>

      {state.draft ? (
        <>
          {state.lastOutcome ? (
            <OutcomeNotice outcome={state.lastOutcome} hasDraft draftVisible headingLevel={2} />
          ) : null}
          <DraftPanel />
        </>
      ) : (
        <div className="empty-state" data-testid="draft-empty">
          <Glyph name="draft" className="empty-state__icon" />
          <h2>Aucun texte en attente</h2>
          <p className="muted">
            Si le champ change pendant une dictée, ou si vous dictez dans Sambler, le texte apparaît ici avec Copier et
            Effacer.
          </p>
          {!isTakeActive(state) ? (
            <div className="button-row">
              <button
                type="button"
                className="button"
                onClick={() => void dictate.run(commands.dictateInSambler)}
                disabled={!canStartTake(state.availability)}
              >
                <Glyph name="mic" />
                Dicter dans Sambler
              </button>
            </div>
          ) : null}
          {dictate.error ? <FieldError>{commandErrorText(dictate.error)}</FieldError> : null}
        </div>
      )}
    </div>
  );
}
