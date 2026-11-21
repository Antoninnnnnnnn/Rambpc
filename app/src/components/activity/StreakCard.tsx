// Série: days in a row, the record, and the map of the last 16 weeks (one
// cell per day, Monday at the top, a linear scale of words per day). Today
// is ringed; the streak in progress and the record are marked under the map.

import { useMemo, type CSSProperties } from "react";
import type { ActivityStats } from "../../ipc/contract";
import { formatCount } from "../../labels/fr";
import { Glyph } from "../Icon";
import { daysText, formatDay, formatFigure, rangeText, shortDay, shortMonth, wordsText } from "./format";
import { buildDayMap, HEAT_LEVELS, type DayMap, type MapCell, type Streak } from "./model";
import { ActivityCard, DataTable, useChartTip, type TableCell } from "./parts";

const WEEKDAYS = [
  ["lun.", "lundi"],
  ["mar.", "mardi"],
  ["mer.", "mercredi"],
  ["jeu.", "jeudi"],
  ["ven.", "vendredi"],
  ["sam.", "samedi"],
  ["dim.", "dimanche"],
] as const;

const within = (cell: MapCell, streak: Streak | null) =>
  Boolean(streak && cell.date >= streak.from && cell.date <= streak.to);

function cellTip(cell: MapCell, map: DayMap): { value: string; label: string } | null {
  if (cell.kind === "outside" || cell.kind === "future") return null;
  const value =
    cell.kind === "missing" ? "donnée indisponible" : cell.words ? wordsText(cell.words) : "aucune dictée";
  const notes = [shortDay(cell.date)];
  if (cell.today) notes.push("aujourd’hui");
  if (within(cell, map.current)) notes.push(map.recordIsCurrent ? "série record en cours" : "série en cours");
  else if (within(cell, map.record)) notes.push("record");
  return { value, label: notes.join(" · ") };
}

function mapSummary(map: DayMap, weeks: number): string {
  const active = map.cells.filter((cell) => cell.kind === "day" && (cell.words ?? 0) > 0).length;
  const missing = map.cells.filter((cell) => cell.kind === "missing").length;
  const parts = [
    `Carte des jours dictés sur ${weeks} semaines, ${rangeText(map.first, map.last, map.last)} : ` +
      `${active} ${active < 2 ? "jour" : "jours"} avec au moins une dictée`,
  ];
  if (missing) parts.push(`${missing} ${missing < 2 ? "jour" : "jours"} sans donnée`);
  if (map.current) parts.push(`série en cours de ${daysText(map.current.length)} depuis le ${formatDay(map.current.from, map.last)}`);
  if (map.record && !map.recordIsCurrent) {
    parts.push(`record de ${daysText(map.record.length)} ${rangeText(map.record.from, map.record.to, map.last)}`);
  }
  return `${parts.join(" ; ")}. Le détail jour par jour est dans le tableau qui suit.`;
}

function Mark({ streak, kind, label, weeks }: { streak: Streak; kind: "current" | "record"; label: string; weeks: number }) {
  if (!streak.weeks) return null;
  const { from, to } = streak.weeks;
  const style: CSSProperties = { gridColumn: `${from + 1} / ${to + 2}`, gridRow: kind === "current" ? 1 : 2 };
  const end = (from + to + 1) / 2 > weeks / 2;
  return (
    <span className={`daymap__mark daymap__mark--${kind}${end ? " daymap__mark--end" : ""}`} style={style}>
      <span className="daymap__mark-line" />
      <span className="daymap__mark-label">{label}</span>
    </span>
  );
}

function DayMapChart({ map, weeks }: { map: DayMap; weeks: number }) {
  const { hostRef, handlers, tooltip } = useChartTip();
  const style = { "--weeks": map.weeks } as CSSProperties;

  const columns: TableCell[] = [
    { text: "Semaine" },
    ...WEEKDAYS.map(([text, spoken]) => ({ text, spoken, numeric: true })),
    { text: "Total", numeric: true },
  ];
  const rows = Array.from({ length: map.weeks }, (_, week) => {
    const cells = map.cells.slice(week * 7, week * 7 + 7);
    const total = cells.reduce((sum, cell) => sum + (cell.words ?? 0), 0);
    const hasData = cells.some((cell) => cell.kind === "day");
    return {
      key: cells[0].key,
      cells: [
        { text: `Semaine du ${formatDay(cells[0].date, map.last)}` },
        ...cells.map((cell): TableCell => {
          if (cell.kind === "outside") return { text: "—", spoken: "hors période", numeric: true };
          if (cell.kind === "future") return { text: "—", spoken: "à venir", numeric: true };
          if (cell.kind === "missing") return { text: "indisponible", numeric: true };
          const value = formatCount(cell.words ?? 0);
          return cell.today
            ? { text: value, spoken: `${value}, aujourd’hui`, numeric: true, strong: true }
            : { text: value, numeric: true };
        }),
        hasData ? { text: formatCount(total), numeric: true, strong: true } : { text: "indisponible", numeric: true },
      ],
    };
  });

  const currentLabel = map.recordIsCurrent ? "série record en cours" : "série en cours";

  return (
    <figure className="daymap" style={style}>
      <div className="daymap__frame" ref={hostRef} {...handlers}>
        <div className="daymap__months" aria-hidden="true">
          {map.months.map((month) => (
            <span key={month.week} style={{ gridColumn: month.week + 1 }}>
              {shortMonth(month.date)}
            </span>
          ))}
        </div>
        <div className="daymap__weekdays" aria-hidden="true">
          {WEEKDAYS.map(([short], index) => (
            <span key={short}>{index % 2 === 0 ? short.replace(".", "") : ""}</span>
          ))}
        </div>
        <div className="daymap__cells" role="img" aria-label={mapSummary(map, weeks)} data-testid="daymap">
          {map.cells.map((cell) => {
            const tip = cellTip(cell, map);
            return (
              <span
                key={cell.key}
                className="daymap__cell"
                data-kind={cell.kind}
                data-level={cell.kind === "day" ? cell.level : undefined}
                data-today={cell.today || undefined}
                data-tip-value={tip?.value}
                data-tip-label={tip?.label}
              />
            );
          })}
        </div>
        {map.current?.weeks || (map.record?.weeks && !map.recordIsCurrent) ? (
          <div className="daymap__marks" aria-hidden="true">
            {map.current ? (
              <Mark streak={map.current} kind="current" weeks={map.weeks} label={`${currentLabel} · ${daysText(map.current.length)}`} />
            ) : null}
            {map.record && !map.recordIsCurrent ? (
              <Mark streak={map.record} kind="record" weeks={map.weeks} label={`record · ${daysText(map.record.length)}`} />
            ) : null}
          </div>
        ) : null}
        {tooltip}
      </div>
      <figcaption className="daymap__legend">
        <span className="daymap__legend-title">Mots par jour</span>
        <ul className="daymap__legend-scale">
          <li>
            <span className="daymap__swatch" data-level={0} />0
          </li>
          {map.scale.bounds.slice(0, HEAT_LEVELS).map((bound, index) => (
            <li key={bound}>
              <span className="daymap__swatch" data-level={index + 1} />
              <span aria-hidden="true">≤ {formatFigure(bound)}</span>
              <span className="visually-hidden">
                {index === 0 ? "de 1" : `de ${formatFigure(map.scale.bounds[index - 1] + 1)}`} à {formatFigure(bound)}
              </span>
            </li>
          ))}
          <li>
            <span className="daymap__swatch daymap__swatch--today" />
            aujourd’hui
          </li>
        </ul>
      </figcaption>
      <DataTable
        summary="Voir le tableau des jours"
        caption={`Mots dictés par jour, ${weeks} dernières semaines`}
        columns={columns}
        rows={rows}
        testId="daymap-table"
      />
    </figure>
  );
}

export function StreakCard({ activity }: { activity: ActivityStats }) {
  const map = useMemo(() => buildDayMap(activity), [activity]);
  const weeks = Math.max(1, Math.round(activity.days.length / 7));
  const streak = activity.currentStreak;
  const today = map?.last ?? new Date();
  const lastWords = activity.days[activity.days.length - 1]?.words ?? 0;

  let hint: string | null = null;
  if (streak === 0) hint = "Aucune série en cours : dictez aujourd’hui pour en commencer une.";
  else if (lastWords === 0) hint = "Dictez aujourd’hui pour la prolonger.";

  const record = map?.record;
  return (
    <ActivityCard title="Série" meta={`${weeks} dernières semaines`} className="activity-streak" testId="activity-streak">
      <div className="streak">
        <p className="streak__now" data-testid="streak-current">
          <span className="streak__number">{formatFigure(streak)}</span>
          <span className="streak__unit"> {streak < 2 ? "jour d’affilée" : "jours d’affilée"}</span>
        </p>
        {record ? (
          <p className="streak__record" data-testid="streak-record">
            <Glyph name="activity" />
            <span>
              Record : <strong>{daysText(record.length)}</strong>
              {map.recordIsCurrent ? ", la série en cours" : `, ${rangeText(record.from, record.to, today)}`}
            </span>
          </p>
        ) : null}
      </div>
      {hint ? <p className="streak__hint">{hint}</p> : null}
      {map ? <DayMapChart map={map} weeks={weeks} /> : null}
    </ActivityCard>
  );
}
