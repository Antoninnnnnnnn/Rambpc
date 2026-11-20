// Accueil: greeting, live dictation state, last result, usage counters and
// the « Récents » list (the only text this screen shows, asked on demand).

import { useEffect, useId, useRef } from "react";
import { AvailabilityBanner } from "../components/AvailabilityBanner";
import { OutcomeNotice } from "../components/OutcomeNotice";
import { RecentList, type RecentListHandle } from "../components/RecentList";
import { StatsRow } from "../components/StatsRow";
import { StatusCard } from "../components/StatusCard";
import { greeting, isTakeActive } from "../labels/fr";
import { useRecents } from "../state/recents";
import { useSettings } from "../state/settings";
import { useUiState } from "../state/ui";
import "./Home.css";

/** `focusRecents`: a navigation nonce asking to bring « Récents » into view. */
export function Home({ focusRecents = 0 }: { focusRecents?: number }) {
  const state = useUiState();
  const { settings } = useSettings();
  const recents = useRecents();
  const listRef = useRef<RecentListHandle>(null);
  const outcomeHeading = useId();
  const active = isTakeActive(state);
  const minutes = settings?.draftMinutes ?? 10;

  useEffect(() => {
    if (focusRecents) listRef.current?.focus();
  }, [focusRecents]);

  return (
    <div className="screen home">
      <header className="screen__header home__header">
        <h1 tabIndex={-1} data-screen-title className="home__greeting">
          <span className="visually-hidden">Accueil, </span>
          {greeting()}
        </h1>
        <p className="screen__lead">Votre voix, au curseur, dans toutes vos applications.</p>
      </header>

      <AvailabilityBanner />

      <StatusCard />

      {state.lastOutcome && !active ? (
        <section aria-labelledby={outcomeHeading} className="home__outcome">
          <h2 id={outcomeHeading} className="visually-hidden">
            Dernière dictée
          </h2>
          <OutcomeNotice outcome={state.lastOutcome} hasDraft={state.draft !== null} headingLevel={3} />
        </section>
      ) : null}

      <StatsRow />

      <RecentList ref={listRef} id="home-recents" store={recents} minutes={minutes} />
    </div>
  );
}
