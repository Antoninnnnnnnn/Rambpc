import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type {
  Expression,
  LexiconEntry,
  PersonalRules,
  PersonalRulesInput,
  RuleConflict,
  Settings,
} from "../../ipc/contract";
import { createFakeBridge, type FakeBridge } from "../../test/fakeBridge";
import { appReady, axeViolations, liveRegion, recordAnnouncements, renderApp } from "../../test/render";

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

async function openRules(rules: Partial<PersonalRules> = {}, settings: Partial<Settings> = {}) {
  const bridge = createFakeBridge({ rules, settings });
  const view = renderApp(bridge);
  await appReady();
  await view.user.click(screen.getByRole("button", { name: "Réglages" }));
  const panel = await screen.findByRole("region", { name: "Mots et expressions" });
  await within(panel).findByRole("heading", { name: "Mots personnels" });
  return { bridge, panel, ...view };
}

const savedRules = (bridge: FakeBridge) =>
  bridge.invoke.mock.calls
    .filter(([command]) => command === "save_personal_rules")
    .map(([, args]) => (args as { rules: PersonalRulesInput }).rules);

const lastSavedRules = (bridge: FakeBridge) => {
  const all = savedRules(bridge);
  return all[all.length - 1];
};

describe("Mots et expressions", () => {
  it("lists the stored words and expressions", async () => {
    const { panel } = await openRules({ lexicon: [WORD], expressions: [SIGNATURE] });
    const word = within(panel).getByTestId("rule-lexicon");
    expect(word).toHaveTextContent("projet mélusine");
    expect(word).toHaveTextContent("projet Mélusine");
    const expression = within(panel).getByTestId("rule-expression");
    expect(expression).toHaveTextContent("insère ma signature");
    expect(expression.querySelector(".rule__payload")!.textContent).toContain("Bien à vous,\nMarie Durand");
    expect(within(panel).getByText("Les expressions sont inactives en profil Fidèle.")).toBeInTheDocument();
  });

  it("adds a word and saves the whole set", async () => {
    const { bridge, panel, user } = await openRules({ expressions: [SIGNATURE] });
    await user.click(within(panel).getByRole("button", { name: "Ajouter un mot" }));
    const form = within(panel).getByRole("form", { name: "Nouveau mot" });
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
      lexicon: [
        {
          id: expect.any(String),
          heard: "projet mélusine",
          written: "projet Mélusine",
          enabled: true,
          applyInFaithful: true,
        },
      ],
      expressions: [SIGNATURE],
    });
    expect(await within(panel).findByTestId("rule-lexicon")).toHaveTextContent("Aussi en Fidèle");
    expect(within(panel).getByRole("button", { name: "Ajouter un mot" })).toHaveFocus();
  });

  it("edits a multi-line expression with a preview before saving", async () => {
    const { bridge, panel, user } = await openRules({ expressions: [SIGNATURE] });
    await user.click(within(panel).getByRole("button", { name: /Modifier.*insère ma signature/ }));
    const form = within(panel).getByRole("form", { name: "Modifier l’expression" });
    const text = within(form).getByLabelText("Texte à insérer");
    await user.clear(text);
    await user.type(text, "Cordialement,{Enter}Marie{Enter}06 00 00 00 00");
    expect(within(form).getByTestId("expression-preview").textContent).toBe("Cordialement,\nMarie\n06 00 00 00 00");
    expect(savedRules(bridge)).toHaveLength(0);

    await user.click(within(form).getByRole("button", { name: "Enregistrer" }));
    await waitFor(() =>
      expect(lastSavedRules(bridge).expressions).toEqual([
        { ...SIGNATURE, text: "Cordialement,\nMarie\n06 00 00 00 00" },
      ]),
    );
    expect(within(panel).getByRole("button", { name: /Modifier.*insère ma signature/ })).toHaveFocus();
  });

  it("cancels an edit with Escape", async () => {
    const { bridge, panel, user } = await openRules({ lexicon: [WORD] });
    await user.click(within(panel).getByRole("button", { name: /Modifier.*projet mélusine/ }));
    await user.keyboard("{Escape}");
    expect(within(panel).queryByRole("form")).not.toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: /Modifier.*projet mélusine/ })).toHaveFocus();
    expect(savedRules(bridge)).toHaveLength(0);
  });

  it("enables, disables and deletes entries", async () => {
    const { bridge, panel, user } = await openRules({ lexicon: [WORD], expressions: [SIGNATURE] });
    await user.click(within(panel).getByRole("checkbox", { name: /Actif.*projet mélusine/ }));
    await waitFor(() => expect(lastSavedRules(bridge)?.lexicon[0].enabled).toBe(false));
    expect(within(panel).getByTestId("rule-lexicon")).toHaveTextContent("Désactivé");

    await user.click(within(panel).getByRole("button", { name: /Supprimer.*insère ma signature/ }));
    await waitFor(() => expect(lastSavedRules(bridge)?.expressions).toEqual([]));
    expect(within(panel).getByText("Aucune expression.")).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: "Ajouter une expression" })).toHaveFocus();
  });

  const A: LexiconEntry = { ...WORD, id: "a", heard: "rdv", written: "rendez-vous" };
  const B: LexiconEntry = { ...WORD, id: "b", heard: "rdv", written: "RDV" };
  const T1: Expression = { id: "t1", trigger: "ma signature", text: "Marie", enabled: true };
  const T2: Expression = { id: "t2", trigger: "insère ma signature", text: "Marie D.", enabled: true };
  const L1: LexiconEntry = { ...WORD, id: "l1", heard: "ma signature rapide", written: "x" };

  it.each<[string, PersonalRules, RuleConflict, string, string | null]>([
    [
      "emptyMatch",
      { lexicon: [A], expressions: [], unreadable: false },
      { reason: "emptyMatch", rule: { kind: "lexicon", id: "a" }, related: null },
      "« Rambler écrit » est vide.",
      null,
    ],
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
      "multilineMatch",
      { lexicon: [A], expressions: [], unreadable: false },
      { reason: "multilineMatch", rule: { kind: "lexicon", id: "a" }, related: null },
      "« Rambler écrit » doit tenir sur une seule ligne.",
      null,
    ],
    [
      "duplicateHeard",
      { lexicon: [A, B], expressions: [], unreadable: false },
      { reason: "duplicateHeard", rule: { kind: "lexicon", id: "a" }, related: { kind: "lexicon", id: "b" } },
      "Même mot que la règle « rdv » : gardez-en une seule active.",
      "Même mot que la règle « rdv » : gardez-en une seule active.",
    ],
    [
      "duplicateTrigger",
      { lexicon: [], expressions: [T1, { ...T1, id: "t3" }], unreadable: false },
      {
        reason: "duplicateTrigger",
        rule: { kind: "expression", id: "t1" },
        related: { kind: "expression", id: "t3" },
      },
      "Même déclencheur que l’expression « ma signature » : gardez-en une seule active.",
      "Même déclencheur que l’expression « ma signature » : gardez-en une seule active.",
    ],
    [
      "triggerShadowsHeard",
      { lexicon: [L1], expressions: [T1], unreadable: false },
      {
        reason: "triggerShadowsHeard",
        rule: { kind: "expression", id: "t1" },
        related: { kind: "lexicon", id: "l1" },
      },
      "Ce déclencheur se trouve dans le mot « ma signature rapide » du lexique : l’expression passerait avant ce mot.",
      "Contient le déclencheur « ma signature » : l’expression passerait avant ce mot.",
    ],
    [
      "triggerOverlap",
      { lexicon: [], expressions: [T1, T2], unreadable: false },
      { reason: "triggerOverlap", rule: { kind: "expression", id: "t1" }, related: { kind: "expression", id: "t2" } },
      "Ce déclencheur est contenu dans le déclencheur « insère ma signature ».",
      "Contient le déclencheur plus court « ma signature ».",
    ],
  ])("explains the %s conflict next to the rules involved", async (_reason, rules, conflict, ruleText, relatedText) => {
    const { bridge, panel, user } = await openRules(rules);
    bridge.handlers.save_personal_rules = () => ({ saved: false, rules, conflicts: [conflict] });
    // Any change triggers a save of the whole set.
    const firstToggle = within(panel).getAllByRole("checkbox")[0];
    await user.click(firstToggle);
    await user.click(firstToggle);

    const summary = await within(panel).findByTestId("rules-conflicts");
    expect(summary).toHaveTextContent("1 conflit empêche l’enregistrement.");
    const ruleItem = panel.querySelector(`[data-rule-id="${conflict.rule.id}"]`)!;
    expect(ruleItem).toHaveTextContent(ruleText);
    if (relatedText && conflict.related) {
      const relatedItem = panel.querySelector(`[data-rule-id="${conflict.related.id}"]`)!;
      expect(relatedItem).toHaveTextContent(relatedText);
    }
  });

  it("keeps the edited set unsaved on conflict and can restore the stored rules", async () => {
    const { bridge, panel, user } = await openRules({ lexicon: [A] });
    bridge.handlers.save_personal_rules = ({ rules }: { rules: PersonalRulesInput }) => ({
      saved: false,
      rules: { lexicon: [A], expressions: [], unreadable: false },
      conflicts: [
        {
          reason: "duplicateHeard",
          rule: { kind: "lexicon", id: "a" },
          related: { kind: "lexicon", id: rules.lexicon[1].id },
        },
      ],
    });
    await user.click(within(panel).getByRole("button", { name: "Ajouter un mot" }));
    await user.type(within(panel).getByLabelText("Rambler écrit"), "rdv");
    await user.type(within(panel).getByLabelText("Je veux"), "RDV");
    await user.click(within(panel).getByRole("button", { name: "Enregistrer" }));

    expect(await within(panel).findByTestId("rules-conflicts")).toHaveTextContent(
      "Les dernières modifications ne sont pas enregistrées",
    );
    expect(within(panel).getAllByTestId("rule-lexicon")).toHaveLength(2);
    await user.click(within(panel).getByRole("button", { name: "Rétablir les règles enregistrées" }));
    expect(within(panel).getAllByTestId("rule-lexicon")).toHaveLength(1);
    expect(within(panel).queryByTestId("rules-conflicts")).not.toBeInTheDocument();
  });

  it("notes that expressions are inactive in Fidèle", async () => {
    const { panel } = await openRules({ expressions: [SIGNATURE] }, { profile: "faithful" });
    expect(within(panel).getByTestId("faithful-note")).toHaveTextContent(
      "Profil Fidèle actif : vos expressions ne s’appliquent pas.",
    );
    expect(within(panel).getByText(/seuls les mots marqués « Aussi en Fidèle » sont remplacés/)).toBeInTheDocument();
  });

  it("explains unreadable rules and does not allow overwriting them", async () => {
    const { panel } = await openRules({ unreadable: true });
    expect(within(panel).getByTestId("rules-unreadable")).toHaveTextContent(
      "Vos mots et expressions ont été créés sous un autre compte Windows ou sur un autre PC. Ils sont conservés, mais ne peuvent pas être lus ni modifiés ici.",
    );
    expect(within(panel).queryByRole("button", { name: "Ajouter un mot" })).not.toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: "Ajouter une expression" })).not.toBeInTheDocument();
  });

  it("tries a sentence with preview_render and the current settings", async () => {
    const { bridge, panel, user } = await openRules({ expressions: [SIGNATURE] });
    bridge.handlers.preview_render = ({ sample }: { sample: string }) =>
      sample.replace("insère ma signature", SIGNATURE.text);
    await user.type(within(panel).getByLabelText("Phrase telle que Rambler l’écrirait"), "merci insère ma signature");
    await user.click(within(panel).getByRole("button", { name: "Essayer" }));
    expect(bridge.invoke).toHaveBeenCalledWith("preview_render", {
      sample: "merci insère ma signature",
      settings: expect.objectContaining({ profile: "natural", language: "french" }),
    });
    expect((await within(panel).findByTestId("rules-try-result")).textContent).toContain("Bien à vous,\nMarie Durand");
  });

  it("never announces rule text", async () => {
    const { bridge, panel, user } = await openRules({ lexicon: [WORD], expressions: [SIGNATURE] });
    const spoken = recordAnnouncements();
    await user.click(within(panel).getByRole("button", { name: "Ajouter une expression" }));
    await user.type(within(panel).getByLabelText("Déclencheur"), "formule secrète");
    await user.type(within(panel).getByLabelText("Texte à insérer"), "Code 4521");
    await user.click(within(panel).getByRole("button", { name: "Enregistrer" }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Expression ajoutée."));
    await user.click(within(panel).getByRole("checkbox", { name: /Actif.*projet mélusine/ }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("Mot désactivé."));
    bridge.handlers.save_personal_rules = () => ({
      saved: false,
      rules: bridge.rules,
      conflicts: [{ reason: "emptyMatch", rule: { kind: "lexicon", id: "w1" }, related: null }],
    });
    await user.click(within(panel).getByRole("button", { name: /Supprimer.*formule secrète/ }));
    await waitFor(() => expect(liveRegion()).toHaveTextContent("1 conflit empêche l’enregistrement."));
    spoken.stop();
    expect(spoken.texts.length).toBeGreaterThanOrEqual(3);
    for (const text of spoken.texts) {
      for (const secret of ["mélusine", "Mélusine", "signature", "Marie", "formule", "4521"]) {
        expect(text).not.toContain(secret);
      }
    }
  });

  it("has no detectable accessibility violations, forms open", async () => {
    const { panel, user } = await openRules({ lexicon: [WORD], expressions: [SIGNATURE] });
    await user.click(within(panel).getByRole("button", { name: "Ajouter un mot" }));
    expect(await axeViolations()).toEqual([]);
    await user.keyboard("{Escape}");
    await user.click(within(panel).getByRole("button", { name: /Modifier.*insère ma signature/ }));
    expect(await axeViolations()).toEqual([]);
  });
});
