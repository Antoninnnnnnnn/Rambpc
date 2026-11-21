import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { HistoryEntry, Settings } from "../ipc/contract";
import { formatEntryDate } from "../labels/history";
import { createFakeBridge, historyEntry, type FakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, liveRegion, recordAnnouncements, renderApp } from "../test/render";

const ON: Partial<Settings> = { history: { enabled: true, retentionDays: 7 } };

async function openHistory(options: Parameters<typeof createFakeBridge>[0] = {}) {
  const bridge = createFakeBridge(options);
  const view = renderApp(bridge);
  await appReady();
  await view.user.click(screen.getByRole("button", { name: "Historique" }));
  await screen.findByRole("region", { name: "Conservation" });
  return { bridge, ...view };
}

const historyCalls = (bridge: FakeBridge) => bridge.invoke.mock.calls.filter(([command]) => command === "get_history");

const ENTRIES: HistoryEntry[] = [
  historyEntry("h1", "Rendez-vous jeudi à 14 h avec le docteur Martin.", 5),
  historyEntry("h2", "Réunion budget\nPoints : salaires, loyers.", 60),
  historyEntry("h3", null, 120),
];

describe("Historique", () => {
  it("is off by default and explains what would be kept", async () => {
    const { bridge } = await openHistory();
    const toggle = screen.getByRole("switch", { name: "Conserver mes dictées sur ce PC" });
    expect(toggle).not.toBeChecked();
    expect(toggle).toHaveAccessibleDescription(
      /Texte seulement, jamais le son\. Les dictées restent sur ce PC, chiffrées pour votre compte Windows/,
    );
    expect(screen.getByRole("radio", { name: "7 jours" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "1 jour" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "30 jours" })).toBeInTheDocument();
    expect(screen.getByTestId("history-off")).toHaveTextContent("L’historique est désactivé");
    expect(historyCalls(bridge)).toHaveLength(0);
  });

  it("turns on with save_settings, then lists the entries", async () => {
    const { bridge, user } = await openHistory({ history: ENTRIES });
    bridge.history = [];
    await user.click(screen.getByRole("switch", { name: "Conserver mes dictées sur ce PC" }));
    expect(bridge.invoke).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ history: { enabled: true, retentionDays: 7 } }),
    });
    expect(await screen.findByTestId("history-empty")).toHaveTextContent("Aucune dictée enregistrée pour le moment.");
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Historique activé."));

    await user.click(screen.getByRole("radio", { name: "30 jours" }));
    expect(bridge.invoke).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ history: { enabled: true, retentionDays: 30 } }),
    });
  });

  it("shows entries as plain selectable text, unreadable ones as such", async () => {
    await openHistory({ settings: ON, history: ENTRIES });
    const items = await screen.findAllByTestId("history-entry");
    expect(items).toHaveLength(3);
    expect(within(items[0]).getByRole("heading", { level: 3 })).toBeInTheDocument();
    expect(items[0]).toHaveTextContent("Français · Naturel · Expire dans 6 jours");
    expect(within(items[1]).getByTestId("history-text").textContent).toBe("Réunion budget\nPoints : salaires, loyers.");
    expect(items[2]).toHaveTextContent("Texte illisible sous ce compte Windows");
    expect(within(items[2]).getByRole("button", { name: /^Copier/ })).toBeDisabled();
  });

  it("names no language for an automatic dictation", async () => {
    const entry = { ...historyEntry("auto", "Please review the PR, merci.", 5), language: "auto" as const };
    await openHistory({ settings: ON, history: [entry] });
    const [item] = await screen.findAllByTestId("history-entry");
    expect(within(item).getByText(/Expire dans/)).toHaveTextContent(/^Naturel · Expire dans 6 jours$/);
    expect(within(item).getByTestId("history-text")).not.toHaveAttribute("lang");
  });

  it("copies and deletes one entry", async () => {
    const { bridge, user } = await openHistory({ settings: ON, history: ENTRIES });
    const [first, second] = await screen.findAllByTestId("history-entry");
    await user.click(within(first).getByRole("button", { name: /^Copier/ }));
    expect(bridge.invoke).toHaveBeenCalledWith("copy_history_entry", { id: "h1" });
    expect(await within(first).findByText("Copiée.")).toBeInTheDocument();

    bridge.handlers.copy_history_entry = () => {
      throw { code: "clipboard-busy", message: "" };
    };
    await user.click(within(second).getByRole("button", { name: /^Copier/ }));
    expect(
      await within(second).findByText("Le presse-papiers est occupé par une autre application. Réessayez dans un instant."),
    ).toBeInTheDocument();

    await user.click(within(first).getByRole("button", { name: /^Supprimer/ }));
    expect(bridge.invoke).toHaveBeenCalledWith("delete_history_entry", { id: "h1" });
    await waitFor(() => expect(screen.getAllByTestId("history-entry")).toHaveLength(2));
    await waitFor(() =>
      expect(within(screen.getAllByTestId("history-entry")[0]).getByRole("heading", { level: 3 })).toHaveFocus(),
    );
    expect(screen.getByTestId("history-count")).toHaveTextContent("2 dictées enregistrées.");
  });

  it("asks before erasing everything", async () => {
    const { bridge, user } = await openHistory({ settings: ON, history: ENTRIES });
    await screen.findAllByTestId("history-entry");
    await user.click(screen.getByRole("button", { name: "Tout effacer" }));
    expect(screen.getByText("Effacer définitivement les 3 dictées enregistrées ?")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Effacer définitivement" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("button", { name: "Tout effacer" })).toHaveFocus();
    expect(bridge.invoke).not.toHaveBeenCalledWith("clear_history", expect.anything());

    await user.click(screen.getByRole("button", { name: "Tout effacer" }));
    await user.click(screen.getByRole("button", { name: "Effacer définitivement" }));
    expect(bridge.invoke).toHaveBeenCalledWith("clear_history", { disable: false });
    expect(await screen.findByTestId("history-empty")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Conserver mes dictées sur ce PC" })).toBeChecked();
  });

  it("disables and erases", async () => {
    const { bridge, user } = await openHistory({ settings: ON, history: ENTRIES });
    await screen.findAllByTestId("history-entry");
    await user.click(screen.getByRole("button", { name: "Désactiver et effacer" }));
    await user.click(within(screen.getByRole("group", { name: /Désactiver l’historique/ })).getByRole("button", {
      name: "Désactiver et effacer",
    }));
    expect(bridge.invoke).toHaveBeenCalledWith("clear_history", { disable: true });
    await waitFor(() => expect(screen.getByRole("switch", { name: "Conserver mes dictées sur ce PC" })).not.toBeChecked());
    expect(screen.queryAllByTestId("history-entry")).toHaveLength(0);
    await waitFor(() => expect(screen.getByRole("heading", { level: 1, name: "Historique" })).toHaveFocus());
  });

  it("skips the history for the next dictation", async () => {
    const { bridge, user } = await openHistory({ settings: ON });
    const skip = screen.getByRole("switch", { name: "Sans historique pour la prochaine dictée" });
    await user.click(skip);
    expect(bridge.invoke).toHaveBeenCalledWith("skip_history_next_take", { skip: true });
    await waitFor(() => expect(skip).toBeChecked());
  });

  it("pages with nextCursor", async () => {
    const many = Array.from({ length: 60 }, (_, index) => historyEntry(`p${index}`, `Dictée ${index}`, index));
    const { bridge, user } = await openHistory({ settings: ON, history: many });
    expect(await screen.findAllByTestId("history-entry")).toHaveLength(50);
    await user.click(screen.getByRole("button", { name: "Afficher plus" }));
    await waitFor(() => expect(screen.getAllByTestId("history-entry")).toHaveLength(60));
    expect(bridge.invoke).toHaveBeenLastCalledWith("get_history", {
      query: { search: null, limit: 50, cursor: "50" },
    });
    expect(screen.queryByRole("button", { name: "Afficher plus" })).not.toBeInTheDocument();
  });

  it("searches without speaking the query or the results", async () => {
    const { bridge, user } = await openHistory({ settings: ON, history: ENTRIES });
    await screen.findAllByTestId("history-entry");
    const spoken = recordAnnouncements();
    await user.type(screen.getByRole("searchbox", { name: "Rechercher dans l’historique" }), "budget");
    await user.click(screen.getByRole("button", { name: "Rechercher" }));
    expect(bridge.invoke).toHaveBeenCalledWith("get_history", { query: { search: "budget", limit: 50, cursor: null } });
    await waitFor(() => expect(screen.getAllByTestId("history-entry")).toHaveLength(1));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("1 dictée trouvée."));

    await user.click(screen.getByRole("button", { name: "Effacer la recherche" }));
    await waitFor(() => expect(screen.getAllByTestId("history-entry")).toHaveLength(3));
    spoken.stop();
    for (const text of spoken.texts) {
      expect(text).not.toContain("budget");
      expect(text).not.toContain("Martin");
      expect(text).not.toContain("salaires");
    }
  });

  it("never announces history text while copying and deleting", async () => {
    const { user } = await openHistory({ settings: ON, history: ENTRIES });
    const [first] = await screen.findAllByTestId("history-entry");
    const spoken = recordAnnouncements();
    await user.click(within(first).getByRole("button", { name: /^Copier/ }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Dictée copiée dans le presse-papiers."));
    await user.click(within(first).getByRole("button", { name: /^Supprimer/ }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Dictée supprimée."));
    spoken.stop();
    expect(spoken.texts.length).toBeGreaterThanOrEqual(2);
    for (const text of spoken.texts) {
      expect(text).not.toContain("Rendez-vous");
      expect(text).not.toContain("Martin");
    }
  });

  it("reloads when a dictation finishes", async () => {
    const { bridge } = await openHistory({ settings: ON, history: ENTRIES });
    await screen.findAllByTestId("history-entry");
    const before = historyCalls(bridge).length;
    bridge.history = [historyEntry("new", "Nouvelle dictée.", 0), ...bridge.history];
    act(() => {
      bridge.pushState({ lastOutcome: { kind: "inserted" } });
    });
    await waitFor(() => expect(screen.getAllByTestId("history-entry")).toHaveLength(4));
    expect(historyCalls(bridge).length).toBeGreaterThan(before);
  });

  it("explains the ephemeral session", async () => {
    await openHistory({ settings: ON, historyAvailable: false });
    expect(screen.getByTestId("history-ephemeral")).toHaveTextContent("Sambler fonctionne en session éphémère");
    expect(screen.getByRole("switch", { name: "Conserver mes dictées sur ce PC" })).toBeDisabled();
    expect(screen.queryByRole("search")).not.toBeInTheDocument();
  });

  it("has no detectable accessibility violations", async () => {
    await openHistory({ settings: ON, history: ENTRIES });
    await screen.findAllByTestId("history-entry");
    expect(await axeViolations()).toEqual([]);
  });
});

describe("Historique, jour par jour", () => {
  // Noon today, then earlier days: away from midnight whatever the clock says.
  const noon = new Date();
  noon.setHours(12, 0, 0, 0);
  const daysAgo = (days: number, hour: number) => {
    const date = new Date(noon);
    date.setDate(date.getDate() - days);
    date.setHours(hour, 5);
    return date.getTime();
  };
  const BY_DAY: HistoryEntry[] = [
    historyEntry("t1", "Rendez-vous jeudi à 14 h.", 0, daysAgo(0, 11)),
    historyEntry("t2", "Penser au devis.", 0, daysAgo(0, 9)),
    historyEntry("y1", null, 0, daysAgo(1, 18)),
  ];

  it("groups the dictations by day, in lists named by their day", async () => {
    await openHistory({ settings: ON, history: BY_DAY });
    await screen.findAllByTestId("history-entry");
    const today = screen.getByRole("list", { name: "Aujourd’hui 2 dictées · 8 mots" });
    const yesterday = screen.getByRole("list", { name: "Hier 1 dictée" });
    expect(within(today).getAllByTestId("history-entry")).toHaveLength(2);
    expect(within(yesterday).getAllByTestId("history-entry")).toHaveLength(1);

    // The time is shown; the heading still reads the whole date.
    const [first] = within(today).getAllByTestId("history-entry");
    expect(first).toHaveTextContent("11:05");
    expect(first).toHaveTextContent("5 mots");
    expect(within(first).getByRole("heading", { level: 3 })).toHaveAccessibleName(formatEntryDate(daysAgo(0, 11)));
    expect(within(first).getByRole("button", { name: /^Copier/ })).toHaveAccessibleName(
      `Copier la dictée du ${formatEntryDate(daysAgo(0, 11))}`,
    );
    expect(within(first).getByRole("button", { name: /^Supprimer/ })).toHaveAccessibleName(
      `Supprimer la dictée du ${formatEntryDate(daysAgo(0, 11))}`,
    );
  });

  it("says when a day may continue on the next page", async () => {
    const many = Array.from({ length: 60 }, (_, index) => historyEntry(`p${index}`, "Une dictée.", index, daysAgo(0, 11)));
    await openHistory({ settings: ON, history: many });
    await screen.findAllByTestId("history-entry");
    expect(screen.getByRole("list", { name: "Aujourd’hui 50 dictées affichées · 100 mots" })).toBeInTheDocument();
  });

  it("shows an empty search as such, the way back included", async () => {
    const { user } = await openHistory({ settings: ON, history: ENTRIES });
    await screen.findAllByTestId("history-entry");
    await user.type(screen.getByRole("searchbox", { name: "Rechercher dans l’historique" }), "introuvable");
    await user.click(screen.getByRole("button", { name: "Rechercher" }));
    const empty = await screen.findByTestId("history-empty");
    expect(empty).toHaveTextContent("Rien trouvé");
    expect(empty).toHaveTextContent("Aucune dictée ne correspond à cette recherche.");
    expect(screen.getByRole("button", { name: "Effacer la recherche" })).toBeInTheDocument();
    expect(await axeViolations()).toEqual([]);
  });

  it("offers to retry when the list cannot be read, without stale entries", async () => {
    const { bridge, user } = await openHistory({ settings: ON, history: ENTRIES });
    await screen.findAllByTestId("history-entry");
    const working = bridge.handlers.get_history;
    bridge.handlers.get_history = () => {
      throw { code: "io", message: "" };
    };
    await user.type(screen.getByRole("searchbox", { name: "Rechercher dans l’historique" }), "budget");
    await user.click(screen.getByRole("button", { name: "Rechercher" }));
    const problem = await screen.findByTestId("history-list-error");
    expect(problem).toHaveTextContent("L’historique n’a pas pu être lu.");
    expect(screen.queryAllByTestId("history-entry")).toHaveLength(0);
    expect(screen.queryByText("Chargement…")).not.toBeInTheDocument();
    expect(await axeViolations()).toEqual([]);

    bridge.handlers.get_history = working;
    await user.click(within(problem).getByRole("button", { name: "Réessayer" }));
    await waitFor(() => expect(screen.getAllByTestId("history-entry")).toHaveLength(1));
    expect(screen.queryByTestId("history-list-error")).not.toBeInTheDocument();
  });

  it("offers to retry when the status cannot be read", async () => {
    const bridge = createFakeBridge({ settings: ON, history: ENTRIES });
    const working = bridge.handlers.get_history_status;
    bridge.handlers.get_history_status = () => {
      throw { code: "io", message: "" };
    };
    const { user } = renderApp(bridge);
    await appReady();
    await user.click(screen.getByRole("button", { name: "Historique" }));
    const problem = await screen.findByTestId("history-status-error");
    expect(problem).toHaveTextContent("L’état de l’historique n’a pas pu être lu.");

    bridge.handlers.get_history_status = working;
    await user.click(within(problem).getByRole("button", { name: "Réessayer" }));
    await waitFor(() => expect(screen.queryByTestId("history-status-error")).not.toBeInTheDocument());
    expect(await screen.findAllByTestId("history-entry")).toHaveLength(3);
  });
});
