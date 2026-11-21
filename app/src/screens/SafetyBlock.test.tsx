// A `no-text` outcome whose draft has a `blocked` part: Rambler's safety
// filter refused the passage. Also the copy disclosure near « Copier ».
import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DraftView } from "../ipc/contract";
import { completeDraft, createFakeBridge, draftWithGaps, historyEntry } from "../test/fakeBridge";
import { appReady, liveRegion, recordAnnouncements, renderApp } from "../test/render";

const BLOCKED_TITLE = "Rambler a refusé de transcrire ce passage (filtre du service).";
const BLOCKED_SENTENCE =
  "Rambler a refusé de transcrire ce passage (filtre du service). Rien n’a été inséré. Reformulez ou dictez à nouveau.";

function blockedDraft(take = 3): DraftView {
  const now = Date.now();
  return {
    meta: { take, createdAtMs: now, expiresAtMs: now + 600_000, complete: false },
    parts: [
      { status: "admitted", text: "How do I" },
      { status: "blocked", text: null },
    ],
    fullText: null,
  };
}

const normalized = (name: string) => name.replace(/\s+/g, " ");

describe("safety filter refusal", () => {
  it("replaces « Aucun texte obtenu » when the draft has a blocked part", async () => {
    const draft = blockedDraft();
    const bridge = createFakeBridge({ state: { lastOutcome: { kind: "no-text" }, draft: draft.meta }, draft, recents: [] });
    renderApp(bridge);
    await appReady();
    const notice = screen.getByTestId("outcome");
    expect(
      await within(notice).findByRole("heading", { name: (name) => normalized(name) === BLOCKED_TITLE }),
    ).toBeInTheDocument();
    expect(notice).toHaveTextContent("Rien n’a été inséré. Reformulez ou dictez à nouveau.");
    expect(notice).not.toHaveTextContent("Aucun texte obtenu");
    expect(within(notice).getByRole("button", { name: "Voir dans Récents" })).toBeInTheDocument();
    // Only the part statuses are used: the text never shows outside Récents.
    expect(document.body).not.toHaveTextContent("How do I");
  });

  it("keeps « Aucun texte obtenu » without a blocked part or without a draft", async () => {
    const draft = completeDraft("Texte.");
    const withDraft = createFakeBridge({ state: { lastOutcome: { kind: "no-text" }, draft: draft.meta }, draft });
    const first = renderApp(withDraft);
    await appReady();
    await waitFor(() => expect(withDraft.invoke).toHaveBeenCalledWith("get_draft"));
    expect(screen.getByTestId("outcome")).toHaveTextContent("Aucun texte obtenu");
    first.unmount();

    const noDraft = createFakeBridge({ state: { lastOutcome: { kind: "no-text" } } });
    renderApp(noDraft);
    await appReady();
    expect(screen.getByTestId("outcome")).toHaveTextContent("Aucun texte obtenu");
    expect(noDraft.invoke).not.toHaveBeenCalledWith("get_draft");
  });

  it("does not blame the filter for an earlier take's draft", async () => {
    const earlier = blockedDraft(2);
    const bridge = createFakeBridge({ state: { draft: earlier.meta }, draft: earlier, recents: [] });
    renderApp(bridge);
    await appReady();
    act(() => {
      bridge.pushState({ phase: { kind: "listening" }, take: 5, listeningMs: 0 });
    });
    act(() => {
      bridge.pushState({ phase: { kind: "idle" }, take: null, lastOutcome: { kind: "no-text" } });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Aucun texte obtenu."));
    expect(screen.getByTestId("outcome")).toHaveTextContent("Aucun texte obtenu");
    expect(bridge.invoke).not.toHaveBeenCalledWith("get_draft");
  });

  it("is announced in French, with the other changes, never with dictated text", async () => {
    const draft = blockedDraft();
    const bridge = createFakeBridge({ draft, recents: [] });
    renderApp(bridge);
    await appReady();
    const spoken = recordAnnouncements();
    act(() => {
      bridge.pushState({ phase: { kind: "finalizing" }, take: 3 });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Traitement en cours."));
    act(() => {
      bridge.pushState({
        phase: { kind: "idle" },
        take: null,
        draft: draft.meta,
        lastOutcome: { kind: "no-text" },
        availability: { kind: "unavailable", reason: "service" },
      });
    });
    await waitFor(() => expect(liveRegion()).toHaveTextContent(BLOCKED_SENTENCE));
    expect(liveRegion()).toHaveTextContent("Dictée indisponible.");
    spoken.stop();
    for (const text of spoken.texts) {
      expect(text).not.toContain("How do I");
      expect(text).not.toContain("Aucun texte obtenu");
    }
  });

  it("explains the blocked part next to it in Récents", async () => {
    const draft = draftWithGaps();
    const bridge = createFakeBridge({ state: { lastOutcome: { kind: "no-text" }, draft: draft.meta }, draft });
    const { user } = renderApp(bridge);
    await appReady();
    const item = await screen.findByTestId("recent-item");
    await user.click(within(item).getByRole("button", { name: /^Afficher tout/ }));
    expect(within(item).getByTestId("gap-help-blocked")).toHaveTextContent(`bloqué : ${BLOCKED_SENTENCE}`);
  });
});

describe("copy disclosure", () => {
  it("is next to Copier in Récents", async () => {
    const draft = completeDraft("Bonjour.");
    const bridge = createFakeBridge({ state: { draft: draft.meta }, draft });
    renderApp(bridge);
    await appReady();
    const item = await screen.findByTestId("recent-item");
    expect(screen.getByTestId("recents-copy-note")).toHaveTextContent(
      "Copier remplace le presse-papiers, sans passer par l’historique Win+V.",
    );
    expect(within(item).getByRole("button", { name: /^Copier/ })).toHaveAccessibleDescription(
      /^Copier remplace le presse-papiers, sans passer par l’historique Win\+V\.$/,
    );
  });

  it("is next to Copier in the history", async () => {
    const bridge = createFakeBridge({
      settings: { history: { enabled: true, retentionDays: 7 } },
      history: [historyEntry("h1", "Une dictée.")],
    });
    const { user } = renderApp(bridge);
    await appReady();
    await user.click(screen.getByRole("button", { name: "Historique" }));
    const [entry] = await screen.findAllByTestId("history-entry");
    expect(screen.getByTestId("history-copy-note")).toHaveTextContent(
      "Copier remplace le contenu actuel du presse-papiers. La copie est exclue de l’historique du presse-papiers de Windows (Win+V).",
    );
    expect(within(entry).getByRole("button", { name: /^Copier/ })).toHaveAccessibleDescription(
      /^Copier remplace le contenu actuel du presse-papiers\./,
    );
  });
});
