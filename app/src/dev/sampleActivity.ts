// DEV PREVIEW AND TESTS ONLY: plausible `get_activity` data, the same every
// time for a given day, coherent with itself and with the sample counters:
// 112 days ending today, a current streak of 23 days, a record of 31, 48 312
// words in 1 146 takes at 182 words per minute (so about 15 h 42 saved against
// typing at 40 words per minute), the last seven days as in the design brief.
// Never part of a production build (imported by the dev preview bridge and by
// the test double only).

import type { ActivityStats, DayActivity, UsageStats } from "../ipc/contract";

export const SAMPLE_ACTIVITY = {
  days: 112,
  words: 48_312,
  takes: 1_146,
  currentStreak: 23,
  longestStreak: 31,
  averageWpm: 182,
  /** Last seven days, oldest first, today last. */
  lastWeek: [820, 1_410, 960, 1_730, 2_105, 1_541, 1_284],
  /** Index of the first day with a take; days before it are empty. */
  firstIndex: 6,
  /** The record streak covers these indexes (inclusive). */
  longestFrom: 40,
  longestTo: 70,
} as const;

// Ids as in dev/previewApps.ts (lowercased executable name).
const APPS = [
  { app: "Word", words: 16_426, id: "winword" },
  { app: "Brave", words: 10_629, id: "brave" },
  { app: "Discord", words: 7_247, id: "discord" },
  { app: "Visual Studio Code", words: 5_797, id: "code" },
  { app: "Outlook", words: 4_348, id: "outlook" },
  { app: "Bloc-notes", words: 2_104, id: "notepad" },
  { app: "Slack", words: 1_761, id: "slack" },
];

/** Small deterministic generator (mulberry32). */
function seeded(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

/** Local calendar date, "YYYY-MM-DD". */
export function localDate(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

export function sampleActivity(today: Date = new Date()): ActivityStats {
  const S = SAMPLE_ACTIVITY;
  const random = seeded(0x5a3b1e);
  const n = S.days;
  const currentFrom = n - S.currentStreak;

  // Which days had at least one take.
  const active = Array.from({ length: n }, (_, index) => {
    if (index < S.firstIndex) return false;
    if (index === S.firstIndex) return true;
    if (index >= S.longestFrom && index <= S.longestTo) return true;
    if (index >= currentFrom) return true;
    if (index === S.longestFrom - 1 || index === S.longestTo + 1 || index === currentFrom - 1) return false;
    return random() < 0.68;
  });
  // Keep every other run well below the record.
  let run = 0;
  for (let index = 0; index < currentFrom - 1; index += 1) {
    const fixed = index >= S.longestFrom && index <= S.longestTo;
    run = active[index] && !fixed ? run + 1 : 0;
    if (run > 9) {
      active[index] = false;
      run = 0;
    }
  }

  // Words: the last week as given, the rest shared out to reach the total.
  const weekFrom = n - S.lastWeek.length;
  const weights = active.map((on, index) => (on && index < weekFrom ? 0.3 + random() * random() * 1.6 : 0));
  const remaining = S.words - S.lastWeek.reduce((sum, value) => sum + value, 0);
  const weightSum = weights.reduce((sum, value) => sum + value, 0);
  const words = weights.map((weight) => (weight ? Math.max(40, Math.floor((weight / weightSum) * remaining)) : 0));
  S.lastWeek.forEach((value, offset) => {
    words[weekFrom + offset] = value;
  });
  // Rounding leftovers go to the busiest earlier day.
  const busiest = words.slice(0, weekFrom).reduce((best, value, index, list) => (value > list[best] ? index : best), 0);
  words[busiest] += S.words - words.reduce((sum, value) => sum + value, 0);

  // Takes: about one per 42 words, at least one on an active day.
  const takes = words.map((value) => (value ? Math.max(1, Math.round((value * S.takes) / S.words)) : 0));
  let gap = S.takes - takes.reduce((sum, value) => sum + value, 0);
  const order = words.map((value, index) => [value, index] as const).sort((a, b) => b[0] - a[0]);
  for (let step = 0; gap !== 0 && step < 100_000; step += 1) {
    const index = order[step % order.length][1];
    if (gap > 0) {
      takes[index] += 1;
      gap -= 1;
    } else if (takes[index] > 1) {
      takes[index] -= 1;
      gap += 1;
    }
  }

  const dateAt = (index: number) =>
    localDate(new Date(today.getFullYear(), today.getMonth(), today.getDate() - (n - 1 - index)));
  const days: DayActivity[] = words.map((value, index) => ({
    date: dateAt(index),
    words: value,
    takes: takes[index],
  }));

  return {
    days,
    currentStreak: S.currentStreak,
    longestStreak: S.longestStreak,
    longestStreakEnd: dateAt(S.longestTo),
    firstDay: dateAt(S.firstIndex),
    listeningMsTotal: Math.round((S.words / S.averageWpm) * 60_000),
    apps: APPS.map((share) => ({ ...share })),
  };
}

/** `get_stats` counters that agree with `sampleActivity(today)`. */
export function sampleStats(activity: ActivityStats, today: Date = new Date()): UsageStats {
  const sinceMonday = (today.getDay() + 6) % 7;
  const week = activity.days.slice(activity.days.length - 1 - sinceMonday);
  return {
    wordsToday: activity.days[activity.days.length - 1]?.words ?? 0,
    wordsThisWeek: week.reduce((sum, day) => sum + day.words, 0),
    wordsTotal: activity.days.reduce((sum, day) => sum + day.words, 0),
    takesTotal: activity.days.reduce((sum, day) => sum + day.takes, 0),
    averageWpm: SAMPLE_ACTIVITY.averageWpm,
  };
}
