// Global shortcut capture and validation (docs/windows/05, "Choix de la combinaison").
// Produces Tauri accelerator syntax ("Ctrl+Shift+F2") parsed by the
// global-shortcut plugin. Pure functions: no DOM, no React.

export interface KeyInput {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  /** `getModifierState("AltGraph")`: AltGr reports as Ctrl+Alt on Windows. */
  altGraph?: boolean;
}

export type ShortcutRejection =
  | "modifier-only"
  | "windows-key"
  | "f12"
  | "ctrl-alt"
  | "needs-modifier"
  | "needs-two-modifiers"
  | "reserved"
  | "unsupported-key";

export type CaptureResult =
  /** Only modifiers held so far: keep listening. */
  | { status: "pending"; modifiers: string[] }
  | { status: "ok"; accelerator: string }
  | { status: "rejected"; reason: ShortcutRejection };

const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "AltGraph", "Meta", "OS", "Super", "Hyper"]);

type MainKey = { token: string; family: "letter" | "digit" | "function" | "space" };

/**
 * Main key of the combination, layout-aware where Windows virtual keys are:
 * letters follow the character printed on the key (VK_A is the "A" key on
 * AZERTY too), digits and function keys follow the physical key.
 */
function mainKey(input: KeyInput): MainKey | null {
  if (/^[a-z]$/i.test(input.key)) return { token: input.key.toUpperCase(), family: "letter" };
  const letterCode = /^Key([A-Z])$/.exec(input.code);
  if (letterCode && /^\p{L}$/u.test(input.key)) {
    // Non-Latin layouts (a Cyrillic letter, say): fall back to the physical
    // position. Punctuation keys (AZERTY "," on KeyM) are not letters.
    return { token: letterCode[1], family: "letter" };
  }
  const digit = /^Digit([0-9])$/.exec(input.code);
  if (digit) return { token: digit[1], family: "digit" };
  const fn = /^F([1-9]|1[0-9]|2[0-4])$/.exec(input.key) ?? /^F([1-9]|1[0-9]|2[0-4])$/.exec(input.code);
  if (fn) return { token: `F${fn[1]}`, family: "function" };
  if (input.code === "Space" || input.key === " ") return { token: "Space", family: "space" };
  return null;
}

function heldModifiers(input: KeyInput): string[] {
  const mods: string[] = [];
  if (input.ctrlKey) mods.push("Ctrl");
  if (input.altKey) mods.push("Alt");
  if (input.shiftKey) mods.push("Shift");
  return mods;
}

/** Interpret one keydown while the shortcut field is capturing. */
export function captureShortcut(input: KeyInput): CaptureResult {
  if (input.metaKey || input.key === "Meta" || input.key === "OS") {
    return { status: "rejected", reason: "windows-key" };
  }
  if (input.altGraph || input.key === "AltGraph" || (input.ctrlKey && input.altKey)) {
    return { status: "rejected", reason: "ctrl-alt" };
  }
  if (MODIFIER_KEYS.has(input.key)) {
    return { status: "pending", modifiers: heldModifiers(input) };
  }
  const key = mainKey(input);
  if (!key) return { status: "rejected", reason: "unsupported-key" };
  if (key.token === "F12") return { status: "rejected", reason: "f12" };

  const mods = heldModifiers(input);
  if (key.family === "function") {
    const number = Number(key.token.slice(1));
    if (mods.length === 0 && number <= 12) return { status: "rejected", reason: "needs-modifier" };
    if (input.altKey && key.token === "F4") return { status: "rejected", reason: "reserved" };
  } else {
    // Letters, digits and Space: Shift alone types a character, Ctrl or Alt
    // alone collides with application commands (Ctrl+C, Alt+F menus...).
    if (mods.length === 0) return { status: "rejected", reason: "needs-modifier" };
    if (mods.length < 2) return { status: "rejected", reason: "needs-two-modifiers" };
  }
  return { status: "ok", accelerator: [...mods, key.token].join("+") };
}

const DISPLAY: Record<string, string> = {
  CTRL: "Ctrl",
  CONTROL: "Ctrl",
  SHIFT: "Maj",
  ALT: "Alt",
  OPTION: "Alt",
  SUPER: "Windows",
  META: "Windows",
  CMD: "Windows",
  COMMAND: "Windows",
  CMDORCTRL: "Ctrl",
  COMMANDORCONTROL: "Ctrl",
  COMMANDORCTRL: "Ctrl",
  CMDORCONTROL: "Ctrl",
  SPACE: "Espace",
  ESCAPE: "Échap",
  ESC: "Échap",
};

/** Keys of an accelerator, in French display form: ["Ctrl", "Maj", "F2"]. */
export function shortcutKeys(accelerator: string): string[] {
  return accelerator
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const upper = part.toUpperCase();
      if (DISPLAY[upper]) return DISPLAY[upper];
      const keyLetter = /^KEY([A-Z])$/.exec(upper);
      if (keyLetter) return keyLetter[1];
      const digit = /^DIGIT([0-9])$/.exec(upper);
      if (digit) return digit[1];
      return part.length === 1 ? part.toUpperCase() : part;
    });
}

/** "Ctrl+Shift+F2" -> "Ctrl+Maj+F2". */
export function formatShortcut(accelerator: string): string {
  return shortcutKeys(accelerator).join("+");
}

/** Same combination, whatever the spelling ("Control+shift+f2" == "Ctrl+Shift+F2"). */
export function sameShortcut(a: string, b: string): boolean {
  const norm = (s: string) => [...shortcutKeys(s)].map((k) => k.toUpperCase()).sort().join("+");
  return norm(a) === norm(b);
}

/** Does this keyboard event match the accelerator? (Used by the dev preview only.) */
export function matchesShortcut(accelerator: string, input: KeyInput): boolean {
  const result = captureShortcut(input);
  return result.status === "ok" && sameShortcut(result.accelerator, accelerator);
}
