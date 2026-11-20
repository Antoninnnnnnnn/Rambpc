import { act, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { EVENT_MIC_TEST_LEVEL, type AccessStatus } from "../ipc/contract";
import { createFakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, renderApp } from "../test/render";

const NOT_CONFIGURED: AccessStatus = { configured: false, source: "none", authority: null, canRemember: true };

async function startFirstRun(access: AccessStatus = NOT_CONFIGURED) {
  const bridge = createFakeBridge({ state: { availability: { kind: "needs-consent" } }, access });
  const view = renderApp(bridge);
  await appReady();
  return { bridge, ...view };
}

describe("Premier lancement", () => {
  it("takes over until consent and explains what Google receives", async () => {
    await startFirstRun();
    expect(
      screen.getByRole("heading", { level: 1, name: "Votre voix est transmise à Google pendant que vous parlez" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ce que reçoit Google" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ce qui reste sur ce PC" })).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Navigation principale" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Fermer" })).not.toBeInTheDocument();
  });

  it("tests the microphone level before consent", async () => {
    const { bridge, user } = await startFirstRun();
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    expect(screen.getByRole("heading", { level: 1, name: "Choisissez votre micro" })).toHaveFocus();
    expect(await screen.findByRole("option", { name: "Casque USB" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Tester le micro" }));
    expect(bridge.invoke).toHaveBeenCalledWith("start_mic_test");
    await waitFor(() => expect(bridge.listenerCount(EVENT_MIC_TEST_LEVEL)).toBe(1));
    act(() => bridge.emit(EVENT_MIC_TEST_LEVEL, 0.4));
    expect(screen.getByText("Son détecté : le micro fonctionne.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Arrêter le test" }));
    expect(bridge.invoke).toHaveBeenCalledWith("stop_mic_test");
  });

  it("saves a chosen microphone", async () => {
    const { bridge, user } = await startFirstRun();
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    await screen.findByRole("option", { name: "Casque USB" });
    await user.selectOptions(screen.getByRole("combobox", { name: "Micro" }), "Casque USB");
    expect(bridge.invoke).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ microphone: { kind: "named", id: "mic-2", name: "Casque USB" } }),
    });
  });

  it("collects the Rambler access once and never shows it back", async () => {
    const { bridge, user } = await startFirstRun();
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    expect(screen.getByRole("heading", { level: 1, name: "Accès à Rambler" })).toBeInTheDocument();

    // Required fields are checked before anything is sent.
    await user.click(screen.getByRole("button", { name: "Enregistrer et continuer" }));
    expect(screen.getAllByText("Ce champ est obligatoire.")).toHaveLength(3);
    expect(screen.getByLabelText("Clé d’accès")).toHaveFocus();
    expect(bridge.invoke).not.toHaveBeenCalledWith("set_access", expect.anything());

    await user.type(screen.getByLabelText("Clé d’accès"), "cle-secrete-123");
    await user.type(screen.getByLabelText("Package Android"), "org.example.app");
    await user.type(screen.getByLabelText("Certificat"), "AB:CD:EF");
    expect(screen.getByLabelText("Adresse du service (facultatif)")).toHaveAttribute(
      "placeholder",
      "speechs3proto2-pa.googleapis.com:443",
    );
    await user.click(screen.getByLabelText("Mémoriser sur ce PC (chiffré pour votre compte Windows)"));
    await user.click(screen.getByRole("button", { name: "Enregistrer et continuer" }));

    expect(bridge.invoke).toHaveBeenCalledWith("set_access", {
      access: {
        authority: "",
        apiKey: "cle-secrete-123",
        androidPackage: "org.example.app",
        androidCert: "AB:CD:EF",
        remember: true,
      },
    });
    expect(await screen.findByRole("heading", { level: 1, name: "Activer la dictée" })).toBeInTheDocument();
    expect(document.body.innerHTML).not.toContain("cle-secrete-123");
    expect(document.body.innerHTML).not.toContain("AB:CD:EF");
  });

  it("hides « Mémoriser » in the portable edition", async () => {
    const { user } = await startFirstRun({ ...NOT_CONFIGURED, canRemember: false });
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    await screen.findByLabelText("Clé d’accès");
    expect(screen.queryByLabelText(/Mémoriser sur ce PC/)).not.toBeInTheDocument();
    expect(screen.getByText(/Édition portable/)).toBeInTheDocument();
  });

  it("enables dictation explicitly, then guides a first try", async () => {
    const { bridge, user } = await startFirstRun({ ...NOT_CONFIGURED, configured: true, source: "environment" });
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    expect(await screen.findByText("Un accès au service est déjà configuré sur ce PC.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continuer" }));

    expect(bridge.invoke).not.toHaveBeenCalledWith("set_consent", expect.anything());
    await user.click(screen.getByRole("button", { name: "Activer la dictée avec Rambler" }));
    expect(bridge.invoke).toHaveBeenCalledWith("set_consent", { enabled: true });

    expect(await screen.findByRole("heading", { level: 1, name: "Essayez une phrase" })).toHaveFocus();
    expect(screen.getByTestId("try-instruction")).toHaveTextContent(
      "Maintenez Ctrl+Maj+F2, dites « Rendez-vous jeudi à 14 h », puis relâchez.",
    );
    expect(screen.getByRole("button", { name: "Fermer" })).toBeInTheDocument();

    act(() => {
      bridge.pushState({ availability: { kind: "ready" }, phase: { kind: "listening" }, take: 1, listeningMs: 0 });
    });
    expect(screen.getByTestId("status-card")).toHaveTextContent("Écoute");
    expect(screen.getByRole("button", { name: "Annuler" })).toBeInTheDocument();

    // The toggle alternative.
    await user.click(screen.getByRole("switch", { name: "Mode bascule" }));
    expect(bridge.invoke).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ gesture: "toggle" }),
    });

    await user.click(screen.getByRole("button", { name: "Terminer la prise en main" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Accueil" })).toHaveFocus();
  });

  it("is reachable again from the sidebar", async () => {
    const bridge = createFakeBridge();
    const { user } = renderApp(bridge);
    await appReady();
    await user.click(screen.getByRole("button", { name: "Prise en main" }));
    expect(
      screen.getByRole("heading", { level: 1, name: "Votre voix est transmise à Google pendant que vous parlez" }),
    ).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Fermer" }));
    expect(screen.getByRole("heading", { level: 1, name: "Accueil" })).toHaveFocus();
  });

  it("opens from the banner after consent was withdrawn, and can be closed", async () => {
    const bridge = createFakeBridge();
    const { user } = renderApp(bridge);
    await appReady();
    act(() => {
      bridge.pushState({ availability: { kind: "needs-consent" } });
    });
    await user.click(await screen.findByRole("button", { name: "Activer la dictée…" }));
    expect(
      screen.getByRole("heading", { level: 1, name: "Votre voix est transmise à Google pendant que vous parlez" }),
    ).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Fermer" }));
    expect(screen.getByRole("heading", { level: 1, name: "Accueil" })).toBeInTheDocument();
  });

  it("has no detectable accessibility violations", async () => {
    const { user } = await startFirstRun();
    expect(await axeViolations()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    await screen.findByRole("option", { name: "Casque USB" });
    expect(await axeViolations()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    await screen.findByLabelText("Clé d’accès");
    expect(await axeViolations()).toEqual([]);
  });
});
