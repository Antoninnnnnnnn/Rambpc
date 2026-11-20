// DEV PREVIEW ONLY: simulated personal rules, history and launch at startup
// for the browser preview. Never part of a production build.

import type {
  CommandError,
  HistoryEntry,
  HistoryQuery,
  HistoryStatus,
  PersonalRules,
  PersonalRulesInput,
  RuleConflict,
  Settings,
} from "../ipc/contract";

const DAY = 86_400_000;

function sampleHistory(retentionDays: number): HistoryEntry[] {
  const now = Date.now();
  const texts = [
    "Bonjour Claire, je ne pourrai pas venir jeudi à 14 h. Pouvons-nous décaler à vendredi ?",
    "Points pour la réunion budget :\n- loyers\n- salaires\n- investissements",
    "Rendez-vous jeudi à 14 h.",
    null,
  ];
  return Array.from({ length: 57 }, (_, index) => {
    const created = now - index * 47 * 60_000;
    return {
      id: `preview-${index}`,
      createdAtMs: created,
      expiresAtMs: created + retentionDays * DAY,
      language: index % 5 === 4 ? "english" : "french",
      profile: index % 3 === 2 ? "faithful" : "natural",
      text: texts[index % texts.length],
    } satisfies HistoryEntry;
  });
}

/** Very small stand-in for `validate_rules` (enough to see conflicts in the UI). */
function validate(rules: PersonalRulesInput): RuleConflict[] {
  const conflicts: RuleConflict[] = [];
  const lexicon = rules.lexicon.filter((entry) => entry.enabled);
  const expressions = rules.expressions.filter((entry) => entry.enabled);
  const key = (text: string) => text.trim().toLowerCase();
  for (const entry of lexicon) {
    if (!entry.heard.trim()) conflicts.push({ reason: "emptyMatch", rule: { kind: "lexicon", id: entry.id }, related: null });
  }
  lexicon.forEach((a, i) =>
    lexicon.slice(i + 1).forEach((b) => {
      if (key(a.heard) === key(b.heard)) {
        conflicts.push({ reason: "duplicateHeard", rule: { kind: "lexicon", id: a.id }, related: { kind: "lexicon", id: b.id } });
      }
    }),
  );
  expressions.forEach((a, i) =>
    expressions.slice(i + 1).forEach((b) => {
      const [short, long] = key(a.trigger).length <= key(b.trigger).length ? [a, b] : [b, a];
      if (key(a.trigger) === key(b.trigger)) {
        conflicts.push({ reason: "duplicateTrigger", rule: { kind: "expression", id: a.id }, related: { kind: "expression", id: b.id } });
      } else if (` ${key(long.trigger)} `.includes(` ${key(short.trigger)} `)) {
        conflicts.push({ reason: "triggerOverlap", rule: { kind: "expression", id: short.id }, related: { kind: "expression", id: long.id } });
      }
    }),
  );
  return conflicts;
}

export function createPreviewPersonal(getSettings: () => Settings, setSettings: (settings: Settings) => void) {
  let rules: PersonalRules = {
    lexicon: [{ id: "lx-1", heard: "projet mélusine", written: "projet Mélusine", enabled: true, applyInFaithful: false }],
    expressions: [
      { id: "ex-1", trigger: "insère ma signature", text: "Bien à vous,\nMarie Durand", enabled: true },
    ],
    unreadable: false,
  };
  let history: HistoryEntry[] = sampleHistory(getSettings().history.retentionDays);
  let skipNext = false;

  const status = (): HistoryStatus => ({
    enabled: getSettings().history.enabled,
    retentionDays: getSettings().history.retentionDays,
    entryCount: history.length,
    skipNext,
    available: true,
  });

  const fail = (code: string, message: string): never => {
    throw { code, message } satisfies CommandError;
  };

  return {
    get_personal_rules: () => structuredClone(rules),
    save_personal_rules: (args?: Record<string, unknown>) => {
      const input = args!.rules as PersonalRulesInput;
      const conflicts = validate(input);
      if (conflicts.length > 0) return { saved: false, rules: structuredClone(rules), conflicts };
      rules = { ...structuredClone(input), unreadable: false };
      return { saved: true, rules: structuredClone(rules), conflicts: [] };
    },
    get_history: (args?: Record<string, unknown>) => {
      const query = (args?.query ?? {}) as HistoryQuery;
      const search = query.search?.toLowerCase() ?? "";
      const matching = history.filter((entry) => !search || (entry.text ?? "").toLowerCase().includes(search));
      const start = query.cursor ? Number(query.cursor) : 0;
      const limit = Math.min(query.limit ?? 50, 200);
      return {
        entries: matching.slice(start, start + limit),
        nextCursor: start + limit < matching.length ? String(start + limit) : null,
      };
    },
    copy_history_entry: (args?: Record<string, unknown>) => {
      const entry = history.find((item) => item.id === args!.id);
      if (!entry) fail("not-found", "Cette dictée n'existe plus.");
    },
    delete_history_entry: (args?: Record<string, unknown>) => {
      history = history.filter((item) => item.id !== args!.id);
      return status();
    },
    clear_history: (args?: Record<string, unknown>) => {
      history = [];
      if (args!.disable) {
        const settings = getSettings();
        setSettings({ ...settings, history: { ...settings.history, enabled: false } });
      }
      return status();
    },
    get_history_status: () => status(),
    skip_history_next_take: (args?: Record<string, unknown>) => {
      skipNext = Boolean(args!.skip);
      return status();
    },
    set_launch_at_startup: (args?: Record<string, unknown>) => {
      setSettings({ ...getSettings(), launchAtStartup: Boolean(args!.enabled) });
    },
  };
}
