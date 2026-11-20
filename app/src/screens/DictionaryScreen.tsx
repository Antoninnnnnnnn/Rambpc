// Dictionnaire: whole-word replacements of what Rambler writes.

import { LexiconList, RulesStatus, TryRules } from "../components/rules/RuleLists";
import { usePersonalRules } from "../components/rules/useRules";
import { useSettings } from "../state/settings";
import { ScreenHeader } from "./ScreenHeader";

export function DictionaryScreen() {
  const store = usePersonalRules();
  const { settings } = useSettings();
  const loaded = store.working !== null && store.stored !== null;

  return (
    <div className="screen">
      <ScreenHeader title="Dictionnaire">
        Apprenez à Sambler vos noms propres, sigles et termes métier : un mot entier écrit par Rambler est remplacé par
        votre graphie. Vos mots restent sur ce PC, chiffrés pour votre compte Windows, et s’appliquent à la prochaine
        dictée.
      </ScreenHeader>
      <RulesStatus store={store} />
      {store.loadError ? null : !loaded ? (
        <p className="muted">Chargement…</p>
      ) : (
        <>
          <LexiconList store={store} faithful={settings?.profile === "faithful"} />
          {settings ? <TryRules settings={settings} placeholder="par exemple : réunion projet mélusine jeudi" /> : null}
        </>
      )}
    </div>
  );
}
