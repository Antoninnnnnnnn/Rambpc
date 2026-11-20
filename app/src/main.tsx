import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App, { AppProviders } from "./App";
import { createTauriBridge, type Bridge } from "./ipc/bridge";
import "./styles/fonts.css";
import "./styles/tokens.css";
import "./styles/base.css";

/**
 * The real bridge, except in a plain browser during `npm run dev`. The
 * `import.meta.env.DEV` guard is a compile-time constant: production builds
 * drop the branch and never emit the dev preview module.
 */
async function pickBridge(): Promise<Bridge> {
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    const preview = await import("./dev/previewBridge");
    return preview.installPreview();
  }
  return createTauriBridge();
}

void pickBridge().then((bridge) => {
  createRoot(document.getElementById("root") as HTMLElement).render(
    <StrictMode>
      <AppProviders bridge={bridge}>
        <App />
      </AppProviders>
    </StrictMode>,
  );
});
