import { describe, expect, it } from "vitest";
import { captureShortcut, formatShortcut, sameShortcut, shortcutKeys, type KeyInput } from "./shortcut";

const key = (k: string, code: string, mods: Partial<KeyInput> = {}): KeyInput => ({
  key: k,
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

describe("captureShortcut", () => {
  it("produces Tauri accelerator syntax", () => {
    expect(captureShortcut(key("F2", "F2", { ctrlKey: true, shiftKey: true }))).toEqual({
      status: "ok",
      accelerator: "Ctrl+Shift+F2",
    });
    expect(captureShortcut(key("D", "KeyD", { ctrlKey: true, shiftKey: true }))).toEqual({
      status: "ok",
      accelerator: "Ctrl+Shift+D",
    });
    expect(captureShortcut(key(" ", "Space", { altKey: true, shiftKey: true }))).toEqual({
      status: "ok",
      accelerator: "Alt+Shift+Space",
    });
  });

  it("keeps listening while only modifiers are held", () => {
    expect(captureShortcut(key("Control", "ControlLeft", { ctrlKey: true }))).toEqual({
      status: "pending",
      modifiers: ["Ctrl"],
    });
    expect(captureShortcut(key("Shift", "ShiftLeft", { ctrlKey: true, shiftKey: true }))).toEqual({
      status: "pending",
      modifiers: ["Ctrl", "Shift"],
    });
  });

  it.each([
    ["Windows key", key("Meta", "MetaLeft", { metaKey: true }), "windows-key"],
    ["Win+D", key("d", "KeyD", { metaKey: true, shiftKey: true }), "windows-key"],
    ["F12", key("F12", "F12", { ctrlKey: true }), "f12"],
    ["Ctrl+Alt", key("k", "KeyK", { ctrlKey: true, altKey: true }), "ctrl-alt"],
    ["AltGr", key("€", "KeyE", { altGraph: true }), "ctrl-alt"],
    ["bare F2", key("F2", "F2"), "needs-modifier"],
    ["bare letter", key("a", "KeyA"), "needs-modifier"],
    ["Ctrl+letter", key("c", "KeyC", { ctrlKey: true }), "needs-two-modifiers"],
    ["Shift+letter", key("A", "KeyA", { shiftKey: true }), "needs-two-modifiers"],
    ["Alt+F4", key("F4", "F4", { altKey: true }), "reserved"],
    ["Enter", key("Enter", "Enter", { ctrlKey: true, shiftKey: true }), "unsupported-key"],
    ["punctuation", key(",", "KeyM", { ctrlKey: true, shiftKey: true }), "unsupported-key"],
  ])("rejects %s", (_label, input, reason) => {
    expect(captureShortcut(input)).toEqual({ status: "rejected", reason });
  });

  it("accepts F13 to F24 alone", () => {
    expect(captureShortcut(key("F13", "F13"))).toEqual({ status: "ok", accelerator: "F13" });
  });

  it("follows the printed letter on AZERTY and the physical key for digits", () => {
    // AZERTY: the key printed "A" sits where QWERTY has Q.
    expect(captureShortcut(key("A", "KeyQ", { ctrlKey: true, shiftKey: true }))).toEqual({
      status: "ok",
      accelerator: "Ctrl+Shift+A",
    });
    // AZERTY digit row produces "&" without Shift: the virtual key is still 1.
    expect(captureShortcut(key("&", "Digit1", { ctrlKey: true, shiftKey: true }))).toEqual({
      status: "ok",
      accelerator: "Ctrl+Shift+1",
    });
  });
});

describe("display", () => {
  it("formats in French", () => {
    expect(formatShortcut("Ctrl+Shift+F2")).toBe("Ctrl+Maj+F2");
    expect(shortcutKeys("Alt+Shift+Space")).toEqual(["Alt", "Maj", "Espace"]);
    expect(shortcutKeys("CommandOrControl+Shift+KeyD")).toEqual(["Ctrl", "Maj", "D"]);
  });

  it("compares spellings", () => {
    expect(sameShortcut("control+shift+f2", "Ctrl+Shift+F2")).toBe(true);
    expect(sameShortcut("Ctrl+Shift+F3", "Ctrl+Shift+F2")).toBe(false);
  });
});
