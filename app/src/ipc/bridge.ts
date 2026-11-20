// Thin typed wrapper over Tauri's `invoke` / `listen` and the frameless main
// window's own controls, handed to components through a React context so
// tests (and the dev-only browser preview) can inject another implementation.
// This module never simulates anything.

import { createContext, createElement, useContext, type ReactNode } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { CommandError, CommandMap, CommandName, EventMap, EventName } from "./contract";

/** No arguments, optional arguments (`copy_draft`, `clear_draft`) or required ones. */
type ArgsOf<C extends CommandName> = CommandMap[C] extends { args: void }
  ? []
  : CommandMap[C] extends { args: infer A }
    ? object extends A
      ? [A?]
      : [A]
    : never;

export type Unlisten = () => void;

/**
 * The main window draws its own title bar (`decorations: false`). Allowed by
 * capabilities/main.json: start-dragging (drag region), minimize,
 * toggle-maximize, close (hides to the tray natively) and is-maximized.
 */
export interface WindowControls {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  isMaximized(): Promise<boolean>;
  /** Called after every resize (maximize and restore included). */
  onResized(handler: () => void): Promise<Unlisten>;
}

export interface Bridge {
  /** Rejects with a `CommandError` (normalized). */
  invoke<C extends CommandName>(command: C, ...args: ArgsOf<C>): Promise<CommandMap[C]["result"]>;
  listen<E extends EventName>(event: E, handler: (payload: EventMap[E]) => void): Promise<Unlisten>;
  appWindow: WindowControls;
}

export function isCommandError(value: unknown): value is CommandError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as CommandError).code === "string" &&
    typeof (value as CommandError).message === "string"
  );
}

/**
 * Normalize any rejection to a `CommandError`. Non-contract errors (plain
 * strings from the IPC layer) are not displayed as-is: their text is dropped.
 */
export function toCommandError(value: unknown): CommandError {
  if (isCommandError(value)) return { code: value.code, message: value.message };
  return { code: "unknown", message: "" };
}

export function createTauriBridge(): Bridge {
  return {
    async invoke(command, ...args) {
      try {
        return await tauriInvoke(command, args[0] as Record<string, unknown> | undefined);
      } catch (error) {
        throw toCommandError(error);
      }
    },
    async listen(event, handler) {
      return tauriListen(event, (message) => handler(message.payload as EventMap[typeof event]));
    },
    appWindow: {
      minimize: () => getCurrentWindow().minimize(),
      toggleMaximize: () => getCurrentWindow().toggleMaximize(),
      close: () => getCurrentWindow().close(),
      isMaximized: () => getCurrentWindow().isMaximized(),
      onResized: (handler) => getCurrentWindow().onResized(() => handler()),
    },
  } as Bridge;
}

const BridgeContext = createContext<Bridge | null>(null);

export function BridgeProvider({ bridge, children }: { bridge: Bridge; children: ReactNode }) {
  return createElement(BridgeContext.Provider, { value: bridge }, children);
}

export function useBridge(): Bridge {
  const bridge = useContext(BridgeContext);
  if (!bridge) throw new Error("useBridge must be used inside <BridgeProvider>");
  return bridge;
}
