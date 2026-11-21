// Main navigation, the live dictation state and the way back to the guide.

import type { UiState } from "../ipc/contract";
import { describeStatus } from "../labels/fr";
import { useNavigation, type View } from "../state/navigation";
import { useUiState } from "../state/ui";
import { Keys } from "./controls";
import { Glyph, StatusIcon, type GlyphName } from "./Icon";
import "./Sidebar.css";

export const NAV_ITEMS: { view: View; label: string; glyph: GlyphName }[] = [
  { view: "home", label: "Accueil", glyph: "home" },
  { view: "carnet", label: "Carnet", glyph: "notebook" },
  { view: "dictionary", label: "Dictionnaire", glyph: "dictionary" },
  { view: "shortcuts", label: "Raccourcis vocaux", glyph: "shortcuts" },
  { view: "style", label: "Style", glyph: "style" },
  { view: "history", label: "Historique", glyph: "history" },
  { view: "activity", label: "Activité", glyph: "activity" },
];

/** A result of the latest take waits in Sambler (not inserted, or only partly). */
export function textWaiting(state: UiState): boolean {
  const outcome = state.lastOutcome;
  if (!state.draft || !outcome) return false;
  return (
    outcome.kind === "kept-in-sambler" ||
    outcome.kind === "inserted-partially" ||
    (outcome.kind === "interrupted" && outcome.recovered)
  );
}

export function Sidebar() {
  const state = useUiState();
  const { view, go, openFirstRun } = useNavigation();
  const status = describeStatus(state);
  const waiting = textWaiting(state);
  const idleReady = state.phase.kind === "idle" && state.availability.kind === "ready";

  const item = (target: View, label: string, glyph: GlyphName, badge = false) => (
    <li key={target}>
      <button
        type="button"
        className="nav-item"
        aria-current={view === target ? "page" : undefined}
        title={label}
        onClick={() => go(target)}
      >
        <Glyph name={glyph} />
        <span className="nav-item__label">{label}</span>
        {badge ? (
          <span className="nav-item__badge" data-testid="nav-badge">
            <span className="visually-hidden"> : un texte vous attend</span>
          </span>
        ) : null}
      </button>
    </li>
  );

  return (
    <aside className="sidebar">
      <nav className="sidebar__nav" aria-label="Navigation principale">
        <ul>
          {NAV_ITEMS.map((entry) => item(entry.view, entry.label, entry.glyph, entry.view === "home" && waiting))}
        </ul>
        <ul className="sidebar__nav-end">{item("settings", "Réglages", "settings")}</ul>
      </nav>
      <div className="sidebar__footer">
        <div className={`sidebar__status sidebar__status--${status.shape}`} data-testid="sidebar-status">
          <span className="sidebar__status-icon">
            <StatusIcon shape={status.shape} />
          </span>
          <span className="sidebar__status-text">
            <span className="sidebar__status-label">{status.label}</span>
            {idleReady ? (
              <span className="sidebar__status-keys">
                <span className="visually-hidden">Raccourci : </span>
                <Keys shortcut={state.shortcut} />
              </span>
            ) : null}
          </span>
        </div>
        <button type="button" className="nav-item nav-item--quiet" title="Prise en main" onClick={() => openFirstRun("consent")}>
          <Glyph name="guide" />
          <span className="nav-item__label">Prise en main</span>
        </button>
      </div>
    </aside>
  );
}
