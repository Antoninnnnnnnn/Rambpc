import { useMemo } from "react";
import { useBridge } from "../ipc/bridge";
import type { TakeId } from "../ipc/contract";
import { useUiStore } from "./ui";

/** Commands that return a fresh `UiState`, applied through the UI store. */
export function useUiCommands() {
  const bridge = useBridge();
  const { run } = useUiStore();
  return useMemo(
    () => ({
      dictateInSambler: () => run(() => bridge.invoke("dictate_in_sambler")),
      retryConnection: () => run(() => bridge.invoke("retry_connection")),
      finishTake: (take: TakeId) => run(() => bridge.invoke("finish_take", { take })),
      cancelTake: (take: TakeId) => run(() => bridge.invoke("cancel_take", { take })),
      clearDraft: () => run(() => bridge.invoke("clear_draft")),
      setConsent: (enabled: boolean) => run(() => bridge.invoke("set_consent", { enabled })),
      openMicPrivacySettings: () => bridge.invoke("open_mic_privacy_settings"),
    }),
    [bridge, run],
  );
}
