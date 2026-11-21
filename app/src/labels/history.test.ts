import { describe, expect, it } from "vitest";
import { dayCountText, dayLabel, formatEntryTime, groupByDay, wordCount, wordsText } from "./history";

const NOW = new Date(2026, 10, 21, 10, 30).getTime();
const at = (year: number, month: number, day: number, hour = 9, minute = 5) =>
  new Date(year, month, day, hour, minute).getTime();

describe("history labels", () => {
  it("names the day: today, yesterday, then the date, with the year when it changes", () => {
    expect(dayLabel(at(2026, 10, 21, 0, 1), NOW)).toBe("Aujourd’hui");
    expect(dayLabel(at(2026, 10, 20, 23, 59), NOW)).toBe("Hier");
    expect(dayLabel(at(2026, 10, 18), NOW)).toBe("Mercredi 18 novembre");
    expect(dayLabel(at(2025, 11, 31), NOW)).toBe("Mercredi 31 décembre 2025");
    // Across a month boundary.
    expect(dayLabel(at(2026, 10, 30), new Date(2026, 11, 1, 8).getTime())).toBe("Hier");
  });

  it("shows the time of a dictation in a day group", () => {
    expect(formatEntryTime(at(2026, 10, 21, 9, 5))).toBe("09:05");
  });

  it("groups the newest first entries by local day, keeping their order", () => {
    const entries = [
      { id: "a", createdAtMs: at(2026, 10, 21, 10) },
      { id: "b", createdAtMs: at(2026, 10, 21, 0, 2) },
      { id: "c", createdAtMs: at(2026, 10, 20, 23, 58) },
      { id: "d", createdAtMs: at(2026, 10, 18) },
    ];
    const days = groupByDay(entries, NOW);
    expect(days.map((day) => day.label)).toEqual(["Aujourd’hui", "Hier", "Mercredi 18 novembre"]);
    expect(days.map((day) => day.entries.map((entry) => entry.id))).toEqual([["a", "b"], ["c"], ["d"]]);
    expect(groupByDay([], NOW)).toEqual([]);
  });

  it("counts words, unreadable text included as none", () => {
    expect(wordCount("Rendez-vous jeudi à 14 h.")).toBe(5);
    expect(wordCount("Points :\n- loyers\n- salaires")).toBe(6);
    expect(wordCount(null)).toBe(0);
    expect(wordCount("   ")).toBe(0);
    expect(wordsText(1)).toBe("1 mot");
    // French grouping: a narrow no-break space (U+202F) between thousands.
    expect(wordsText(1234)).toBe("1 234 mots");
  });

  it("sums up a day, saying when more may follow", () => {
    expect(dayCountText(6, 81)).toBe("6 dictées · 81 mots");
    expect(dayCountText(1, 1)).toBe("1 dictée · 1 mot");
    expect(dayCountText(2, 0)).toBe("2 dictées");
    expect(dayCountText(3, 33, true)).toBe("3 dictées affichées · 33 mots");
    expect(dayCountText(1, 4, true)).toBe("1 dictée affichée · 4 mots");
  });
});
