// Premier lancement: what Google receives, microphone + level test, Rambler
// access if missing, explicit « Activer la dictée avec Rambler », then a
// guided try with live status. Takes over the window until consent is given.

import { useEffect, useId, useRef, useState, type ReactNode, type RefObject } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { AccessForm, AccessSummary } from "../components/AccessForm";
import { AvailabilityBanner } from "../components/AvailabilityBanner";
import { FieldError, Keys, Switch } from "../components/controls";
import { DraftPanel } from "../components/DraftPanel";
import { AppMark, Glyph } from "../components/Icon";
import { MicrophonePicker } from "../components/Microphone";
import { OutcomeNotice } from "../components/OutcomeNotice";
import { ShortcutField } from "../components/ShortcutField";
import { StatusCard } from "../components/StatusCard";
import { canStartTake, commandErrorText, hasConsent, isTakeActive } from "../labels/fr";
import { useAccess } from "../state/access";
import { useUiCommands } from "../state/commands";
import type { FirstRunStep } from "../state/navigation";
import { useSettings } from "../state/settings";
import { useUiState } from "../state/ui";
import { useAction } from "../state/useAction";
import "./FirstRun.css";

const STEPS: { id: FirstRunStep; title: string }[] = [
  { id: "voice", title: "Votre voix" },
  { id: "microphone", title: "Micro" },
  { id: "access", title: "Accès à Rambler" },
  { id: "consent", title: "Activation" },
  { id: "try", title: "Essai" },
];

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

export function FirstRun({ initialStep = "voice", openedByPerson = false, onClose }: Props) {
  const state = useUiState();
  const [step, setStep] = useState<FirstRunStep>(initialStep);
  const titleRef = useRef<HTMLHeadingElement>(null);
  const focusedStep = useRef<FirstRunStep | null>(openedByPerson ? null : initialStep);
  const index = STEPS.findIndex((item) => item.id === step);
  const consent = hasConsent(state.availability);
  const closable = consent || openedByPerson;

  // Move focus to the new step's title so screen readers read it.
  useEffect(() => {
    if (focusedStep.current === step) return;
    focusedStep.current = step;
    titleRef.current?.focus();
  }, [step]);

  const go = (next: FirstRunStep) => setStep(next);
  const next = () => go(STEPS[Math.min(index + 1, STEPS.length - 1)].id);
  const back = () => go(STEPS[Math.max(index - 1, 0)].id);

  return (
    <div className="first-run">
      <header className="first-run__header">
        <p className="first-run__brand">
          <AppMark /> Sambler
        </p>
        <div className="first-run__progress">
          <ol aria-label="Étapes de la prise en main">
            {STEPS.map((item, position) => (
              <li
                key={item.id}
                className={
                  position === index ? "is-current" : position < index ? "is-done" : undefined
                }
                aria-current={position === index ? "step" : undefined}
              >
                <span className="first-run__step-number" aria-hidden="true">
                  {position < index ? "✓" : position + 1}
                </span>
                <span>
                  {item.title}
                  {position < index ? <span className="visually-hidden"> (terminée)</span> : null}
                </span>
              </li>
            ))}
          </ol>
        </div>
        {closable ? (
          <button type="button" className="button button--subtle first-run__close" onClick={onClose}>
            Fermer
          </button>
        ) : null}
      </header>

      <main className="first-run__main" id="main">
        <p className="first-run__counter">
          Étape {index + 1} sur {STEPS.length}
        </p>
        {step === "voice" ? (
          <VoiceStep titleRef={titleRef} onNext={next} />
        ) : step === "microphone" ? (
          <MicrophoneStep titleRef={titleRef} onNext={next} onBack={back} />
        ) : step === "access" ? (
          <AccessStep titleRef={titleRef} onNext={next} onBack={back} />
        ) : step === "consent" ? (
          <ConsentStep titleRef={titleRef} onNext={next} onBack={back} />
        ) : (
          <TryStep titleRef={titleRef} onBack={back} onDone={onClose} />
        )}
      </main>
    </div>
  );
}

type TitleRef = RefObject<HTMLHeadingElement | null>;

function StepTitle({ titleRef, children }: { titleRef: TitleRef; children: ReactNode }) {
  return (
    <h1 ref={titleRef} tabIndex={-1} className="first-run__title">
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
    <div className="first-run__nav">
      {onBack ? (
        <button type="button" className="button" onClick={onBack}>
          Retour
        </button>
      ) : (
        <span />
      )}
      <div className="button-row">
        {extra}
        {onNext ? (
          <button type="button" className={nextPrimary ? "button button--primary" : "button"} onClick={onNext}>
            {nextLabel}
          </button>
        ) : null}
      </div>
    </div>
  );
}

function VoiceStep({ titleRef, onNext }: { titleRef: TitleRef; onNext: () => void }) {
  const sentId = useId();
  const keptId = useId();
  return (
    <div className="first-run__step">
      <StepTitle titleRef={titleRef}>Votre voix est transmise à Google pendant que vous parlez</StepTitle>
      <p className="first-run__lead">
        Sambler transcrit uniquement avec Rambler, le service de reconnaissance vocale de Google. Rien n’est envoyé
        tant que vous n’avez pas activé la dictée.
      </p>
      <div className="first-run__columns">
        <section className="first-run__card" aria-labelledby={sentId}>
          <h2 id={sentId}>Ce que reçoit Google</h2>
          <ul>
            <li>Le son de votre voix, seulement pendant une dictée.</li>
            <li>La langue choisie et des paramètres techniques.</li>
            <li>Les données d’accès à Rambler de la session.</li>
            <li>L’adresse réseau de votre connexion.</li>
          </ul>
        </section>
        <section className="first-run__card" aria-labelledby={keptId}>
          <h2 id={keptId}>Ce qui reste sur ce PC</h2>
          <ul>
            <li>Le nom de l’application et le titre de la fenêtre où vous écrivez.</li>
            <li>Le texte de vos documents, votre sélection et votre presse-papiers.</li>
            <li>Le test du micro : il mesure le niveau ici, sans rien envoyer.</li>
            <li>Aucun enregistrement audio n’est conservé.</li>
          </ul>
        </section>
      </div>
      <p>
        <strong>Annuler</strong> une dictée, ou désactiver la transmission dans les réglages, coupe aussitôt la capture
        et l’envoi. Ce qui a déjà été transmis reste du côté du service. Les conditions de conservation et d’usage
        sont celles du fournisseur de votre accès à Rambler.
      </p>
      <StepNav onNext={onNext} />
    </div>
  );
}

function MicrophoneStep({ titleRef, onNext, onBack }: { titleRef: TitleRef; onNext: () => void; onBack: () => void }) {
  return (
    <div className="first-run__step">
      <StepTitle titleRef={titleRef}>Choisissez votre micro</StepTitle>
      <p className="first-run__lead">
        Essayez le niveau : parlez quelques secondes et regardez la jauge. Ce test ne transmet rien.
      </p>
      <div className="first-run__card">
        <MicrophonePicker />
      </div>
      <StepNav onBack={onBack} onNext={onNext} />
    </div>
  );
}

function AccessStep({ titleRef, onNext, onBack }: { titleRef: TitleRef; onNext: () => void; onBack: () => void }) {
  const { status, loadFailed, refresh } = useAccess();
  return (
    <div className="first-run__step">
      <StepTitle titleRef={titleRef}>Accès à Rambler</StepTitle>
      {status === null ? (
        loadFailed ? (
          <div className="stack">
            <FieldError>L’état de l’accès n’a pas pu être lu.</FieldError>
            <div className="button-row">
              <button type="button" className="button" onClick={refresh}>
                Réessayer
              </button>
            </div>
          </div>
        ) : (
          <p className="muted">Chargement…</p>
        )
      ) : status.configured ? (
        <>
          <p className="first-run__lead">Un accès au service est déjà configuré sur ce PC.</p>
          <div className="first-run__card">
            <AccessSummary status={status} />
          </div>
        </>
      ) : (
        <>
          <p className="first-run__lead">
            Sambler a besoin de vos données d’accès au service. Elles sont transmises une fois à la partie native de
            l’application, puis ne sont plus jamais affichées.
          </p>
          <div className="first-run__card">
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
        <p className="field-hint first-run__aside">
          Sans accès, la dictée restera indisponible. Vous pourrez le configurer dans Réglages.
        </p>
      ) : null}
    </div>
  );
}

function ConsentStep({ titleRef, onNext, onBack }: { titleRef: TitleRef; onNext: () => void; onBack: () => void }) {
  const state = useUiState();
  const commands = useUiCommands();
  const announce = useAnnounce();
  const action = useAction();
  const consent = hasConsent(state.availability);

  const enable = async () => {
    const result = await action.run(() => commands.setConsent(true));
    // `save-failed`: enabled for this session, only the disk write failed.
    if (result.ok || result.error.code === "save-failed") {
      announce(announcement("Dictée avec Rambler activée."));
      onNext();
    }
  };

  return (
    <div className="first-run__step">
      <StepTitle titleRef={titleRef}>Activer la dictée</StepTitle>
      {consent ? (
        <>
          <p className="first-run__lead">La dictée avec Rambler est activée sur ce PC.</p>
          <p className="muted">Vous pouvez la désactiver à tout moment dans Réglages, Confidentialité.</p>
          <StepNav onBack={onBack} onNext={onNext} />
        </>
      ) : (
        <>
          <p className="first-run__lead">
            En activant la dictée, vous choisissez que votre voix soit transmise à Google pendant que vous parlez.
          </p>
          <ul className="first-run__list">
            <li>L’envoi commence seulement quand vous déclenchez une dictée.</li>
            <li>Annuler, ou désactiver dans Réglages, coupe aussitôt la capture et l’envoi.</li>
            <li>Le texte est inséré une seule fois, dans le champ où vous avez commencé.</li>
          </ul>
          {action.error ? <FieldError>{commandErrorText(action.error)}</FieldError> : null}
          <StepNav
            onBack={onBack}
            extra={
              <button
                type="button"
                className="button button--primary button--large"
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

function TryStep({ titleRef, onBack, onDone }: { titleRef: TitleRef; onBack: () => void; onDone: () => void }) {
  const state = useUiState();
  const { settings, update, errors } = useSettings();
  const commands = useUiCommands();
  const dictate = useAction();
  const active = isTakeActive(state);
  const hold = state.gesture === "hold";
  const outcomeId = useId();
  const othersId = useId();

  return (
    <div className="first-run__step">
      <StepTitle titleRef={titleRef}>Essayez une phrase</StepTitle>
      <p className="first-run__instruction" data-testid="try-instruction">
        {hold ? "Maintenez " : "Appuyez sur "}
        <Keys shortcut={state.shortcut} />, dites « {TRY_SENTENCE} », puis{" "}
        {hold ? "relâchez." : "appuyez de nouveau."}
      </p>
      <p className="muted">
        Attendez l’état « Écoute » avant de parler. Ici, le texte reste dans Sambler pour que vous puissiez le relire.
      </p>

      <AvailabilityBanner headingLevel={2} />
      <StatusCard headingLevel={2} />

      {state.lastOutcome ? (
        <section aria-labelledby={outcomeId} className="first-run__result">
          <h2 id={outcomeId} className="visually-hidden">
            Résultat de l’essai
          </h2>
          <OutcomeNotice
            outcome={state.lastOutcome}
            hasDraft={state.draft !== null}
            headingLevel={3}
            draftVisible={state.draft !== null}
          />
        </section>
      ) : null}
      {state.draft ? <DraftPanel headingLevel={2} title="Texte obtenu" /> : null}

      <section className="first-run__card stack" aria-labelledby={othersId}>
        <h2 id={othersId} className="first-run__card-title">
          Autres façons d’essayer
        </h2>
        {settings ? (
          <>
            <Switch
              label="Mode bascule"
              description="Appuyez une fois pour commencer, une seconde fois pour terminer, mains libres pendant que vous parlez."
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
          </>
        ) : null}
        {!active ? (
          <div className="stack stack--tight">
            <div className="button-row">
              <button
                type="button"
                className="button"
                onClick={() => void dictate.run(commands.dictateInSambler)}
                disabled={!canStartTake(state.availability)}
              >
                <Glyph name="mic" />
                Dicter dans Sambler, sans raccourci
              </button>
            </div>
            {dictate.error ? <FieldError>{commandErrorText(dictate.error)}</FieldError> : null}
          </div>
        ) : null}
        <p className="field-hint">
          Pour annuler pendant l’essai, appuyez sur <Keys shortcut="Escape" /> ou choisissez « Annuler ».
        </p>
      </section>

      <StepNav onBack={onBack} onNext={onDone} nextLabel="Terminer la prise en main" />
    </div>
  );
}
