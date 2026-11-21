// Prise en main: what Google receives and the explicit « Activer la dictée
// avec Rambler », then the microphone and its level test, the Rambler access
// (only when none is configured), and a guided try with live status. Takes
// over the window until consent is given.

import { useEffect, useId, useRef, useState, type ReactNode, type RefObject } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { AccessForm, AccessSummary } from "../components/AccessForm";
import { AvailabilityBanner } from "../components/AvailabilityBanner";
import { FieldError, Keys, Switch } from "../components/controls";
import { AppMark, Glyph } from "../components/Icon";
import { MicrophonePicker } from "../components/Microphone";
import { OutcomeNotice } from "../components/OutcomeNotice";
import { RecentList } from "../components/RecentList";
import { ShortcutField } from "../components/ShortcutField";
import { StatusCard } from "../components/StatusCard";
import { commandErrorText, hasConsent } from "../labels/fr";
import { useAccess } from "../state/access";
import { useUiCommands } from "../state/commands";
import type { FirstRunStep } from "../state/navigation";
import { useRecents } from "../state/recents";
import { useSettings } from "../state/settings";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import "./FirstRun.css";

const TITLES: Record<FirstRunStep, string> = {
  consent: "Activation",
  microphone: "Micro",
  access: "Accès à Rambler",
  try: "Essai",
};

// A no-break space in "14 h": the hour never splits across lines.
export const TRY_SENTENCE = "Rendez-vous jeudi à 14 h";

interface Props {
  initialStep?: FirstRunStep;
  /**
   * Opened by the person (sidebar, banner, settings) rather than at startup:
   * the step title takes the focus and the flow can be closed. At startup the
   * flow takes over until consent is given.
   */
  openedByPerson?: boolean;
  onClose: () => void;
}

export function FirstRun({ initialStep = "consent", openedByPerson = false, onClose }: Props) {
  const state = useUiState();
  const { status } = useAccess();
  const [step, setStep] = useState<FirstRunStep>(initialStep);
  // The access step is shown only when no access is configured; decided once
  // so that saving the access does not remove the step being shown.
  const [includeAccess, setIncludeAccess] = useState<boolean | null>(() =>
    status ? !status.configured || initialStep === "access" : null,
  );
  useEffect(() => {
    if (includeAccess === null && status) setIncludeAccess(!status.configured);
  }, [includeAccess, status]);

  const steps: FirstRunStep[] = ["consent", "microphone", ...(includeAccess !== false ? (["access"] as const) : []), "try"];
  const titleRef = useRef<HTMLHeadingElement>(null);
  const focusedStep = useRef<FirstRunStep | null>(openedByPerson ? null : initialStep);
  const index = Math.max(0, steps.indexOf(step));
  const consent = hasConsent(state.availability);
  const closable = consent || openedByPerson;

  // Move focus to the new step's title so screen readers read it.
  useEffect(() => {
    if (focusedStep.current === step) return;
    focusedStep.current = step;
    titleRef.current?.focus();
  }, [step]);

  const next = () => setStep(steps[Math.min(index + 1, steps.length - 1)]);
  const back = () => setStep(steps[Math.max(index - 1, 0)]);

  return (
    <div className="onboard">
      <main className="onboard__main" id="main">
        <div className="onboard__panel">
          <div className="onboard__top">
            <ol className="onboard__steps" aria-label="Étapes de la prise en main">
              {steps.map((item, position) => (
                <li
                  key={item}
                  className={position === index ? "is-current" : position < index ? "is-done" : undefined}
                  aria-current={position === index ? "step" : undefined}
                >
                  <span className="onboard__dot" aria-hidden="true">
                    {position < index ? <Glyph name="check" /> : position + 1}
                  </span>
                  <span className="onboard__step-name">
                    {TITLES[item]}
                    {position < index ? <span className="visually-hidden"> (terminée)</span> : null}
                  </span>
                </li>
              ))}
            </ol>
            <p className="onboard__counter">
              Étape {index + 1} sur {steps.length}
            </p>
            {closable ? (
              <button type="button" className="btn btn--sm btn--ghost onboard__close" onClick={onClose}>
                Fermer la prise en main
              </button>
            ) : null}
          </div>

          {step === "consent" ? (
            <ConsentStep titleRef={titleRef} onNext={next} />
          ) : step === "microphone" ? (
            <MicrophoneStep titleRef={titleRef} onNext={next} onBack={back} />
          ) : step === "access" ? (
            <AccessStep titleRef={titleRef} onNext={next} onBack={back} />
          ) : (
            <TryStep titleRef={titleRef} onBack={back} onDone={onClose} />
          )}
        </div>
      </main>
    </div>
  );
}

type TitleRef = RefObject<HTMLHeadingElement | null>;

function StepTitle({ titleRef, children }: { titleRef: TitleRef; children: ReactNode }) {
  return (
    <h1 ref={titleRef} tabIndex={-1} className="onboard__title">
      {children}
    </h1>
  );
}

function StepNav({
  onBack,
  onNext,
  nextLabel = "Continuer",
  nextPrimary = true,
  extra,
}: {
  onBack?: () => void;
  onNext?: () => void;
  nextLabel?: string;
  nextPrimary?: boolean;
  extra?: ReactNode;
}) {
  return (
    <div className="onboard__nav">
      {onBack ? (
        <button type="button" className="btn btn--ghost" onClick={onBack}>
          Retour
        </button>
      ) : (
        <span />
      )}
      <div className="btn-row">
        {extra}
        {onNext ? (
          <button type="button" className={nextPrimary ? "btn btn--primary btn--lg" : "btn btn--lg"} onClick={onNext}>
            {nextLabel}
            {nextPrimary ? <Glyph name="arrow-right" /> : null}
          </button>
        ) : null}
      </div>
    </div>
  );
}

function ConsentStep({ titleRef, onNext }: { titleRef: TitleRef; onNext: () => void }) {
  const state = useUiState();
  const commands = useUiCommands();
  const announce = useAnnounce();
  const action = useAction();
  const consent = hasConsent(state.availability);
  const sentId = useId();
  const keptId = useId();

  const enable = async () => {
    const result = await action.run(() => commands.setConsent(true));
    // `save-failed`: enabled for this session, only the disk write failed.
    if (result.ok || result.error.code === "save-failed") {
      announce(announcement("Dictée avec Rambler activée."));
      onNext();
    }
  };

  return (
    <div className="onboard__step">
      {/* Once enabled, the confirmation takes the mark's place. */}
      {consent ? null : (
        <div className="onboard__hero">
          <AppMark className="onboard__mark" />
        </div>
      )}
      <StepTitle titleRef={titleRef}>Votre voix est transmise à Google pendant que vous parlez</StepTitle>
      <p className="onboard__lead">
        Sambler transcrit uniquement avec Rambler, le service de reconnaissance vocale de Google. Rien n’est envoyé tant
        que vous n’avez pas activé la dictée.
      </p>
      <div className="onboard__columns">
        <section className="onboard__card" aria-labelledby={sentId}>
          <h2 id={sentId} className="onboard__card-title">
            <Glyph name="globe" />
            Ce que reçoit Google
          </h2>
          <ul>
            <li>Le son de votre voix, seulement pendant une dictée.</li>
            <li>La langue choisie et des paramètres techniques.</li>
            <li>Les données d’accès à Rambler de la session.</li>
            <li>L’adresse réseau de votre connexion.</li>
          </ul>
        </section>
        <section className="onboard__card" aria-labelledby={keptId}>
          <h2 id={keptId} className="onboard__card-title">
            <Glyph name="shield" />
            Ce qui reste sur ce PC
          </h2>
          <ul>
            <li>Le nom de l’application et le titre de la fenêtre où vous écrivez.</li>
            <li>Le texte de vos documents, votre sélection et votre presse-papiers.</li>
            <li>Le test du micro : il mesure le niveau ici, sans rien envoyer.</li>
            <li>Aucun enregistrement audio n’est conservé.</li>
          </ul>
        </section>
      </div>
      <p className="onboard__small">
        <strong>Annuler</strong> une dictée, ou désactiver la transmission dans les réglages, coupe aussitôt la capture
        et l’envoi. Ce qui a déjà été transmis reste du côté du service. Les conditions de conservation et d’usage sont
        celles du fournisseur de votre accès à Rambler.
      </p>
      {consent ? (
        <>
          <p className="onboard__done">
            <Glyph name="check" />
            La dictée avec Rambler est activée sur ce PC (désactivable dans Réglages, Confidentialité).
          </p>
          <StepNav onNext={onNext} />
        </>
      ) : (
        <>
          {action.error ? <FieldError>{commandErrorText(action.error)}</FieldError> : null}
          <StepNav
            extra={
              <button
                type="button"
                className="btn btn--primary btn--lg"
                onClick={() => void enable()}
                aria-disabled={action.busy || undefined}
              >
                Activer la dictée avec Rambler
              </button>
            }
          />
        </>
      )}
    </div>
  );
}

function MicrophoneStep({ titleRef, onNext, onBack }: { titleRef: TitleRef; onNext: () => void; onBack: () => void }) {
  return (
    <div className="onboard__step">
      <StepTitle titleRef={titleRef}>Choisissez votre micro</StepTitle>
      <p className="onboard__lead">Essayez le niveau : parlez quelques secondes et regardez les barres. Ce test ne transmet rien.</p>
      <div className="onboard__card">
        <MicrophonePicker />
      </div>
      <StepNav onBack={onBack} onNext={onNext} />
    </div>
  );
}

function AccessStep({ titleRef, onNext, onBack }: { titleRef: TitleRef; onNext: () => void; onBack: () => void }) {
  const { status, loadFailed, refresh } = useAccess();
  return (
    <div className="onboard__step">
      <StepTitle titleRef={titleRef}>Accès à Rambler</StepTitle>
      {status === null ? (
        loadFailed ? (
          <div className="stack">
            <FieldError>L’état de l’accès n’a pas pu être lu.</FieldError>
            <div className="btn-row">
              <button type="button" className="btn" onClick={refresh}>
                Réessayer
              </button>
            </div>
          </div>
        ) : (
          <p className="muted">Chargement…</p>
        )
      ) : status.configured ? (
        <>
          <p className="onboard__lead">Un accès au service est configuré sur ce PC.</p>
          <div className="onboard__card">
            <AccessSummary status={status} />
          </div>
        </>
      ) : (
        <>
          <p className="onboard__lead">
            Sambler a besoin de vos données d’accès au service. Elles sont transmises une fois à la partie native de
            l’application, puis ne sont plus jamais affichées.
          </p>
          <div className="onboard__card">
            <AccessForm onSaved={onNext} submitLabel="Enregistrer et continuer" />
          </div>
        </>
      )}
      <StepNav
        onBack={onBack}
        onNext={onNext}
        nextLabel={status?.configured ? "Continuer" : "Plus tard"}
        nextPrimary={Boolean(status?.configured)}
      />
      {status && !status.configured ? (
        <p className="hint onboard__aside">Sans accès, la dictée restera indisponible. Vous pourrez le configurer dans Réglages.</p>
      ) : null}
    </div>
  );
}

function TryStep({ titleRef, onBack, onDone }: { titleRef: TitleRef; onBack: () => void; onDone: () => void }) {
  const state = useUiState();
  const { settings, update, errors } = useSettings();
  const recents = useRecents();
  const hold = state.gesture === "hold";
  const outcomeId = useId();
  const othersId = useId();
  const minutes = settings?.draftMinutes ?? 10;

  return (
    <div className="onboard__step">
      <StepTitle titleRef={titleRef}>Essayez une phrase</StepTitle>
      <p className="onboard__instruction" data-testid="try-instruction">
        {hold ? "Maintenez " : "Appuyez sur "}
        <Keys shortcut={state.shortcut} />, dites{" "}
        <span className="onboard__phrase">« {TRY_SENTENCE} »</span>, puis {hold ? "relâchez." : "appuyez de nouveau."}
      </p>

      <AvailabilityBanner headingLevel={2} />
      <StatusCard headingLevel={2} dictateLabel="Dicter dans Sambler, sans raccourci" />

      {state.lastOutcome ? (
        <section aria-labelledby={outcomeId} className="onboard__result">
          <h2 id={outcomeId} className="visually-hidden">
            Résultat de l’essai
          </h2>
          <OutcomeNotice
            outcome={state.lastOutcome}
            hasDraft={state.draft !== null}
            headingLevel={3}
            recentsVisible={state.draft !== null}
          />
        </section>
      ) : null}
      {state.draft || (recents.items?.length ?? 0) > 0 ? (
        <RecentList store={recents} minutes={minutes} title="Texte obtenu" newestOnly />
      ) : null}

      <section className="onboard__card stack" aria-labelledby={othersId}>
        <h2 id={othersId} className="onboard__card-title">
          Autres façons d’essayer
        </h2>
        {settings ? (
          <div className="onboard__options">
            <Switch
              label="Mode bascule"
              description="Une pression pour commencer, une autre pour terminer : mains libres pendant que vous parlez."
              checked={settings.gesture === "toggle"}
              onChange={(toggle) => void update("gesture", (s) => ({ ...s, gesture: toggle ? "toggle" : "hold" }))}
              error={errors.gesture ? commandErrorText(errors.gesture) : undefined}
            />
            <ShortcutField
              label="Combinaison"
              value={settings.shortcut}
              onChange={(shortcut) => update("shortcut", (s) => ({ ...s, shortcut }))}
              error={errors.shortcut}
            />
          </div>
        ) : null}
        <p className="hint">
          Pour annuler pendant l’essai, appuyez sur <Keys shortcut="Escape" /> ou choisissez « Annuler ».
        </p>
      </section>

      <StepNav onBack={onBack} onNext={onDone} nextLabel="Terminer la prise en main" />
    </div>
  );
}
