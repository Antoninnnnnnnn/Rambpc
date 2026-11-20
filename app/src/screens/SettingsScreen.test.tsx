import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Settings, UiState } from "../ipc/contract";
import { createFakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, renderApp } from "../test/render";

async function openSettings(options: { state?: Partial<UiState>; settings?: Partial<Settings> } = {}) {
  const bridge = createFakeBridge(options);
  const view = renderApp(bridge);
  await appReady();
  await view.user.click(screen.getByRole("button", { name: "Réglages" }));
  await screen.findByRole("heading", { level: 1, name: "Réglages" });
  return { bridge, ...view };
}

const savedSettings = (bridge: ReturnType<typeof createFakeBridge>) =>
  bridge.invoke.mock.calls
    .filter(([command]) => command === "save_settings")
    .map(([, args]) => (args as { settings: Settings }).settings);

const lastSaved = (bridge: ReturnType<typeof createFakeBridge>) => {
  const saved = savedSettings(bridge);
  return saved[saved.length - 1];
};

describe("Réglages", () => {
  it("says that changes apply to the next dictation", async () => {
    await openSettings();
    expect(screen.getByText(/s’applique à la prochaine dictée/)).toBeInTheDocument();
  });

  it("saves each change with save_settings", async () => {
    const { bridge, user } = await openSettings();
    await user.click(screen.getByRole("radio", { name: "Anglais" }));
    await user.click(screen.getByRole("switch", { name: "Toujours garder le texte dans Sambler" }));
    await user.click(screen.getByRole("radio", { name: "5 minutes" }));
    await user.click(screen.getByRole("radio", { name: /Fidèle/ }));
    await waitFor(() => expect(savedSettings(bridge)).toHaveLength(4));
    expect(lastSaved(bridge)).toMatchObject({ language: "english", alwaysKeepInSambler: true, draftMinutes: 5, profile: "faithful" });
    // Faithful ignores the Naturel rules.
    expect(screen.getByRole("switch", { name: "Ponctuation légère" })).toBeDisabled();
  });

  it("rolls back a refused change and shows the error", async () => {
    const { bridge, user } = await openSettings();
    bridge.handlers.save_settings = () => {
      throw { code: "invalid", message: "Valeur refusée." };
    };
    const sound = screen.getByRole("switch", { name: "Son de début et de fin" });
    await user.click(sound);
    expect(await screen.findByText("Ce réglage n’a pas pu être enregistré. Valeur refusée.")).toBeInTheDocument();
    expect(sound).not.toBeChecked();
  });

  it("captures a new shortcut in accelerator syntax", async () => {
    const { bridge, user } = await openSettings();
    await user.click(screen.getByRole("button", { name: "Modifier le raccourci" }));
    const capture = screen.getByRole("textbox", { name: "Nouvelle combinaison" });
    expect(capture).toHaveFocus();
    fireEvent.keyDown(capture, { key: "Control", code: "ControlLeft", ctrlKey: true });
    fireEvent.keyDown(capture, { key: "Shift", code: "ShiftLeft", ctrlKey: true, shiftKey: true });
    expect(capture).toHaveTextContent("Ctrl+Maj+…");
    fireEvent.keyDown(capture, { key: "D", code: "KeyD", ctrlKey: true, shiftKey: true });
    await waitFor(() => expect(lastSaved(bridge)?.shortcut).toBe("Ctrl+Shift+D"));
    expect(screen.getByTestId("shortcut-value")).toHaveTextContent("Ctrl+Maj+D");
    expect(screen.getByRole("button", { name: "Modifier le raccourci" })).toHaveFocus();
  });

  it.each([
    [{ key: "Meta", code: "MetaLeft", metaKey: true }, "Les combinaisons avec la touche Windows sont réservées à Windows."],
    [{ key: "F12", code: "F12", ctrlKey: true }, "F12 est réservée par Windows : choisissez une autre touche."],
    [{ key: "k", code: "KeyK", ctrlKey: true, altKey: true }, "Ctrl+Alt sert à écrire des caractères (AltGr) : choisissez une autre combinaison."],
  ])("rejects %o", async (event, message) => {
    const { bridge, user } = await openSettings();
    await user.click(screen.getByRole("button", { name: "Modifier le raccourci" }));
    const capture = screen.getByRole("textbox", { name: "Nouvelle combinaison" });
    fireEvent.keyDown(capture, event);
    expect(screen.getByText(message)).toBeInTheDocument();
    expect(capture).toHaveAttribute("aria-invalid", "true");
    expect(savedSettings(bridge)).toHaveLength(0);
  });

  it("rejects bare modifiers and cancels with Escape", async () => {
    const { bridge, user } = await openSettings();
    await user.click(screen.getByRole("button", { name: "Modifier le raccourci" }));
    const capture = screen.getByRole("textbox", { name: "Nouvelle combinaison" });
    fireEvent.keyDown(capture, { key: "Control", code: "ControlLeft", ctrlKey: true });
    fireEvent.keyUp(capture, { key: "Control", code: "ControlLeft" });
    expect(
      screen.getByText("Ajoutez une touche principale : une lettre, un chiffre, Espace ou une touche de fonction."),
    ).toBeInTheDocument();
    fireEvent.keyDown(capture, { key: "Escape", code: "Escape" });
    expect(screen.queryByRole("textbox", { name: "Nouvelle combinaison" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Modifier le raccourci" })).toHaveFocus();
    expect(savedSettings(bridge)).toHaveLength(0);
  });

  it("keeps the old shortcut when the native side refuses the new one", async () => {
    const { bridge, user } = await openSettings();
    bridge.handlers.save_settings = () => {
      throw { code: "shortcut-unavailable", message: "Cette combinaison est déjà utilisée." };
    };
    await user.click(screen.getByRole("button", { name: "Modifier le raccourci" }));
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Nouvelle combinaison" }), {
      key: "J",
      code: "KeyJ",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(await screen.findByText(/Le raccourci n’a pas pu être enregistré/)).toHaveTextContent(
      "Le raccourci n’a pas pu être enregistré. Cette combinaison est déjà utilisée. L’ancien raccourci reste actif : Ctrl+Maj+F2.",
    );
    expect(screen.getByTestId("shortcut-value")).toHaveTextContent("Ctrl+Maj+F2");
  });

  it("lets Tab leave the capture field (no keyboard trap)", async () => {
    const { user } = await openSettings();
    await user.click(screen.getByRole("button", { name: "Modifier le raccourci" }));
    await user.tab();
    expect(screen.queryByRole("textbox", { name: "Nouvelle combinaison" })).not.toBeInTheDocument();
  });

  it("lists microphones and flags a missing chosen one", async () => {
    await openSettings({ settings: { microphone: { kind: "named", id: "gone", name: "Micro Bluetooth" } } });
    const select = screen.getByRole("combobox", { name: "Micro" });
    await within(select).findByRole("option", { name: "Casque USB" });
    expect(within(select).getByRole("option", { name: "Micro par défaut de Windows (Réseau de microphones)" })).toBeInTheDocument();
    expect(within(select).getByRole("option", { name: "Micro Bluetooth (introuvable)" })).toBeInTheDocument();
    expect(screen.getByText(/Ce micro n’est pas branché/)).toBeInTheDocument();
  });

  it("links to the microphone privacy settings when Windows denies access", async () => {
    const { bridge, user } = await openSettings();
    bridge.handlers.start_mic_test = () => {
      throw { code: "mic-access-denied", message: "Accès au micro refusé par Windows." };
    };
    await user.click(screen.getByRole("button", { name: "Tester le micro" }));
    await user.click(await screen.findByRole("button", { name: "Ouvrir les réglages de confidentialité du micro" }));
    expect(bridge.invoke).toHaveBeenCalledWith("open_mic_privacy_settings");
  });

  it("shows the access status without secrets and can clear it", async () => {
    const { bridge, user } = await openSettings();
    const access = screen.getByTestId("access-summary");
    expect(access).toHaveTextContent("Accès configuré");
    expect(access).toHaveTextContent("Mémorisé sur ce PC, chiffré pour votre compte Windows");
    expect(access).toHaveTextContent("speech.example.test:443");
    await user.click(screen.getByRole("button", { name: "Retirer l’accès" }));
    expect(bridge.invoke).toHaveBeenCalledWith("clear_access");
    expect(await screen.findByText("Aucun accès configuré")).toBeInTheDocument();
  });

  it("withdraws consent immediately", async () => {
    const { bridge, user } = await openSettings();
    const consent = screen.getByRole("switch", { name: "Dictée avec Rambler (Google)" });
    expect(consent).toBeChecked();
    expect(consent).toHaveAccessibleDescription(/Votre voix est transmise à Google pendant que vous parlez/);
    await user.click(consent);
    expect(bridge.invoke).toHaveBeenCalledWith("set_consent", { enabled: false });
    await waitFor(() => expect(consent).not.toBeChecked());
  });

  it("clears the draft now", async () => {
    const now = Date.now();
    const { bridge, user } = await openSettings({
      state: { draft: { take: 1, createdAtMs: now, expiresAtMs: now + 60_000, complete: true } },
    });
    await user.click(screen.getByRole("button", { name: "Effacer maintenant" }));
    expect(bridge.invoke).toHaveBeenCalledWith("clear_draft");
  });

  it("previews the rendering only when the optional native command exists", async () => {
    const { bridge, unmount } = await openSettings();
    await waitFor(() =>
      expect(bridge.invokeOptional).toHaveBeenCalledWith("preview_render", {
        sample: "je ne pourrai pas venir jeudi à 14 h est-ce que mardi vous convient",
        settings: expect.objectContaining({ profile: "natural" }),
      }),
    );
    expect(screen.queryByTestId("render-preview")).not.toBeInTheDocument();
    unmount();

    const withPreview = createFakeBridge();
    withPreview.handlers.preview_render = () => "Je ne pourrai pas venir jeudi à 14 h.";
    const view = renderApp(withPreview);
    await appReady();
    await view.user.click(screen.getByRole("button", { name: "Réglages" }));
    expect(await screen.findByTestId("render-preview")).toHaveTextContent("Je ne pourrai pas venir jeudi à 14 h.");
  });

  it("has no detectable accessibility violations", async () => {
    await openSettings();
    await screen.findByRole("option", { name: "Casque USB" });
    expect(await axeViolations()).toEqual([]);
  });
});
