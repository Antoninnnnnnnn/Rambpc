// The sample `get_activity` data must stay coherent: the Activité screen
// (phase 2) is designed on it.

import { describe, expect, it } from "vitest";
import { localDate, SAMPLE_ACTIVITY, sampleActivity, sampleStats } from "./sampleActivity";

/** Consecutive active days ending today, or yesterday if today has none yet. */
function currentStreak(days: { words: number }[]): number {
  let index = days.length - 1;
  if (days[index].words === 0) index -= 1;
  let streak = 0;
  while (index >= 0 && days[index].words > 0) {
    streak += 1;
    index -= 1;
  }
  return streak;
}

function longestStreak(days: { date: string; words: number }[]): { length: number; end: string | null } {
  let best = { length: 0, end: null as string | null };
  let run = 0;
  for (const day of days) {
    run = day.words > 0 ? run + 1 : 0;
    if (run > best.length) best = { length: run, end: day.date };
  }
  return best;
}

describe("sample activity", () => {
  const today = new Date(2026, 10, 21, 22, 41);
  const activity = sampleActivity(today);

  it("covers 112 consecutive local days ending today", () => {
    expect(activity.days).toHaveLength(112);
    expect(activity.days[activity.days.length - 1].date).toBe("2026-11-21");
    for (let index = 1; index < activity.days.length; index += 1) {
      const previous = new Date(`${activity.days[index - 1].date}T12:00:00`);
      previous.setDate(previous.getDate() + 1);
      expect(localDate(previous)).toBe(activity.days[index].date);
    }
  });

  it("agrees with its own streaks and first day", () => {
    expect(activity.currentStreak).toBe(23);
    expect(currentStreak(activity.days)).toBe(23);
    expect(activity.longestStreak).toBe(31);
    expect(longestStreak(activity.days)).toEqual({ length: 31, end: activity.longestStreakEnd });
    const first = activity.days.findIndex((day) => day.words > 0);
    expect(activity.days[first].date).toBe(activity.firstDay);
    expect(activity.days.every((day) => (day.words > 0) === (day.takes > 0))).toBe(true);
  });

  it("adds up to 48 312 words in 1 146 takes, the last week as designed", () => {
    const words = activity.days.reduce((sum, day) => sum + day.words, 0);
    const takes = activity.days.reduce((sum, day) => sum + day.takes, 0);
    expect(words).toBe(48_312);
    expect(takes).toBe(1_146);
    expect(activity.days.slice(-7).map((day) => day.words)).toEqual([...SAMPLE_ACTIVITY.lastWeek]);
    expect(activity.apps!.reduce((sum, share) => sum + share.words, 0)).toBe(48_312);
    expect(activity.apps!.map((share) => share.words)).toEqual(
      [...activity.apps!.map((share) => share.words)].sort((a, b) => b - a),
    );
  });

  it("gives 182 words per minute and about 15 h 42 saved against typing", () => {
    const minutes = activity.listeningMsTotal / 60_000;
    expect(Math.round(48_312 / minutes)).toBe(182);
    const saved = 48_312 / 40 - minutes;
    expect(Math.floor(saved / 60)).toBe(15);
    expect(Math.floor(saved % 60)).toBe(42);
  });

  it("is the same for the same day", () => {
    expect(sampleActivity(today)).toEqual(activity);
  });

  it("matches the usage counters", () => {
    const stats = sampleStats(activity, today);
    expect(stats).toMatchObject({ wordsToday: 1_284, wordsTotal: 48_312, takesTotal: 1_146, averageWpm: 182 });
    // Saturday 21 November: Monday to today is the last six days.
    expect(stats.wordsThisWeek).toBe(1_410 + 960 + 1_730 + 2_105 + 1_541 + 1_284);
  });
});
