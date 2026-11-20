// The polite live region announces phases and outcomes in French and never
// the dictated words.
import { act, screen, waitFor } from "@testing-library/react";
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
    const bridge = createFakeBridge({ draft });
    const { user } = renderApp(bridge);
    await appReady();
    const spoken = recordAnnouncements();

    act(() => {
      bridge.pushState({ phase: { kind: "preparing" }, take: 4, destination: "sambler" });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Préparation du micro."));
    act(() => {
      bridge.pushState({ phase: { kind: "listening" }, listeningMs: 0 });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Écoute : vous pouvez parler."));
    act(() => {
      bridge.pushState({ phase: { kind: "finalizing" } });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Traitement en cours."));
    act(() => {
      bridge.pushState({
        phase: { kind: "idle" },
        take: null,
        destination: null,
        draft: draft.meta,
        lastOutcome: { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } },
      });
    });
    await waitFor(() =>
      expect(liveRegion()).toHaveTextContent("Texte disponible. Vous avez choisi de garder le texte dans Sambler."),
    );

    // Display, copy and clear the draft: the text is on screen, not spoken.
    await user.click(screen.getByRole("button", { name: /Brouillon/ }));
    expect(await screen.findByTestId("draft-text")).toHaveTextContent(SECRET);
    await user.click(screen.getByRole("button", { name: "Copier" }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Texte copié dans le presse-papiers."));
    await user.click(screen.getByRole("button", { name: "Effacer" }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Brouillon effacé."));

    act(() => {
      bridge.pushState({ availability: { kind: "unavailable", reason: "network" } });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Dictée indisponible."));

    spoken.stop();
    expect(spoken.texts.length).toBeGreaterThanOrEqual(7);
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
