// Test-only rendering helpers.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { expect } from "vitest";
import App, { AppProviders } from "../App";
import type { FakeBridge } from "./fakeBridge";

export function renderApp(bridge: FakeBridge) {
  const user = userEvent.setup();
  const result = render(
    <AppProviders bridge={bridge}>
      <App />
    </AppProviders>,
  );
  return { user, ...result };
}

/** Wait until the app left its loading screen. */
export async function appReady() {
  await waitFor(() => expect(screen.queryByText("Chargement…")).not.toBeInTheDocument());
}

export function liveRegion(): HTMLElement {
  return screen.getByTestId("live-region");
}

/** Record every text the live region ever held (to check what is spoken). */
export function recordAnnouncements(): { texts: string[]; stop: () => void } {
  const texts: string[] = [];
  const region = liveRegion();
  const observer = new MutationObserver(() => {
    if (region.textContent) texts.push(region.textContent);
  });
  observer.observe(region, { childList: true, characterData: true, subtree: true });
  return { texts, stop: () => observer.disconnect() };
}

/** axe-core accessibility check (color contrast is not computable in jsdom). */
export async function axeViolations(container: Element = document.body) {
  const results = await axe.run(container, {
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations.map((violation) => ({
    id: violation.id,
    nodes: violation.nodes.map((node) => node.html).slice(0, 3),
  }));
}
