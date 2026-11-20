import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DraftView } from "../ipc/contract";
import { completeDraft, createFakeBridge, draftWithGaps } from "../test/fakeBridge";
import { appReady, axeViolations, renderApp } from "../test/render";

async function openDraft(draft: DraftView | null) {
  const bridge = createFakeBridge({
    state: {
      draft: draft?.meta ?? null,
      lastOutcome: draft ? { kind: "kept-in-sambler", reason: { kind: "incomplete" } } : null,
    },
    draft,
  });
  const view = renderApp(bridge);
  await appReady();
  await view.user.click(screen.getByRole("button", { name: /Brouillon/ }));
  return { bridge, ...view };
}

describe("Brouillon", () => {
  it("fetches the text only when the draft view displays it", async () => {
    const draft = draftWithGaps();
    const bridge = createFakeBridge({ state: { draft: draft.meta }, draft });
    const { user } = renderApp(bridge);
    await appReady();
    expect(bridge.invoke).not.toHaveBeenCalledWith("get_draft");
    await user.click(screen.getByRole("button", { name: /Brouillon/ }));
    await screen.findByTestId("draft-text");
    expect(bridge.invoke).toHaveBeenCalledWith("get_draft");
  });

  it("shows admitted text and clearly marked gaps", async () => {
    await openDraft(draftWithGaps());
    const text = await screen.findByTestId("draft-text");
    expect(text).toHaveTextContent("Je ne pourrai pas venir jeudi,");
    expect(text).toHaveTextContent("mais vendredi me convient.");
    expect(text).toHaveTextContent("vers dix heures");
    expect(text.querySelector('[data-gap="missing"]')).toHaveTextContent("[passage manquant]");
    expect(text.querySelector('[data-gap="needs-review"]')).toHaveTextContent("[à revoir]");
    expect(text.querySelector('[data-gap="blocked"]')).toHaveTextContent("[bloqué]");
    expect(text.querySelector('[data-gap="no-text"]')).toHaveTextContent("[aucun texte obtenu]");
    expect(screen.getByText(/Texte incomplet/)).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Signification des repères" })).toBeInTheDocument();
  });

  it("shows the full text of a complete draft as plain, selectable text", async () => {
    await openDraft(completeDraft("<b>Rendez-vous</b> jeudi à 14 h."));
    const text = await screen.findByTestId("draft-text");
    expect(text).toHaveTextContent("<b>Rendez-vous</b> jeudi à 14 h.");
    expect(text.querySelector("b")).toBeNull();
    expect(screen.getByText("Texte complet.")).toBeInTheDocument();
  });

  it("counts down to expiry", async () => {
    await openDraft(draftWithGaps());
    const panel = screen.getByTestId("draft-panel");
    expect(within(panel).getByText(/Effacé automatiquement dans/)).toHaveTextContent(/9 min (29|30) s/);
  });

  it("copies with copy_draft", async () => {
    const { bridge, user } = await openDraft(completeDraft("Bonjour."));
    await screen.findByTestId("draft-text");
    await user.click(screen.getByRole("button", { name: "Copier" }));
    expect(bridge.invoke).toHaveBeenCalledWith("copy_draft");
    expect(await screen.findByTestId("copy-feedback")).toHaveTextContent("Texte copié.");
  });

  it.each([
    ["busy", "Le presse-papiers est occupé par une autre application. Réessayez dans un instant."],
    [
      "exclusion-failed",
      "Copie annulée : Windows n’a pas pu exclure ce texte de l’historique du presse-papiers (Win+V). Le texte reste dans Sambler.",
    ],
    ["other", "La copie a échoué. Le texte reste dans Sambler."],
  ])("reports the clipboard error %s in French", async (code, message) => {
    const { bridge, user } = await openDraft(completeDraft("Bonjour."));
    bridge.handlers.copy_draft = () => {
      throw { code, message: "" };
    };
    await user.click(screen.getByRole("button", { name: "Copier" }));
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.queryByTestId("copy-feedback")).not.toBeInTheDocument();
  });

  it("clears with clear_draft and drops the text", async () => {
    const { bridge, user } = await openDraft(completeDraft("Texte à effacer."));
    await screen.findByTestId("draft-text");
    await user.click(screen.getByRole("button", { name: "Effacer" }));
    expect(bridge.invoke).toHaveBeenCalledWith("clear_draft");
    expect(await screen.findByTestId("draft-empty")).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("Texte à effacer.");
  });

  it("drops the text when the draft disappears from the state", async () => {
    const { bridge } = await openDraft(completeDraft("Disparaît bientôt."));
    await screen.findByTestId("draft-text");
    act(() => {
      bridge.pushState({ draft: null });
    });
    await waitFor(() => expect(document.body).not.toHaveTextContent("Disparaît bientôt."));
  });

  it("shows an empty state without draft", async () => {
    await openDraft(null);
    expect(screen.getByRole("heading", { name: "Aucun texte en attente" })).toBeInTheDocument();
  });

  it("has no detectable accessibility violations", async () => {
    await openDraft(draftWithGaps());
    await screen.findByTestId("draft-text");
    expect(await axeViolations()).toEqual([]);
  });
});
