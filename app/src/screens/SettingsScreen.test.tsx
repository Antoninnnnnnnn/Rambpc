import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Settings, UiState } from "../ipc/contract";
import { createFakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, liveRegion, renderApp } from "../test/render";

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

  it("lists its sections and moves the focus to the one chosen", async () => {
    const { user } = await openSettings();
    const toc = screen.getByRole("navigation", { name: "Sections des réglages" });
    for (const name of [
      "Micro",
      "Raccourci",
      "Langue",
      "Insertion",
      "Destination",
      "Barre de dictée",
      "Récents",
      "Démarrage",
      "Accès à Rambler",
      "Confidentialité",
    ]) {
      expect(within(toc).getByRole("link", { name })).toBeInTheDocument();
    }
    await user.click(within(toc).getByRole("link", { name: "Insertion" }));
    expect(screen.getByRole("region", { name: "Insertion" })).toHaveFocus();
  });

  it("saves each change with save_settings", async () => {
    const { bridge, user } = await openSettings();
    await user.click(screen.getByRole("radio", { name: "Anglais" }));
    await user.click(screen.getByRole("switch", { name: "Toujours garder le texte dans Sambler" }));
    await user.click(screen.getByRole("radio", { name: "5 minutes" }));
    await user.click(screen.getByRole("radio", { name: /Bascule/ }));
    await waitFor(() => expect(savedSettings(bridge)).toHaveLength(4));
    expect(lastSaved(bridge)).toMatchObject({
      language: "english",
      alwaysKeepInSambler: true,
      draftMinutes: 5,
      gesture: "toggle",
    });
  });

  it("chooses how text is inserted, instant paste by default", async () => {
    const { bridge, user } = await openSettings();
    const paste = screen.getByRole("radio", { name: "Instantanée (collage, presse-papiers restauré)" });
    const typing = screen.getByRole("radio", { name: "Frappe caractère par caractère" });
    expect(paste).toBeChecked();
    expect(paste).toHaveAccessibleDescription(/votre presse-papiers précédent est remis en place/);
    expect(typing).toHaveAccessibleDescription(/sans jamais toucher au presse-papiers\. Une seule ligne à la fois\./);
    await user.click(typing);
    await waitFor(() => expect(lastSaved(bridge)).toMatchObject({ insertion: "typing" }));
    expect(typing).toBeChecked();
  });

  it("chooses where the text goes, the field of departure by default", async () => {
    const { bridge, user } = await openSettings();
    const group = screen.getByRole("group", { name: "Où insérer le texte" });
    expect(group).toHaveAccessibleDescription("Un champ de mot de passe ne reçoit jamais de texte.");
    const original = within(group).getByRole("radio", { name: "Dans le champ de départ" });
    const cursor = within(group).getByRole("radio", { name: "Là où se trouve le curseur à la fin" });
    expect(original).toBeChecked();
    expect(original).toHaveAccessibleDescription(
      /Même si vous changez de fenêtre pendant la dictée, du moment que vous y revenez\s; sinon le texte reste dans Sambler\./,
    );
    await user.click(cursor);
    await waitFor(() => expect(lastSaved(bridge)).toMatchObject({ insertionTarget: "cursor-at-end" }));

    // Without effect while the text always stays in Sambler.
    await user.click(screen.getByRole("switch", { name: "Toujours garder le texte dans Sambler" }));
    await waitFor(() => expect(cursor).toBeDisabled());
    expect(group).toHaveAccessibleDescription(/^Sans effet tant que le texte est toujours gardé dans Sambler\./);
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

  it("moves the dictation bar", async () => {
    const { bridge, user } = await openSettings();
    await user.selectOptions(screen.getByRole("combobox", { name: "Position de la barre" }), "top-center");
    await waitFor(() => expect(lastSaved(bridge)?.indicator.position).toBe("top-center"));
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
      throw {
        code: "shortcut",
        message: "Le raccourci n'a pas pu être enregistré (HotKey already registered). L'ancien est conservé.",
      };
    };
    await user.click(screen.getByRole("button", { name: "Modifier le raccourci" }));
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Nouvelle combinaison" }), {
      key: "J",
      code: "KeyJ",
      ctrlKey: true,
      shiftKey: true,
    });
    const error = await screen.findByText(/Le raccourci n’a pas pu être enregistré/);
    expect(error).toHaveTextContent(
      "Le raccourci n’a pas pu être enregistré. Cette combinaison est peut-être déjà utilisée par Windows ou par une autre application. L’ancien raccourci reste actif : Ctrl+Maj+F2.",
    );
    // The technical reason from the hotkey layer is not shown.
    expect(error).not.toHaveTextContent("HotKey");
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

  it("clears the recent results now", async () => {
    const now = Date.now();
    const { bridge, user } = await openSettings({
      state: { draft: { take: 1, createdAtMs: now, expiresAtMs: now + 60_000, complete: true }, recentCount: 2 },
    });
    await user.click(screen.getByRole("button", { name: "Effacer maintenant" }));
    expect(bridge.invoke).toHaveBeenCalledWith("clear_draft");
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Textes récents effacés."));
    expect(screen.getByRole("button", { name: "Effacer maintenant" })).toBeDisabled();
  });

  it("keeps a setting that applied but could not be written to disk", async () => {
    const { bridge, user } = await openSettings();
    bridge.handlers.save_settings = () => {
      throw { code: "save-failed", message: "Les réglages s'appliquent mais n'ont pas pu être enregistrés sur le disque." };
    };
    const sound = screen.getByRole("switch", { name: "Son de début et de fin" });
    await user.click(sound);
    expect(
      await screen.findByText("Les réglages s'appliquent mais n'ont pas pu être enregistrés sur le disque."),
    ).toBeInTheDocument();
    expect(sound).toBeChecked();
  });

  it("registers the launch at startup", async () => {
    const { bridge, user } = await openSettings();
    const startup = screen.getByRole("switch", { name: "Démarrer avec Windows" });
    expect(startup).not.toBeChecked();
    await user.click(startup);
    expect(bridge.invoke).toHaveBeenCalledWith("set_launch_at_startup", { enabled: true });
    await waitFor(() => expect(startup).toBeChecked());
    expect(savedSettings(bridge)).toHaveLength(0);
  });

  it("explains that the portable edition never registers itself", async () => {
    const { bridge, user } = await openSettings();
    bridge.handlers.set_launch_at_startup = () => {
      throw { code: "portable", message: "Édition portable." };
    };
    const startup = screen.getByRole("switch", { name: "Démarrer avec Windows" });
    await user.click(startup);
    expect(await screen.findByText(/L’édition portable ne s’inscrit jamais au démarrage de ce PC/)).toBeInTheDocument();
    expect(startup).not.toBeChecked();
    expect(startup).toBeDisabled();
  });

  it("reaches the history and the guided tour from Confidentialité", async () => {
    const { user } = await openSettings();
    await user.click(screen.getByRole("button", { name: "Revoir la prise en main" }));
    expect(
      screen.getByRole("heading", { level: 1, name: "Votre voix est transmise à Google pendant que vous parlez" }),
    ).toHaveFocus();
  });

  it("has no detectable accessibility violations", async () => {
    await openSettings();
    await screen.findByRole("option", { name: "Casque USB" });
    expect(await axeViolations()).toEqual([]);
  });
});
