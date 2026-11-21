// Activité: key figures (`get_stats`), the streak and the 16-week map, words
// per day over the last week and the share per application (`get_activity`).
// Counters only, never text. A native side without `get_activity` still gets
// the counters, with a short note.

import { useMemo } from "react";
import { AppsCard } from "../components/activity/AppsCard";
import { formatDay, savedExplanation } from "../components/activity/format";
import { KeyFigures } from "../components/activity/KeyFigures";
import { keyFigures, parseDay } from "../components/activity/model";
import { StreakCard } from "../components/activity/StreakCard";
import { useActivity } from "../components/activity/useActivity";
import { WeekCard } from "../components/activity/WeekCard";
import { Glyph } from "../components/Icon";
import { Notice } from "../components/Notice";
import type { ActivityStats, UsageStats } from "../ipc/contract";
import { ScreenHeader } from "./ScreenHeader";
import "../components/activity/activity.css";
import "./ActivityScreen.css";

const DEFAULT_LEAD = "Vos mots dictés, jour après jour, et les applications où vous dictez.";

function isEmpty(stats: UsageStats | null, activity: ActivityStats | null): boolean {
  const noCounters = !stats || (stats.takesTotal === 0 && stats.wordsTotal === 0);
  const noDays = !activity || (activity.firstDay === null && activity.days.every((day) => day.words === 0));
  return noCounters && noDays;
}

export function ActivityScreen() {
  const { stats, activity, loading, reload } = useActivity();
  const figures = useMemo(() => keyFigures(stats, activity), [stats, activity]);
  const first = parseDay(activity?.firstDay);
  const lastDay = parseDay(activity?.days[activity.days.length - 1]?.date) ?? new Date();
  const empty = !loading && Boolean(stats || activity) && isEmpty(stats, activity);

  let body;
  if (loading) {
    body = (
      <p className="muted" role="status">
        Chargement de l’activité…
      </p>
    );
  } else if (!stats && !activity) {
    body = (
      <Notice
        tone="error"
        title="L’activité n’a pas pu être chargée"
        testId="activity-error"
        actions={
          <button type="button" className="btn" onClick={reload}>
            Réessayer
          </button>
        }
      >
        Sambler n’a pas répondu. Vos compteurs ne sont pas perdus.
      </Notice>
    );
  } else if (empty) {
    body = (
      <div className="empty activity-empty" data-testid="activity-empty">
        <span className="empty__icon">
          <Glyph name="activity" />
        </span>
        <h2 className="section-title">Pas encore d’activité</h2>
        <p className="muted">
          Dictez une première fois : vos mots, votre série de jours et les applications où vous dictez apparaîtront ici.
        </p>
      </div>
    );
  } else {
    body = (
      <>
        <KeyFigures figures={figures} />
        {activity ? (
          <>
            <div className="activity__grid">
              <StreakCard activity={activity} />
              <WeekCard activity={activity} />
            </div>
            <AppsCard apps={activity.apps} />
          </>
        ) : (
          <Notice
            tone="info"
            quiet
            title="Détail par jour indisponible"
            testId="activity-fallback"
            actions={
              <button type="button" className="btn btn--sm" onClick={reload}>
                Réessayer
              </button>
            }
          >
            La série, les mots par jour et la répartition par application ne sont pas disponibles pour le moment.
            Les compteurs ci-dessus restent exacts.
          </Notice>
        )}
        {figures.saved && figures.words !== null && figures.words > 0 ? (
          <p className="activity__method" data-testid="activity-method">
            {savedExplanation(figures.words, figures.saved)}
          </p>
        ) : null}
      </>
    );
  }

  return (
    <div className="screen activity">
      <ScreenHeader title="Activité">
        {first && !empty ? `Depuis votre première dictée, le ${formatDay(first, lastDay)}.` : DEFAULT_LEAD}
      </ScreenHeader>
      {body}
    </div>
  );
}
