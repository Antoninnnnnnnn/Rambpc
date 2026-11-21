// Style > Commandes de dictée: the switch, the list from get_voice_commands,
// the paste reminder, the preview, and the degraded states.
import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Settings, VoiceCommand } from "../ipc/contract";
import { VOICE_COMMANDS_SAMPLE } from "../labels/style";
import { createFakeBridge, type FakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, renderApp } from "../test/render";

const COMMANDS: VoiceCommand[] = [
  { spoken: "à la ligne", result: "↵", description: "Saut de ligne, avec une pause ou en début de phrase." },
  { spoken: "nouveau paragraphe", result: "↵↵", description: "Deux sauts de ligne, avec une pause ou en début de phrase." },
  { spoken: "ouvrez les guillemets", result: "«", description: "Ouvre les guillemets français." },
];

const TEXT_WITHOUT_COMMANDS = { lightPunctuation: true, typographicSpaces: true, sentenceCase: true };

async function openCommands(settings: Partial<Settings> = {}, setup: (bridge: FakeBridge) => void = () => {}) {
  const bridge = createFakeBridge({ settings });
  bridge.handlers.get_voice_commands = () => COMMANDS;
  setup(bridge);
  const view = renderApp(bridge);
  await appReady();
  await view.user.click(screen.getByRole("button", { name: "Style" }));
  await screen.findByRole("heading", { level: 1, name: "Style" });
  const card = screen.getByRole("region", { name: "Commandes de dictée" });
  return { bridge, card, ...view };
}

/** The spoken phrases of the list (the preview has terms of its own). */
async function commandTerms(card: HTMLElement) {
  let terms: HTMLElement[] = [];
  await waitFor(() => {
    terms = within(card)
      .getAllByRole("term")
      .filter((term) => !term.closest("[data-testid='voice-commands-preview']"));
    expect(terms.length).toBeGreaterThan(0);
  });
  return terms;
}

const lastSaved = (bridge: FakeBridge) => {
  const saved = bridge.invoke.mock.calls.filter(([command]) => command === "save_settings");
  return (saved[saved.length - 1]?.[1] as { settings: Settings } | undefined)?.settings;
};

describe("Style > Commandes de dictée", () => {
  it("lists what is said and what is written, in Naturel and French only", async () => {
    const { card } = await openCommands();
    expect(card).toHaveTextContent(/En profil Naturel et en français seulement\./);
    const terms = await commandTerms(card);
    // French guillemets with no-break spaces inside.
    expect(terms[0].textContent).toBe("« à la ligne »");
    expect(terms.map((term) => term.textContent?.replace(/ /g, " "))).toEqual([
      "« à la ligne »",
      "« nouveau paragraphe »",
      "« ouvrez les guillemets »",
    ]);
    const definitions = within(card).getAllByRole("definition");
    expect(definitions[0]).toHaveTextContent("↵Saut de ligne, avec une pause ou en début de phrase.");
    expect(definitions[2]).toHaveTextContent("«Ouvre les guillemets français.");
  });

  it("switches the commands with text.voiceCommands", async () => {
    const { bridge, card, user } = await openCommands();
    const toggle = within(card).getByRole("switch", { name: "Utiliser les commandes de dictée" });
    expect(toggle).toBeChecked();
    expect(toggle).toHaveAccessibleDescription(/Marquez une courte pause avant/);
    await user.click(toggle);
    await waitFor(() => expect(lastSaved(bridge)?.text).toMatchObject({ voiceCommands: false, sentenceCase: true }));
    expect(toggle).not.toBeChecked();
  });

  it("says the commands are ignored in Fidèle and in English", async () => {
    const { card } = await openCommands({ profile: "faithful" });
    const toggle = within(card).getByRole("switch", { name: "Utiliser les commandes de dictée" });
    expect(toggle).toBeDisabled();
    expect(toggle).toHaveAccessibleDescription(/^Ignorées en Fidèle/);
  });

  it("says the commands are French", async () => {
    const { card } = await openCommands({ language: "english" });
    const toggle = within(card).getByRole("switch", { name: "Utiliser les commandes de dictée" });
    expect(toggle).toBeEnabled();
    expect(toggle).toHaveAccessibleDescription(/^Ignorées pendant une dictée en anglais/);
  });

  it("reminds that line breaks need the paste insertion", async () => {
    const { card } = await openCommands();
    expect(within(card).getByTestId("line-break-reminder")).toHaveTextContent(
      /Les sauts de ligne demandent l’insertion par collage\s: c’est votre réglage actuel\./,
    );
  });

  it("warns when the text is typed and leads to the insertion setting", async () => {
    const { card, user } = await openCommands({ insertion: "typing" });
    const reminder = within(card).getByTestId("line-break-reminder");
    expect(reminder).toHaveTextContent(/Avec la frappe caractère par caractère, votre réglage actuel/);
    await user.click(within(reminder).getByRole("button", { name: "Choisir l’insertion" }));
    await screen.findByRole("heading", { level: 1, name: "Réglages" });
    await waitFor(() => expect(screen.getByRole("region", { name: "Insertion" })).toHaveFocus());
  });

  it("previews a sentence with commands through preview_render", async () => {
    const { bridge, card } = await openCommands({}, (fake) => {
      fake.handlers.preview_render = ({ sample, settings }: { sample: string; settings: Settings }) =>
        settings.text.voiceCommands ? sample.replace(/,? à la ligne /, "\n") : sample;
    });
    const preview = await within(card).findByTestId("voice-commands-preview");
    expect(preview).toHaveTextContent(/Vous dites\s*bonjour Claire, à la ligne le client/);
    const [, written] = within(preview).getAllByRole("definition");
    expect(within(preview).getAllByRole("term")[1]).toHaveTextContent("Sambler écrit");
    expect(written.textContent).toMatch(/^bonjour Claire\nle client a dit/);
    expect(bridge.invoke).toHaveBeenCalledWith("preview_render", {
      sample: VOICE_COMMANDS_SAMPLE,
      settings: expect.objectContaining({ text: expect.objectContaining({ voiceCommands: true }) }),
    });
  });

  it("keeps the switch when the list cannot be read, and retries", async () => {
    let calls = 0;
    const { card, user } = await openCommands({}, (fake) => {
      fake.handlers.get_voice_commands = () => {
        calls += 1;
        if (calls === 1) throw { code: "unknown-command", message: "" };
        return COMMANDS;
      };
    });
    expect(await within(card).findByText("La liste des commandes n’a pas pu être lue.")).toBeInTheDocument();
    expect(within(card).getByRole("switch", { name: "Utiliser les commandes de dictée" })).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Réessayer" }));
    expect(await commandTerms(card)).toHaveLength(3);
  });

  it("degrades when the native side predates the commands", async () => {
    const { card } = await openCommands({ text: TEXT_WITHOUT_COMMANDS }, (fake) => {
      fake.handlers.get_voice_commands = () => {
        throw { code: "unknown-command", message: "" };
      };
    });
    expect(
      within(card).getByText("Cette version de Sambler ne prend pas encore en charge les commandes de dictée."),
    ).toBeInTheDocument();
    expect(within(card).queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByTestId("voice-commands-preview")).not.toBeInTheDocument();
  });

  it("has no detectable accessibility violations", async () => {
    const { card } = await openCommands({ insertion: "typing" });
    await commandTerms(card);
    await within(card).findByTestId("voice-commands-preview");
    expect(await axeViolations()).toEqual([]);
  });
});
