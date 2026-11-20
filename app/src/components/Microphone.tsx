// Microphone choice and the voluntary level test. The test measures the level
// on this PC only (nothing is sent) and works before consent or while the
// service is down.

import { useCallback, useEffect, useId, useRef, useState } from "react";
import { announcement, useAnnounce } from "../a11y/announcer";
import { toCommandError, useBridge } from "../ipc/bridge";
import { EVENT_MIC_TEST_LEVEL, type CommandError, type MicChoice, type MicInfo } from "../ipc/contract";
import { commandErrorText, isMicAccessDenied } from "../labels/fr";
import { useUiCommands } from "../state/commands";
import { useSettings } from "../state/settings";
import { useAction } from "../state/useAction";
import { FieldError } from "./controls";
import { Glyph } from "./Icon";
import { Waveform } from "./Waveform";
import "./Microphone.css";

const DEFAULT_VALUE = "__system-default__";
/** Release the microphone even if the person forgets to stop the test. */
const TEST_MAX_MS = 60_000;
const HEARD_THRESHOLD = 0.08;

export function MicrophonePicker() {
  const bridge = useBridge();
  const { settings, update, errors } = useSettings();
  const [mics, setMics] = useState<MicInfo[] | null>(null);
  const [listError, setListError] = useState<CommandError | null>(null);
  const selectId = useId();

  const refresh = useCallback(() => {
    setListError(null);
    bridge
      .invoke("list_microphones")
      .then(setMics)
      .catch((error) => {
        setMics([]);
        setListError(toCommandError(error));
      });
  }, [bridge]);

  useEffect(refresh, [refresh]);

  if (!settings) return null;
  const choice = settings.microphone;
  const defaultMic = mics?.find((mic) => mic.is_default);
  const missing = choice.kind === "named" && mics !== null && !mics.some((mic) => mic.id === choice.id);
  const value = choice.kind === "named" ? choice.id : DEFAULT_VALUE;
  const saveError = errors.microphone;

  const onChange = (next: string) => {
    let microphone: MicChoice = { kind: "system-default" };
    if (next !== DEFAULT_VALUE) {
      const mic = mics?.find((item) => item.id === next);
      microphone = mic ? { kind: "named", id: mic.id, name: mic.name } : choice.kind === "named" ? choice : microphone;
    }
    void update("microphone", (current) => ({ ...current, microphone }));
  };

  const describedBy = [missing ? `${selectId}-missing` : "", saveError ? `${selectId}-err` : "", `${selectId}-hint`]
    .filter(Boolean)
    .join(" ");

  return (
    <div className="mic">
      <div className="field">
        <label htmlFor={selectId} className="field__label">
          Micro
        </label>
        <div className="mic__row">
          <select
            id={selectId}
            className="select"
            value={value}
            onChange={(event) => onChange(event.currentTarget.value)}
            aria-describedby={describedBy}
            aria-invalid={missing || saveError ? true : undefined}
          >
            <option value={DEFAULT_VALUE}>
              {defaultMic ? `Micro par défaut de Windows (${defaultMic.name})` : "Micro par défaut de Windows"}
            </option>
            {mics?.map((mic) => (
              <option key={mic.id} value={mic.id}>
                {mic.name}
              </option>
            ))}
            {missing && choice.kind === "named" ? (
              <option value={choice.id}>{`${choice.name} (introuvable)`}</option>
            ) : null}
          </select>
          <button type="button" className="btn btn--ghost" onClick={refresh}>
            Actualiser la liste
          </button>
        </div>
        <p id={`${selectId}-hint`} className="hint">
          Le micro par défaut est celui choisi dans Windows au début de chaque dictée.
        </p>
        {missing ? (
          <FieldError id={`${selectId}-missing`}>
            Ce micro n’est pas branché. Rebranchez-le ou choisissez-en un autre : Sambler n’ouvre jamais un autre micro à
            votre insu.
          </FieldError>
        ) : null}
        {saveError ? (
          <FieldError id={`${selectId}-err`}>Le micro n’a pas pu être enregistré. {commandErrorText(saveError, "")}</FieldError>
        ) : null}
        {listError ? (
          <FieldError>La liste des micros n’a pas pu être lue. {commandErrorText(listError, "")}</FieldError>
        ) : null}
        {listError && isMicAccessDenied(listError) ? <MicPrivacyLink /> : null}
      </div>
      <MicTest microphone={choice} />
    </div>
  );
}

export function MicPrivacyLink() {
  const commands = useUiCommands();
  const action = useAction();
  return (
    <p>
      <button type="button" className="link-btn" onClick={() => void action.run(commands.openMicPrivacySettings)}>
        Ouvrir les réglages de confidentialité du micro
        <Glyph name="external" />
      </button>
    </p>
  );
}

function MicTest({ microphone }: { microphone: MicChoice }) {
  const bridge = useBridge();
  const announce = useAnnounce();
  const [testing, setTesting] = useState(false);
  const [heard, setHeard] = useState(false);
  const [error, setError] = useState<CommandError | null>(null);
  const testingRef = useRef(false);
  const heardRef = useRef(false);
  const statusId = useId();

  const stop = useCallback(async () => {
    if (!testingRef.current) return;
    testingRef.current = false;
    setTesting(false);
    try {
      await bridge.invoke("stop_mic_test");
    } catch {
      // The native side releases the microphone on its own failure paths.
    }
  }, [bridge]);

  const start = async () => {
    setError(null);
    setHeard(false);
    heardRef.current = false;
    testingRef.current = true;
    setTesting(true);
    try {
      await bridge.invoke("start_mic_test");
    } catch (raw) {
      testingRef.current = false;
      setTesting(false);
      const failure = toCommandError(raw);
      setError(failure);
      announce(announcement("Le test du micro n’a pas pu démarrer."));
    }
  };

  // Stop on unmount, after the time limit, and when the chosen mic changes.
  useEffect(() => () => void stop(), [stop]);
  useEffect(() => {
    if (!testing) return;
    const timer = window.setTimeout(() => void stop(), TEST_MAX_MS);
    return () => window.clearTimeout(timer);
  }, [testing, stop]);
  const micKey = microphone.kind === "named" ? microphone.id : "default";
  useEffect(() => {
    void stop();
  }, [micKey, stop]);

  const onLevel = useCallback(
    (level: number) => {
      if (level >= HEARD_THRESHOLD && !heardRef.current) {
        heardRef.current = true;
        setHeard(true);
        announce(announcement("Son détecté : le micro fonctionne."));
      }
    },
    [announce],
  );

  return (
    <div className={testing ? "mic-test mic-test--on" : "mic-test"} data-testid="mic-test">
      <div className="mic-test__row">
        <button
          type="button"
          className={testing ? "btn btn--soft" : "btn"}
          onClick={() => void (testing ? stop() : start())}
          aria-describedby={statusId}
        >
          <Glyph name={testing ? "stop" : "mic"} />
          {testing ? "Arrêter le test" : "Tester le micro"}
        </button>
        <Waveform event={EVENT_MIC_TEST_LEVEL} active={testing} onLevel={onLevel} bars={27} className="mic-test__wave" />
      </div>
      <p id={statusId} className={heard && testing ? "hint mic-test__status mic-test__status--ok" : "hint mic-test__status"}>
        {testing
          ? heard
            ? "Son détecté : le micro fonctionne."
            : "Parlez normalement : les barres doivent bouger."
          : "Le test mesure le niveau sur ce PC, sans rien envoyer, et libère le micro à l’arrêt."}
      </p>
      {error ? (
        <>
          <FieldError>{commandErrorText(error, "Le test du micro n’a pas pu démarrer.")}</FieldError>
          {isMicAccessDenied(error) ? <MicPrivacyLink /> : null}
        </>
      ) : null}
    </div>
  );
}
