// The polite live region announces phases and outcomes in French and never
// the dictated words.
import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { completeDraft, createFakeBridge } from "../test/fakeBridge";
import { appReady, liveRegion, recordAnnouncements, renderApp } from "../test/render";

const SECRET = "Le code de la porte est 4521 et mon médecin s’appelle Durand";

describe("live region", () => {
  it("is a single polite status region", async () => {
    renderApp(createFakeBridge());
    await appReady();
    const region = liveRegion();
    expect(region).toHaveAttribute("aria-live", "polite");
    expect(region).toHaveAttribute("role", "status");
    expect(screen.getAllByRole("status")).toHaveLength(1);
  });

  it("announces phase changes and outcomes, never the dictated text", async () => {
    const draft = completeDraft(SECRET);
    const bridge = createFakeBridge({ recents: [] });
    const { user } = renderApp(bridge);
    await appReady();
    const spoken = recordAnnouncements();

    act(() => {
      bridge.pushState({ phase: { kind: "listening" }, take: 9, destination: "sambler", listeningMs: 0 });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Écoute : vous pouvez parler."));
    act(() => {
      bridge.pushState({ phase: { kind: "finalizing" } });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Traitement en cours."));
    bridge.recents = [draft];
    act(() => {
      bridge.pushState({
        phase: { kind: "idle" },
        take: null,
        destination: null,
        draft: draft.meta,
        recentCount: 1,
        lastOutcome: { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } },
      });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Texte dans Sambler. Comme demandé"));

    // The text is on screen in Récents, never spoken; copy and clear are.
    const item = await screen.findByTestId("recent-item");
    expect(within(item).getByTestId("recent-text")).toHaveTextContent(SECRET);
    await user.click(within(item).getByRole("button", { name: /^Copier/ }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Texte copié dans le presse-papiers."));
    await user.click(within(item).getByRole("button", { name: /^Effacer/ }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Texte effacé."));

    act(() => {
      bridge.pushState({ availability: { kind: "unavailable", reason: "network" } });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Dictée indisponible."));

    spoken.stop();
    expect(spoken.texts.length).toBeGreaterThanOrEqual(6);
    for (const text of spoken.texts) {
      expect(text).not.toContain("4521");
      expect(text).not.toContain("Durand");
      expect(text).not.toContain("code de la porte");
    }
  });

  it("does not announce the initial state", async () => {
    renderApp(createFakeBridge({ state: { lastOutcome: { kind: "inserted" } } }));
    await appReady();
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(liveRegion()).toHaveTextContent("");
  });
});
