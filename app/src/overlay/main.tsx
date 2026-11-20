// Entry of the floating dictation bar (overlay.html): a transparent,
// click-through, never-focused window that Rust shows at the bottom of the
// active screen. Its capability allows events only: it listens to
// `sambler://state` and `sambler://level`, calls no command and never
// receives dictated text. No framework, to keep it light.

import { listen } from "@tauri-apps/api/event";
import { EVENT_LEVEL, EVENT_STATE, type UiState } from "../ipc/contract";
import { mountOverlay, type OverlaySource } from "./bar";
import "../styles/fonts.css";
import "./overlay.css";

const tauriSource: OverlaySource = {
  onState: (handler) => void listen<UiState>(EVENT_STATE, (event) => handler(event.payload)),
  onLevel: (handler) => void listen<number>(EVENT_LEVEL, (event) => handler(event.payload)),
};

async function start() {
  const root = document.getElementById("bar");
  if (!root) return;
  // DEV PREVIEW ONLY (plain browser on `npm run dev`): `import.meta.env.DEV`
  // is a compile-time constant, so production builds drop this branch.
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    const preview = await import("../dev/overlayPreview");
    mountOverlay(root, preview.installOverlayPreview());
    return;
  }
  mountOverlay(root, tauriSource);
}

void start();
