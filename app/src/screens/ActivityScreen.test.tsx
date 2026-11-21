// Activité: key figures, streak and 16-week map, last seven days and the share
// per application, with their table views; the get_stats fallback, the
// waiting state of the per-app breakdown, the empty, loading and error states.

import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { sampleActivity, sampleStats } from "../dev/sampleActivity";
import type { ActivityStats, UsageStats } from "../ipc/contract";
import { createFakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, renderApp } from "../test/render";

const today = new Date(2026, 10, 21, 22, 41); // Saturday 21 November 2026
const activity = sampleActivity(today);
const stats = sampleStats(activity, today);

/** Text with French no-break spaces turned into plain spaces. */
const plain = (element: Element | null) => (element?.textContent ?? "").replace(/[  ]/g, " ");
const plainAttr = (element: Element, name: string) => (element.getAttribute(name) ?? "").replace(/[  ]/g, " ");

const EMPTY_ACTIVITY: ActivityStats = {
  days: activity.days.map((day) => ({ ...day, words: 0, takes: 0 })),
  currentStreak: 0,
  longestStreak: 0,
  longestStreakEnd: null,
  firstDay: null,
  listeningMsTotal: 0,
  apps: null,
};

async function openActivity(options: { stats?: UsageStats; activity?: ActivityStats } = {}) {
  const bridge = createFakeBridge({ stats, activity, ...options });
  return openWith(bridge);
}

async function openWith(bridge: ReturnType<typeof createFakeBridge>) {
  const view = renderApp(bridge);
  await appReady();
  const nav = screen.getByRole("navigation", { name: "Navigation principale" });
  await view.user.click(within(nav).getByRole("button", { name: "Activité" }));
  await screen.findByRole("heading", { level: 1, name: "Activité" });
  return { bridge, ...view };
}

const failing = () => {
  throw { code: "unknown-command", message: "" };
};

describe("Activité", () => {
  it("shows the key figures, from get_stats, and the time saved with its method", async () => {
    await openActivity();
    const figures = await screen.findByTestId("activity-figures");
    expect(within(figures).getByRole("heading", { level: 2, name: "Depuis le début" })).toBeInTheDocument();
    expect(plain(screen.getByTestId("figure-words"))).toMatch(/mots au total\s*48 312/);
    expect(plain(screen.getByTestId("figure-takes"))).toMatch(/dictées\s*1 146/);
    expect(plain(screen.getByTestId("figure-wpm"))).toMatch(/mots\/min en moyenne\s*182/);
    expect(screen.getByTestId("figure-saved")).toHaveTextContent(/gagnées depuis le début, vs frappe à 40 mots\/min/);
    expect(screen.getByTestId("figure-saved")).toHaveTextContent("15 heures et 42 minutes");
    expect(plain(screen.getByTestId("activity-method"))).toBe(
      "Temps gagné : 48 312 mots ÷ 40 mots/min, soit 20 h 07 de frappe au clavier, moins 4 h 25 de parole réelle.",
    );
    expect(screen.getByText("Depuis votre première dictée, le 8 août.")).toBeInTheDocument();
  });

  it("shows the streak, the record and the 16-week map to scale", async () => {
    const { user } = await openActivity();
    const card = await screen.findByTestId("activity-streak");
    expect(within(card).getByRole("heading", { level: 2, name: "Série" })).toBeInTheDocument();
    expect(within(card).getByText("16 dernières semaines")).toBeInTheDocument();
    expect(plain(screen.getByTestId("streak-current"))).toBe("23 jours d’affilée");
    expect(plain(screen.getByTestId("streak-record"))).toBe("Record : 31 jours, du 11 septembre au 11 octobre");

    const map = screen.getByRole("img", { name: /^Carte des jours dictés sur 16 semaines/ });
    const label = plainAttr(map, "aria-label");
    expect(label).toContain("du 2 août au 21 novembre");
    expect(label).toContain("série en cours de 23 jours depuis le 30 octobre");
    expect(label).toContain("record de 31 jours du 11 septembre au 11 octobre");
    const cells = map.querySelectorAll(".daymap__cell");
    expect(cells).toHaveLength(17 * 7);
    expect(map.querySelectorAll('[data-kind="day"]')).toHaveLength(112);
    expect(map.querySelectorAll("[data-today]")).toHaveLength(1);
    expect(map.querySelector("[data-today]")).toHaveAttribute("data-level", "3");
    expect(plainAttr(map.querySelector("[data-today]")!, "data-tip-value")).toBe("1 284 mots");

    // Legend: equal ranges of words per day.
    expect(plain(card.querySelector(".daymap__legend"))).toMatch(/≤ 600.*≤ 1 200.*≤ 1 800.*≤ 2 400/);

    // Table view: one row per week, Monday to Sunday, the week total.
    await user.click(within(card).getByText("Voir le tableau des jours"));
    const table = within(card).getByRole("table", { name: "Mots dictés par jour, 16 dernières semaines" });
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(1 + 17);
    expect(within(rows[0]).getAllByRole("columnheader").map((cell) => cell.textContent)).toEqual([
      "Semaine",
      "lun.lundi",
      "mar.mardi",
      "mer.mercredi",
      "jeu.jeudi",
      "ven.vendredi",
      "sam.samedi",
      "dim.dimanche",
      "Total",
    ]);
    const lastWeek = rows[rows.length - 1];
    expect(within(lastWeek).getByRole("rowheader")).toHaveTextContent("Semaine du 16 novembre");
    expect(plain(lastWeek)).toContain("1 284, aujourd’hui");
    expect(plain(lastWeek)).toContain("à venir");
    // Monday 16 to Saturday 21 November.
    expect(plain(lastWeek.lastElementChild)).toBe("9 030");
  });

  it("shows the words per day of the last seven days from zero, with a table", async () => {
    const { user } = await openActivity();
    const card = await screen.findByTestId("activity-week");
    expect(plain(within(card).getByText(/^7 derniers jours/))).toBe("7 derniers jours · 9 850 mots");
    const chart = within(card).getByRole("img");
    expect(plainAttr(chart, "aria-label")).toBe(
      "Mots par jour sur les 7 derniers jours : dimanche 15 novembre, 820 mots ; lundi 16 novembre, 1 410 mots ; " +
        "mardi 17 novembre, 960 mots ; mercredi 18 novembre, 1 730 mots ; jeudi 19 novembre, 2 105 mots ; " +
        "hier, 1 541 mots ; aujourd’hui, 1 284 mots. Total : 9 850 mots.",
    );
    // Round ticks from 0; bars as tall as their share of the axis.
    expect(Array.from(chart.querySelectorAll(".week-chart__tick-label")).map(plain)).toEqual(["0", "1 000", "2 000"]);
    const heights = Array.from(chart.querySelectorAll<HTMLElement>(".week-chart__bar")).map((bar) => parseFloat(bar.style.height));
    expect(heights[4] / heights[0]).toBeCloseTo(2_105 / 820, 5);
    // Only the busiest day and today carry their value.
    expect(Array.from(chart.querySelectorAll(".week-chart__value")).map(plain)).toEqual(["2 105", "1 284"]);

    await user.click(within(card).getByText("Voir le tableau des 7 jours"));
    const table = within(card).getByRole("table", { name: "Mots et dictées par jour, 7 derniers jours" });
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(8);
    expect(plain(rows[7])).toMatch(/^aujourd’hui1 284/);
  });

  it("tells a day without takes (0) from a day without data", async () => {
    const gappy: ActivityStats = {
      ...activity,
      days: activity.days
        .filter((day) => day.date !== "2026-11-19")
        .map((day) => (day.date === "2026-11-18" ? { ...day, words: 0, takes: 0 } : day)),
    };
    const { user } = await openActivity({ activity: gappy });
    const card = await screen.findByTestId("activity-week");
    const label = plainAttr(within(card).getByRole("img"), "aria-label");
    expect(label).toContain("mercredi 18 novembre, 0 mot ;");
    expect(label).toContain("jeudi 19 novembre, donnée indisponible ;");
    expect(plain(within(card).getByText(/^7 derniers jours/))).toMatch(/total partiel/);
    expect(Array.from(card.querySelectorAll(".week-chart__value")).map(plain)).toContain("—");

    await user.click(within(card).getByText("Voir le tableau des 7 jours"));
    const rows = within(within(card).getByRole("table")).getAllByRole("row");
    // Words « 0 », takes « 0 » (read « 0 dictée »).
    expect(plain(rows[4])).toBe("mercredi 18 novembre000 dictée");
    expect(plain(rows[5])).toBe("jeudi 19 novembreindisponibleindisponible");
    const map = screen.getByRole("img", { name: /^Carte des jours/ });
    expect(plainAttr(map, "aria-label")).toContain("1 jour sans donnée");
    expect(map.querySelector('[data-kind="missing"]')).toHaveAttribute("data-tip-value", "donnée indisponible");
  });

  it("shows the share of words per application, each opening Style", async () => {
    const { user } = await openActivity();
    const card = await screen.findByTestId("activity-apps");
    expect(within(card).getByText("Part des mots insérés")).toBeInTheDocument();
    const buttons = within(screen.getByTestId("apps-list")).getAllByRole("button");
    expect(buttons.map((button) => plain(button))).toEqual([
      "WWord34 %, 16 426 mots",
      "BBrave22 %, 10 629 mots",
      "DDiscord15 %, 7 247 mots",
      "VVisual Studio Code12 %, 5 797 mots",
      "OOutlook9 %, 4 348 mots",
      "…Autres (2 applications)8 %, 3 865 mots. Afficher le détail",
    ]);
    expect(buttons[0]).toHaveAccessibleName(/^Word 34/);
    expect(buttons[0]).toHaveAccessibleDescription("Choisissez une application pour régler son style.");
    expect(buttons[0]).toHaveAttribute("data-app-id", "winword");
    // Bars against a round 40 % scale.
    expect(parseFloat(buttons[0].querySelector<HTMLElement>(".app-row__bar")!.style.width)).toBeCloseTo(85, 0);

    await user.click(buttons[0]);
    expect(await screen.findByRole("heading", { level: 1, name: "Style" })).toHaveFocus();
  });

  it("unfolds the small applications and folds them back", async () => {
    const { user } = await openActivity();
    const others = await screen.findByRole("button", { name: /^Autres/ });
    expect(others).toHaveAttribute("aria-expanded", "false");
    await user.click(others);
    const notepad = screen.getByRole("button", { name: /^Bloc-notes 4/ });
    expect(notepad).toHaveFocus();
    expect(notepad).toHaveAttribute("data-app-id", "notepad");
    expect(screen.getByRole("button", { name: /^Slack 4/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Regrouper les petites applications" }));
    expect(screen.queryByRole("button", { name: /^Slack/ })).not.toBeInTheDocument();
  });

  it("explains when the per-application breakdown will appear", async () => {
    await openActivity({ activity: { ...activity, apps: null } });
    const card = await screen.findByTestId("activity-apps");
    expect(within(card).getByTestId("apps-waiting")).toHaveTextContent(
      "La répartition apparaîtra iciDès votre première dictée insérée dans une application",
    );
    expect(within(card).getByTestId("apps-waiting")).toHaveTextContent("Les textes gardés dans Sambler ne comptent pas.");
    expect(within(card).queryByRole("list")).not.toBeInTheDocument();
    // The rest of the screen is unaffected.
    expect(screen.getByTestId("activity-streak")).toBeInTheDocument();
    expect(await axeViolations()).toEqual([]);
  });

  it("falls back on the get_stats counters when get_activity fails", async () => {
    const bridge = createFakeBridge({ stats, activity });
    bridge.handlers.get_activity = failing;
    const { user } = await openWith(bridge);
    const note = await screen.findByTestId("activity-fallback");
    expect(note).toHaveTextContent("Détail par jour indisponible");
    expect(note).toHaveTextContent("Les compteurs ci-dessus restent exacts.");
    expect(plain(screen.getByTestId("figure-words"))).toMatch(/48 312/);
    expect(plain(screen.getByTestId("figure-takes"))).toMatch(/1 146/);
    expect(plain(screen.getByTestId("figure-wpm"))).toMatch(/182/);
    // Speaking time estimated from the average speed: same result, said so.
    expect(screen.getByTestId("figure-saved")).toHaveTextContent("15 heures et 42 minutes");
    expect(screen.getByTestId("figure-saved")).toHaveTextContent("estimation vs frappe à 40 mots/min");
    expect(plain(screen.getByTestId("activity-method"))).toContain("moins environ 4 h 25 de parole, d’après votre vitesse moyenne");
    expect(screen.queryByTestId("activity-streak")).not.toBeInTheDocument();
    expect(screen.queryByTestId("activity-apps")).not.toBeInTheDocument();
    expect(screen.getByText("Vos mots dictés, jour après jour, et les applications où vous dictez.")).toBeInTheDocument();
    expect(await axeViolations()).toEqual([]);

    bridge.handlers.get_activity = () => activity;
    await user.click(within(note).getByRole("button", { name: "Réessayer" }));
    expect(await screen.findByTestId("activity-streak")).toBeInTheDocument();
    expect(screen.queryByTestId("activity-fallback")).not.toBeInTheDocument();
  });

  it("says an unknown figure is unknown, never 0", async () => {
    const bridge = createFakeBridge({ stats, activity: { ...activity, firstDay: "2026-01-04" } });
    bridge.handlers.get_stats = failing;
    await openWith(bridge);
    const words = await screen.findByTestId("figure-words");
    expect(words).toHaveTextContent("indisponible");
    expect(words).not.toHaveTextContent(/\d/);
    expect(screen.getByTestId("figure-saved")).toHaveTextContent("durée de parole inconnue");
    expect(screen.queryByTestId("activity-method")).not.toBeInTheDocument();
  });

  it("shows an empty state before the first take", async () => {
    await openActivity({
      stats: { wordsToday: 0, wordsThisWeek: 0, wordsTotal: 0, takesTotal: 0, averageWpm: null },
      activity: EMPTY_ACTIVITY,
    });
    const empty = await screen.findByTestId("activity-empty");
    expect(within(empty).getByRole("heading", { level: 2, name: "Pas encore d’activité" })).toBeInTheDocument();
    expect(screen.queryByTestId("activity-figures")).not.toBeInTheDocument();
    expect(await axeViolations()).toEqual([]);
  });

  it("says when nothing could be loaded, and retries", async () => {
    const bridge = createFakeBridge({ stats, activity });
    bridge.handlers.get_stats = failing;
    bridge.handlers.get_activity = failing;
    const { user } = await openWith(bridge);
    const error = await screen.findByTestId("activity-error");
    expect(error).toHaveTextContent("L’activité n’a pas pu être chargée");
    bridge.handlers.get_stats = () => stats;
    bridge.handlers.get_activity = () => activity;
    await user.click(within(error).getByRole("button", { name: "Réessayer" }));
    expect(await screen.findByTestId("activity-figures")).toBeInTheDocument();
  });

  it("shows that it is loading", async () => {
    const bridge = createFakeBridge({ stats, activity });
    bridge.handlers.get_activity = () => new Promise(() => {});
    await openWith(bridge);
    expect(screen.getByText("Chargement de l’activité…")).toHaveAttribute("role", "status");
    expect(screen.getByRole("heading", { level: 1, name: "Activité" })).toHaveFocus();
  });

  it("refreshes when a take ends", async () => {
    const { bridge } = await openActivity();
    await screen.findByTestId("activity-figures");
    const calls = () => bridge.invoke.mock.calls.filter(([command]) => command === "get_activity").length;
    const before = calls();
    act(() => {
      bridge.pushState({ phase: { kind: "listening" }, take: 3, listeningMs: 0 });
    });
    act(() => {
      bridge.pushState({ phase: { kind: "idle" }, take: null });
    });
    await waitFor(() => expect(calls()).toBe(before + 1));
  });

  it("has no detectable accessibility violations, table views open", async () => {
    const { user } = await openActivity();
    await screen.findByTestId("activity-apps");
    await user.click(screen.getByText("Voir le tableau des jours"));
    await user.click(screen.getByText("Voir le tableau des 7 jours"));
    expect(await axeViolations()).toEqual([]);
  });
});

describe("Accueil › Voir l’activité", () => {
  it("opens Activité from the counters", async () => {
    const bridge = createFakeBridge({ stats, activity });
    const { user } = renderApp(bridge);
    await appReady();
    const counters = await screen.findByTestId("stats");
    await user.click(within(counters).getByRole("button", { name: "Voir l’activité" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Activité" })).toHaveFocus();
  });
});
