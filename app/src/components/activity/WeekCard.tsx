// Mots par jour: the last seven days as columns from a zero baseline, round
// ticks, the busiest day and today labelled, every value in the table view.

import { useMemo } from "react";
import type { ActivityStats } from "../../ipc/contract";
import { formatCount, takesText } from "../../labels/fr";
import { formatFigure, relativeDay, shortDay, shortWeekday, wordsText } from "./format";
import { barAxis, lastDays, type DayBar } from "./model";
import { ActivityCard, DataTable, useChartTip } from "./parts";

function spoken(bar: DayBar, today: Date): string {
  const value = bar.words === null ? "donnée indisponible" : wordsText(bar.words);
  return `${relativeDay(bar.date, today)}, ${value}`;
}

export function WeekCard({ activity }: { activity: ActivityStats }) {
  const bars = useMemo(() => lastDays(activity, 7), [activity]);
  const { hostRef, handlers, tooltip } = useChartTip();
  if (bars.length === 0) return null;

  const today = bars[bars.length - 1].date;
  const values = bars.map((bar) => bar.words ?? 0);
  const total = values.reduce((sum, value) => sum + value, 0);
  const max = Math.max(...values);
  const axis = barAxis(max);
  const busiest = max > 0 ? values.lastIndexOf(max) : -1;
  const unknown = bars.filter((bar) => bar.words === null).length;
  const label =
    `Mots par jour sur les 7 derniers jours : ${bars.map((bar) => spoken(bar, today)).join(" ; ")}.` +
    (unknown ? "" : ` Total : ${wordsText(total)}.`);

  return (
    <ActivityCard
      title="Mots par jour"
      meta={`7 derniers jours · ${unknown ? "total partiel, " : ""}${wordsText(total)}`}
      className="activity-week"
      testId="activity-week"
    >
      <figure className="week-chart">
        <div className="week-chart__frame" ref={hostRef} {...handlers}>
          <div className="week-chart__plot" role="img" aria-label={label} data-testid="week-chart">
            {axis.ticks.map((tick) => (
              <span key={tick} className="week-chart__tick" style={{ bottom: `${(tick / axis.max) * 100}%` }}>
                <span className="week-chart__tick-label">{formatFigure(tick)}</span>
              </span>
            ))}
            <div className="week-chart__bars">
              {bars.map((bar, index) => {
                const words = bar.words;
                const labelled = words === null || words === 0 || index === busiest || bar.today;
                return (
                  <div
                    key={bar.key}
                    className="week-chart__col"
                    data-today={bar.today || undefined}
                    data-tip-anchor="first"
                    data-tip-value={words === null ? "donnée indisponible" : wordsText(words)}
                    data-tip-label={bar.today ? `aujourd’hui · ${shortDay(bar.date)}` : shortDay(bar.date)}
                  >
                    {labelled ? (
                      <span className="week-chart__value" data-unknown={words === null || undefined}>
                        {words === null ? "—" : formatFigure(words)}
                      </span>
                    ) : null}
                    <span
                      className="week-chart__bar"
                      data-today={bar.today || undefined}
                      style={{ height: `${((words ?? 0) / axis.max) * 100}%` }}
                    />
                  </div>
                );
              })}
            </div>
          </div>
          <div className="week-chart__days" aria-hidden="true">
            {bars.map((bar) => (
              <span key={bar.key} data-today={bar.today || undefined}>
                {bar.today ? "auj." : shortWeekday(bar.date)}
              </span>
            ))}
          </div>
          {tooltip}
        </div>
        <DataTable
          summary="Voir le tableau des 7 jours"
          caption="Mots et dictées par jour, 7 derniers jours"
          columns={[{ text: "Jour" }, { text: "Mots", numeric: true }, { text: "Dictées", numeric: true }]}
          rows={bars.map((bar) => ({
            key: bar.key,
            cells: [
              { text: relativeDay(bar.date, today), strong: bar.today },
              bar.words === null ? { text: "indisponible", numeric: true } : { text: formatCount(bar.words), numeric: true },
              bar.takes === null
                ? { text: "indisponible", numeric: true }
                : { text: formatCount(bar.takes), spoken: takesText(bar.takes), numeric: true },
            ],
          }))}
          testId="week-table"
        />
      </figure>
    </ActivityCard>
  );
}
