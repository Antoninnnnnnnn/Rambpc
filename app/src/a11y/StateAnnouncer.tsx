// Announces phase, outcome and availability changes in French. Built only
// from the text-free UiState: dictated words never reach the live region.

import { useEffect, useRef } from "react";
import type { UiState } from "../ipc/contract";
import { describeOutcome, unavailableReasonText } from "../labels/fr";
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

/** French sentences describing what changed between two states. */
export function describeChange(before: UiState, after: UiState): string[] {
  const messages: string[] = [];

  if (after.phase.kind !== before.phase.kind) {
    const phase = PHASE_ANNOUNCEMENTS[after.phase.kind];
    if (phase) messages.push(phase);
  }

  const takeEnded = before.phase.kind !== "idle" && after.phase.kind === "idle";
  if (after.lastOutcome && (takeEnded || outcomeKey(after) !== outcomeKey(before))) {
    const view = describeOutcome(after.lastOutcome, after.draft !== null);
    messages.push(view.detail ? `${view.title}. ${view.detail}` : `${view.title}.`);
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

  return messages;
}

export function StateAnnouncer() {
  const state = useUiState();
  const announce = useAnnounce();
  const previous = useRef<UiState | null>(null);

  useEffect(() => {
    const before = previous.current;
    previous.current = state;
    if (!before) return; // The initial state is shown, not announced.
    const messages = describeChange(before, state);
    if (messages.length > 0) announce(announcement(messages.join(" ")));
  }, [state, announce]);

  return null;
}
