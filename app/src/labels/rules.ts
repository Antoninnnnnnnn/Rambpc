// French explanations of personal-rule conflicts (text/personal.rs
// `validate_rules`). Shown next to the rules involved; never announced.

import type { RuleConflict, RuleKind, RuleRef } from "../ipc/contract";

/** Quoted name of a rule as the person wrote it, for visible text only. */
export type DescribeRule = (ref: RuleRef) => string;

const MATCH_FIELD: Record<RuleKind, string> = {
  lexicon: "« Rambler écrit »",
  expression: "Le déclencheur",
};

/**
 * Message for one side of a conflict. `side` is "rule" for `conflict.rule`
 * and "related" for `conflict.related`.
 */
export function conflictText(conflict: RuleConflict, side: "rule" | "related", describe: DescribeRule): string {
  const own = side === "rule" ? conflict.rule : (conflict.related ?? conflict.rule);
  const other = side === "rule" ? conflict.related : conflict.rule;
  const otherName = other ? describe(other) : "une autre règle";
  const field = MATCH_FIELD[own.kind];
  switch (conflict.reason) {
    case "emptyMatch":
      return own.kind === "lexicon" ? "« Rambler écrit » est vide." : "Le déclencheur est vide.";
    case "emptyReplacement":
      return own.kind === "lexicon" ? "« Je veux » est vide." : "Le texte à insérer est vide.";
    case "noMatchableWords":
      return `${field} ne contient aucun mot : seulement de la ponctuation ou des symboles.`;
    case "multilineMatch":
      return `${field} doit tenir sur une seule ligne.`;
    case "duplicateHeard":
      return `Même mot que la règle ${otherName} : gardez-en une seule active.`;
    case "duplicateTrigger":
      return `Même déclencheur que l’expression ${otherName} : gardez-en une seule active.`;
    case "triggerShadowsHeard":
      return side === "rule"
        ? `Ce déclencheur se trouve dans le mot ${otherName} du lexique : l’expression passerait avant ce mot.`
        : `Contient le déclencheur ${otherName} : l’expression passerait avant ce mot.`;
    case "triggerOverlap":
      return side === "rule"
        ? `Ce déclencheur est contenu dans le déclencheur ${otherName}.`
        : `Contient le déclencheur plus court ${otherName}.`;
  }
}

/** Conflict messages concerning one rule (as `rule` or as `related`). */
export function conflictsFor(
  ref: RuleRef,
  conflicts: RuleConflict[],
  describe: DescribeRule,
): string[] {
  const messages: string[] = [];
  for (const conflict of conflicts) {
    if (conflict.rule.kind === ref.kind && conflict.rule.id === ref.id) {
      messages.push(conflictText(conflict, "rule", describe));
    } else if (conflict.related && conflict.related.kind === ref.kind && conflict.related.id === ref.id) {
      messages.push(conflictText(conflict, "related", describe));
    }
  }
  return messages;
}

export function conflictSummary(count: number): string {
  return count === 1
    ? "1 conflit empêche l’enregistrement. Corrigez ou désactivez la règle signalée."
    : `${count} conflits empêchent l’enregistrement. Corrigez ou désactivez les règles signalées.`;
}
