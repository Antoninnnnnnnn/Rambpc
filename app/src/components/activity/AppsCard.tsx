// Par application: share of the words inserted in each application, as bars
// against a round scale. Each application opens Style. Only takes inserted in
// an application count; until there is one, `apps` is null and the card says
// when the breakdown will appear.

import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { AppShare } from "../../ipc/contract";
import { useNavigation } from "../../state/navigation";
import { Glyph } from "../Icon";
import { wordsText } from "./format";
import { appBreakdown, type AppRow } from "./model";
import { ActivityCard } from "./parts";

/** French narrow no-break space (U+202F), before « % ». */
const NNBSP = " ";

function percentText(percent: number, words: number): string {
  return percent === 0 && words > 0 ? `<${NNBSP}1${NNBSP}%` : `${percent}${NNBSP}%`;
}

function initial(name: string): string {
  return (Array.from(name.trim())[0] ?? "?").toLocaleUpperCase("fr-FR");
}

function Bar({ width }: { width: number }) {
  return (
    <span className="app-row__track" aria-hidden="true">
      <span className="app-row__bar" style={{ width: `${Math.max(0, Math.min(1, width)) * 100}%` }} />
    </span>
  );
}

export function AppsCard({ apps }: { apps: AppShare[] | null }) {
  const { go } = useNavigation();
  const hintId = useId();
  const breakdown = useMemo(() => appBreakdown(apps), [apps]);
  const [expanded, setExpanded] = useState(false);
  const firstRevealed = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (expanded) firstRevealed.current?.focus();
  }, [expanded]);

  const row = (share: AppRow, reveal = false) => (
    <li key={`${share.id ?? ""}:${share.app}`}>
      <button
        ref={reveal ? firstRevealed : undefined}
        type="button"
        className="app-row"
        data-app-id={share.id}
        aria-describedby={hintId}
        onClick={() => go("style")}
      >
        <span className="app-row__mark" aria-hidden="true">
          {initial(share.app)}
        </span>
        <span className="app-row__name">{share.app}</span>
        <Bar width={share.width} />
        <span className="app-row__value">
          {percentText(share.percent, share.words)}
          <span className="visually-hidden">, {wordsText(share.words)}</span>
        </span>
        <Glyph name="arrow-right" className="app-row__go" />
      </button>
    </li>
  );

  return (
    <ActivityCard
      title="Par application"
      meta={breakdown ? "Part des mots insérés" : undefined}
      className="activity-apps"
      testId="activity-apps"
    >
      {breakdown ? (
        <>
          <ul className="apps" data-testid="apps-list">
            {breakdown.rows.map((share) => row(share))}
            {breakdown.others && !expanded ? (
              <li key="others">
                <button
                  type="button"
                  className="app-row app-row--others"
                  aria-expanded={false}
                  onClick={() => setExpanded(true)}
                >
                  <span className="app-row__mark" aria-hidden="true">
                    …
                  </span>
                  <span className="app-row__name">
                    Autres
                    <span className="app-row__count">
                      {" "}
                      ({breakdown.others.count}
                      <span className="visually-hidden"> applications</span>)
                    </span>
                  </span>
                  <Bar width={breakdown.others.width} />
                  <span className="app-row__value">
                    {percentText(breakdown.others.percent, breakdown.others.words)}
                    <span className="visually-hidden">, {wordsText(breakdown.others.words)}. Afficher le détail</span>
                  </span>
                  <Glyph name="chevron-down" className="app-row__go" />
                </button>
              </li>
            ) : null}
            {breakdown.others && expanded ? breakdown.rest.map((share, index) => row(share, index === 0)) : null}
          </ul>
          <div className="apps__foot">
            <p id={hintId} className="apps__hint">
              Choisissez une application pour régler son style.
            </p>
            {breakdown.others && expanded ? (
              <button type="button" className="link-btn apps__fold" onClick={() => setExpanded(false)}>
                Regrouper les petites applications
              </button>
            ) : null}
          </div>
        </>
      ) : (
        <div className="apps-waiting" data-testid="apps-waiting">
          <span className="apps-waiting__icon">
            <Glyph name="apps" />
          </span>
          <div>
            <p className="apps-waiting__title">La répartition apparaîtra ici</p>
            <p className="apps-waiting__text">
              Dès votre première dictée insérée dans une application (Word, votre navigateur, votre messagerie…).
              Les textes gardés dans Sambler ne comptent pas.
            </p>
          </div>
        </div>
      )}
    </ActivityCard>
  );
}
