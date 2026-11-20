// "Dictée indisponible" with its human reason and « Réessayer », or the
// call to action when consent or access is missing. Nothing while ready.

import { commandErrorText, unavailableReasonText } from "../labels/fr";
import { useUiCommands } from "../state/commands";
import { useNavigation } from "../state/navigation";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import { FieldError } from "./controls";
import { Notice } from "./Notice";

export function AvailabilityBanner({ headingLevel = 2 }: { headingLevel?: 2 | 3 }) {
  const { availability } = useUiState();
  const commands = useUiCommands();
  const nav = useNavigation();
  const retry = useAction();

  switch (availability.kind) {
    case "ready":
    case "connecting":
      return null;
    case "needs-consent":
      return (
        <Notice
          tone="info"
          title="La dictée n’est pas activée"
          headingLevel={headingLevel}
          testId="availability-banner"
          actions={
            <button type="button" className="btn btn--sm btn--primary" onClick={() => nav.openFirstRun("consent")}>
              Activer la dictée…
            </button>
          }
        >
          <p>Votre voix est transmise à Google pendant que vous parlez. Activez la dictée pour commencer.</p>
        </Notice>
      );
    case "needs-access":
      return (
        <Notice
          tone="info"
          title="Accès à Rambler requis"
          headingLevel={headingLevel}
          testId="availability-banner"
          actions={
            <button type="button" className="btn btn--sm btn--primary" onClick={() => nav.go("settings", "access")}>
              Configurer l’accès
            </button>
          }
        >
          <p>Renseignez vos données d’accès au service pour dicter.</p>
        </Notice>
      );
    case "unavailable": {
      const accessProblem = availability.reason === "access-refused" || availability.reason === "invalid-access";
      return (
        <Notice
          tone="error"
          title="Dictée indisponible"
          headingLevel={headingLevel}
          testId="availability-banner"
          actions={
            <>
              <button
                type="button"
                className="btn btn--sm btn--primary"
                aria-disabled={retry.busy || undefined}
                onClick={() => {
                  if (!retry.busy) void retry.run(commands.retryConnection);
                }}
              >
                {retry.busy ? "Nouvelle tentative…" : "Réessayer"}
              </button>
              {accessProblem ? (
                <button type="button" className="btn btn--sm" onClick={() => nav.go("settings", "access")}>
                  Modifier l’accès
                </button>
              ) : null}
            </>
          }
        >
          <p>{unavailableReasonText(availability.reason)}</p>
          <p>Les réglages, les textes récents et la copie restent disponibles.</p>
          {retry.error ? <FieldError>{commandErrorText(retry.error)}</FieldError> : null}
        </Notice>
      );
    }
  }
}
