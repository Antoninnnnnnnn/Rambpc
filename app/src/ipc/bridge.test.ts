import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  window: {
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
    isMaximized: vi.fn(),
    onResized: vi.fn(),
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => tauri.window }));

import { createTauriBridge, toCommandError } from "./bridge";
import { defaultSettings, EVENT_STATE } from "./contract";

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

  it("passes milestone 2 arguments with their contract names", async () => {
    tauri.invoke.mockResolvedValue(undefined);
    const bridge = createTauriBridge();
    await bridge.invoke("get_history", { query: { search: "réunion", limit: 50, cursor: null } });
    await bridge.invoke("clear_history", { disable: true });
    await bridge.invoke("save_personal_rules", { rules: { lexicon: [], expressions: [] } });
    await bridge.invoke("set_launch_at_startup", { enabled: true });
    expect(tauri.invoke.mock.calls).toEqual([
      ["get_history", { query: { search: "réunion", limit: 50, cursor: null } }],
      ["clear_history", { disable: true }],
      ["save_personal_rules", { rules: { lexicon: [], expressions: [] } }],
      ["set_launch_at_startup", { enabled: true }],
    ]);
  });

  it("passes milestone 3 commands, with an optional take", async () => {
    tauri.invoke.mockResolvedValue(undefined);
    const bridge = createTauriBridge();
    await bridge.invoke("get_recent_drafts");
    await bridge.invoke("copy_draft", { take: 7 });
    await bridge.invoke("copy_draft");
    await bridge.invoke("clear_draft", { take: 7 });
    await bridge.invoke("clear_draft");
    await bridge.invoke("get_stats");
    expect(tauri.invoke.mock.calls).toEqual([
      ["get_recent_drafts", undefined],
      ["copy_draft", { take: 7 }],
      ["copy_draft", undefined],
      ["clear_draft", { take: 7 }],
      ["clear_draft", undefined],
      ["get_stats", undefined],
    ]);
  });

  it("sends the insertion method with the settings", async () => {
    tauri.invoke.mockResolvedValue(undefined);
    const bridge = createTauriBridge();
    await bridge.invoke("save_settings", { settings: { ...defaultSettings(), insertion: "typing" } });
    expect(tauri.invoke).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ insertion: "typing" }),
    });
    expect(defaultSettings().insertion).toBe("paste");
    expect(defaultSettings().insertionTarget).toBe("original-field");
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

  it("drives the frameless main window through its allowed controls only", async () => {
    tauri.window.isMaximized.mockResolvedValue(true);
    tauri.window.onResized.mockResolvedValue(() => {});
    const bridge = createTauriBridge();
    await bridge.appWindow.minimize();
    await bridge.appWindow.toggleMaximize();
    await bridge.appWindow.close();
    expect(await bridge.appWindow.isMaximized()).toBe(true);
    await bridge.appWindow.onResized(() => {});
    expect(tauri.window.minimize).toHaveBeenCalledTimes(1);
    expect(tauri.window.toggleMaximize).toHaveBeenCalledTimes(1);
    expect(tauri.window.close).toHaveBeenCalledTimes(1);
    expect(tauri.window.onResized).toHaveBeenCalledTimes(1);
  });
});

describe("toCommandError", () => {
  it("keeps only code and message", () => {
    expect(toCommandError({ code: "x", message: "m", extra: "secret" })).toEqual({ code: "x", message: "m" });
    expect(toCommandError(new Error("boom"))).toEqual({ code: "unknown", message: "" });
  });
});
