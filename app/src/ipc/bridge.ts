// Thin typed wrapper over Tauri's `invoke` / `listen`, handed to components
// through a React context so tests (and the dev-only browser preview) can
// inject another implementation. This module never simulates anything.

import { createContext, createElement, useContext, type ReactNode } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type {
  CommandError,
  CommandMap,
  CommandName,
  EventMap,
  EventName,
  OptionalCommandMap,
  OptionalCommandName,
} from "./contract";

type ArgsOf<M, C extends keyof M> = M[C] extends { args: void }
  ? []
  : M[C] extends { args: infer A }
    ? [A]
    : never;

export type Unlisten = () => void;

export interface Bridge {
  /** Rejects with a `CommandError` (normalized). */
  invoke<C extends CommandName>(
    command: C,
    ...args: ArgsOf<CommandMap, C>
  ): Promise<CommandMap[C]["result"]>;
  /**
   * For optional capabilities (see `OptionalCommandMap`). Resolves `undefined`
   * when the native side does not register the command.
   */
  invokeOptional<C extends OptionalCommandName>(
    command: C,
    ...args: ArgsOf<OptionalCommandMap, C>
  ): Promise<OptionalCommandMap[C]["result"] | undefined>;
  listen<E extends EventName>(event: E, handler: (payload: EventMap[E]) => void): Promise<Unlisten>;
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

function isMissingCommand(value: unknown): boolean {
  const text = typeof value === "string" ? value : value instanceof Error ? value.message : "";
  return /not found|not allowed|unknown command/i.test(text);
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
    async invokeOptional(command, ...args) {
      try {
        return await tauriInvoke(command, args[0] as Record<string, unknown> | undefined);
      } catch (error) {
        if (isMissingCommand(error)) return undefined;
        throw toCommandError(error);
      }
    },
    async listen(event, handler) {
      return tauriListen(event, (message) => handler(message.payload as EventMap[typeof event]));
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
