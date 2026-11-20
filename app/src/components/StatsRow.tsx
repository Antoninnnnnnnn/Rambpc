// Usage counters from `get_stats`: words today, this week, in total, and the
// average speed. Counts only, never text. Hidden if the counters cannot be read.

import { useId } from "react";
import { formatCount, takesText, wordsUnit } from "../labels/fr";
import { useStats } from "../state/stats";
import "./StatsRow.css";

export function StatsRow() {
  const { stats } = useStats();
  const headingId = useId();
  if (!stats) return null;

  const tiles = [
    { key: "today", value: formatCount(stats.wordsToday), unit: wordsUnit(stats.wordsToday), label: "aujourd’hui" },
    {
      key: "week",
      value: formatCount(stats.wordsThisWeek),
      unit: wordsUnit(stats.wordsThisWeek),
      label: "cette semaine",
    },
    {
      key: "total",
      value: formatCount(stats.wordsTotal),
      unit: wordsUnit(stats.wordsTotal),
      label: `au total, en ${takesText(stats.takesTotal)}`,
    },
    stats.averageWpm !== null
      ? { key: "wpm", value: formatCount(stats.averageWpm), unit: "mots/min", label: "vitesse moyenne" }
      : { key: "wpm", value: "—", unit: "", label: "vitesse moyenne, après quelques dictées" },
  ];

  return (
    <section className="stats" aria-labelledby={headingId} data-testid="stats">
      <h2 id={headingId} className="visually-hidden">
        Votre activité
      </h2>
      <dl className="stats__grid">
        {tiles.map((tile) => (
          <div key={tile.key} className="stats__tile" data-testid={`stat-${tile.key}`}>
            <dt className="stats__label">{tile.label}</dt>
            <dd className="stats__value">
              <span className="stats__number">{tile.value}</span>
              {tile.unit ? <span className="stats__unit"> {tile.unit}</span> : null}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
