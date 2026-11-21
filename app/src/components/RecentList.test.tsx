// « Récents » on Accueil: the latest results, asked with get_recent_drafts,
// each with Copier (copy_draft { take }), Afficher tout and Effacer
// (clear_draft { take }). The text is shown, never announced.
import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DraftView } from "../ipc/contract";
import { completeDraft, createFakeBridge, draftWithGaps } from "../test/fakeBridge";
import { appReady, axeViolations, liveRegion, recordAnnouncements, renderApp } from "../test/render";

const now = Date.now();
const NEWEST = completeDraft("Rendez-vous jeudi à 14 h avec le docteur Martin.", now, 12);
const GAPS = draftWithGaps(now - 3 * 60_000, 11);
const OLDEST = completeDraft("Penser au devis.", now - 7 * 60_000, 10);

async function renderRecents(recents: DraftView[] = [NEWEST, GAPS, OLDEST]) {
  const bridge = createFakeBridge({ recents, state: { draft: recents[0]?.meta ?? null } });
  const view = renderApp(bridge);
  await appReady();
  return { bridge, ...view };
}

const items = () => screen.getAllByTestId("recent-item");

describe("Récents", () => {
  it("lists the recent results newest first, as plain text", async () => {
    const { bridge } = await renderRecents([NEWEST, completeDraft("<b>Gras</b> ?", now - 60_000, 8)]);
    expect(await screen.findAllByTestId("recent-item")).toHaveLength(2);
    expect(bridge.invoke).toHaveBeenCalledWith("get_recent_drafts");
    expect(within(items()[0]).getByTestId("recent-text")).toHaveTextContent("Rendez-vous jeudi à 14 h");
    expect(within(items()[0]).getByRole("heading", { level: 3 })).toHaveTextContent("À l’instant");
    expect(items()[0]).toHaveTextContent(/Effacé dans 10 min/);
    // A short, complete text is shown whole: nothing to expand.
    expect(within(items()[0]).queryByRole("button", { name: /^Afficher tout/ })).not.toBeInTheDocument();
    expect(items()[1]).toHaveTextContent("<b>Gras</b> ?");
    expect(items()[1].querySelector("b")).toBeNull();
  });

  it("marks the gaps in place and explains them when expanded", async () => {
    const { user } = await renderRecents();
    const item = (await screen.findAllByTestId("recent-item"))[1];
    expect(item).toHaveTextContent("Incomplet");
    const text = within(item).getByTestId("recent-text");
    expect(text.querySelector('[data-gap="missing"]')).toHaveTextContent("[passage manquant]");
    expect(text.querySelector('[data-gap="needs-review"]')).toHaveTextContent("[à revoir]");
    expect(text.querySelector('[data-gap="blocked"]')).toHaveTextContent("[bloqué]");
    expect(text.querySelector('[data-gap="no-text"]')).toHaveTextContent("[aucun texte obtenu]");
    expect(within(item).queryByRole("list", { name: "Signification des repères" })).not.toBeInTheDocument();

    const toggle = within(item).getByRole("button", { name: /^Afficher tout/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(within(item).getByRole("button", { name: /^Réduire/ })).toHaveAttribute("aria-expanded", "true");
    expect(within(item).getByRole("list", { name: "Signification des repères" })).toBeInTheDocument();
    expect(within(item).getByTestId("gap-help-missing")).toHaveTextContent("perdu lors d’une coupure");
    expect(item).toHaveTextContent(/Texte incomplet/);
  });

  it("copies one result by its take, with the copy disclosure", async () => {
    const { bridge, user } = await renderRecents();
    const second = (await screen.findAllByTestId("recent-item"))[1];
    const copy = within(second).getByRole("button", { name: /^Copier/ });
    expect(copy).toHaveAccessibleDescription(
      /Copier remplace le presse-papiers, sans passer par l’historique Win\+V\. Les passages manquants ne sont pas copiés\./,
    );
    await user.click(copy);
    expect(bridge.invoke).toHaveBeenCalledWith("copy_draft", { take: 11 });
    expect(await within(second).findByTestId("copy-feedback")).toHaveTextContent("Texte copié.");
  });

  it.each([
    ["clipboard-busy", "Le presse-papiers est occupé par une autre application. Réessayez dans un instant."],
    [
      "clipboard-exclusion-failed",
      "Copie annulée : Windows n’a pas pu exclure ce texte de l’historique du presse-papiers (Win+V). Le texte reste dans Sambler.",
    ],
    ["clipboard-other", "La copie a échoué. Le texte reste dans Sambler."],
    ["no-draft", "Il n’y a plus de texte à copier."],
  ])("reports the clipboard error %s in French", async (code, message) => {
    const { bridge, user } = await renderRecents([NEWEST]);
    bridge.handlers.copy_draft = () => {
      throw { code, message: "" };
    };
    await user.click(within(await screen.findByTestId("recent-item")).getByRole("button", { name: /^Copier/ }));
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.queryByTestId("copy-feedback")).not.toBeInTheDocument();
  });

  it("clears one result by its take and keeps the keyboard nearby", async () => {
    const { bridge, user } = await renderRecents();
    const first = (await screen.findAllByTestId("recent-item"))[0];
    await user.click(within(first).getByRole("button", { name: /^Effacer/ }));
    expect(bridge.invoke).toHaveBeenCalledWith("clear_draft", { take: 12 });
    await waitFor(() => expect(items()).toHaveLength(2));
    expect(document.body).not.toHaveTextContent("docteur Martin");
    await waitFor(() => expect(within(items()[0]).getByRole("heading", { level: 3 })).toHaveFocus());
  });

  it("asks before clearing every result", async () => {
    const { bridge, user } = await renderRecents();
    await screen.findAllByTestId("recent-item");
    await user.click(screen.getByRole("button", { name: "Tout effacer les textes récents" }));
    expect(screen.getByText("Effacer les 3 textes récents ?")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(bridge.invoke).not.toHaveBeenCalledWith("clear_draft", expect.anything());
    expect(bridge.invoke).not.toHaveBeenCalledWith("clear_draft");

    await user.click(screen.getByRole("button", { name: "Tout effacer les textes récents" }));
    await user.click(screen.getByRole("button", { name: "Effacer" }));
    expect(bridge.invoke).toHaveBeenCalledWith("clear_draft");
    expect(await screen.findByTestId("recents-empty")).toHaveTextContent("Aucun texte récent");
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Textes récents effacés."));
  });

  it("refetches when a new result arrives and drops the text when results go away", async () => {
    const { bridge } = await renderRecents([OLDEST]);
    await screen.findByTestId("recent-item");
    bridge.recents = [NEWEST, OLDEST];
    act(() => {
      bridge.pushState({ recentCount: 2, draft: NEWEST.meta, lastOutcome: { kind: "inserted" } });
    });
    await waitFor(() => expect(items()).toHaveLength(2));
    bridge.recents = [];
    act(() => {
      bridge.pushState({ recentCount: 0, draft: null });
    });
    await waitFor(() => expect(document.body).not.toHaveTextContent("Penser au devis."));
    expect(screen.getByTestId("recents-empty")).toBeInTheDocument();
  });

  it("drops a result at its expiry", async () => {
    const soon = completeDraft("Bientôt expiré.", Date.now() - 9 * 60_000, 5);
    soon.meta.expiresAtMs = Date.now() + 1200;
    await renderRecents([soon]);
    expect(await screen.findByTestId("recent-item")).toHaveTextContent(/Effacé dans \d s/);
    await waitFor(() => expect(document.body).not.toHaveTextContent("Bientôt expiré."), { timeout: 3000 });
  });

  it("explains the empty list with the retention delay", async () => {
    const bridge = createFakeBridge({ settings: { draftMinutes: 5 } });
    renderApp(bridge);
    await appReady();
    expect(await screen.findByTestId("recents-empty")).toHaveTextContent(
      "Après chaque dictée, le texte s’affiche ici pendant 5 minutes, prêt à être copié.",
    );
    expect(bridge.invoke).not.toHaveBeenCalledWith("get_recent_drafts");
  });

  it("never announces the text", async () => {
    const { user } = await renderRecents();
    const spoken = recordAnnouncements();
    const first = (await screen.findAllByTestId("recent-item"))[0];
    await user.click(within(first).getByRole("button", { name: /^Copier/ }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Texte copié dans le presse-papiers."));
    await user.click(within(first).getByRole("button", { name: /^Effacer/ }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Texte effacé."));
    spoken.stop();
    for (const text of spoken.texts) {
      expect(text).not.toContain("Martin");
      expect(text).not.toContain("jeudi");
    }
  });

  it("has no detectable accessibility violations, expanded", async () => {
    const { user } = await renderRecents();
    const item = (await screen.findAllByTestId("recent-item"))[1];
    await user.click(within(item).getByRole("button", { name: /^Afficher tout/ }));
    expect(await axeViolations()).toEqual([]);
  });
});
