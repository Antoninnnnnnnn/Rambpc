import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { EVENT_MIC_TEST_LEVEL, type AccessStatus } from "../ipc/contract";
import { completeDraft, createFakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, renderApp } from "../test/render";

const NOT_CONFIGURED: AccessStatus = { configured: false, source: "none", authority: null, canRemember: true };
const VOICE_TITLE = "Votre voix est transmise à Google pendant que vous parlez";

async function startFirstRun(access: AccessStatus = NOT_CONFIGURED) {
  const bridge = createFakeBridge({ state: { availability: { kind: "needs-consent" } }, access });
  const view = renderApp(bridge);
  await appReady();
  return { bridge, ...view };
}

const steps = () =>
  within(screen.getByRole("list", { name: "Étapes de la prise en main" }))
    .getAllByRole("listitem")
    .map((item) => item.textContent);

describe("Prise en main", () => {
  it("takes over until consent and starts with what Google receives", async () => {
    await startFirstRun();
    expect(screen.getByRole("heading", { level: 1, name: VOICE_TITLE })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ce que reçoit Google" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ce qui reste sur ce PC" })).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Navigation principale" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Fermer la prise en main" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Continuer" })).not.toBeInTheDocument();
    // The window can still be moved, reduced or hidden.
    expect(screen.getByRole("button", { name: "Fermer la fenêtre" })).toBeInTheDocument();
    expect(steps()).toEqual(["1Activation", "2Micro", "3Accès à Rambler", "4Essai"]);
  });

  it("skips the access step when an access is already configured", async () => {
    await startFirstRun({ ...NOT_CONFIGURED, configured: true, source: "environment" });
    expect(steps()).toEqual(["1Activation", "2Micro", "3Essai"]);
    expect(screen.getByText("Étape 1 sur 3")).toBeInTheDocument();
  });

  it("enables dictation explicitly, then tests the microphone level", async () => {
    const { bridge, user } = await startFirstRun();
    expect(bridge.invoke).not.toHaveBeenCalledWith("set_consent", expect.anything());
    await user.click(screen.getByRole("button", { name: "Activer la dictée avec Rambler" }));
    expect(bridge.invoke).toHaveBeenCalledWith("set_consent", { enabled: true });

    expect(await screen.findByRole("heading", { level: 1, name: "Choisissez votre micro" })).toHaveFocus();
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
    await user.click(screen.getByRole("button", { name: "Activer la dictée avec Rambler" }));
    await screen.findByRole("option", { name: "Casque USB" });
    await user.selectOptions(screen.getByRole("combobox", { name: "Micro" }), "Casque USB");
    expect(bridge.invoke).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ microphone: { kind: "named", id: "mic-2", name: "Casque USB" } }),
    });
  });

  it("collects the Rambler access once and never shows it back", async () => {
    const { bridge, user } = await startFirstRun();
    await user.click(screen.getByRole("button", { name: "Activer la dictée avec Rambler" }));
    await user.click(await screen.findByRole("button", { name: "Continuer" }));
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
    expect(await screen.findByRole("heading", { level: 1, name: "Essayez une phrase" })).toBeInTheDocument();
    expect(document.body.innerHTML).not.toContain("cle-secrete-123");
    expect(document.body.innerHTML).not.toContain("AB:CD:EF");
  });

  it("hides « Mémoriser » in the portable edition", async () => {
    const { user } = await startFirstRun({ ...NOT_CONFIGURED, canRemember: false });
    await user.click(screen.getByRole("button", { name: "Activer la dictée avec Rambler" }));
    await user.click(await screen.findByRole("button", { name: "Continuer" }));
    await screen.findByLabelText("Clé d’accès");
    expect(screen.queryByLabelText(/Mémoriser sur ce PC/)).not.toBeInTheDocument();
    expect(screen.getByText(/Édition portable/)).toBeInTheDocument();
  });

  it("guides a first try with live status and shows the text obtained", async () => {
    const { bridge, user } = await startFirstRun({ ...NOT_CONFIGURED, configured: true, source: "environment" });
    await user.click(screen.getByRole("button", { name: "Activer la dictée avec Rambler" }));
    await user.click(await screen.findByRole("button", { name: "Continuer" }));

    expect(await screen.findByRole("heading", { level: 1, name: "Essayez une phrase" })).toHaveFocus();
    expect(screen.getByTestId("try-instruction")).toHaveTextContent(
      "Maintenez Ctrl+Maj+F2, dites « Rendez-vous jeudi à 14 h », puis relâchez.",
    );
    expect(screen.getByRole("button", { name: "Fermer la prise en main" })).toBeInTheDocument();

    act(() => {
      bridge.pushState({ availability: { kind: "ready" }, phase: { kind: "listening" }, take: 1, listeningMs: 0 });
    });
    expect(screen.getByTestId("status-card")).toHaveTextContent("Écoute");
    expect(screen.getByRole("button", { name: "Annuler" })).toBeInTheDocument();

    const draft = completeDraft("Rendez-vous jeudi à 14 h.", Date.now(), 1);
    bridge.recents = [draft];
    act(() => {
      bridge.pushState({
        phase: { kind: "idle" },
        take: null,
        draft: draft.meta,
        recentCount: 1,
        lastOutcome: { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } },
      });
    });
    const obtained = await screen.findByRole("region", { name: "Texte obtenu" });
    expect(within(obtained).getByTestId("recent-text")).toHaveTextContent("Rendez-vous jeudi à 14 h.");
    expect(within(screen.getByTestId("outcome")).queryByRole("button", { name: "Voir dans Récents" })).toBeNull();

    // The toggle alternative.
    await user.click(screen.getByRole("switch", { name: "Mode bascule" }));
    expect(bridge.invoke).toHaveBeenCalledWith("save_settings", {
      settings: expect.objectContaining({ gesture: "toggle" }),
    });

    await user.click(screen.getByRole("button", { name: "Terminer la prise en main" }));
    expect(await screen.findByRole("heading", { level: 1, name: /^Accueil/ })).toHaveFocus();
  });

  it("is reachable again from the sidebar", async () => {
    const bridge = createFakeBridge();
    const { user } = renderApp(bridge);
    await appReady();
    await user.click(screen.getByRole("button", { name: "Prise en main" }));
    expect(screen.getByRole("heading", { level: 1, name: VOICE_TITLE })).toHaveFocus();
    expect(screen.getByText(/La dictée avec Rambler est activée sur ce PC/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Fermer la prise en main" }));
    expect(screen.getByRole("heading", { level: 1, name: /^Accueil/ })).toHaveFocus();
  });

  it("opens from the banner after consent was withdrawn, and can be closed", async () => {
    const bridge = createFakeBridge();
    const { user } = renderApp(bridge);
    await appReady();
    act(() => {
      bridge.pushState({ availability: { kind: "needs-consent" } });
    });
    await user.click(await screen.findByRole("button", { name: "Activer la dictée…" }));
    expect(screen.getByRole("heading", { level: 1, name: VOICE_TITLE })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Fermer la prise en main" }));
    expect(screen.getByRole("heading", { level: 1, name: /^Accueil/ })).toBeInTheDocument();
  });

  it("has no detectable accessibility violations", async () => {
    const { user } = await startFirstRun();
    expect(await axeViolations()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Activer la dictée avec Rambler" }));
    await screen.findByRole("option", { name: "Casque USB" });
    expect(await axeViolations()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Continuer" }));
    await screen.findByLabelText("Clé d’accès");
    expect(await axeViolations()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Plus tard" }));
    await screen.findByRole("heading", { level: 1, name: "Essayez une phrase" });
    expect(await axeViolations()).toEqual([]);
  });
});
