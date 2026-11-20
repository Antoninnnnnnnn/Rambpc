# Thèmes de Sambler

Un thème change l'apparence, jamais la structure : mêmes écrans, mêmes composants, même ARIA, mêmes
tests. L'utilisateur choisit le thème et le mode (Système, Clair, Sombre) dans Réglages > Apparence.

## Comment ça marche

- `registry.ts` : liste des thèmes (`THEME_IDS`, `THEMES`, `DEFAULT_THEME`). Données pures.
- Le choix est enregistré nativement dans `Settings.appearance` (`{ theme, colorScheme }`) ; la barre
  flottante le reçoit dans `UiState.appearance`. Une copie en localStorage
  (`sambler.appearance.v1`) sert seulement au premier affichage et de repli si le natif est ancien.
- `appearance.ts` : valide le choix et pose `data-theme="<id>"` et `data-scheme="light|dark"` sur
  `<html>`. Le mode « Système » est résolu ici ; les CSS de thème ne contiennent donc **aucune**
  media query `prefers-color-scheme`.
- `<id>/theme.css` : les jetons du thème, sous `:root[data-theme="<id>"]` (clair) et
  `:root[data-theme="<id>"][data-scheme="dark"]` (sombre).
- Les composants n'utilisent que des jetons sémantiques (`var(--…)`), jamais une couleur en dur.

## Ajouter un thème

1. Ajouter l'id dans `THEME_IDS` et son manifeste dans `THEMES` (nom, phrase, couleurs d'aperçu).
2. Créer `<id>/theme.css` qui définit **tous** les jetons listés dans `sable/theme.css` (clair et
   sombre), et l'importer dans `themes/index.css`.
3. Polices : embarquées localement (CSP `'self'`), via un paquet `@fontsource` ou des fichiers dans
   le dépôt, déclarées dans `<id>/fonts.css`.
4. Facultatif : un décor de fond ou une peau de barre flottante propres au thème (voir les sections
   ajoutées par le thème Verre et aurore ci-dessous).
5. `npm run typecheck && npm test && npm run build`.

<!-- La liste complète des jetons et des emplacements facultatifs est complétée par le lot
     « moteur de thèmes ». -->
