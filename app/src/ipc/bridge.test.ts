import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

import { createTauriBridge, toCommandError } from "./bridge";
import { EVENT_STATE } from "./contract";

describe("createTauriBridge", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.listen.mockReset();
  });

  it("calls commands by their snake_case name with camelCase arguments", async () => {
    tauri.invoke.mockResolvedValue({ ok: true });
    const bridge = createTauriBridge();
    await bridge.invoke("finish_take", { take: 42 });
    await bridge.invoke("get_state");
    expect(tauri.invoke).toHaveBeenNthCalledWith(1, "finish_take", { take: 42 });
    expect(tauri.invoke).toHaveBeenNthCalledWith(2, "get_state", undefined);
  });

  it("passes CommandError through and drops other error text", async () => {
    const bridge = createTauriBridge();
    tauri.invoke.mockRejectedValueOnce({ code: "busy", message: "Presse-papiers occupé." });
    await expect(bridge.invoke("copy_draft")).rejects.toEqual({ code: "busy", message: "Presse-papiers occupé." });
    tauri.invoke.mockRejectedValueOnce("invalid args `settings` for command `save_settings`: secret");
    await expect(bridge.invoke("copy_draft")).rejects.toEqual({ code: "unknown", message: "" });
  });

  it("resolves undefined for an optional command the native side lacks", async () => {
    const bridge = createTauriBridge();
    tauri.invoke.mockRejectedValueOnce("command preview_render not found");
    await expect(
      bridge.invokeOptional("preview_render", { sample: "x", settings: {} as never }),
    ).resolves.toBeUndefined();
  });

  it("unwraps event payloads", async () => {
    const unlisten = vi.fn();
    tauri.listen.mockImplementation(async (_event: string, handler: (event: { payload: unknown }) => void) => {
      handler({ payload: { phase: { kind: "idle" } } });
      return unlisten;
    });
    const bridge = createTauriBridge();
    const received: unknown[] = [];
    const stop = await bridge.listen(EVENT_STATE, (payload) => received.push(payload));
    expect(tauri.listen).toHaveBeenCalledWith("sambler://state", expect.any(Function));
    expect(received).toEqual([{ phase: { kind: "idle" } }]);
    stop();
    expect(unlisten).toHaveBeenCalled();
  });
});

describe("toCommandError", () => {
  it("keeps only code and message", () => {
    expect(toCommandError({ code: "x", message: "m", extra: "secret" })).toEqual({ code: "x", message: "m" });
    expect(toCommandError(new Error("boom"))).toEqual({ code: "unknown", message: "" });
  });
});
