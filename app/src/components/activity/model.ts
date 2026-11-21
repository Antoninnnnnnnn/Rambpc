// Pure computations for the Activité screen: local dates, the 16-week map,
// the last seven days, the shares per application, the key figures and the
// time saved against typing. Counters only, never dictated text.

import type { ActivityStats, AppShare, DayActivity, UsageStats } from "../../ipc/contract";

/** Typing speed the time saved is measured against. */
export const TYPING_WPM = 40;

// ------------------------------------------------------------------ dates

/** "YYYY-MM-DD" as a local date (midnight), or null if malformed. */
export function parseDay(value: string | null | undefined): Date | null {
  const match = value ? /^(\d{4})-(\d{2})-(\d{2})$/.exec(value) : null;
  if (!match) return null;
  return new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
}

/** Local calendar date, "YYYY-MM-DD". */
export function dayKey(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

export function addDays(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

/** Whole calendar days from `a` to `b` (daylight saving safe). */
export function daysBetween(a: Date, b: Date): number {
  return Math.round((b.getTime() - a.getTime()) / 86_400_000);
}

/** Monday 0 … Sunday 6. */
export function weekdayIndex(date: Date): number {
  return (date.getDay() + 6) % 7;
}

// ------------------------------------------------------------------ scales

/** Smallest "round" number at least `value` (1, 1.5, 2, 2.5, 3, 4, 5, 6, 8 × 10ⁿ), an integer. */
export function niceCeil(value: number, steps: readonly number[] = [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10]): number {
  if (!(value > 1)) return 1;
  const power = 10 ** Math.floor(Math.log10(value));
  const step = steps.find((candidate) => candidate * power >= value - 1e-9) ?? 10;
  return Math.ceil(step * power);
}

/** Number of colour steps of the day map above « no take ». */
export const HEAT_LEVELS = 4;

export interface HeatScale {
  /** Words per level: level k covers ((k − 1) × step, k × step]. */
  step: number;
  /** Upper bound of each level, 1 to HEAT_LEVELS. */
  bounds: number[];
}

/** Linear scale (equal word ranges) over the busiest day. */
export function heatScale(max: number): HeatScale {
  const step = niceCeil(Math.max(1, max) / HEAT_LEVELS);
  return { step, bounds: Array.from({ length: HEAT_LEVELS }, (_, index) => step * (index + 1)) };
}

export function heatLevel(words: number, scale: HeatScale): number {
  if (words <= 0) return 0;
  return Math.min(HEAT_LEVELS, Math.ceil(words / scale.step));
}

export interface Axis {
  /** Top of the value axis: bars are `words / max` tall, from zero. */
  max: number;
  ticks: number[];
}

/** Zero-based value axis with two or three round ticks and room for a label. */
export function barAxis(maxValue: number): Axis {
  if (maxValue <= 0) return { max: 1, ticks: [0] };
  const step = niceCeil(maxValue / 3, [1, 2, 2.5, 5, 10]);
  const max = Math.max(maxValue * 1.12, step);
  const ticks: number[] = [];
  for (let tick = 0; tick <= max; tick += step) ticks.push(tick);
  return { max, ticks };
}

// ------------------------------------------------------------------ day map

export type CellKind = "day" | "missing" | "future" | "outside";

export interface MapCell {
  key: string;
  date: Date;
  /** `missing`: inside the period but absent from the data (unknown, not 0). */
  kind: CellKind;
  words: number | null;
  takes: number | null;
  level: number;
  today: boolean;
  week: number;
  weekday: number;
}

export interface Streak {
  from: Date;
  to: Date;
  length: number;
  /** Columns of the map it covers, or null when entirely outside it. */
  weeks: { from: number; to: number } | null;
}

export interface DayMap {
  /** Week columns, Monday to Sunday (16 or 17: the period rarely starts on a Monday). */
  weeks: number;
  /** Column after column, Monday first. */
  cells: MapCell[];
  months: { week: number; date: Date }[];
  scale: HeatScale;
  first: Date;
  last: Date;
  current: Streak | null;
  record: Streak | null;
  /** The record is the streak in progress. */
  recordIsCurrent: boolean;
}

export function sortedDays(activity: ActivityStats): DayActivity[] {
  return activity.days.filter((day) => parseDay(day.date)).sort((a, b) => (a.date < b.date ? -1 : a.date > b.date ? 1 : 0));
}

/** Last day of the streak in progress: today, or yesterday if nothing yet today. */
export function currentStreakEnd(activity: ActivityStats): Date | null {
  const days = sortedDays(activity);
  const last = parseDay(days[days.length - 1]?.date);
  if (!last || activity.currentStreak <= 0) return null;
  return days[days.length - 1].words > 0 ? last : addDays(last, -1);
}

export function streaks(activity: ActivityStats): { current: Omit<Streak, "weeks"> | null; record: Omit<Streak, "weeks"> | null } {
  const currentEnd = currentStreakEnd(activity);
  const current =
    currentEnd && activity.currentStreak > 0
      ? { from: addDays(currentEnd, -(activity.currentStreak - 1)), to: currentEnd, length: activity.currentStreak }
      : null;
  const recordEnd = parseDay(activity.longestStreakEnd);
  const record =
    recordEnd && activity.longestStreak > 0
      ? { from: addDays(recordEnd, -(activity.longestStreak - 1)), to: recordEnd, length: activity.longestStreak }
      : null;
  return { current, record };
}

export function buildDayMap(activity: ActivityStats): DayMap | null {
  const days = sortedDays(activity);
  const first = parseDay(days[0]?.date);
  const last = parseDay(days[days.length - 1]?.date);
  if (!first || !last) return null;

  const start = addDays(first, -weekdayIndex(first));
  const end = addDays(last, 6 - weekdayIndex(last));
  const weeks = (daysBetween(start, end) + 1) / 7;
  const byDate = new Map(days.map((day) => [day.date, day]));
  const scale = heatScale(Math.max(0, ...days.map((day) => day.words)));

  const cells: MapCell[] = [];
  for (let index = 0; index < weeks * 7; index += 1) {
    const date = addDays(start, index);
    const key = dayKey(date);
    const day = byDate.get(key);
    const kind: CellKind = date < first ? "outside" : date > last ? "future" : day ? "day" : "missing";
    cells.push({
      key,
      date,
      kind,
      words: day ? day.words : null,
      takes: day ? day.takes : null,
      level: day ? heatLevel(day.words, scale) : 0,
      today: date.getTime() === last.getTime(),
      week: Math.floor(index / 7),
      weekday: index % 7,
    });
  }

  // A month name over the column where it begins, never two names closer
  // than three columns (the partial first month gives way).
  const months: { week: number; date: Date }[] = [];
  for (let week = 0; week < weeks; week += 1) {
    const date = addDays(start, week * 7);
    const previous = months[months.length - 1];
    if (previous && previous.date.getMonth() === date.getMonth()) continue;
    if (previous && week - previous.week < 3) {
      if (previous.week === 0) months.pop();
      else continue;
    }
    months.push({ week, date });
  }

  const place = (streak: Omit<Streak, "weeks"> | null): Streak | null => {
    if (!streak) return null;
    const from = streak.from < first ? first : streak.from;
    const to = streak.to > last ? last : streak.to;
    const onMap = to >= first && from <= last;
    return {
      ...streak,
      weeks: onMap ? { from: Math.floor(daysBetween(start, from) / 7), to: Math.floor(daysBetween(start, to) / 7) } : null,
    };
  };
  const { current, record } = streaks(activity);
  return {
    weeks,
    cells,
    months,
    scale,
    first,
    last,
    current: place(current),
    record: place(record),
    recordIsCurrent: Boolean(
      current && record && current.length === record.length && current.to.getTime() === record.to.getTime(),
    ),
  };
}

// ------------------------------------------------------------------ last seven days

export interface DayBar {
  key: string;
  date: Date;
  /** null: the day is missing from the data (unknown, not 0). */
  words: number | null;
  takes: number | null;
  today: boolean;
}

/** The last `count` days of the period, oldest first, the latest last. */
export function lastDays(activity: ActivityStats, count = 7): DayBar[] {
  const days = sortedDays(activity);
  const last = parseDay(days[days.length - 1]?.date);
  if (!last) return [];
  const byDate = new Map(days.map((day) => [day.date, day]));
  return Array.from({ length: count }, (_, index) => {
    const date = addDays(last, index - (count - 1));
    const day = byDate.get(dayKey(date));
    return {
      key: dayKey(date),
      date,
      words: day ? day.words : null,
      takes: day ? day.takes : null,
      today: index === count - 1,
    };
  });
}

// ------------------------------------------------------------------ applications

export interface AppRow {
  app: string;
  id?: string;
  words: number;
  /** Rounded so that every row (and « Autres ») adds up to 100. */
  percent: number;
  share: number;
  /** Bar length, 0 to 1, against a round scale over the largest share. */
  width: number;
}

export interface AppBreakdown {
  total: number;
  rows: AppRow[];
  /** Folded into « Autres » until the person asks for them. */
  rest: AppRow[];
  others: { count: number; words: number; percent: number; share: number; width: number } | null;
}

/** Largest remainder rounding: integer percents that add up to exactly 100. */
export function roundedPercents(values: number[]): number[] {
  const total = values.reduce((sum, value) => sum + value, 0);
  if (total <= 0) return values.map(() => 0);
  const exact = values.map((value) => (value / total) * 100);
  const floors = exact.map(Math.floor);
  let left = 100 - floors.reduce((sum, value) => sum + value, 0);
  const order = exact.map((value, index) => [value - floors[index], index] as const).sort((a, b) => b[0] - a[0]);
  for (const [, index] of order) {
    if (left <= 0) break;
    floors[index] += 1;
    left -= 1;
  }
  return floors;
}

/** Up to `maxRows` rows: when there are more apps, the top ones and « Autres ». */
export function appBreakdown(apps: AppShare[] | null, maxRows = 6): AppBreakdown | null {
  const valid = (apps ?? []).filter((share) => share.words > 0).sort((a, b) => b.words - a.words);
  const total = valid.reduce((sum, share) => sum + share.words, 0);
  if (total <= 0) return null;
  const percents = roundedPercents(valid.map((share) => share.words));
  const maxShare = valid[0].words / total;
  const scale = Math.min(1, Math.ceil(maxShare * 10 - 1e-9) / 10) || 1;
  const all: AppRow[] = valid.map((share, index) => ({
    app: share.app,
    id: share.id,
    words: share.words,
    percent: percents[index],
    share: share.words / total,
    width: share.words / total / scale,
  }));
  if (all.length <= maxRows) return { total, rows: all, rest: [], others: null };
  const rows = all.slice(0, maxRows - 1);
  const rest = all.slice(maxRows - 1);
  const words = rest.reduce((sum, row) => sum + row.words, 0);
  return {
    total,
    rows,
    rest,
    others: {
      count: rest.length,
      words,
      percent: rest.reduce((sum, row) => sum + row.percent, 0),
      share: words / total,
      width: words / total / scale,
    },
  };
}

// ------------------------------------------------------------------ key figures

export interface SavedTime {
  /** Typing at 40 words per minute minus the time spent speaking, whole minutes, never below 0. */
  minutes: number;
  typingMinutes: number;
  speakingMinutes: number;
  /** Speaking time derived from the average speed (no per-day data). */
  estimated: boolean;
}

export function savedTime(words: number, speakingMs: number, estimated = false): SavedTime {
  const typingMinutes = Math.floor(words / TYPING_WPM);
  const speakingMinutes = Math.floor(speakingMs / 60_000);
  return { minutes: Math.max(0, typingMinutes - speakingMinutes), typingMinutes, speakingMinutes, estimated };
}

/** null means unknown (shown as such), never 0. */
export interface KeyFigures {
  words: number | null;
  takes: number | null;
  wpm: number | null;
  saved: SavedTime | null;
}

/** The 112 days hold every take: totals can come from them. */
function daysCoverHistory(activity: ActivityStats): boolean {
  const days = sortedDays(activity);
  if (days.length === 0) return false;
  return activity.firstDay === null || activity.firstDay >= days[0].date;
}

export function keyFigures(stats: UsageStats | null, activity: ActivityStats | null): KeyFigures {
  const covered = activity && daysCoverHistory(activity) ? sortedDays(activity) : null;
  const words = stats ? stats.wordsTotal : covered ? covered.reduce((sum, day) => sum + day.words, 0) : null;
  const takes = stats ? stats.takesTotal : covered ? covered.reduce((sum, day) => sum + day.takes, 0) : null;
  const listening = activity && activity.listeningMsTotal > 0 ? activity.listeningMsTotal : null;
  const wpm = stats
    ? stats.averageWpm
    : words !== null && words > 0 && listening
      ? Math.round(words / (listening / 60_000))
      : null;

  let saved: SavedTime | null = null;
  if (words === 0) saved = savedTime(0, 0);
  else if (words !== null && listening) saved = savedTime(words, listening);
  else if (words !== null && wpm) saved = savedTime(words, (words / wpm) * 60_000, true);

  return { words, takes, wpm, saved };
}
