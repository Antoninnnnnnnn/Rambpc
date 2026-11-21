// The shell: own title bar (frameless window), sidebar navigation, focus
// after navigation and the « text waiting » cue.
import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { completeDraft, createFakeBridge } from "./test/fakeBridge";
import { appReady, axeViolations, renderApp } from "./test/render";

describe("title bar", () => {
  it("drags the window from the whole bar, never from its buttons", async () => {
    renderApp(createFakeBridge());
    await appReady();
    const bar = document.querySelector("header.titlebar")!;
    expect(bar).toHaveAttribute("data-tauri-drag-region", "deep");
    for (const button of within(bar as HTMLElement).getAllByRole("button")) {
      expect(button).not.toHaveAttribute("data-tauri-drag-region");
    }
  });

  it("minimizes, maximizes and restores, and hides the window", async () => {
    const bridge = createFakeBridge();
    const { user } = renderApp(bridge);
    await appReady();
    const controls = screen.getByRole("group", { name: "Fenêtre" });
    await user.click(within(controls).getByRole("button", { name: "Réduire la fenêtre" }));
    expect(bridge.appWindow.minimize).toHaveBeenCalledTimes(1);

    await user.click(within(controls).getByRole("button", { name: "Agrandir la fenêtre" }));
    expect(bridge.appWindow.toggleMaximize).toHaveBeenCalledTimes(1);
    act(() => bridge.appWindow.resize(true));
    expect(await within(controls).findByRole("button", { name: "Restaurer la fenêtre" })).toBeInTheDocument();
    act(() => bridge.appWindow.resize(false));
    expect(await within(controls).findByRole("button", { name: "Agrandir la fenêtre" })).toBeInTheDocument();

    await user.click(within(controls).getByRole("button", { name: "Fermer la fenêtre" }));
    expect(bridge.appWindow.close).toHaveBeenCalledTimes(1);
  });

  it("is present while loading, so the window can always be moved or hidden", () => {
    const bridge = createFakeBridge();
    bridge.handlers.get_state = () => new Promise(() => {});
    renderApp(bridge);
    expect(screen.getByText("Chargement…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Fermer la fenêtre" })).toBeInTheDocument();
  });
});

describe("navigation", () => {
  it("offers the eight destinations and marks the current one", async () => {
    const { user } = renderApp(createFakeBridge());
    await appReady();
    const nav = screen.getByRole("navigation", { name: "Navigation principale" });
    const names = within(nav).getAllByRole("button").map((button) => button.textContent);
    expect(names).toEqual([
      "Accueil",
      "Carnet",
      "Dictionnaire",
      "Raccourcis vocaux",
      "Style",
      "Historique",
      "Activité",
      "Réglages",
    ]);
    expect(within(nav).getByRole("button", { name: "Accueil" })).toHaveAttribute("aria-current", "page");

    for (const [button, title] of [
      ["Carnet", "Carnet"],
      ["Dictionnaire", "Dictionnaire"],
      ["Raccourcis vocaux", "Raccourcis vocaux"],
      ["Style", "Style"],
      ["Historique", "Historique"],
      ["Activité", "Activité"],
      ["Réglages", "Réglages"],
    ]) {
      await user.click(within(nav).getByRole("button", { name: button }));
      expect(await screen.findByRole("heading", { level: 1, name: title })).toHaveFocus();
      expect(within(nav).getByRole("button", { name: button })).toHaveAttribute("aria-current", "page");
    }
  });

  it("does not grab the focus at startup", async () => {
    renderApp(createFakeBridge());
    await appReady();
    expect(screen.getByRole("heading", { level: 1 })).not.toHaveFocus();
  });

  it("skips to the content", async () => {
    const { user } = renderApp(createFakeBridge());
    await appReady();
    await user.tab();
    const skip = screen.getByRole("link", { name: "Aller au contenu" });
    expect(skip).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("main")).toHaveFocus();
  });

  it("shows the live state and the shortcut in the sidebar", async () => {
    const bridge = createFakeBridge();
    renderApp(bridge);
    await appReady();
    const status = screen.getByTestId("sidebar-status");
    expect(status).toHaveTextContent("Disponible");
    expect(status).toHaveTextContent("Raccourci : Ctrl+Maj+F2");
    expect(status.querySelector('svg[data-shape="available"]')).not.toBeNull();
    act(() => {
      bridge.pushState({ phase: { kind: "listening" }, take: 1, listeningMs: 0 });
    });
    expect(status).toHaveTextContent("Écoute");
    expect(status).not.toHaveTextContent("Raccourci");
  });

  it("marks Accueil while a text waits in Sambler", async () => {
    const draft = completeDraft("Gardé.");
    const bridge = createFakeBridge();
    renderApp(bridge);
    await appReady();
    expect(screen.queryByTestId("nav-badge")).not.toBeInTheDocument();
    act(() => {
      bridge.pushState({
        draft: draft.meta,
        recentCount: 1,
        lastOutcome: { kind: "kept-in-sambler", reason: { kind: "uncertain-target", reason: "elevated" } },
      });
    });
    expect(screen.getByRole("button", { name: "Accueil : un texte vous attend" })).toBeInTheDocument();
    act(() => {
      bridge.pushState({ lastOutcome: { kind: "inserted" } });
    });
    await waitFor(() => expect(screen.queryByTestId("nav-badge")).not.toBeInTheDocument());
  });

  it("has no detectable accessibility violations on every screen", async () => {
    const { user } = renderApp(createFakeBridge());
    await appReady();
    const nav = screen.getByRole("navigation", { name: "Navigation principale" });
    for (const name of ["Dictionnaire", "Raccourcis vocaux", "Historique"]) {
      await user.click(within(nav).getByRole("button", { name }));
      await screen.findByRole("heading", { level: 1, name });
      await waitFor(() => expect(screen.queryByText(/^Chargement/)).not.toBeInTheDocument());
      expect(await axeViolations()).toEqual([]);
    }
  });
});
