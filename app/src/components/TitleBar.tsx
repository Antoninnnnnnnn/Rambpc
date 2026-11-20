// Sambler's own title bar (the native one is off: `decorations: false`), the
// same on every OS. The whole bar drags the window (double-click maximizes);
// its buttons do not. « Fermer » hides Sambler to the notification area.

import { useEffect, useState, type ReactNode } from "react";
import { useBridge } from "../ipc/bridge";
import { AppMark, Glyph } from "./Icon";
import "./TitleBar.css";

export function TitleBar({ children, showBrand = false }: { children?: ReactNode; showBrand?: boolean }) {
  const { appWindow } = useBridge();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const sync = () => {
      appWindow
        .isMaximized()
        .then((value) => {
          if (!disposed) setMaximized(value);
        })
        .catch(() => {});
    };
    sync();
    appWindow
      .onResized(sync)
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [appWindow]);

  return (
    <header className="titlebar" data-tauri-drag-region="deep">
      {showBrand ? (
        <p className="titlebar__brand">
          <AppMark />
          <span>Sambler</span>
        </p>
      ) : null}
      <div className="titlebar__slot">{children}</div>
      <div className="titlebar__controls" role="group" aria-label="Fenêtre">
        <button
          type="button"
          className="titlebar__button"
          aria-label="Réduire la fenêtre"
          title="Réduire"
          onClick={() => void appWindow.minimize().catch(() => {})}
        >
          <Glyph name="minimize" />
        </button>
        <button
          type="button"
          className="titlebar__button"
          aria-label={maximized ? "Restaurer la fenêtre" : "Agrandir la fenêtre"}
          title={maximized ? "Restaurer" : "Agrandir"}
          onClick={() => void appWindow.toggleMaximize().catch(() => {})}
        >
          <Glyph name={maximized ? "restore" : "maximize"} />
        </button>
        <button
          type="button"
          className="titlebar__button titlebar__button--close"
          aria-label="Fermer la fenêtre"
          title="Fermer (Sambler reste dans la zone de notification)"
          onClick={() => void appWindow.close().catch(() => {})}
        >
          <Glyph name="close" />
        </button>
      </div>
    </header>
  );
}
