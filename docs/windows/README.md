# Construire Sambler pour Windows

**Sambler permet de dicter au curseur dans l’application active. Rambler/Jetson de Google est son unique moteur de reconnaissance.** Sur Windows 11 x64, on maintient un raccourci, on dicte un courriel en français ou un prompt en anglais, puis on relâche. Le texte apparaît dans le champ de départ. Si on a changé de champ, la phrase attend dans Sambler. L’application est gratuite et s’utilise sans compte Sambler.

Le SDK Rust existe. L’application, son installateur et son édition portable restent à construire. Les essais du dépôt attestent de sessions distantes ponctuelles et d’un traitement segmenté sur serveur de test. L’accès utilisable par le public, la qualité avec un vrai microphone et l’insertion dans les éditeurs Windows restent à vérifier. Décisions produit du **20 novembre 2026**, état technique examiné à la révision `3683b57`. [État du moteur](../../app/README.md) · [Portée des observations distantes](../research/native-access-public-research.md) · [Essais d’intégration](../evidence/rust-integration-outcomes-windows-20261119.md)

## Lire le dossier

| Pour… | Document |
|---|---|
| Comprendre ce que la V1 apporte et quels réglages proposer | [03 · Parcours, réglages et priorités](03-produit-et-priorites.md) |
| Comprendre les choix face à Wispr Flow, Superwhisper et la dictée Windows | [01 · Produits et usages](01-produits-et-usages.md) |
| Voir quelles solutions reprendre ou écarter parmi quatre projets libres | [02 · Architectures ouvertes](02-architectures-ouvertes.md) |
| Organiser le code de l’application et ses échanges avec le SDK | [07 · Architecture de l’application](07-architecture-application.md) |
| Relier le micro à Rambler et décider quel texte a été validé | [04 · Contrat du moteur Rust](04-integration-moteur.md) |
| Gérer les raccourcis, le champ actif, la saisie et les incidents Windows | [05 · Intégration Windows](05-integration-windows.md) |
| Comprendre ce que reçoit Google, ce que garde le PC et l’effet d’une coupure | [06 · Confidentialité et continuité](06-confidentialite-et-continuite.md) |
| Confier le premier lot à Claude Code ou Codex et juger le résultat de la V1 | [08 · Plan de construction et recette](08-plan-de-construction.md) |
| Préparer l’installateur et le ZIP portable d’une même version GitHub | [09 · Distribution à budget nul et édition portable](09-distribution-et-version-portable.md) |

## Décisions actées

| Sujet | Décision pour Windows |
|---|---|
| Reconnaissance | Le client Rust envoie la voix directement à Rambler/Jetson après information de la personne. Aucun modèle local, fournisseur secondaire ou service vocal financé par le projet. En cas de panne, la dictée s’interrompt. Les passages déjà validés peuvent être récupérés. Le retour du service permet de commencer une nouvelle prise explicitement. |
| Usage quotidien | Windows 11 x64, interface française, dictée en français par défaut et en anglais sur sélection. Maintien pour parler et bascule sont livrés ensemble. Les parcours de référence couvrent Outlook, Word, un formulaire Edge et Chrome et un prompt d’IA dans VS Code. |
| Texte | **Naturel** par défaut, **Fidèle** au choix. Un lexique et des expressions personnels sont disponibles dès la V1. Le texte validé peut être mis en forme par des règles déterministes sur le PC. Les préférences se modifient dans les réglages. Une phrase dictée ne déclenche jamais l’envoi d’un message. |
| Confidentialité | Aucun audio enregistré sur disque par Sambler. Dernier brouillon en mémoire jusqu’à dix minutes, historique local désactivé au départ. Si on l’active, la durée initiale est de sept jours et les transcriptions sont protégées par le compte Windows. |
| Distribution | Code original libre sous MIT sur GitHub. Préparer deux téléchargements dans la même GitHub Release, un installateur par utilisateur et un ZIP portable à décompresser. Budget du projet **0 €**. Les deux éditions offrent la même dictée. Le dossier portable contient ses réglages. [Licence MIT](../../LICENSE.md) · [Détails de distribution](09-distribution-et-version-portable.md) |
| Construction | Tauri 2 avec React/TypeScript pour l’interface. Rust possède la capture, la session Rambler, les données et l’insertion Windows. Le simulateur et les réponses préenregistrées servent exclusivement aux tests. |

## Quand la V1 peut sortir

1. **Prouver que les destinataires pourront dicter.** Sur un poste propre, vérifier que l’accès Rambler applicable au public visé fonctionne, sans transmettre de secret privé via le dépôt ni engager de dépense. Consigner les conditions effectivement connues. Si l’accès reste réservé aux essais ou disparaît, le code peut rester publié et la dictée est annoncée indisponible pour les personnes concernées. [Accès et interruptions](06-confidentialite-et-continuite.md)
2. **Faire la recette avec de vraies voix.** Viser au moins 90 dictées utilisables sur les 100 premières tentatives, 70 en français et 30 en anglais, et zéro doublon, mauvaise destination ou envoi. Pour les phrases courtes à chaud, au moins 95 % doivent être visibles dans le champ en deux secondes au plus après le geste d’arrêt. Exercer aussi des prises de cinq minutes, l’accessibilité et la récupération sur incident. [Protocole et dénominateurs](08-plan-de-construction.md)
3. **Exercer les deux téléchargements.** Sous compte standard, essayer d’abord le ZIP sur un poste vierge de Sambler. Vérifier aussi un support en lecture seule et un changement de compte. Depuis un état initial équivalent, tester l’installateur et sa mise à niveau. Contrôler WebView2, l’emplacement des données, les empreintes et le statut de la signature. [Recette installateur et portable](09-distribution-et-version-portable.md)

Les décisions de produit sont prises. Les points encore ouverts appellent des mesures ou une vérification auprès du fournisseur. Le plan donne pour chacun une valeur de départ et la conduite à tenir selon le résultat.
