// Dictionnaire (lexicon) and Raccourcis vocaux (expressions): rule text is
// read by these screens only, saved as a whole set, never announced.
import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type {
  Expression,
  LexiconEntry,
  PersonalRules,
  PersonalRulesInput,
  RuleConflict,
  Settings,
} from "../ipc/contract";
import { createFakeBridge, type FakeBridge } from "../test/fakeBridge";
import { appReady, axeViolations, liveRegion, recordAnnouncements, renderApp } from "../test/render";

const WORD: LexiconEntry = {
  id: "w1",
  heard: "projet mélusine",
  written: "projet Mélusine",
  enabled: true,
  applyInFaithful: false,
};
const SIGNATURE: Expression = {
  id: "e1",
  trigger: "insère ma signature",
  text: "Bien à vous,\nMarie Durand",
  enabled: true,
};

async function open(screenName: "Dictionnaire" | "Raccourcis vocaux", rules: Partial<PersonalRules> = {}, settings: Partial<Settings> = {}) {
  const bridge = createFakeBridge({ rules, settings });
  const view = renderApp(bridge);
  await appReady();
  await view.user.click(screen.getByRole("button", { name: screenName }));
  await screen.findByRole("heading", { level: 1, name: screenName });
  const list = await screen.findByTestId(screenName === "Dictionnaire" ? "lexicon-section" : "expression-section");
  return { bridge, list, ...view };
}

const savedRules = (bridge: FakeBridge) =>
  bridge.invoke.mock.calls
    .filter(([command]) => command === "save_personal_rules")
    .map(([, args]) => (args as { rules: PersonalRulesInput }).rules);

const lastSavedRules = (bridge: FakeBridge) => {
  const all = savedRules(bridge);
  return all[all.length - 1];
};

describe("Dictionnaire", () => {
  it("lists the stored words", async () => {
    const { list } = await open("Dictionnaire", { lexicon: [WORD], expressions: [SIGNATURE] });
    const word = within(list).getByTestId("rule-lexicon");
    expect(word).toHaveTextContent("projet mélusine devient projet Mélusine");
    expect(within(list).getByRole("heading", { name: /Vos mots/ })).toHaveTextContent("1");
    expect(screen.queryByTestId("rule-expression")).not.toBeInTheDocument();
  });

  it("adds a word and saves the whole set, expressions included", async () => {
    const { bridge, list, user } = await open("Dictionnaire", { expressions: [SIGNATURE] });
    expect(within(list).getByText("Aucun mot pour le moment")).toBeInTheDocument();
    await user.click(within(list).getByRole("button", { name: "Ajouter un mot" }));
    const form = within(list).getByRole("form", { name: "Nouveau mot" });
    expect(within(form).getByLabelText("Rambler écrit")).toHaveFocus();

    await user.click(within(form).getByRole("button", { name: "Enregistrer" }));
    expect(within(form).getAllByText("Ce champ est obligatoire.")).toHaveLength(2);
    expect(savedRules(bridge)).toHaveLength(0);

    await user.type(within(form).getByLabelText("Rambler écrit"), "projet mélusine");
    await user.type(within(form).getByLabelText("Je veux"), "projet Mélusine");
    await user.click(within(form).getByLabelText("Appliquer aussi en Fidèle"));
    await user.click(within(form).getByRole("button", { name: "Enregistrer" }));

    await waitFor(() => expect(savedRules(bridge)).toHaveLength(1));
    expect(lastSavedRules(bridge)).toEqual({
      lexicon: [{ id: expect.any(String), heard: "projet mélusine", written: "projet Mélusine", enabled: true, applyInFaithful: true }],
      expressions: [SIGNATURE],
    });
    expect(await within(list).findByTestId("rule-lexicon")).toHaveTextContent("Aussi en Fidèle");
    expect(within(list).getByRole("button", { name: "Ajouter un mot" })).toHaveFocus();
  });

  it("cancels an edit with Escape", async () => {
    const { bridge, list, user } = await open("Dictionnaire", { lexicon: [WORD] });
    await user.click(within(list).getByRole("button", { name: /Modifier.*projet mélusine/ }));
    await user.keyboard("{Escape}");
    expect(within(list).queryByRole("form")).not.toBeInTheDocument();
    expect(within(list).getByRole("button", { name: /Modifier.*projet mélusine/ })).toHaveFocus();
    expect(savedRules(bridge)).toHaveLength(0);
  });

  it("enables, disables and deletes words", async () => {
    const { bridge, list, user } = await open("Dictionnaire", { lexicon: [WORD] });
    await user.click(within(list).getByRole("switch", { name: /Actif.*projet mélusine/ }));
    await waitFor(() => expect(lastSavedRules(bridge)?.lexicon[0].enabled).toBe(false));
    expect(within(list).getByTestId("rule-lexicon")).toHaveTextContent("Désactivé");
    await user.click(within(list).getByRole("button", { name: /Supprimer.*projet mélusine/ }));
    await waitFor(() => expect(lastSavedRules(bridge)?.lexicon).toEqual([]));
    expect(within(list).getByRole("button", { name: "Ajouter un mot" })).toHaveFocus();
  });

  it("notes the Fidèle style", async () => {
    await open("Dictionnaire", { lexicon: [WORD] }, { profile: "faithful" });
    expect(screen.getByTestId("faithful-note")).toHaveTextContent(
      "Style Fidèle actif : seuls les mots marqués « Aussi en Fidèle » sont remplacés.",
    );
  });

  it("explains unreadable rules and does not allow overwriting them", async () => {
    await open("Dictionnaire", { unreadable: true });
    expect(screen.getByTestId("rules-unreadable")).toHaveTextContent(
      "Vos mots et expressions ont été créés sous un autre compte Windows ou sur un autre PC. Ils sont conservés, mais ne peuvent pas être lus ni modifiés ici.",
    );
    expect(screen.queryByRole("button", { name: "Ajouter un mot" })).not.toBeInTheDocument();
  });

  it("tries a sentence with preview_render and the current settings", async () => {
    const { bridge, user } = await open("Dictionnaire", { lexicon: [WORD] });
    bridge.handlers.preview_render = ({ sample }: { sample: string }) => sample.replace("mélusine", "Mélusine");
    await user.type(screen.getByLabelText("Phrase telle que Rambler l’écrirait"), "réunion projet mélusine");
    await user.click(screen.getByRole("button", { name: "Essayer" }));
    expect(bridge.invoke).toHaveBeenCalledWith("preview_render", {
      sample: "réunion projet mélusine",
      settings: expect.objectContaining({ profile: "natural", language: "french" }),
    });
    expect(await screen.findByTestId("rules-try-result")).toHaveTextContent("réunion projet Mélusine");
  });

  const A: LexiconEntry = { ...WORD, id: "a", heard: "rdv", written: "rendez-vous" };

  it("keeps the edited set unsaved on conflict and can restore the stored rules", async () => {
    const { bridge, list, user } = await open("Dictionnaire", { lexicon: [A] });
    bridge.handlers.save_personal_rules = ({ rules }: { rules: PersonalRulesInput }) => ({
      saved: false,
      rules: { lexicon: [A], expressions: [], unreadable: false },
      conflicts: [
        { reason: "duplicateHeard", rule: { kind: "lexicon", id: "a" }, related: { kind: "lexicon", id: rules.lexicon[1].id } },
      ],
    });
    await user.click(within(list).getByRole("button", { name: "Ajouter un mot" }));
    await user.type(within(list).getByLabelText("Rambler écrit"), "rdv");
    await user.type(within(list).getByLabelText("Je veux"), "RDV");
    await user.click(within(list).getByRole("button", { name: "Enregistrer" }));

    const banner = await screen.findByTestId("rules-conflicts");
    expect(banner).toHaveTextContent("1 conflit empêche l’enregistrement.");
    expect(banner).toHaveTextContent("Les dernières modifications ne sont pas enregistrées");
    const rows = within(list).getAllByTestId("rule-lexicon");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("Même mot que la règle « rdv » : gardez-en une seule active.");
    await user.click(screen.getByRole("button", { name: "Rétablir les règles enregistrées" }));
    expect(within(list).getAllByTestId("rule-lexicon")).toHaveLength(1);
    expect(screen.queryByTestId("rules-conflicts")).not.toBeInTheDocument();
  });

  it("has no detectable accessibility violations, form open", async () => {
    const { list, user } = await open("Dictionnaire", { lexicon: [WORD] });
    await user.click(within(list).getByRole("button", { name: "Ajouter un mot" }));
    expect(await axeViolations()).toEqual([]);
  });
});

describe("Raccourcis vocaux", () => {
  it("lists the stored expressions with their multi-line text", async () => {
    const { list } = await open("Raccourcis vocaux", { lexicon: [WORD], expressions: [SIGNATURE] });
    const expression = within(list).getByTestId("rule-expression");
    expect(expression).toHaveTextContent("insère ma signature");
    expect(expression.querySelector(".rule__payload")!.textContent).toContain("Bien à vous,\nMarie Durand");
    expect(within(list).getByText("Les expressions sont inactives en style Fidèle.")).toBeInTheDocument();
    expect(screen.queryByTestId("rule-lexicon")).not.toBeInTheDocument();
  });

  it("edits a multi-line expression with a preview before saving", async () => {
    const { bridge, list, user } = await open("Raccourcis vocaux", { expressions: [SIGNATURE] });
    await user.click(within(list).getByRole("button", { name: /Modifier.*insère ma signature/ }));
    const form = within(list).getByRole("form", { name: "Modifier l’expression" });
    const text = within(form).getByLabelText("Texte à insérer");
    await user.clear(text);
    await user.type(text, "Cordialement,{Enter}Marie{Enter}06 00 00 00 00");
    expect(within(form).getByTestId("expression-preview").textContent).toBe("Cordialement,\nMarie\n06 00 00 00 00");
    expect(savedRules(bridge)).toHaveLength(0);

    await user.click(within(form).getByRole("button", { name: "Enregistrer" }));
    await waitFor(() =>
      expect(lastSavedRules(bridge).expressions).toEqual([{ ...SIGNATURE, text: "Cordialement,\nMarie\n06 00 00 00 00" }]),
    );
    expect(within(list).getByRole("button", { name: /Modifier.*insère ma signature/ })).toHaveFocus();
  });

  it("deletes an expression and returns the keyboard to « Ajouter »", async () => {
    const { bridge, list, user } = await open("Raccourcis vocaux", { expressions: [SIGNATURE] });
    await user.click(within(list).getByRole("button", { name: /Supprimer.*insère ma signature/ }));
    await waitFor(() => expect(lastSavedRules(bridge)?.expressions).toEqual([]));
    expect(within(list).getByText("Aucune expression pour le moment")).toBeInTheDocument();
    expect(within(list).getByRole("button", { name: "Ajouter une expression" })).toHaveFocus();
  });

  it("notes that expressions are inactive in Fidèle", async () => {
    await open("Raccourcis vocaux", { expressions: [SIGNATURE] }, { profile: "faithful" });
    expect(screen.getByTestId("faithful-note")).toHaveTextContent("Style Fidèle actif : vos expressions ne s’appliquent pas.");
  });

  const T1: Expression = { id: "t1", trigger: "ma signature", text: "Marie", enabled: true };
  const T2: Expression = { id: "t2", trigger: "insère ma signature", text: "Marie D.", enabled: true };
  const L1: LexiconEntry = { ...WORD, id: "l1", heard: "ma signature rapide", written: "x" };

  it.each<[string, PersonalRules, RuleConflict, string, string | null]>([
    [
      "emptyReplacement",
      { lexicon: [], expressions: [T1], unreadable: false },
      { reason: "emptyReplacement", rule: { kind: "expression", id: "t1" }, related: null },
      "Le texte à insérer est vide.",
      null,
    ],
    [
      "noMatchableWords",
      { lexicon: [], expressions: [T1], unreadable: false },
      { reason: "noMatchableWords", rule: { kind: "expression", id: "t1" }, related: null },
      "Le déclencheur ne contient aucun mot : seulement de la ponctuation ou des symboles.",
      null,
    ],
    [
      "duplicateTrigger",
      { lexicon: [], expressions: [T1, { ...T1, id: "t3" }], unreadable: false },
      { reason: "duplicateTrigger", rule: { kind: "expression", id: "t1" }, related: { kind: "expression", id: "t3" } },
      "Même déclencheur que l’expression « ma signature » : gardez-en une seule active.",
      "Même déclencheur que l’expression « ma signature » : gardez-en une seule active.",
    ],
    [
      "triggerShadowsHeard",
      { lexicon: [L1], expressions: [T1], unreadable: false },
      { reason: "triggerShadowsHeard", rule: { kind: "expression", id: "t1" }, related: { kind: "lexicon", id: "l1" } },
      "Ce déclencheur se trouve dans le mot « ma signature rapide » du lexique : l’expression passerait avant ce mot.",
      null,
    ],
    [
      "triggerOverlap",
      { lexicon: [], expressions: [T1, T2], unreadable: false },
      { reason: "triggerOverlap", rule: { kind: "expression", id: "t1" }, related: { kind: "expression", id: "t2" } },
      "Ce déclencheur est contenu dans le déclencheur « insère ma signature ».",
      "Contient le déclencheur plus court « ma signature ».",
    ],
  ])("explains the %s conflict next to the rules involved", async (_reason, rules, conflict, ruleText, relatedText) => {
    const { bridge, list, user } = await open("Raccourcis vocaux", rules);
    bridge.handlers.save_personal_rules = () => ({ saved: false, rules, conflicts: [conflict] });
    const firstToggle = within(list).getAllByRole("switch")[0];
    await user.click(firstToggle);
    await user.click(firstToggle);
    expect(await screen.findByTestId("rules-conflicts")).toHaveTextContent("1 conflit empêche l’enregistrement.");
    expect(list.querySelector(`[data-rule-id="${conflict.rule.id}"]`)).toHaveTextContent(ruleText);
    if (relatedText && conflict.related) {
      expect(list.querySelector(`[data-rule-id="${conflict.related.id}"]`)).toHaveTextContent(relatedText);
    }
  });

  it("never announces rule text", async () => {
    const { bridge, list, user } = await open("Raccourcis vocaux", { lexicon: [WORD], expressions: [SIGNATURE] });
    const spoken = recordAnnouncements();
    await user.click(within(list).getByRole("button", { name: "Ajouter une expression" }));
    await user.type(within(list).getByLabelText("Déclencheur"), "formule secrète");
    await user.type(within(list).getByLabelText("Texte à insérer"), "Code 4521");
    await user.click(within(list).getByRole("button", { name: "Enregistrer" }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Expression ajoutée."));
    await user.click(within(list).getByRole("switch", { name: /Active.*insère ma signature/ }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Expression désactivée."));
    bridge.handlers.save_personal_rules = () => ({
      saved: false,
      rules: bridge.rules,
      conflicts: [{ reason: "emptyMatch", rule: { kind: "lexicon", id: "w1" }, related: null }],
    });
    await user.click(within(list).getByRole("button", { name: /Supprimer.*formule secrète/ }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("1 conflit empêche l’enregistrement."));
    spoken.stop();
    expect(spoken.texts.length).toBeGreaterThanOrEqual(3);
    for (const text of spoken.texts) {
      for (const secret of ["mélusine", "Mélusine", "signature", "Marie", "formule", "4521"]) {
        expect(text).not.toContain(secret);
      }
    }
  });

  it("has no detectable accessibility violations, form open", async () => {
    const { list, user } = await open("Raccourcis vocaux", { expressions: [SIGNATURE] });
    await user.click(within(list).getByRole("button", { name: /Modifier.*insère ma signature/ }));
    expect(await axeViolations()).toEqual([]);
  });
});
