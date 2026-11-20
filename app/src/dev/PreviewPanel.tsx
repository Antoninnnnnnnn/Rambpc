// DEV PREVIEW ONLY: a floating panel to drive the simulated states while
// designing in a browser. Never part of a production build.

import { useSyncExternalStore } from "react";
import type { TakeOutcome, UnavailableReason } from "../ipc/contract";
import type { CopyMode, PreviewControls } from "./previewBridge";
import "./preview.css";

type DraftKind = "none" | "complete" | "gaps";

const OUTCOMES: { label: string; outcome: TakeOutcome | null; draft: DraftKind }[] = [
  { label: "(aucune)", outcome: null, draft: "none" },
  { label: "Inserted", outcome: { kind: "inserted" }, draft: "complete" },
  { label: "InsertedPartially", outcome: { kind: "inserted-partially" }, draft: "complete" },
  { label: "Kept: SamblerDestination", outcome: { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } }, draft: "complete" },
  { label: "Kept: UncertainTarget", outcome: { kind: "kept-in-sambler", reason: { kind: "uncertain-target", reason: "terminal" } }, draft: "complete" },
  { label: "Kept: TargetChanged", outcome: { kind: "kept-in-sambler", reason: { kind: "target-changed", change: "focus-moved" } }, draft: "complete" },
  { label: "Kept: NotWritten", outcome: { kind: "kept-in-sambler", reason: { kind: "not-written", reason: "unsupported" } }, draft: "complete" },
  { label: "Kept: Incomplete (gaps)", outcome: { kind: "kept-in-sambler", reason: { kind: "incomplete" } }, draft: "gaps" },
  { label: "NoText", outcome: { kind: "no-text" }, draft: "none" },
  { label: "NoText + blocked passage", outcome: { kind: "no-text" }, draft: "gaps" },
  { label: "Cancelled", outcome: { kind: "cancelled" }, draft: "none" },
  { label: "Interrupted: network, recovered", outcome: { kind: "interrupted", reason: { kind: "network" }, recovered: true }, draft: "gaps" },
  { label: "Interrupted: mic denied", outcome: { kind: "interrupted", reason: { kind: "microphone", error: { kind: "access-denied" } }, recovered: false }, draft: "none" },
  { label: "Interrupted: duration limit", outcome: { kind: "interrupted", reason: { kind: "duration-limit" }, recovered: true }, draft: "complete" },
  { label: "Refused: ProtectedField", outcome: { kind: "refused", reason: { kind: "protected-field" } }, draft: "none" },
  { label: "Refused: Microphone not found", outcome: { kind: "refused", reason: { kind: "microphone", error: { kind: "not-found" } } }, draft: "none" },
];

const AVAILABILITY: { label: string; value: "auto" | "connecting" | UnavailableReason }[] = [
  { label: "auto (consent/access/ready)", value: "auto" },
  { label: "connecting", value: "connecting" },
  { label: "unavailable: network", value: "network" },
  { label: "unavailable: access-refused", value: "access-refused" },
  { label: "unavailable: throttled", value: "throttled" },
  { label: "unavailable: invalid-access", value: "invalid-access" },
  { label: "unavailable: service", value: "service" },
];

export function PreviewPanel({ controls }: { controls: PreviewControls }) {
  const state = useSyncExternalStore(controls.subscribe, controls.state);
  return (
    <details className="dev-preview" lang="en">
      <summary>DEV preview · {state.phase.kind} · {state.availability.kind}</summary>
      <div className="dev-preview__body">
        <label>
          Availability
          <select onChange={(event) => controls.setAvailability(event.currentTarget.value as never)}>
            {AVAILABILITY.map((item) => (
              <option key={item.value} value={item.value}>
                {item.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Outcome
          <select
            onChange={(event) => {
              const item = OUTCOMES[Number(event.currentTarget.value)];
              controls.setOutcome(item.outcome, item.draft);
            }}
          >
            {OUTCOMES.map((item, index) => (
              <option key={item.label} value={index}>
                {item.label}
              </option>
            ))}
          </select>
        </label>
        <fieldset>
          <legend>Phase</legend>
          {(["idle", "preparing", "listening", "finalizing"] as const).map((phase) => (
            <button key={phase} type="button" onClick={() => controls.setPhase(phase)}>
              {phase}
            </button>
          ))}
        </fieldset>
        <button type="button" onClick={controls.fillRecents}>
          Fill recent results
        </button>
        <label>
          Next copy
          <select onChange={(event) => controls.setCopyMode(event.currentTarget.value as CopyMode)}>
            <option value="ok">ok</option>
            <option value="busy">busy</option>
            <option value="exclusion-failed">exclusion-failed</option>
          </select>
        </label>
        <label className="dev-preview__check">
          <input type="checkbox" onChange={(event) => controls.setFailNextSave(event.currentTarget.checked)} />
          Fail next settings save
        </label>
        <button type="button" onClick={controls.resetFirstRun}>
          Reset to first run
        </button>
      </div>
    </details>
  );
}
