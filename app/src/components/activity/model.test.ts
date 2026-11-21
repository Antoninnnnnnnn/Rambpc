// The Activité computations: scales, the 16-week map, the last seven days,
// the shares per application and the key figures must agree with the data
// and with each other.

import { describe, expect, it } from "vitest";
import { SAMPLE_ACTIVITY, sampleActivity, sampleStats } from "../../dev/sampleActivity";
import type { ActivityStats } from "../../ipc/contract";
import { formatDay, formatDuration, rangeText, savedExplanation, spokenDuration } from "./format";
import {
  appBreakdown,
  barAxis,
  buildDayMap,
  dayKey,
  heatLevel,
  heatScale,
  keyFigures,
  lastDays,
  niceCeil,
  parseDay,
  roundedPercents,
  savedTime,
} from "./model";

const today = new Date(2026, 10, 21, 22, 41); // Saturday 21 November 2026
const activity = sampleActivity(today);
const stats = sampleStats(activity, today);

describe("scales", () => {
  it("rounds up to round numbers", () => {
    expect(niceCeil(526)).toBe(600);
    expect(niceCeil(1_000)).toBe(1_000);
    expect(niceCeil(0)).toBe(1);
    expect(niceCeil(1.2)).toBe(2);
  });

  it("splits the day map into four equal word ranges over the busiest day", () => {
    const scale = heatScale(2_105);
    expect(scale.bounds).toEqual([600, 1_200, 1_800, 2_400]);
    expect(heatLevel(0, scale)).toBe(0);
    expect(heatLevel(1, scale)).toBe(1);
    expect(heatLevel(600, scale)).toBe(1);
    expect(heatLevel(601, scale)).toBe(2);
    expect(heatLevel(2_105, scale)).toBe(4);
  });

  it("starts the bar axis at zero with round ticks above the busiest day", () => {
    const axis = barAxis(2_105);
    expect(axis.ticks).toEqual([0, 1_000, 2_000]);
    expect(axis.max).toBeGreaterThan(2_105);
    expect(barAxis(0)).toEqual({ max: 1, ticks: [0] });
  });
});

describe("day map", () => {
  const map = buildDayMap(activity)!;

  it("lays the 112 days out in Monday-first weeks, today included", () => {
    // Sunday 2 August to Saturday 21 November: 17 columns, from Monday 27 July to Sunday 22 November.
    expect(map.weeks).toBe(17);
    expect(map.cells).toHaveLength(17 * 7);
    expect(map.cells.filter((cell) => cell.kind === "day")).toHaveLength(112);
    expect(map.cells.filter((cell) => cell.kind === "outside")).toHaveLength(6);
    expect(map.cells.filter((cell) => cell.kind === "future").map((cell) => cell.key)).toEqual(["2026-11-22"]);
    const todayCell = map.cells.find((cell) => cell.today)!;
    expect(todayCell.key).toBe("2026-11-21");
    expect(todayCell.weekday).toBe(5);
    expect(todayCell.words).toBe(1_284);
    expect(map.cells.every((cell) => cell.weekday === map.cells.indexOf(cell) % 7)).toBe(true);
  });

  it("keeps every word of the period, at the right level", () => {
    const words = map.cells.reduce((sum, cell) => sum + (cell.words ?? 0), 0);
    expect(words).toBe(SAMPLE_ACTIVITY.words);
    for (const cell of map.cells.filter((item) => item.kind === "day")) {
      expect(cell.level).toBe(heatLevel(cell.words!, map.scale));
    }
  });

  it("names months at least three columns apart", () => {
    const weeks = map.months.map((month) => month.week);
    for (let index = 1; index < weeks.length; index += 1) expect(weeks[index] - weeks[index - 1]).toBeGreaterThanOrEqual(3);
  });

  it("places the streak in progress and the record", () => {
    expect(map.current).toMatchObject({ length: 23 });
    expect(dayKey(map.current!.to)).toBe("2026-11-21");
    expect(dayKey(map.current!.from)).toBe("2026-10-30");
    expect(map.current!.weeks).toEqual({ from: 13, to: 16 });
    expect(map.record).toMatchObject({ length: 31 });
    expect(dayKey(map.record!.to)).toBe(activity.longestStreakEnd);
    expect(map.recordIsCurrent).toBe(false);
    // Every day of both streaks has words.
    for (const streak of [map.current!, map.record!]) {
      const inside = map.cells.filter((cell) => cell.date >= streak.from && cell.date <= streak.to);
      expect(inside).toHaveLength(streak.length);
      expect(inside.every((cell) => (cell.words ?? 0) > 0)).toBe(true);
    }
  });

  it("tells a missing day (unknown) from a day without takes (0)", () => {
    const gappy: ActivityStats = {
      ...activity,
      days: activity.days.filter((day) => day.date !== "2026-11-19").map((day) => (day.date === "2026-11-18" ? { ...day, words: 0, takes: 0 } : day)),
    };
    const cells = buildDayMap(gappy)!.cells;
    expect(cells.find((cell) => cell.key === "2026-11-19")).toMatchObject({ kind: "missing", words: null });
    expect(cells.find((cell) => cell.key === "2026-11-18")).toMatchObject({ kind: "day", words: 0, level: 0 });
    const bars = lastDays(gappy);
    expect(bars.find((bar) => bar.key === "2026-11-19")?.words).toBeNull();
    expect(bars.find((bar) => bar.key === "2026-11-18")?.words).toBe(0);
  });

  it("marks a record that is the streak in progress", () => {
    const record = buildDayMap({ ...activity, longestStreak: 23, longestStreakEnd: "2026-11-21" })!;
    expect(record.recordIsCurrent).toBe(true);
  });

  it("ends the streak yesterday while nothing was dictated today", () => {
    const days = activity.days.map((day, index, list) => (index === list.length - 1 ? { ...day, words: 0, takes: 0 } : day));
    const map = buildDayMap({ ...activity, days, currentStreak: 22 })!;
    expect(dayKey(map.current!.to)).toBe("2026-11-20");
  });
});

describe("last seven days", () => {
  it("are the sample week, oldest first, today last", () => {
    const bars = lastDays(activity);
    expect(bars.map((bar) => bar.words)).toEqual([...SAMPLE_ACTIVITY.lastWeek]);
    expect(bars.map((bar) => bar.today)).toEqual([false, false, false, false, false, false, true]);
    expect(bars[0].key).toBe("2026-11-15");
  });
});

describe("applications", () => {
  it("rounds shares to whole percents that add up to 100", () => {
    expect(roundedPercents([1, 1, 1])).toEqual([34, 33, 33]);
    expect(roundedPercents([0, 0])).toEqual([0, 0]);
  });

  it("shows the top five and folds the rest into « Autres »", () => {
    const breakdown = appBreakdown(activity.apps)!;
    expect(breakdown.total).toBe(48_312);
    expect(breakdown.rows.map((row) => [row.app, row.id, row.percent])).toEqual([
      ["Word", "winword", 34],
      ["Brave", "brave", 22],
      ["Discord", "discord", 15],
      ["Visual Studio Code", "code", 12],
      ["Outlook", "outlook", 9],
    ]);
    expect(breakdown.others).toMatchObject({ count: 2, words: 2_104 + 1_761, percent: 8 });
    expect(breakdown.rest.map((row) => row.id)).toEqual(["notepad", "slack"]);
    const sum = breakdown.rows.reduce((total, row) => total + row.percent, 0) + breakdown.others!.percent;
    expect(sum).toBe(100);
    // Bars against a round 40 % scale: Word (34 %) fills 85 % of the track.
    expect(breakdown.rows[0].width).toBeCloseTo(16_426 / 48_312 / 0.4, 5);
    expect(breakdown.rows.every((row, index, list) => index === 0 || row.width <= list[index - 1].width)).toBe(true);
  });

  it("lists every application when there are six or fewer", () => {
    const breakdown = appBreakdown(activity.apps!.slice(0, 6))!;
    expect(breakdown.rows).toHaveLength(6);
    expect(breakdown.others).toBeNull();
  });

  it("has nothing to show before the first insertion", () => {
    expect(appBreakdown(null)).toBeNull();
    expect(appBreakdown([])).toBeNull();
  });
});

describe("key figures", () => {
  it("take the counters from get_stats and the speaking time from get_activity", () => {
    const figures = keyFigures(stats, activity);
    expect(figures).toMatchObject({ words: 48_312, takes: 1_146, wpm: 182 });
    // 48 312 words ÷ 40 = 1 207 min of typing; 265 min of speech; 942 min saved.
    expect(figures.saved).toEqual({ minutes: 942, typingMinutes: 1_207, speakingMinutes: 265, estimated: false });
    expect(figures.saved!.typingMinutes - figures.saved!.speakingMinutes).toBe(figures.saved!.minutes);
    expect(formatDuration(figures.saved!.minutes)).toBe("15 h 42");
    expect(savedExplanation(48_312, figures.saved!).replace(/ /g, " ").replace(/ /g, " ")).toBe(
      "Temps gagné : 48 312 mots ÷ 40 mots/min, soit 20 h 07 de frappe au clavier, moins 4 h 25 de parole réelle.",
    );
  });

  it("estimate the speaking time from the average speed without get_activity", () => {
    const figures = keyFigures(stats, null);
    expect(figures).toMatchObject({ words: 48_312, takes: 1_146, wpm: 182 });
    expect(figures.saved).toMatchObject({ minutes: 942, estimated: true });
  });

  it("add up the days when get_stats fails and the 112 days hold every take", () => {
    const figures = keyFigures(null, activity);
    expect(figures).toMatchObject({ words: 48_312, takes: 1_146, wpm: 182 });
  });

  it("stay unknown rather than wrong", () => {
    // Takes before the 112 days: the days do not hold the totals.
    const older = { ...activity, firstDay: "2026-01-04" };
    expect(keyFigures(null, older)).toEqual({ words: null, takes: null, wpm: null, saved: null });
    // No speaking time and no speed: no time saved.
    const silent = { ...stats, averageWpm: null };
    expect(keyFigures(silent, { ...activity, listeningMsTotal: 0 }).saved).toBeNull();
    expect(keyFigures(silent, null)).toMatchObject({ wpm: null, saved: null });
  });

  it("never saves less than nothing", () => {
    expect(savedTime(100, 10 * 60_000).minutes).toBe(0);
    expect(keyFigures({ ...stats, wordsTotal: 0, takesTotal: 0 }, null).saved?.minutes).toBe(0);
  });
});

describe("French wording", () => {
  it("writes dates the French way", () => {
    expect(formatDay(new Date(2026, 6, 1), today)).toBe("1er juillet");
    expect(formatDay(new Date(2025, 11, 24), today)).toBe("24 décembre 2025");
    expect(rangeText(new Date(2026, 7, 24), new Date(2026, 8, 23), today)).toBe("du 24 août au 23 septembre");
    expect(rangeText(new Date(2026, 7, 24), new Date(2026, 7, 24), today)).toBe("le 24 août");
    expect(parseDay("2026-02-30x")).toBeNull();
  });

  it("writes durations for the eye and for the ear", () => {
    expect(formatDuration(42)).toBe("42 min");
    expect(formatDuration(0)).toBe("0 min");
    expect(formatDuration(60 * 125 + 5)).toBe("125 h 05");
    expect(spokenDuration(942)).toBe("15 heures et 42 minutes");
    expect(spokenDuration(61)).toBe("1 heure et 1 minute");
    expect(spokenDuration(120)).toBe("2 heures");
  });
});
