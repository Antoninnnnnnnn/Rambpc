import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { AnnouncerProvider } from "./a11y/announcer";
import { StateAnnouncer } from "./a11y/StateAnnouncer";
import { AppMark, Glyph, StatusIcon, type GlyphName } from "./components/Icon";
import { BridgeProvider, type Bridge } from "./ipc/bridge";
import { describeStatus, hasConsent } from "./labels/fr";
import { FirstRun } from "./screens/FirstRun";
import { DraftScreen } from "./screens/DraftScreen";
import { HistoryScreen } from "./screens/HistoryScreen";
import { Home } from "./screens/Home";
import { SettingsScreen } from "./screens/SettingsScreen";
import { AccessProvider } from "./state/access";
import {
  NavigationContext,
  useNavigation,
  type FirstRunStep,
  type Navigation,
  type SettingsSection,
  type View,
} from "./state/navigation";
import { SettingsProvider } from "./state/settings";
import { UiStateProvider, useUiState, useUiStore } from "./state/ui";
import "./styles/layout.css";

/** Everything the screens need: bridge, live region and the three stores. */
export function AppProviders({ bridge, children }: { bridge: Bridge; children: ReactNode }) {
  return (
    <BridgeProvider bridge={bridge}>
      <AnnouncerProvider>
        <UiStateProvider>
          <SettingsProvider>
            <AccessProvider>{children}</AccessProvider>
          </SettingsProvider>
        </UiStateProvider>
      </AnnouncerProvider>
    </BridgeProvider>
  );
}

export default function App() {
  const { state, failed } = useUiStore();
  if (!state) {
    return (
      <main className="startup" id="main">
        <AppMark />
        <h1 className="startup__title">Sambler</h1>
        <p role={failed ? "alert" : undefined}>
          {failed ? "Sambler ne répond pas. Fermez la fenêtre puis rouvrez Sambler." : "Chargement…"}
        </p>
      </main>
    );
  }
  return <Shell />;
}

const NAV_ITEMS: { view: View; label: string; glyph: GlyphName }[] = [
  { view: "home", label: "Accueil", glyph: "home" },
  { view: "draft", label: "Brouillon", glyph: "draft" },
  { view: "history", label: "Historique", glyph: "history" },
  { view: "settings", label: "Réglages", glyph: "settings" },
];

function Shell() {
  const state = useUiState();
  const [firstRun, setFirstRun] = useState<FirstRunStep | null>(() =>
    hasConsent(state.availability) ? null : "voice",
  );
  const [view, setView] = useState<View>("home");
  const [section, setSection] = useState<{ id?: SettingsSection; nonce: number }>({ nonce: 0 });
  // Focus follows navigation, but the window does not grab focus at startup.
  const [navigated, setNavigated] = useState(false);

  const go = useCallback((next: View, target?: SettingsSection) => {
    setNavigated(true);
    setFirstRun(null);
    setView(next);
    setSection((current) => ({ id: target, nonce: current.nonce + 1 }));
  }, []);
  const openFirstRun = useCallback((step: FirstRunStep = "voice") => {
    setNavigated(true);
    setFirstRun(step);
  }, []);
  const navigation = useMemo<Navigation>(() => ({ view, go, openFirstRun }), [view, go, openFirstRun]);

  return (
    <NavigationContext.Provider value={navigation}>
      <StateAnnouncer />
      {firstRun ? (
        <FirstRun key={firstRun} initialStep={firstRun} openedByPerson={navigated} onClose={() => go("home")} />
      ) : (
        <MainLayout view={view} section={section} autoFocus={navigated} />
      )}
    </NavigationContext.Provider>
  );
}

interface MainLayoutProps {
  view: View;
  section: { id?: SettingsSection; nonce: number };
  autoFocus: boolean;
}

function MainLayout({ view, section, autoFocus }: MainLayoutProps) {
  const state = useUiState();
  const { go, openFirstRun } = useNavigation();
  const mainRef = useRef<HTMLElement>(null);
  const status = describeStatus(state);

  // After navigation, move focus to the new screen's title (a settings
  // section, when targeted, takes the focus itself).
  useEffect(() => {
    if (!autoFocus) return;
    if (view === "settings" && section.id) return;
    mainRef.current?.querySelector<HTMLElement>("[data-screen-title]")?.focus();
  }, [view, section, autoFocus]);

  return (
    <div className="app">
      <a
        className="skip-link"
        href="#main"
        onClick={(event) => {
          event.preventDefault();
          mainRef.current?.focus();
        }}
      >
        Aller au contenu
      </a>
      <aside className="sidebar">
        <p className="sidebar__brand">
          <AppMark />
          <span>Sambler</span>
        </p>
        <nav className="sidebar__nav" aria-label="Navigation principale">
          <ul>
            {NAV_ITEMS.map((item) => (
              <li key={item.view}>
                <button
                  type="button"
                  className="nav-item"
                  aria-current={view === item.view ? "page" : undefined}
                  onClick={() => go(item.view)}
                >
                  <Glyph name={item.glyph} />
                  <span className="nav-item__label">{item.label}</span>
                  {item.view === "draft" && state.draft ? (
                    <span className="nav-item__badge">
                      1<span className="visually-hidden"> texte en attente</span>
                    </span>
                  ) : null}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        <div className="sidebar__footer">
          <p className={`sidebar__status sidebar__status--${status.shape}`} data-testid="sidebar-status">
            <StatusIcon shape={status.shape} />
            <span>{status.label}</span>
          </p>
          <button type="button" className="nav-item nav-item--small" onClick={() => openFirstRun("voice")}>
            <Glyph name="guide" />
            <span className="nav-item__label">Prise en main</span>
          </button>
        </div>
      </aside>
      <main className="main" id="main" ref={mainRef} tabIndex={-1}>
        {view === "home" ? (
          <Home />
        ) : view === "draft" ? (
          <DraftScreen />
        ) : view === "history" ? (
          <HistoryScreen />
        ) : (
          <SettingsScreen key={section.nonce} section={section.id} />
        )}
      </main>
    </div>
  );
}
