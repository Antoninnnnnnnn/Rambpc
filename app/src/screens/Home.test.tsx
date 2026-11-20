import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { EVENT_LEVEL, type TakeOutcome, type UiState } from "../ipc/contract";
import { createFakeBridge, uiState } from "../test/fakeBridge";
import { appReady, axeViolations, renderApp } from "../test/render";

async function renderHome(state: Partial<UiState>) {
  const bridge = createFakeBridge({ state });
  const view = renderApp(bridge);
  await appReady();
  return { bridge, ...view };
}

describe("Accueil: phases", () => {
  it.each([
    [{ phase: { kind: "idle" }, availability: { kind: "ready" } }, "Disponible", "available"],
    [{ phase: { kind: "preparing" }, take: 3 }, "Préparation", "preparing"],
    [{ phase: { kind: "listening" }, take: 3, listeningMs: 1200 }, "Écoute", "listening"],
    [{ phase: { kind: "finalizing" }, take: 3 }, "Traitement", "finalizing"],
    [{ availability: { kind: "connecting" } }, "Connexion à Rambler", "connecting"],
    [{ availability: { kind: "unavailable", reason: "network" } }, "Dictée indisponible", "unavailable"],
  ] as [Partial<UiState>, string, string][])("shows %o as text and a distinct icon shape", async (state, label, shape) => {
    await renderHome(state);
    const card = screen.getByTestId("status-card");
    expect(within(card).getByText(label)).toBeInTheDocument();
    expect(card.querySelector(`svg[data-shape="${shape}"]`)).not.toBeNull();
  });

  it("explains the gesture with the configured shortcut", async () => {
    await renderHome({ gesture: "toggle", shortcut: "Alt+Shift+D" });
    expect(
      screen.getByText("Dans n’importe quelle application, appuyez sur Alt+Maj+D pour commencer, puis de nouveau pour terminer."),
    ).toBeInTheDocument();
  });

  it("shows a visual-only level meter while listening", async () => {
    const { bridge } = await renderHome({ phase: { kind: "listening" }, take: 3, listeningMs: 0 });
    const meter = screen.getByTestId("level-meter");
    expect(meter).toHaveAttribute("aria-hidden", "true");
    await waitFor(() => expect(bridge.listenerCount(EVENT_LEVEL)).toBe(1));
    act(() => bridge.emit(EVENT_LEVEL, 0.5));
    expect(meter.querySelector<HTMLElement>(".level-meter__fill")!.style.transform).toBe("scaleX(0.5)");
  });

  it("finishes and cancels the active take by id", async () => {
    const { bridge, user } = await renderHome({ phase: { kind: "listening" }, take: 12, listeningMs: 0 });
    await user.click(screen.getByRole("button", { name: "Terminer" }));
    expect(bridge.invoke).toHaveBeenCalledWith("finish_take", { take: 12 });
    await user.click(screen.getByRole("button", { name: "Annuler" }));
    expect(bridge.invoke).toHaveBeenCalledWith("cancel_take", { take: 12 });
  });

  it("cancels with Escape in the window when enabled", async () => {
    const { bridge, user } = await renderHome({ phase: { kind: "listening" }, take: 5, listeningMs: 0 });
    await user.keyboard("{Escape}");
    expect(bridge.invoke).toHaveBeenCalledWith("cancel_take", { take: 5 });
  });

  it("offers « Dicter dans Sambler » when idle and ready", async () => {
    const { bridge, user } = await renderHome({});
    await user.click(screen.getByRole("button", { name: "Dicter dans Sambler" }));
    expect(bridge.invoke).toHaveBeenCalledWith("dictate_in_sambler");
  });

  it("has no detectable accessibility violations", async () => {
    await renderHome({
      phase: { kind: "listening" },
      take: 2,
      listeningMs: 0,
      lastOutcome: { kind: "inserted" },
    });
    expect(await axeViolations()).toEqual([]);
  });
});

describe("Accueil: availability banner", () => {
  it("shows « Dictée indisponible » with the reason and retries", async () => {
    const { bridge, user } = await renderHome({ availability: { kind: "unavailable", reason: "throttled" } });
    const banner = screen.getByTestId("availability-banner");
    expect(within(banner).getByRole("heading", { name: "Dictée indisponible" })).toBeInTheDocument();
    expect(
      within(banner).getByText("Le service limite temporairement les demandes. Réessayez dans quelques instants."),
    ).toBeInTheDocument();
    expect(within(banner).getByText("Les réglages, le brouillon et la copie restent disponibles.")).toBeInTheDocument();

    bridge.handlers.retry_connection = () => ({ ...bridge.state, availability: { kind: "ready" } });
    await user.click(within(banner).getByRole("button", { name: "Réessayer" }));
    expect(bridge.invoke).toHaveBeenCalledWith("retry_connection");
    await waitFor(() => expect(screen.queryByTestId("availability-banner")).not.toBeInTheDocument());
  });

  it("links an access refusal to the access settings", async () => {
    const { user } = await renderHome({ availability: { kind: "unavailable", reason: "access-refused" } });
    await user.click(screen.getByRole("button", { name: "Modifier l’accès" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Réglages" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Accès à Rambler" })).toHaveFocus();
  });

  it("asks for access when none is configured", async () => {
    const bridge = createFakeBridge({
      state: { availability: { kind: "needs-access" } },
      access: { configured: false, source: "none", authority: null, canRemember: true },
    });
    const { user } = renderApp(bridge);
    await appReady();
    expect(screen.getByRole("heading", { name: "Accès à Rambler requis" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Configurer l’accès" }));
    expect(await screen.findByLabelText("Clé d’accès")).toBeInTheDocument();
  });

  it("updates when the state event reports an outage", async () => {
    const { bridge } = await renderHome({});
    expect(screen.queryByTestId("availability-banner")).not.toBeInTheDocument();
    act(() => {
      bridge.pushState({ availability: { kind: "unavailable", reason: "network" } });
    });
    expect(await screen.findByTestId("availability-banner")).toHaveTextContent(
      "Le service est injoignable. Vérifiez la connexion Internet de ce PC.",
    );
  });
});

const OUTCOMES: [string, TakeOutcome, boolean, string, string | null, string | null][] = [
  ["Inserted", { kind: "inserted" }, false, "Texte transmis au champ", null, null],
  [
    "InsertedPartially",
    { kind: "inserted-partially" },
    true,
    "Vérifiez le champ, une partie du texte a pu être saisie",
    "Le texte complet reste disponible dans le brouillon.",
    "Voir le brouillon",
  ],
  [
    "KeptInSambler/SamblerDestination",
    { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } },
    true,
    "Texte disponible",
    "Vous avez choisi de garder le texte dans Sambler.",
    "Voir le brouillon",
  ],
  [
    "KeptInSambler/TargetChanged",
    { kind: "kept-in-sambler", reason: { kind: "target-changed", change: "focus-moved" } },
    true,
    "Texte disponible",
    "Le curseur a quitté le champ pendant la dictée.",
    "Voir le brouillon",
  ],
  [
    "KeptInSambler/NotWritten/modifiers-held",
    { kind: "kept-in-sambler", reason: { kind: "not-written", reason: "modifiers-held" } },
    true,
    "Texte disponible",
    "Une touche (Ctrl, Maj, Alt ou Windows) était encore enfoncée : rien n’a été écrit, le texte vous attend ici.",
    "Voir le brouillon",
  ],
  [
    "KeptInSambler/NotWritten/unsupported (multi-line)",
    { kind: "kept-in-sambler", reason: { kind: "not-written", reason: "unsupported" } },
    true,
    "Texte disponible",
    "Ce champ n’accepte pas l’insertion de plusieurs lignes ou de ce texte : le texte vous attend ici, copiez-le puis collez-le.",
    "Voir le brouillon",
  ],
  [
    "KeptInSambler/NotWritten/refused",
    { kind: "kept-in-sambler", reason: { kind: "not-written", reason: "refused" } },
    true,
    "Texte disponible",
    "Windows a refusé la saisie dans ce champ (application administrateur ?) : le texte vous attend ici.",
    "Voir le brouillon",
  ],
  [
    "KeptInSambler/NotWritten/target-changed (unchanged wording)",
    { kind: "kept-in-sambler", reason: { kind: "not-written", reason: "target-changed" } },
    true,
    "Texte disponible",
    "Le champ a changé juste avant l’écriture : rien n’a été saisi.",
    "Voir le brouillon",
  ],
  [
    "KeptInSambler/Incomplete",
    { kind: "kept-in-sambler", reason: { kind: "incomplete" } },
    true,
    "Texte disponible",
    "Des passages manquent ou sont à revoir : rien n’a été inséré automatiquement.",
    "Voir le brouillon",
  ],
  ["NoText", { kind: "no-text" }, false, "Aucun texte obtenu", "Rambler n’a reconnu aucune parole. Vérifiez le micro puis réessayez.", null],
  ["Cancelled", { kind: "cancelled" }, false, "Dictée annulée", "Rien n’a été inséré.", null],
  [
    "Interrupted/recovered",
    { kind: "interrupted", reason: { kind: "network" }, recovered: true },
    true,
    "Dictée interrompue",
    "La connexion au service a été perdue. Les passages déjà reçus sont dans le brouillon.",
    "Voir le brouillon",
  ],
  [
    "Interrupted/not recovered",
    { kind: "interrupted", reason: { kind: "system-interrupted" }, recovered: false },
    false,
    "Dictée interrompue",
    "Windows a été verrouillé ou mis en veille. Aucun texte récupéré.",
    null,
  ],
  [
    "Interrupted/microphone denied",
    { kind: "interrupted", reason: { kind: "microphone", error: { kind: "access-denied" } }, recovered: false },
    false,
    "Dictée interrompue",
    "Windows bloque l’accès au micro pour les applications de bureau. Aucun texte récupéré.",
    "Ouvrir les réglages de confidentialité du micro",
  ],
  [
    "Interrupted/duration limit",
    { kind: "interrupted", reason: { kind: "duration-limit" }, recovered: true },
    true,
    "Dictée terminée à la durée maximale",
    "La durée maximale d’une dictée (5 minutes) est atteinte. Les passages déjà reçus sont dans le brouillon.",
    "Voir le brouillon",
  ],
  [
    "Refused/UncertainTarget",
    { kind: "refused", reason: { kind: "uncertain-target", reason: "selection" } },
    false,
    "Sambler ne peut pas écrire sûrement dans ce champ",
    "Du texte était sélectionné : il aurait été remplacé. Vous pouvez dicter dans Sambler, puis copier le texte.",
    "Dicter dans Sambler",
  ],
  [
    "Refused/PendingDraft",
    { kind: "refused", reason: { kind: "pending-draft" } },
    true,
    "Un texte attend dans Sambler : copiez-le ou rappuyez sur le raccourci pour le remplacer",
    null,
    "Voir le brouillon",
  ],
  [
    "Refused/ProtectedField",
    { kind: "refused", reason: { kind: "protected-field" } },
    false,
    "Champ protégé",
    "Sambler n’ouvre pas le micro dans un champ de mot de passe ni sur un écran sécurisé.",
    null,
  ],
  [
    "Refused/Unavailable",
    { kind: "refused", reason: { kind: "unavailable", reason: "service" } },
    false,
    "Dictée indisponible",
    "Le service Rambler ne répond pas correctement pour le moment.",
    "Réessayer",
  ],
  [
    "Refused/Microphone",
    { kind: "refused", reason: { kind: "microphone", error: { kind: "unsupported-format", format: "44100 Hz, 6 ch, f32" } } },
    false,
    "Le micro n’a pas pu s’ouvrir",
    "Le format de ce micro n’est pas pris en charge (44100 Hz, 6 ch, f32). Choisissez un autre micro.",
    null,
  ],
  ["Refused/NeedsConsent", { kind: "refused", reason: { kind: "needs-consent" } }, false, "La dictée n’a pas démarré", "Activez d’abord la dictée avec Rambler.", "Activer la dictée…"],
  ["Refused/NeedsAccess", { kind: "refused", reason: { kind: "needs-access" } }, false, "La dictée n’a pas démarré", "Configurez d’abord l’accès à Rambler.", "Configurer l’accès"],
];

describe("Accueil: last outcome", () => {
  it.each(OUTCOMES)("%s", async (_name, outcome, withDraft, title, detail, action) => {
    const now = Date.now();
    await renderHome({
      lastOutcome: outcome,
      draft: withDraft ? { take: 4, createdAtMs: now, expiresAtMs: now + 60_000, complete: true } : null,
    });
    const notice = screen.getByTestId("outcome");
    // Accessible names keep the French non-breaking spaces: compare normalized.
    expect(within(notice).getByRole("heading", { name: (name) => name.replace(/\s+/g, " ") === title })).toBeInTheDocument();
    if (detail) expect(notice).toHaveTextContent(detail);
    if (action) expect(within(notice).getByRole("button", { name: action })).toBeInTheDocument();
    else expect(within(notice).queryByRole("button")).not.toBeInTheDocument();
  });

  it("starts a Sambler take from an uncertain target refusal", async () => {
    const { bridge, user } = await renderHome({
      lastOutcome: { kind: "refused", reason: { kind: "uncertain-target", reason: "terminal" } },
    });
    await user.click(within(screen.getByTestId("outcome")).getByRole("button", { name: "Dicter dans Sambler" }));
    expect(bridge.invoke).toHaveBeenCalledWith("dictate_in_sambler");
  });

  it("opens the Windows microphone privacy settings", async () => {
    const { bridge, user } = await renderHome({
      lastOutcome: { kind: "refused", reason: { kind: "microphone", error: { kind: "access-denied" } } },
    });
    await user.click(screen.getByRole("button", { name: "Ouvrir les réglages de confidentialité du micro" }));
    expect(bridge.invoke).toHaveBeenCalledWith("open_mic_privacy_settings");
  });

  it("reaches the draft from the outcome", async () => {
    const now = Date.now();
    const { user } = await renderHome({
      lastOutcome: { kind: "inserted-partially" },
      draft: { take: 1, createdAtMs: now, expiresAtMs: now + 60_000, complete: true },
    });
    await user.click(screen.getByRole("button", { name: "Voir le brouillon" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Brouillon" })).toHaveFocus();
  });

  it("returns the initial state from get_state", async () => {
    const bridge = createFakeBridge({ state: uiState({ lastOutcome: { kind: "no-text" } }) });
    renderApp(bridge);
    expect(await screen.findByText("Aucun texte obtenu")).toBeInTheDocument();
    expect(bridge.invoke).toHaveBeenCalledWith("get_state");
  });
});
