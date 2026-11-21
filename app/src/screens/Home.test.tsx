import { act, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { EVENT_LEVEL, type TakeOutcome, type UiState, type UsageStats } from "../ipc/contract";
import { completeDraft, createFakeBridge, uiState } from "../test/fakeBridge";
import { appReady, axeViolations, renderApp } from "../test/render";

async function renderHome(state: Partial<UiState>, stats?: UsageStats) {
  const bridge = createFakeBridge({ state, stats });
  const view = renderApp(bridge);
  await appReady();
  return { bridge, ...view };
}

const normalized = (name: string) => name.replace(/\s+/g, " ");

describe("Accueil: greeting and live state", () => {
  it("greets and names the screen for assistive technologies", async () => {
    await renderHome({});
    expect(screen.getByRole("heading", { level: 1, name: /^Accueil, Bon(jour|soir)$/ })).toBeInTheDocument();
  });

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

  it("explains the gesture with the configured shortcut drawn as keys", async () => {
    await renderHome({ gesture: "toggle", shortcut: "Alt+Shift+D" });
    const card = screen.getByTestId("status-card");
    expect(card).toHaveTextContent(
      "Dans n’importe quelle application, appuyez sur Alt+Maj+D pour commencer et pour terminer.",
    );
    expect(within(card).getByText("Maj").tagName).toBe("KBD");
  });

  it("shows visual-only voice bars driven by the level while listening", async () => {
    const { bridge } = await renderHome({ phase: { kind: "listening" }, take: 3, listeningMs: 0 });
    const wave = screen.getByTestId("waveform");
    expect(wave).toHaveAttribute("aria-hidden", "true");
    await waitFor(() => expect(bridge.listenerCount(EVENT_LEVEL)).toBe(1));
    act(() => bridge.emit(EVENT_LEVEL, 0.5));
    expect(wave).toHaveAttribute("data-level", "0.50");
  });

  it("shows the elapsed time against the 5-minute limit", async () => {
    await renderHome({ phase: { kind: "listening" }, take: 3, listeningMs: 65_000 });
    expect(screen.getByTestId("status-card")).toHaveTextContent(/Durée : 1:0\d sur 5:00/);
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
    const button = screen.getByRole("button", { name: "Dicter dans Sambler" });
    expect(button).toHaveAccessibleDescription("Sans raccourci, le texte reste dans Sambler.");
    await user.click(button);
    expect(bridge.invoke).toHaveBeenCalledWith("dictate_in_sambler");
  });

  it("has no detectable accessibility violations", async () => {
    await renderHome({ phase: { kind: "listening" }, take: 2, listeningMs: 0, lastOutcome: { kind: "inserted" } });
    await screen.findByTestId("stats");
    expect(await axeViolations()).toEqual([]);
  });
});

describe("Accueil: usage counters", () => {
  it("shows words today, this week, in total and the average speed", async () => {
    const { bridge } = await renderHome({});
    const stats = await screen.findByTestId("stats");
    expect(bridge.invoke).toHaveBeenCalledWith("get_stats");
    expect(within(stats).getByTestId("stat-today")).toHaveTextContent(/aujourd’hui\s*1\s240\s*mots/);
    expect(within(stats).getByTestId("stat-week")).toHaveTextContent(/cette semaine\s*5\s312\s*mots/);
    expect(within(stats).getByTestId("stat-total")).toHaveTextContent(/au total, en 612 dictées\s*48\s210\s*mots/);
    expect(within(stats).getByTestId("stat-wpm")).toHaveTextContent(/vitesse moyenne\s*132\s*mots\/min/);
  });

  it("uses the singular and waits for an average speed", async () => {
    await renderHome({}, { wordsToday: 1, wordsThisWeek: 0, wordsTotal: 1, takesTotal: 1, averageWpm: null });
    const stats = await screen.findByTestId("stats");
    expect(within(stats).getByTestId("stat-today")).toHaveTextContent(/1\s*mot$/);
    expect(within(stats).getByTestId("stat-total")).toHaveTextContent("au total, en 1 dictée");
    expect(within(stats).getByTestId("stat-wpm")).toHaveTextContent("—");
  });

  it("refreshes after a take and stays hidden if the counters cannot be read", async () => {
    const { bridge } = await renderHome({});
    await screen.findByTestId("stats");
    bridge.stats = { ...bridge.stats, wordsToday: 1300 };
    act(() => {
      bridge.pushState({ phase: { kind: "listening" }, take: 4, listeningMs: 0 });
    });
    act(() => {
      bridge.pushState({ phase: { kind: "idle" }, take: null, lastOutcome: { kind: "inserted" } });
    });
    await waitFor(() => expect(screen.getByTestId("stat-today")).toHaveTextContent(/1\s300/));

    const failing = createFakeBridge();
    failing.handlers.get_stats = () => {
      throw { code: "unknown", message: "" };
    };
    renderApp(failing);
    await waitFor(() => expect(failing.invoke).toHaveBeenCalledWith("get_stats"));
    expect(screen.getAllByTestId("stats")).toHaveLength(1);
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
    expect(banner).toHaveTextContent("Les réglages, les textes récents et la copie restent disponibles.");

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
    "Le texte complet reste disponible dans Récents.",
    "Voir dans Récents",
  ],
  [
    "KeptInSambler/SamblerDestination",
    { kind: "kept-in-sambler", reason: { kind: "sambler-destination" } },
    true,
    "Texte dans Sambler",
    "Comme demandé, le texte n’a pas été inséré. Copiez-le quand vous voulez.",
    "Voir dans Récents",
  ],
  [
    "KeptInSambler/UncertainTarget (new: instead of refusing)",
    { kind: "kept-in-sambler", reason: { kind: "uncertain-target", reason: "terminal" } },
    true,
    "Texte dans Sambler",
    "Un terminal ne reçoit pas la dictée directement. Le texte vous attend ici : copiez-le puis collez-le.",
    "Voir dans Récents",
  ],
  [
    "KeptInSambler/TargetChanged",
    { kind: "kept-in-sambler", reason: { kind: "target-changed", change: "focus-moved" } },
    true,
    "Texte dans Sambler",
    "À la fin de la dictée, le curseur n’était plus dans le champ de départ.",
    "Voir dans Récents",
  ],
  [
    "KeptInSambler/NotWritten/modifiers-held",
    { kind: "kept-in-sambler", reason: { kind: "not-written", reason: "modifiers-held" } },
    true,
    "Texte dans Sambler",
    "Une touche (Ctrl, Maj, Alt ou Windows) était encore enfoncée : rien n’a été écrit, le texte vous attend ici.",
    "Voir dans Récents",
  ],
  [
    "KeptInSambler/NotWritten/unsupported (multi-line)",
    { kind: "kept-in-sambler", reason: { kind: "not-written", reason: "unsupported" } },
    true,
    "Texte dans Sambler",
    "Ce champ n’accepte pas l’insertion de plusieurs lignes ou de ce texte : le texte vous attend ici, copiez-le puis collez-le.",
    "Voir dans Récents",
  ],
  [
    "KeptInSambler/NotWritten/refused",
    { kind: "kept-in-sambler", reason: { kind: "not-written", reason: "refused" } },
    true,
    "Texte dans Sambler",
    "Windows a refusé la saisie dans ce champ (application administrateur ?) : le texte vous attend ici.",
    "Voir dans Récents",
  ],
  [
    "KeptInSambler/Incomplete",
    { kind: "kept-in-sambler", reason: { kind: "incomplete" } },
    true,
    "Texte dans Sambler",
    "Des passages manquent ou sont à revoir : rien n’a été inséré automatiquement.",
    "Voir dans Récents",
  ],
  ["NoText", { kind: "no-text" }, false, "Aucun texte obtenu", "Rambler n’a reconnu aucune parole. Vérifiez le micro puis réessayez.", null],
  ["Cancelled", { kind: "cancelled" }, false, "Dictée annulée", "Rien n’a été inséré.", null],
  [
    "Interrupted/recovered",
    { kind: "interrupted", reason: { kind: "network" }, recovered: true },
    true,
    "Dictée interrompue",
    "La connexion au service a été perdue. Les passages déjà reçus sont dans Récents.",
    "Voir dans Récents",
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
    "La durée maximale d’une dictée (5 minutes) est atteinte. Les passages déjà reçus sont dans Récents.",
    "Voir dans Récents",
  ],
  [
    "Refused/UncertainTarget (legacy)",
    { kind: "refused", reason: { kind: "uncertain-target", reason: "selection" } },
    false,
    "Sambler ne peut pas écrire sûrement dans ce champ",
    "Du texte était sélectionné : il aurait été remplacé. Vous pouvez dicter dans Sambler, puis copier le texte.",
    "Dicter dans Sambler",
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
    expect(within(notice).getByRole("heading", { name: (name) => normalized(name) === title })).toBeInTheDocument();
    if (detail) expect(notice).toHaveTextContent(detail);
    if (action) expect(within(notice).getByRole("button", { name: (name) => normalized(name) === action })).toBeInTheDocument();
    else expect(within(notice).queryByRole("button")).not.toBeInTheDocument();
  });

  it("starts a Sambler take from a legacy uncertain target refusal", async () => {
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

  it("brings the waiting text into view from the outcome", async () => {
    const draft = completeDraft("Texte gardé.");
    const bridge = createFakeBridge({
      state: { lastOutcome: { kind: "inserted-partially" }, draft: draft.meta },
      draft,
    });
    const { user } = renderApp(bridge);
    await appReady();
    await screen.findByTestId("recent-item");
    await user.click(screen.getByRole("button", { name: "Voir dans Récents" }));
    await waitFor(() => expect(within(screen.getByTestId("recent-item")).getByRole("heading", { level: 3 })).toHaveFocus());
  });

  it("returns the initial state from get_state", async () => {
    const bridge = createFakeBridge({ state: uiState({ lastOutcome: { kind: "no-text" } }) });
    renderApp(bridge);
    expect(await screen.findByText("Aucun texte obtenu")).toBeInTheDocument();
    expect(bridge.invoke).toHaveBeenCalledWith("get_state");
  });
});
