import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { AnnouncerProvider } from "./a11y/announcer";
import { StateAnnouncer } from "./a11y/StateAnnouncer";
import { AppMark } from "./components/Icon";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { BridgeProvider, type Bridge } from "./ipc/bridge";
import { hasConsent } from "./labels/fr";
import { DictionaryScreen } from "./screens/DictionaryScreen";
import { FirstRun } from "./screens/FirstRun";
import { HistoryScreen } from "./screens/HistoryScreen";
import { Home } from "./screens/Home";
import { SettingsScreen } from "./screens/SettingsScreen";
import { ShortcutsScreen } from "./screens/ShortcutsScreen";
import { StyleScreen } from "./screens/StyleScreen";
import { AccessProvider } from "./state/access";
import { DraftSafetyProvider } from "./state/draftSafety";
import {
  NavigationContext,
  type FirstRunStep,
  type Navigation,
  type Section,
  type SettingsSection,
  type View,
} from "./state/navigation";
import { SettingsProvider } from "./state/settings";
import { UiStateProvider, useUiState, useUiStore } from "./state/ui";
import "./styles/layout.css";

/** Everything the screens need: bridge, live region and the stores. */
export function AppProviders({ bridge, children }: { bridge: Bridge; children: ReactNode }) {
  return (
    <BridgeProvider bridge={bridge}>
      <AnnouncerProvider>
        <UiStateProvider>
          <SettingsProvider>
            <AccessProvider>
              <DraftSafetyProvider>{children}</DraftSafetyProvider>
            </AccessProvider>
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
      <div className="frame">
        <TitleBar />
        <main className="startup" id="main">
          <AppMark className="startup__mark" />
          <h1 className="startup__title">Sambler</h1>
          <p role={failed ? "alert" : undefined} className="muted">
            {failed ? "Sambler ne répond pas. Fermez la fenêtre puis rouvrez Sambler." : "Chargement…"}
          </p>
        </main>
      </div>
    );
  }
  return <Shell />;
}

interface Target {
  section?: Section;
  nonce: number;
}

function Shell() {
  const state = useUiState();
  const [firstRun, setFirstRun] = useState<FirstRunStep | null>(() =>
    hasConsent(state.availability) ? null : "consent",
  );
  const [view, setView] = useState<View>("home");
  const [target, setTarget] = useState<Target>({ nonce: 0 });
  // Focus follows navigation, but the window does not grab focus at startup.
  const [navigated, setNavigated] = useState(false);

  const go = useCallback((next: View, section?: Section) => {
    setNavigated(true);
    setFirstRun(null);
    setView(next);
    setTarget((current) => ({ section, nonce: current.nonce + 1 }));
  }, []);
  const openFirstRun = useCallback((step: FirstRunStep = "consent") => {
    setNavigated(true);
    setFirstRun(step);
  }, []);
  const navigation = useMemo<Navigation>(() => ({ view, go, openFirstRun }), [view, go, openFirstRun]);

  return (
    <NavigationContext.Provider value={navigation}>
      <StateAnnouncer />
      {firstRun ? (
        <div className="frame">
          <TitleBar showBrand />
          <FirstRun key={firstRun} initialStep={firstRun} openedByPerson={navigated} onClose={() => go("home")} />
        </div>
      ) : (
        <MainLayout view={view} target={target} autoFocus={navigated} />
      )}
    </NavigationContext.Provider>
  );
}

interface MainLayoutProps {
  view: View;
  target: Target;
  autoFocus: boolean;
}

function MainLayout({ view, target, autoFocus }: MainLayoutProps) {
  const mainRef = useRef<HTMLElement>(null);

  // After navigation, move focus to the new screen's title (a targeted
  // section takes the focus itself).
  useEffect(() => {
    if (!autoFocus || target.section) return;
    mainRef.current?.scrollTo?.({ top: 0 });
    mainRef.current?.querySelector<HTMLElement>("[data-screen-title]")?.focus();
  }, [view, target, autoFocus]);

  const settingsSection = view === "settings" ? (target.section as SettingsSection | undefined) : undefined;

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
      <TitleBar showBrand />
      <Sidebar />
      <main className="main" id="main" ref={mainRef} tabIndex={-1}>
        <div className="main__inner">
          {view === "home" ? (
            <Home focusRecents={target.section === "home-recents" ? target.nonce : 0} />
          ) : view === "dictionary" ? (
            <DictionaryScreen />
          ) : view === "shortcuts" ? (
            <ShortcutsScreen />
          ) : view === "style" ? (
            <StyleScreen />
          ) : view === "history" ? (
            <HistoryScreen />
          ) : (
            <SettingsScreen key={target.nonce} section={settingsSection} />
          )}
        </div>
      </main>
    </div>
  );
}
