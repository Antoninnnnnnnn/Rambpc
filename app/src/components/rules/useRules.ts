// Personal rules (lexicon + expressions) for the Dictionnaire and Raccourcis
// vocaux screens. Rule text is fetched by the screen that shows it
// (`get_personal_rules`), edited locally and saved as a whole set with
// `save_personal_rules`; conflicts keep the edited set unsaved.
// Announcements never contain rule text.

import { useCallback, useEffect, useRef, useState } from "react";
import { announcement, useAnnounce } from "../../a11y/announcer";
import { toCommandError, useBridge } from "../../ipc/bridge";
import type { CommandError, Expression, LexiconEntry, PersonalRules, RuleConflict } from "../../ipc/contract";
import { conflictSummary, type DescribeRule } from "../../labels/rules";

export interface WorkingRules {
  lexicon: LexiconEntry[];
  expressions: Expression[];
}

export interface RulesStore {
  stored: PersonalRules | null;
  working: WorkingRules | null;
  conflicts: RuleConflict[];
  loadError: boolean;
  saveError: CommandError | null;
  load: () => void;
  /** Apply a change to the working set, then save the whole set. */
  commit: (change: (rules: WorkingRules) => WorkingRules, done: string) => Promise<boolean>;
  /** Back to the stored rules after a refused save. */
  revert: () => void;
  describe: DescribeRule;
}

export function usePersonalRules(): RulesStore {
  const bridge = useBridge();
  const announce = useAnnounce();
  const [stored, setStored] = useState<PersonalRules | null>(null);
  const [working, setWorking] = useState<WorkingRules | null>(null);
  const [conflicts, setConflicts] = useState<RuleConflict[]>([]);
  const [loadError, setLoadError] = useState(false);
  const [saveError, setSaveError] = useState<CommandError | null>(null);
  const workingRef = useRef<WorkingRules | null>(null);
  const queue = useRef<Promise<unknown>>(Promise.resolve());

  const load = useCallback(() => {
    setLoadError(false);
    bridge
      .invoke("get_personal_rules")
      .then((rules) => {
        setStored(rules);
        workingRef.current = { lexicon: rules.lexicon, expressions: rules.expressions };
        setWorking(workingRef.current);
      })
      .catch(() => setLoadError(true));
  }, [bridge]);

  useEffect(load, [load]);

  const commit = useCallback(
    (change: (rules: WorkingRules) => WorkingRules, done: string) => {
      const job = queue.current.then(async () => {
        if (!workingRef.current) return false;
        const next = change(workingRef.current);
        workingRef.current = next;
        setWorking(next);
        setSaveError(null);
        try {
          const result = await bridge.invoke("save_personal_rules", { rules: next });
          if (result.saved) {
            setStored(result.rules);
            workingRef.current = { lexicon: result.rules.lexicon, expressions: result.rules.expressions };
            setWorking(workingRef.current);
            setConflicts([]);
            announce(announcement(done));
          } else {
            // Nothing was written: keep the edited set so it can be fixed.
            setConflicts(result.conflicts);
            announce(announcement(conflictSummary(result.conflicts.length)));
          }
          return result.saved;
        } catch (error) {
          setSaveError(toCommandError(error));
          announce(announcement("Les mots et expressions n’ont pas pu être enregistrés."));
          return false;
        }
      });
      queue.current = job;
      return job;
    },
    [bridge, announce],
  );

  const revert = useCallback(() => {
    if (!stored) return;
    workingRef.current = { lexicon: stored.lexicon, expressions: stored.expressions };
    setWorking(workingRef.current);
    setConflicts([]);
    setSaveError(null);
    announce(announcement("Règles enregistrées rétablies."));
  }, [stored, announce]);

  const describe: DescribeRule = (ref) => {
    if (!working) return "une autre règle";
    if (ref.kind === "lexicon") {
      const entry = working.lexicon.find((item) => item.id === ref.id);
      return entry ? `« ${entry.heard} »` : "une autre règle";
    }
    const expression = working.expressions.find((item) => item.id === ref.id);
    return expression ? `« ${expression.trigger} »` : "une autre expression";
  };

  return { stored, working, conflicts, loadError, saveError, load, commit, revert, describe };
}
