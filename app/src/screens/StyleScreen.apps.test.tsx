// Style > Par application: adding from get_known_apps, the three states of
// each field, the merged preview, removing, saving, and the degraded states.
import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AppIdentity, AppStyle, Settings } from "../ipc/contract";
import { createFakeBridge, type FakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, liveRegion, renderApp } from "../test/render";

const WORD: AppIdentity = { id: "winword", name: "Word" };
const CODE: AppIdentity = { id: "code", name: "Visual Studio Code" };
const SLACK: AppIdentity = { id: "slack", name: "Slack" };

async function openApps(
  options: { styles?: AppStyle[] | undefined; known?: AppIdentity[]; settings?: Partial<Settings> } = {},
  setup: (bridge: FakeBridge) => void = () => {},
) {
  const settings: Partial<Settings> = { ...options.settings };
  if ("styles" in options) settings.appStyles = options.styles;
  const bridge = createFakeBridge({ settings });
  bridge.handlers.get_known_apps = () => options.known ?? [WORD, CODE, SLACK];
  setup(bridge);
  const view = renderApp(bridge);
  await appReady();
  await view.user.click(screen.getByRole("button", { name: "Style" }));
  await screen.findByRole("heading", { level: 1, name: "Style" });
  const card = screen.getByRole("region", { name: "Par application" });
  return { bridge, card, ...view };
}

const lastSaved = (bridge: FakeBridge) => {
  const saved = bridge.invoke.mock.calls.filter(([command]) => command === "save_settings");
  return (saved[saved.length - 1]?.[1] as { settings: Settings } | undefined)?.settings;
};

/** The header button of an application: its name, then its summary. */
const toggleOf = (name: string) => screen.getByRole("button", { name: new RegExp(`^${name} `) });

describe("Style > Par application", () => {
  it("says that every application follows the general setting at first", async () => {
    const { card } = await openApps();
    expect(within(card).getByText("Toutes les applications suivent le réglage général.")).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Ajouter une application" })).toHaveAttribute("aria-expanded", "false");
  });

  it("adds an application from the known ones and saves it", async () => {
    const { bridge, card, user } = await openApps();
    const add = within(card).getByRole("button", { name: "Ajouter une application" });
    await user.click(add);
    expect(add).toHaveAttribute("aria-expanded", "true");
    const picker = within(card).getByRole("group", { name: "Choisissez une application" });
    expect(within(picker).getAllByRole("listitem")).toHaveLength(3);
    for (const name of ["Word", "Visual Studio Code", "Slack", "Annuler"]) {
      expect(within(picker).getByRole("button", { name })).toBeInTheDocument();
    }
    await user.click(within(picker).getByRole("button", { name: "Word" }));

    await waitFor(() => expect(lastSaved(bridge)?.appStyles).toEqual([{ app: WORD }]));
    const toggle = within(card).getByRole("button", { name: "Word Comme le réglage général" });
    expect(toggle).toHaveFocus();
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(within(card).queryByRole("group", { name: "Choisissez une application" })).not.toBeInTheDocument();
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Word a maintenant son propre style."));
  });

  it("proposes only the applications without a style", async () => {
    const { card, user } = await openApps({ styles: [{ app: WORD }] });
    await user.click(within(card).getByRole("button", { name: "Ajouter une application" }));
    const picker = within(card).getByRole("group", { name: "Choisissez une application" });
    expect(within(picker).queryByRole("button", { name: "Word" })).not.toBeInTheDocument();
    expect(within(picker).getByRole("button", { name: "Slack" })).toBeInTheDocument();
  });

  it("explains that applications appear after the first dictations in them", async () => {
    const { card, user } = await openApps({ known: [] });
    await user.click(within(card).getByRole("button", { name: "Ajouter une application" }));
    expect(
      await within(card).findByText(
        "Aucune application pour l’instant. Les applications apparaissent ici après vos premières dictées dedans.",
      ),
    ).toBeInTheDocument();
  });

  it("closes the list with Escape and gives the focus back", async () => {
    const { card, user } = await openApps();
    const add = within(card).getByRole("button", { name: "Ajouter une application" });
    await user.click(add);
    await user.tab();
    expect(within(card).getByRole("button", { name: "Word" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(within(card).queryByRole("group", { name: "Choisissez une application" })).not.toBeInTheDocument();
    expect(add).toHaveFocus();
  });

  it("sets each option in three states: general, on, off", async () => {
    const { bridge, user } = await openApps({ styles: [{ app: WORD }] });
    await user.click(toggleOf("Word"));
    const item = screen.getByTestId("app-style-winword");
    const capitals = within(item).getByRole("group", { name: "Majuscule de phrase" });
    expect(within(capitals).getByRole("radio", { name: "Comme le général (avec)" })).toBeChecked();

    await user.click(within(capitals).getByRole("radio", { name: "Sans" }));
    await waitFor(() => expect(lastSaved(bridge)?.appStyles).toEqual([{ app: WORD, text: { sentenceCase: false } }]));
    expect(toggleOf("Word")).toHaveAccessibleName("Word Sans majuscule de phrase");

    await user.click(within(capitals).getByRole("radio", { name: "Avec" }));
    await waitFor(() => expect(lastSaved(bridge)?.appStyles).toEqual([{ app: WORD, text: { sentenceCase: true } }]));

    await user.click(within(capitals).getByRole("radio", { name: "Comme le général (avec)" }));
    // Back to the general setting: the field (and the empty patch) is dropped.
    await waitFor(() => expect(lastSaved(bridge)?.appStyles).toEqual([{ app: WORD }]));

    const commands = within(item).getByRole("group", { name: "Commandes de dictée" });
    await user.click(within(commands).getByRole("radio", { name: "Sans" }));
    await waitFor(() => expect(lastSaved(bridge)?.appStyles).toEqual([{ app: WORD, text: { voiceCommands: false } }]));
  });

  it("sets the profile in three states and ignores the rules in Fidèle", async () => {
    const { bridge, user } = await openApps({ styles: [{ app: CODE, text: { lightPunctuation: false } }] });
    await user.click(toggleOf("Visual Studio Code"));
    const item = screen.getByTestId("app-style-code");
    const profile = within(item).getByRole("group", { name: "Profil" });
    expect(within(profile).getByRole("radio", { name: "Comme le général (Naturel)" })).toBeChecked();

    await user.click(within(profile).getByRole("radio", { name: "Fidèle" }));
    await waitFor(() =>
      expect(lastSaved(bridge)?.appStyles).toEqual([{ app: CODE, profile: "faithful", text: { lightPunctuation: false } }]),
    );
    expect(within(item).getByText("En Fidèle, les règles ci-dessous sont ignorées pour Visual Studio Code.")).toBeInTheDocument();
    expect(within(item).getByRole("group", { name: "Ponctuation légère" })).toBeDisabled();

    await user.click(within(profile).getByRole("radio", { name: "Comme le général (Naturel)" }));
    await waitFor(() => expect(lastSaved(bridge)?.appStyles).toEqual([{ app: CODE, text: { lightPunctuation: false } }]));
    expect(within(item).getByRole("group", { name: "Ponctuation légère" })).toBeEnabled();
  });

  it("previews the sample sentence with the application's merged settings", async () => {
    const { bridge, user } = await openApps(
      { styles: [{ app: WORD, profile: "faithful" }], settings: { profile: "natural" } },
      (fake) => {
        fake.handlers.preview_render = ({ sample, settings }: { sample: string; settings: Settings }) =>
          settings.profile === "faithful" ? `[fidèle] ${sample}` : `[naturel] ${sample}`;
      },
    );
    await user.click(toggleOf("Word"));
    const preview = await screen.findByTestId("app-preview-winword");
    expect(preview).toHaveTextContent(/^Inséré dans Word\s*\[fidèle\] je ne pourrai pas venir jeudi/);
    expect(screen.getByTestId("render-preview")).toHaveTextContent(/\[naturel\]/);
    expect(bridge.invoke).toHaveBeenCalledWith("preview_render", {
      sample: "je ne pourrai pas venir jeudi à 14 h est-ce que mardi vous convient",
      settings: expect.objectContaining({ profile: "faithful", text: expect.objectContaining({ sentenceCase: true }) }),
    });
  });

  it("summarises a closed application", async () => {
    await openApps({ styles: [{ app: CODE, profile: "faithful", text: { sentenceCase: false, voiceCommands: true } }] });
    const toggle = toggleOf("Visual Studio Code");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveAccessibleName(
      "Visual Studio Code Fidèle · sans majuscule de phrase · avec commandes de dictée",
    );
  });

  it("removes an application and moves the focus to the next one", async () => {
    const { bridge, card, user } = await openApps({ styles: [{ app: WORD, profile: "faithful" }, { app: CODE }] });
    await user.click(within(card).getByRole("button", { name: "Retirer Word" }));
    await waitFor(() => expect(lastSaved(bridge)?.appStyles).toEqual([{ app: CODE }]));
    expect(toggleOf("Visual Studio Code")).toHaveFocus();
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Word suit de nouveau le réglage général."));

    await user.click(within(card).getByRole("button", { name: "Retirer Visual Studio Code" }));
    await waitFor(() => expect(lastSaved(bridge)?.appStyles).toEqual([]));
    expect(within(card).getByRole("button", { name: "Ajouter une application" })).toHaveFocus();
  });

  it("rolls back a refused change and shows the error", async () => {
    const { bridge, card, user } = await openApps({ styles: [] });
    bridge.handlers.save_settings = () => {
      throw { code: "invalid", message: "Valeur refusée." };
    };
    await user.click(within(card).getByRole("button", { name: "Ajouter une application" }));
    await user.click(within(card).getByRole("button", { name: "Word" }));
    expect(await within(card).findByText("Ce réglage n’a pas pu être enregistré. Valeur refusée.")).toBeInTheDocument();
    expect(within(card).queryByTestId("app-style-winword")).not.toBeInTheDocument();
  });

  it("explains when the list of applications cannot be read, and retries", async () => {
    let calls = 0;
    const { card, user } = await openApps({}, (fake) => {
      fake.handlers.get_known_apps = () => {
        calls += 1;
        if (calls <= 2) throw { code: "internal", message: "" };
        return [SLACK];
      };
    });
    await user.click(within(card).getByRole("button", { name: "Ajouter une application" }));
    expect(await within(card).findByText("La liste des applications n’a pas pu être lue.")).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Réessayer" }));
    expect(await within(card).findByRole("button", { name: "Slack" })).toBeInTheDocument();
  });

  it("degrades when the native side predates per-application styles", async () => {
    const { card } = await openApps({ styles: undefined }, (fake) => {
      fake.handlers.get_known_apps = () => {
        throw { code: "unknown-command", message: "" };
      };
    });
    expect(
      within(card).getByText("Cette version de Sambler ne prend pas encore en charge les styles par application."),
    ).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Ajouter une application" })).not.toBeInTheDocument();
  });

  it("has no detectable accessibility violations", async () => {
    const { card, user } = await openApps({ styles: [{ app: WORD, profile: "faithful" }, { app: CODE }] });
    await user.click(toggleOf("Word"));
    await screen.findByTestId("app-preview-winword");
    await user.click(within(card).getByRole("button", { name: "Ajouter une application" }));
    await within(card).findByRole("button", { name: "Slack" });
    expect(await axeViolations()).toEqual([]);
  });
});
