// The four key figures: words, takes, words per minute and time saved
// against typing at 40 words per minute. An unknown value says so; it is
// never shown as 0.

import { useId, type ReactNode } from "react";
import { formatFigure, spokenDuration, splitMinutes } from "./format";
import type { KeyFigures as Figures } from "./model";

interface Tile {
  key: string;
  label: string;
  /** Second line of the label (method, or why the value is unknown). */
  detail?: string;
  value: ReactNode;
}

const UNKNOWN = (
  <>
    <span aria-hidden="true">—</span>
    <span className="visually-hidden">indisponible</span>
  </>
);

function Duration({ minutes }: { minutes: number }) {
  const { hours, minutes: rest } = splitMinutes(minutes);
  return (
    <>
      <span aria-hidden="true">
        {hours > 0 ? (
          <>
            {formatFigure(hours)}
            <small>h</small>
            {String(rest).padStart(2, "0")}
          </>
        ) : (
          <>
            {rest}
            <small>min</small>
          </>
        )}
      </span>
      <span className="visually-hidden">{spokenDuration(minutes)}</span>
    </>
  );
}

export function KeyFigures({ figures }: { figures: Figures }) {
  const headingId = useId();
  const { words, takes, wpm, saved } = figures;
  const tiles: Tile[] = [
    {
      key: "words",
      label: words !== null && words < 2 ? "mot au total" : "mots au total",
      detail: words === null ? "total indisponible" : undefined,
      value: words === null ? UNKNOWN : formatFigure(words),
    },
    {
      key: "takes",
      label: takes !== null && takes < 2 ? "dictée" : "dictées",
      detail: takes === null ? "total indisponible" : undefined,
      value: takes === null ? UNKNOWN : formatFigure(takes),
    },
    {
      key: "wpm",
      label: "mots/min en moyenne",
      detail: wpm === null ? "après quelques dictées" : undefined,
      value: wpm === null ? UNKNOWN : formatFigure(wpm),
    },
    {
      key: "saved",
      label: saved && saved.minutes < 2 ? "gagnée depuis le début" : "gagnées depuis le début",
      detail: saved === null ? "durée de parole inconnue" : saved.estimated ? "estimation vs frappe à 40 mots/min" : "vs frappe à 40 mots/min",
      value: saved === null ? UNKNOWN : <Duration minutes={saved.minutes} />,
    },
  ];

  return (
    <section className="activity-figures" aria-labelledby={headingId} data-testid="activity-figures">
      <h2 id={headingId} className="visually-hidden">
        Depuis le début
      </h2>
      <dl className="activity-figures__grid">
        {tiles.map((tile) => (
          <div key={tile.key} className="activity-figure" data-testid={`figure-${tile.key}`}>
            <dt className="activity-figure__label">
              {tile.label}
              {tile.detail ? (
                <>
                  <span className="visually-hidden">, </span>
                  <span className="activity-figure__detail">{tile.detail}</span>
                </>
              ) : null}
            </dt>
            <dd className="activity-figure__value">{tile.value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
