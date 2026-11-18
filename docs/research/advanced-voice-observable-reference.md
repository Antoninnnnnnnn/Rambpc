# Référence visible : remplacement vocal ciblé

Plan du 18 novembre 2026, **non exécuté**. Le but est d'observer un effet
éditeur précis, pas de découvrir un sélecteur serveur. Android reste une
référence de développement : le moteur Rust natif n'a besoin d'aucun téléphone,
relais, clavier ni ASR de substitution à l'exécution.

## Ce que disent les sources publiques

| Source | Mode et signe visible | Actions annoncées, pas résultats mesurés |
|---|---|---|
| [Google : Rambler voice input on Gboard](https://support.google.com/gboard/answer/17468539?hl=en) | Appareil de la série Pixel 11 ; Gboard à jour, clavier par défaut, autorisation micro. Dans **Voice typing**, choisir **Rambler** (ou suivre l'introduction **Start**), puis toucher le micro ; l'animation lumineuse indique Rambler actif. **Done** arrête ou suspend la capture ; **Standard voice typing** rétablit la dictée standard. | Texte rendu après capture ; retouches ciblées (« Change blue to green »), réécritures, ajout d'emoji et changement de langue en cours de phrase. Hors ligne, nettoyage/ponctuation/majuscules restent possibles, mais l'édition complexe et la réécriture nécessitent la connexion ; **Retry** n'est pas un signe de réussite. |
| PixelBoard, projet indépendant : [README à `4b4e48d`](https://github.com/Akshayykadam/PixelBoard/blob/4b4e48dd4486f29d1140a3b36144933d90a1b7c4/README.md), [release `1.3.5` du 17 novembre 2026](https://github.com/Akshayykadam/PixelBoard/releases/tag/1.3.5) | **Voice typing → Dictation type → Rambler** plutôt que **Standard** ; le README décrit aussi **Enable Advanced Voice Typing** et la barre vocale. C'est la configuration de PixelBoard, pas une attestation d'éligibilité Gboard. | Le README annonce filtre des hésitations, autocorrections et ponctuation. La release annonce la conservation des modifications de texte pour « replace X with Y » et les commandes à mi-phrase, la correction des doublons au départ dans un champ non vide, la continuité à la rotation et moins de dérive entre langues. Les outils distincts de rédaction (reformulation, correction, ton) ne sont pas des commandes vocales validées par ces notes. |

Le [Pixel Drop du 10 novembre 2026](https://blog.google/products-and-platforms/devices/pixel/november-2026-pixel-drop/)
présente aussi Rambler parmi les nouveautés des appareils Pixel 10, avec un
calendrier général de déploiement progressif. Cela élargit les appareils
candidats par rapport à l'aide qui cite la série Pixel 11, sans confirmer la
disponibilité sur chaque modèle, région ou version. Pour le test, le critère
est **Rambler réellement proposé puis visiblement actif dans Gboard officiel**.

Le [premier essai de 9to5Google](https://9to5google.com/2026/08/13/gboard-rambler-prompt/)
observe un indicateur vocal, sans aperçu de texte, puis du texte après
validation et une nouvelle dictée pour retoucher le brouillon. C'est une
observation d'interface ponctuelle, pas la preuve que le serveur n'émet pas
de partiels. Une app peut choisir de ne montrer que le résultat final avec le
[SDK actuel](sdk-integration-guide.md#3-observe-preview-and-progress-without-taking-over-the-collector),
sans supprimer ni modifier les réponses provisoires du moteur.

Le README et la release PixelBoard sont deux publications distinctes, pas la
version mesurée le 16 novembre. Ni la proximité des intitulés ni un micro à
étoile ne prouvent le même mode actif, le même backend ou la même sémantique.

Les trois [observations PixelBoard du 16 novembre](../evidence/android-supervised-public-voice-20261116.json)
ont testé une dictée qui n'a pas préservé son préfixe ni ajouté de ponctuation,
une demande de reformulation enregistrée littéralement, et des callbacks SEND
non attribuables à chaque énoncé. Le cas ci-dessous ajoute une question différente :
**une commande remplace-t-elle un seul mot du texte préexistant, sans sélection et
sans toucher au reste ?** Cela n'exige ni formulation générative évaluée au jugé,
ni véritable envoi, ni mesure de ponctuation.

## Un cas, une séance supervisée, au plus deux énoncés

1. **Avant de parler, vérifier les préconditions.** Gboard officiel, appareil
   déjà éligible, version affichée et langue identique (`en-US`) pour les deux
   passages, connexion déjà disponible, même opérateur et même champ. Utiliser
   l'[éditeur public ordinaire déjà installé](../../tools/reference/android_editor/README.md#isolated-public-voice-reference-editor)
   `org.rambpc.publicvoiceprobe`, sans permission et dont SEND n'envoie rien.
   Ne pas passer à l'éditeur original avec incognito forcé. Pas de vrai message,
   compte, donnée personnelle, capture d'écran/audio ni consultation des fichiers
   privés. L'absence de permission réseau dans l'éditeur n'empêche pas le
   traitement effectué par le clavier.
2. **Témoin Standard, un énoncé.** Vérifier le libellé **Standard voice typing**
   dans Gboard. Dans l'éditeur, remettre à zéro le cas 1, remplacer manuellement
   son texte par `The kite is blue. Marker stays.` et vérifier visuellement la
   chaîne exacte, le curseur final (aucune sélection) et les deux compteurs à zéro.
   Avec le contrôle normal du micro, dire une seule fois `Change blue to green`
   puis arrêter normalement. Noter uniquement le mode, si la capture était
   visible, le texte affiché une fois la capture terminée et les compteurs visibles.
3. **Rambler, le même énoncé une fois.** Vérifier **Rambler** dans **Voice typing**,
   préparer le même champ exactement comme au passage Standard et confirmer
   l'animation lumineuse documentée lorsque le micro est activé, **avant de
   parler**. Répéter le script, arrêter normalement et relever le résultat avant
   toute autre action d'édition. Si activation ambiguë, requête **Retry**, erreur,
   limite d'usage ou précondition manquante, arrêter : noter le motif, pas un
   échec sémantique, et ne pas relancer. Rétablir le mode initial par l'interface.

**Critère fixé à l'avance :** `The kite is green. Marker stays.` exactement,
avec compteurs SEND/éditeur inchangés. Le seul remplacement doit être `blue` →
`green` sur la plage d'entrée UTF-16 `[12,16)` ; les signes de ponctuation, les
espaces et `Marker stays.` restent identiques. Ne pas imposer une position finale
du curseur que Google ne documente pas. Commande transcrite littéralement,
ponctuation seule, texte dupliqué, suppression ou réécriture du voisinage sont
à distinguer de ce résultat ; ne pas normaliser une différence pour la faire
passer. Noter état initial et résultat séparément pour chaque énoncé, sans
publier de capture privée ni d'identifiants.

- **Rambler exact / Standard transcrit sans cette retouche :** différence visible
  pour ce script et cette configuration, pas une preuve que le mode seul cause
  l'écart ni que Standard ne sait jamais éditer.
- **Les deux exacts :** la retouche est observable mais ne distingue pas les modes.
  **Rambler seul exploitable :** une réussite exacte établit une retouche côté
  éditeur pour ce passage, sans contraste. **Rambler actif mais sortie différente :**
  noter ce qui a changé, sans conclure à l'absence générale de fonctionnalité.

## Décision et dépendance

Ce plan compare deux lectures humaines du **même script**, ni le même PCM, ni
une référence Android/native appariée, ni un benchmark WAN. Il ne révèle ni
l'identité du service, ni la requête, la réponse brute, le contexte tardif ni le
filtre local. Les actions publiques ne sont pas les intentions wire 4/5/6
(COMPOSE/RAMBLE/RAMBLE_HISTORY). Aucun résultat UI ne suffit à justifier leur
activation ou une modification native : voir la [matrice](rust-feature-parity.md)
et les [discriminants avant changement client](rust-advanced-mode-contract-audit.md#precise-discriminators-needed-before-another-client-change).

MAIN peut intégrer le plan maintenant. La séance reste conditionnelle : il faut
que l'utilisateur confirme un appareil officiellement éligible avec Gboard en
état de marche et l'éditeur public déjà disponible, puis autorise et conduise
lui-même ces deux interactions, y compris leur éventuel traitement en ligne
ordinaire. La [préparation connue](android-reference-validation.md) ne démontre
pas cette disponibilité. Sans elle, ne rien lancer ni remplacer la référence
par PixelBoard : pas d'installation, d'instrumentation, de contournement de
permission/d'éligibilité, ni d'appel direct au service. Pour une évolution native,
il reste nécessaire d'obtenir un contrat ou une preuve de référence autorisée,
non secrète, du service et de l'accès exacts ; ce plan ne les fournit pas.
