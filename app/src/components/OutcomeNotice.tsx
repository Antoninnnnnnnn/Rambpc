// Result of the latest take, for every `TakeOutcome` variant. Text-free.

import type { TakeOutcome } from "../ipc/contract";
import { commandErrorText, describeOutcome, type OutcomeAction } from "../labels/fr";
import { useUiCommands } from "../state/commands";
import { useSafetyBlocked } from "../state/draftSafety";
import { useNavigation } from "../state/navigation";
import { useAction } from "../state/useAction";
import { FieldError } from "./controls";
import { Glyph } from "./Icon";
import { Notice } from "./Notice";

interface Props {
  outcome: TakeOutcome;
  hasDraft: boolean;
  headingLevel?: 2 | 3;
  /** Hide « Voir dans Récents » where the text is already on screen. */
  recentsVisible?: boolean;
}

export function OutcomeNotice({ outcome, hasDraft, headingLevel = 2, recentsVisible = false }: Props) {
  const blocked = useSafetyBlocked();
  const view = describeOutcome(outcome, hasDraft, outcome.kind === "no-text" && blocked);
  const commands = useUiCommands();
  const nav = useNavigation();
  const action = useAction();
  const actions = view.actions.filter((item) => !(recentsVisible && item === "open-recents"));

  const button = (item: OutcomeAction, primary: boolean) => {
    const className = primary ? "btn btn--sm btn--primary" : "btn btn--sm";
    switch (item) {
      case "open-recents":
        return (
          <button key={item} type="button" className={className} onClick={() => nav.go("home", "home-recents")}>
            Voir dans Récents
            <Glyph name="arrow-right" />
          </button>
        );
      case "dictate-in-sambler":
        return (
          <button key={item} type="button" className={className} onClick={() => void action.run(commands.dictateInSambler)}>
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
          <button key={item} type="button" className={className} onClick={() => nav.openFirstRun("consent")}>
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
          <button key={item} type="button" className={className} onClick={() => void action.run(commands.retryConnection)}>
            Réessayer
          </button>
        );
    }
  };

  return (
    <Notice
      tone={view.tone}
      title={view.title}
      headingLevel={headingLevel}
      testId="outcome"
      quiet={view.tone === "success"}
      actions={actions.length ? actions.map((item, index) => button(item, index === 0)) : undefined}
    >
      {view.detail ? <p>{view.detail}</p> : null}
      {action.error ? <FieldError>{commandErrorText(action.error)}</FieldError> : null}
    </Notice>
  );
}
