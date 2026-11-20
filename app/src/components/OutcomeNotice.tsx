// Result of the latest take, for every `TakeOutcome` variant. Text-free.

import type { TakeOutcome } from "../ipc/contract";
import { commandErrorText, describeOutcome, type OutcomeAction } from "../labels/fr";
import { useUiCommands } from "../state/commands";
import { useSafetyBlocked } from "../state/draftSafety";
import { useUiState } from "../state/ui";
import { useNavigation } from "../state/navigation";
import { useAction } from "../state/useAction";
import { FieldError } from "./controls";
import { Glyph } from "./Icon";
import { InfoBar } from "./InfoBar";

interface Props {
  outcome: TakeOutcome;
  hasDraft: boolean;
  headingLevel?: 2 | 3;
  /** Hide « Voir le brouillon » where the draft is already on screen. */
  draftVisible?: boolean;
}

export function OutcomeNotice({ outcome, hasDraft, headingLevel = 2, draftVisible = false }: Props) {
  const blocked = useSafetyBlocked(useUiState());
  const view = describeOutcome(outcome, hasDraft, outcome.kind === "no-text" && blocked);
  const commands = useUiCommands();
  const nav = useNavigation();
  const action = useAction();
  const actions = view.actions.filter((item) => !(draftVisible && item === "open-draft"));

  const button = (item: OutcomeAction, primary: boolean) => {
    const className = primary ? "button button--primary" : "button";
    switch (item) {
      case "open-draft":
        return (
          <button key={item} type="button" className={className} onClick={() => nav.go("draft")}>
            <Glyph name="draft" />
            Voir le brouillon
          </button>
        );
      case "dictate-in-sambler":
        return (
          <button
            key={item}
            type="button"
            className={className}
            onClick={() => void action.run(commands.dictateInSambler)}
          >
            <Glyph name="mic" />
            Dicter dans Sambler
          </button>
        );
      case "open-mic-privacy":
        return (
          <button
            key={item}
            type="button"
            className={className}
            onClick={() => void action.run(commands.openMicPrivacySettings)}
          >
            Ouvrir les réglages de confidentialité du micro
            <Glyph name="external" />
          </button>
        );
      case "enable-dictation":
        return (
          <button key={item} type="button" className={className} onClick={() => nav.openFirstRun("voice")}>
            Activer la dictée…
          </button>
        );
      case "configure-access":
        return (
          <button key={item} type="button" className={className} onClick={() => nav.go("settings", "access")}>
            Configurer l’accès
          </button>
        );
      case "retry":
        return (
          <button
            key={item}
            type="button"
            className={className}
            onClick={() => void action.run(commands.retryConnection)}
          >
            Réessayer
          </button>
        );
    }
  };

  return (
    <InfoBar
      tone={view.tone}
      title={view.title}
      headingLevel={headingLevel}
      testId="outcome"
      actions={actions.length ? actions.map((item, index) => button(item, index === 0)) : undefined}
    >
      {view.detail ? <p>{view.detail}</p> : null}
      {action.error ? <FieldError>{commandErrorText(action.error)}</FieldError> : null}
    </InfoBar>
  );
}
