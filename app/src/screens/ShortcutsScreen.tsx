// Raccourcis vocaux: say a trigger phrase, get a literal text (multi-line).

import { ExpressionList, RulesStatus, TryRules } from "../components/rules/RuleLists";
import { usePersonalRules } from "../components/rules/useRules";
import { useSettings } from "../state/settings";
import { ScreenHeader } from "./ScreenHeader";

export function ShortcutsScreen() {
  const store = usePersonalRules();
  const { settings } = useSettings();
  const loaded = store.working !== null && store.stored !== null;

  return (
    <div className="screen">
      <ScreenHeader title="Raccourcis vocaux">
        Dites un déclencheur pour insérer le texte choisi, tel quel, même sur plusieurs lignes : une signature, une
        adresse, une formule. Vos expressions restent sur ce PC, chiffrées pour votre compte Windows.
      </ScreenHeader>
      <RulesStatus store={store} />
      {store.loadError ? null : !loaded ? (
        <p className="muted">Chargement…</p>
      ) : (
        <>
          <ExpressionList store={store} faithful={settings?.profile === "faithful"} />
          {settings ? <TryRules settings={settings} placeholder="par exemple : merci insère ma signature" /> : null}
        </>
      )}
    </div>
  );
}
