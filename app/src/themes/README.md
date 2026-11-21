# Thèmes de Sambler

Un thème change l'apparence, jamais la structure : mêmes écrans, mêmes composants, même ARIA, mêmes
tests. L'utilisateur choisit le thème et le mode (Système, Clair, Sombre) dans Réglages > Apparence.

## Comment ça marche

- `registry.ts` : liste des thèmes (`THEME_IDS`, `THEMES`, `DEFAULT_THEME`). Données pures, aussi lues
  par la barre flottante. Le manifeste donne le nom, la phrase, les couleurs d'aperçu (`swatches`,
  par position : fond, surface, texte, accent, second accent facultatif) et, si besoin, les lueurs
  d'ambiance de l'aperçu (`glow`). Deux thèmes : « Verre et aurore » (`verre-aurore`, par défaut) et
  « Doux et rond » (`doux-rond`).
- Un id retiré du registre (« Sable », `sable`, supprimé au jalon 5) peut rester dans les réglages
  natifs ou le cache : il est refusé par `isThemeId` et l'apparence retombe sur `DEFAULT_THEME`, le
  mode choisi étant gardé. La valeur native change au prochain choix dans Réglages > Apparence.
- Le choix est enregistré nativement dans `Settings.appearance` (`{ theme, colorScheme }`) ; la barre
  flottante le reçoit dans `UiState.appearance`. Une copie en localStorage (`sambler.appearance.v1`)
  sert au premier affichage (`main.tsx` l'applique avant le rendu) et de repli si le natif est ancien.
- `appearance.ts` : valide le choix et pose `data-theme="<id>"` et `data-scheme="light|dark"` sur
  `<html>`. Le mode « Système » est résolu ici ; les CSS de thème ne contiennent donc **aucune**
  media query `prefers-color-scheme`.
- `AppearanceProvider.tsx` : `useAppearance()` côté React. La source de vérité est `Settings.appearance`
  (store de réglages, `save_settings`, retour arrière si le natif refuse). Chaque valeur appliquée est
  copiée dans le cache. Si le natif ne renvoie pas le champ, le choix vit dans le cache seul et
  s'applique quand même. En mode « Système », le changement de mode du système est suivi en direct.
- `<id>/theme.css` : **tous** les jetons du thème, sous `:root[data-theme="<id>"]` (clair) et
  `:root[data-theme="<id>"][data-scheme="dark"]` (sombre).
- `index.css` importe toutes les CSS de thème (fenêtre principale). Seules les règles du thème actif
  s'appliquent, et une police n'est téléchargée que si une règle active l'utilise.
- `<id>/preview.css` (facultatif) : la carte du thème dans Réglages > Apparence, visible quel que soit
  le thème actif. Ses règles visent `.theme-preview[data-preview="<id>"]` et n'utilisent que les
  couleurs du manifeste (`--preview-*`).
- `slots.tsx` : emplacements React facultatifs, par id de thème (voir plus bas).
- Les composants n'utilisent que des jetons sémantiques (`var(--…)`), jamais une couleur en dur.
- Les jetons indépendants du thème (échelle typographique `--text-xs…3xl`, espacements `--space-*`,
  rayons `--radius-*`, tailles `--control-h*`, `--titlebar-h`, `--sidebar-w`, `--content-w`, durées
  `--dur*`, `--ease`) sont dans `styles/tokens.css`. Un thème peut en surcharger certains (Verre et
  aurore change les rayons, la largeur du rail et celle du contenu ; Doux et rond aussi, avec la
  hauteur des contrôles).

## Les jetons d'un thème

Chaque thème définit toute la liste ci-dessous, en clair et (pour ceux qui changent) en sombre.
`verre-aurore/theme.css` est la référence : `contract.test.ts` vérifie que chaque thème définit tous
ses jetons, hormis ses variables privées `--va-*`. `doux-rond/theme.css` montre un thème sans verre ni
décor de fond.

### Typographie

| Jeton | Rôle |
| --- | --- |
| `--font-text` | Police du texte courant, des champs et des touches. |
| `--font-display` | Titres (h1, h2), grands chiffres, nom de marque. |
| `--font-mono` | Heures, chronos, compteurs. |
| `--font-features` | `font-feature-settings` du texte (`normal` si rien). |
| `--display-stretch` | `font-stretch` des titres (100 % = normal). |
| `--display-tracking` | Interlettrage des grands titres. |

### Fond et surfaces

| Jeton | Rôle |
| --- | --- |
| `--canvas` | Fond de la fenêtre, sous tout le reste. |
| `--sheet` | Surface pleine posée n'importe où (élément courant, onglet choisi, puits). |
| `--surface`, `--surface-2` | Cartes et plaques ; zones en creux (pistes, légendes). |
| `--surface-hover`, `--surface-pressed` | Survol et appui des boutons et lignes. |
| `--raised` | Boutons secondaires, cartes de choix. |
| `--field-bg` | Fond des champs et listes déroulantes (opaque). |
| `--border`, `--border-strong`, `--input-border` | Filets, bords appuyés, bord des champs. |

### Coque et verre

La coque a au plus trois surfaces dépolies : barre de titre, rail, grande feuille. Jamais de flou
imbriqué : tout ce qui est dans la feuille est une plaque sans flou.

| Jeton | Rôle |
| --- | --- |
| `--titlebar-bg` | Fond de la barre de titre. |
| `--rail-bg`, `--rail-bg-solid`, `--rail-shadow` | Rail (barre latérale) ; sa version pleine (sans flou ou transparence réduite) ; son ombre et ses filets (`box-shadow` complet). |
| `--panel-bg`, `--panel-bg-solid`, `--panel-border`, `--panel-shadow` | Grande feuille du contenu (et panneau de la prise en main), idem. |
| `--glass-blur`, `--glass-saturate` | Flou et saturation du verre. |
| `--glass-filter` | Valeur de `backdrop-filter` des trois surfaces (`none` pour un thème sans verre). |
| `--glass-border`, `--glass-edge`, `--glass-shadow` | Bord du verre, reflet du bord haut, ombre portée. |
| `--glass-strong` | Verre dense sans flou pour les petits éléments flottants dans la feuille (barre d'actions, toast, légende). |

### Texte

`--text`, `--text-2` (secondaire), `--text-3` (tertiaire), `--text-disabled`. Contraste AA exigé pour
`--text`, `--text-2` et `--text-3` sur `--panel-bg` et `--rail-bg`, y compris au-dessus du décor le
plus lumineux.

### Accent, états et voix

| Jeton | Rôle |
| --- | --- |
| `--accent`, `--accent-hover`, `--accent-pressed` | Action principale. |
| `--accent-soft`, `--accent-soft-2`, `--accent-ink`, `--on-accent`, `--focus` | Teinte douce, texte accentué, texte sur l'accent, anneau de focus. |
| `--success`, `--warning`, `--error` (+ `-soft`) | Tons des notices, pastilles et erreurs. |
| `--danger`, `--danger-hover`, `--on-danger` | Bouton de suppression définitive. |
| `--state-ready`, `--state-listening`, `--state-working` (+ `-soft`) | Couleurs vives des états de dictée (points, lueurs, badge). Toujours doublées d'un libellé et d'une forme. |
| `--voice-low`, `--voice-mid`, `--voice-high` | Niveau de voix, du calme au fort (vumètres). |
| `--wave-from`, `--wave-to` | Dégradé des barres de voix actives. |

### Aurore (décor)

| Jeton | Rôle |
| --- | --- |
| `--aurora-base` | Fond du décor. |
| `--aurora-1` … `--aurora-4` | Voiles de lumière, au repos. |
| `--aurora-live-1` … `--aurora-live-4` | Teintes vives là où vit la voix (tuile de l'Accueil). |
| `--aurora-blend`, `--aurora-strength` | Mode de fusion des voiles, intensité. |
| `--aurora-curtain-a`, `--aurora-curtain-b`, `--aurora-curtain-strength` | Rideaux verticaux. |
| `--aurora-grain` | Opacité du grain. |

Un thème sans aurore les pose sur ses propres teintes avec une intensité nulle (voir Doux et rond).

### Contrôles, marque et ombres

| Jeton | Rôle |
| --- | --- |
| `--key-bg`, `--key-border`, `--key-shadow` | Touches de clavier (`kbd`). |
| `--switch-thumb`, `--switch-thumb-off`, `--switch-thumb-shadow` | Pastille des interrupteurs, allumés et éteints. |
| `--chevron` | Flèche des listes déroulantes (image SVG). |
| `--titlebar-close-bg`, `--titlebar-close-pressed`, `--on-titlebar-close` | Survol et appui de « Fermer ». |
| `--mark-tile-from`, `--mark-tile-to`, `--mark-ink`, `--mark-wave`, `--mark-shadow` | Le logo Sambler : tuile, curseur, onde, ombre. |
| `--shadow-xs`, `--shadow-sm`, `--shadow-md` | Ombres (`--shadow-xs` est celle des boutons). |
| `--shadow-accent` | Ombre du bouton principal. |
| `--ring` | Halo de focus des champs. |

Les variables privées d'un thème (préfixe court : `--va-*` pour Verre et aurore, `--dr-*` pour Doux
et rond) ne font pas partie du contrat : seules ses propres CSS les utilisent. Ajouter le préfixe d'un
nouveau thème à `PRIVATE` dans `contract.test.ts`.

## Emplacements facultatifs (`slots.tsx`)

| Emplacement | Où | Verre et aurore | Doux et rond |
| --- | --- | --- | --- |
| `Backdrop` | Derrière toute la fenêtre, barre de titre comprise (`position: fixed; z-index: -1`). | L'aurore animée, qui s'éveille pendant l'écoute (phase) et respire avec le niveau `sambler://level`. | Aucun : la coque d'iris reste unie. |
| `StatusArt` | Dans la carte d'état de l'Accueil, à côté de l'état. Reçoit `phase`, `ready`, `gesture`, `shortcut`. | La tuile d'aurore vive avec le raccourci en grandes touches. | La forme vivante (`Galets.tsx`) : deux galets pêche et lilas qui respirent, gonflent avec le niveau pendant l'écoute, virent au lilas pendant le traitement et à la brume quand la dictée n'est pas disponible. |

Règles : décoratif seulement (`aria-hidden`, rien de focalisable, aucun texte que le contenu
accessible ne porte déjà ; les libellés d'une tuile sont tirés d'attributs `data-*`), jamais de texte
dicté, pause quand la fenêtre est cachée (`visibilitychange`), immobile avec
`prefers-reduced-motion`, masqué en `forced-colors`. Le niveau de voix se suit avec `listenShared`
(`ipc/shared.ts`) : un seul écouteur natif pour les barres de voix et le décor.

## Ajouter un thème

1. Ajouter l'id dans `THEME_IDS` et son manifeste dans `THEMES` (nom, phrase, `swatches` clair et
   sombre, `glow` si le thème a une lumière d'ambiance).
2. Créer `<id>/theme.css` qui définit **tous** les jetons ci-dessus (clair puis sombre), sans media
   query de schéma, et l'importer dans `index.css`. Partir d'une copie de `doux-rond/theme.css` (sans
   verre) ou de `verre-aurore/theme.css`, et renommer les variables privées.
3. Polices : embarquées localement (CSP `'self'`), via un paquet `@fontsource` ou des fichiers dans le
   dépôt, déclarées dans `<id>/fonts.css` et importées dans `index.css`.
4. Facultatif : `<id>/components.css` pour ce que les jetons ne suffisent pas à exprimer (formes,
   dispositions). Toutes les règles commencent par `:root[data-theme="<id>"]` et n'utilisent que des
   jetons. Ne jamais y poser le fond des panneaux : il passe par les jetons, pour que les replis
   (`--rail-bg-solid`, `--panel-bg-solid`) gagnent.
5. Facultatif : des emplacements React dans `slots.tsx` (`THEME_SLOTS[<id>]`) et leur CSS.
6. Facultatif : la peau de la barre flottante, `<id>/overlay.css`, chargée par l'entrée de la barre
   (`overlay.html`, `overlay/main.tsx`), pas par `index.css`, avec ses polices. Propriétés
   personnalisées seulement (liste en tête de `overlay/overlay.css`, galets compris) ; la capsule et
   son ombre tiennent dans la fenêtre de 360 × 72.
7. Facultatif : `<id>/preview.css` pour l'aperçu de Réglages > Apparence.
8. Vérifier le contraste AA du texte sur les panneaux dans les deux schémas, la taille minimale de la
   fenêtre (560 × 480), `forced-colors` et `prefers-reduced-motion`. Aucune couleur en dur hors de
   `theme.css` et de la palette de la peau de barre (`contract.test.ts` le vérifie pour
   `components.css`, `galets.css` et `preview.css`).
9. `npm run typecheck && npm test && npm run build`.
