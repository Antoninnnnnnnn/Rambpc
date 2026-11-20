// Announces phase, outcome and availability changes in French. Built only
// from the text-free UiState (plus the draft's "blocked" flag, a part status):
// dictated words never reach the live region.

import { useEffect, useRef, useState } from "react";
import type { UiState } from "../ipc/contract";
import { describeOutcome, unavailableReasonText, type OutcomeView } from "../labels/fr";
import { safetyKey, useDraftSafety } from "../state/draftSafety";
import { useUiState } from "../state/ui";
import { announcement, useAnnounce } from "./announcer";

const PHASE_ANNOUNCEMENTS = {
  idle: null,
  preparing: "Préparation du micro.",
  listening: "Écoute : vous pouvez parler.",
  finalizing: "Traitement en cours.",
} as const;

function outcomeKey(state: UiState): string {
  return state.lastOutcome ? JSON.stringify(state.lastOutcome) : "";
}

function availabilityKey(state: UiState): string {
  const availability = state.availability;
  return availability.kind === "unavailable" ? `unavailable:${availability.reason}` : availability.kind;
}

/** Title and detail as one spoken sentence. */
export function outcomeSentence(view: OutcomeView): string {
  const title = /[.!?…]$/.test(view.title) ? view.title : `${view.title}.`;
  return view.detail ? `${title} ${view.detail}` : title;
}

export interface Change {
  messages: string[];
  /**
   * A `no-text` outcome that left a draft: its wording depends on whether
   * Rambler's safety filter blocked a passage, known once the check ends.
   */
  deferredOutcome: boolean;
}

/** French sentences describing what changed between two states. */
export function describeChange(before: UiState, after: UiState): Change {
  const messages: string[] = [];
  let deferredOutcome = false;

  if (after.phase.kind !== before.phase.kind) {
    const phase = PHASE_ANNOUNCEMENTS[after.phase.kind];
    if (phase) messages.push(phase);
  }

  const takeEnded = before.phase.kind !== "idle" && after.phase.kind === "idle";
  if (after.lastOutcome && (takeEnded || outcomeKey(after) !== outcomeKey(before))) {
    if (safetyKey(after) !== null) deferredOutcome = true;
    else messages.push(outcomeSentence(describeOutcome(after.lastOutcome, after.draft !== null)));
  }

  if (availabilityKey(after) !== availabilityKey(before)) {
    const availability = after.availability;
    if (availability.kind === "unavailable") {
      messages.push(`Dictée indisponible. ${unavailableReasonText(availability.reason)}`);
    } else if (
      availability.kind === "ready" &&
      (before.availability.kind === "unavailable" || before.availability.kind === "connecting")
    ) {
      messages.push("Dictée disponible.");
    }
  }

  return { messages, deferredOutcome };
}

export function StateAnnouncer() {
  const state = useUiState();
  const safety = useDraftSafety();
  const announce = useAnnounce();
  const previous = useRef<UiState | null>(null);
  // A no-text outcome waiting for the safety check, with the other messages
  // of the same change (spoken together so none replaces another).
  const [waiting, setWaiting] = useState<{ key: string; messages: string[] } | null>(null);

  useEffect(() => {
    const before = previous.current;
    previous.current = state;
    if (!before) return; // The initial state is shown, not announced.
    const change = describeChange(before, state);
    const key = change.deferredOutcome ? safetyKey(state) : null;
    if (key) setWaiting({ key, messages: change.messages });
    else if (change.messages.length > 0) announce(announcement(change.messages.join(" ")));
  }, [state, announce]);

  useEffect(() => {
    if (!waiting || safety.key !== waiting.key || safety.blocked === null) return;
    setWaiting(null);
    const messages = [...waiting.messages];
    if (state.lastOutcome) {
      messages.push(outcomeSentence(describeOutcome(state.lastOutcome, state.draft !== null, safety.blocked)));
    }
    if (messages.length > 0) announce(announcement(messages.join(" ")));
  }, [waiting, safety, state, announce]);

  return null;
}
