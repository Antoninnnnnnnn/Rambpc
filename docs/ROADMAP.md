# Feuille de route du moteur natif Rambler

**Objectif avant l’app.** Livrer un SDK Rust qui prend un flux audio et un contexte d’édition, puis rend un texte fiable ou une erreur exploitable sur Windows, macOS, Linux et Android. La future app fournira le microphone, le document actif et l’interface. Un petit programme de test suffit pour exercer aujourd’hui les mêmes entrées du SDK.

Plan établi le 18 novembre 2026 sur la révision `9a29ab8`. Le Xiaomi disponible a terminé 40 sessions locales avec les mêmes textes que Windows. Deux problèmes étaient reproductibles sans service de reconnaissance : le transport Android ne trouvait pas de racines par défaut, et la limite globale de 30 s interrompait un WAV de 31 s à cadence réelle. [Audit historique](research/audit-moteur-20261118.md) · [Mesures et versions](evidence/audit-moteur-20261118.json)

Depuis l’audit, `7ceb590` a passé les [10 jobs Rust](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/44373609858) et les [4 jobs Python](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/44373609771) avant le passage sur `main`. Le collecteur segmente l’audio sur des RPC successifs et indépendants ; [609,03 s sur localhost](evidence/rust-long-pcm-loopback-windows-20261118.md) sont couverts en quatre segments sans perte ni doublon, avec rétention des seuls segments terminés sur une erreur injectée. Ce test est accéléré, avec des réponses prédéfinies. Il ne qualifie ni la cadence réelle sur cinq minutes, ni la reconnaissance distante.

Les [diagnostics de fin et l’endurance](evidence/rust-native-real-reliability-20261118.json) séparent ce qui est observé par le client d’une livraison au pair. Un `DeadlineExceeded` historique et deux arrêts vers 300 s sont documentés ; leurs causes ne sont pas établies. Aucun succès local ne démontre un accès autorisé au service pour une application distribuée.

| Quand | Travail à engager | Résultat attendu |
| --- | --- | --- |
| Dès maintenant | Corriger la connexion Android et la durée des dictées. Ajouter les horodatages de session et commencer le corpus français. | Un essai reproductible sur le Xiaomi, une dictée longue qui termine et une première mesure du délai après l’arrêt. |
| Avant d’intégrer le moteur | Qualifier les coupures, les éditions, l’endurance et le même parcours SDK sur les quatre systèmes. | L’app peut distinguer un texte final, une proposition à relire, une interruption et une indisponibilité réseau. |
| À partir des mesures | Optimiser le coût qui pèse sur l’utilisateur. Étendre les formats audio, les langues et les modes avancés selon les résultats. | Chaque extension documente les plateformes couvertes, son effet observable et son coût. |

## 1. Faire fonctionner la connexion Android avec les réglages du téléphone

- Intégrer la vérification TLS du serveur au magasin de confiance Android. Sur le Xiaomi, le chargeur actuel trouvait zéro racine par défaut et 120 lorsque le magasin système était indiqué au processus. Utiliser la politique de la plateforme pour tenir compte des différences de versions et de profils.
- Distinguer dans le diagnostic un magasin vide, un certificat serveur refusé, un échec DNS et une expiration de connexion. Conserver la vérification du nom du serveur et la négociation HTTP/2.
- **Vérification.** Depuis un petit APK de test lancé sur le Xiaomi avec ses réglages ordinaires, réussir une connexion à un serveur HTTPS public. Vérifier aussi le rejet d’un serveur de test dont le nom ne correspond pas. Garder le contrôle TLS existant sur les trois systèmes de bureau. [Transport TLS](../crates/rambler-transport/src/connection.rs)

## 2. Donner à la dictée longue un contrat de durée utilisable

- Fournir un profil de dictée en temps réel dans le SDK et la CLI. Définir la durée disponible pour parler et le temps laissé au résultat après le dernier échantillon. Réutiliser `timeout`, `finalize_timeout` et le délai de contexte, et préserver la sémantique des délais qu’un appelant fixe explicitement.
- Prendre **cinq minutes** comme première cible de validation locale. Couvrir 29, 30, 31, 120 et 300 secondes à cadence réelle, une longue pause au milieu et un dernier mot juste avant l’arrêt. Faire varier séparément le retard de fin du serveur.
- **Vérification.** Avec le profil de dictée, le dernier mot témoin et la fin de flux arrivent pour toutes les durées prises en charge. Un serveur qui tarde au-delà du délai de finalisation produit une erreur identifiable. Un arrêt volontaire termine dans le délai annoncé. Vérifier ensuite la durée réellement observée sur le service. [Options et fin de session](../crates/rambler-engine/src/engine.rs)

## 3. Survivre aux coupures et libérer les ressources

- Sur un serveur local contrôlé, couper pendant l’envoi, après le dernier paquet et pendant l’attente de la fin gRPC. Enchaîner Wi-Fi coupé puis rétabli, sortie de veille, annulation et nouvelle dictée. Utiliser la reconnexion explicite pour la **prochaine** session. Seuls les segments déjà terminés sont conservés par le collecteur de dictée longue ; conserver de l’audio pour une tentative manuelle distincte exige un opt-in borné, un EOF normal et un nettoyage réussi.
- Distinguer une coupure réseau, un refus du service et une limitation temporaire de débit. Transmettre à l’hôte un délai de nouvelle tentative lorsque le service en fournit un. Vérifier qu’une série d’échecs ne lance aucune boucle de connexions en arrière-plan.
- Tester un producteur rapide, un consommateur lent, deux sessions simultanées et la fermeture de l’engine pendant une capture. Relever octets, échantillons, tâches, descripteurs et mémoire pendant une série de sessions sur un même processus. Vérifier la borne des files et le retour des ressources au niveau de repos après échauffement.
- Garder séparés les deux incidents historiques. Pour la session native incomplète à 282 s : relever le dernier échantillon, la fin d’envoi, la fin S3 et le statut gRPC lors d’une nouvelle observation autorisée. Côté Python, deux défauts distincts d’annulation ont été reproduits et corrigés, et 200 répétitions bornées du scénario audité ont terminé. La cause de l’attente historique n’a pas été établie. [Validation Python](evidence/python-transport-cancelled-close-windows-20261118.md)
- **Vérification.** Un succès exige l’envoi complet de la source, sa fermeture, la fin S3 et le statut gRPC OK. Dans la matrice de coupures, zéro texte partiel présenté comme final, zéro audio renvoyé automatiquement et zéro résultat attribué à la dictée suivante. La nouvelle session fonctionne après rétablissement du réseau. Le bilan d’endurance inclut les échecs et les ressources encore ouvertes. [Cycle de connexion](research/rust-connection-lifecycle.md) · [Incident natif](evidence/rust-incomplete-diagnostic-followup.json) · [Test Python](../tests/test_transport.py)

## 4. Mesurer et améliorer la qualité sur de vraies voix

- Commencer par un lot proposé de **40 énoncés spontanés**, majoritairement en français, complété par de l’anglais et des phrases bilingues. Varier locuteurs, accents, débit, bruit et silences. Inclure négations, dates, heures, noms propres, hésitations, autocorrections et répétitions voulues. Le corpus naturel déjà passé dans le moteur Rust représente 18,885 secondes. [Corpus existant](evidence/rust-native-natural-public.json)
- Annoter avant l’essai le texte acceptable, les informations à préserver et, pour une commande, l’action attendue. Réserver des phrases et des locuteurs pour vérifier les changements ultérieurs. Pour une reformulation, accepter plusieurs formulations si elles conservent les faits et répondent à la consigne.
- Inclure deux cas où la même opération de nettoyage aurait des effets opposés. Dans « mardi à quinze heures… non, jeudi à quinze heures », le jour attendu est jeudi. Dans « c’est très, très important », les deux « très » font partie du message attendu.
- Mesurer le taux d’erreur sur les mots pour la dictée littérale, la ponctuation, les erreurs sur les nombres et négations, les répétitions perdues et la réussite des éditions. Tester le même énoncé avec et sans vocabulaire contextuel ou langue secondaire lorsque ce réglage est en question.
- **Vérification.** Publier pour chaque catégorie le nombre de cas, les réussites, les erreurs et un exemple explicatif. Retester en particulier « Merci de envoyer », le changement de langue au milieu d’une phrase et une séquence avec une répétition intentionnelle. Comparer le rendu Rust à PixelBoard 1.3.5 sur le Xiaomi avec texte initial, sélection et énoncé appariés. Consigner toute différence de prise de son. [Campagne réelle](evidence/rust-native-real-campaign-20261118.json) · [Observations PixelBoard](evidence/pixelboard-vs-native-real-20261118.json)

## 5. Appliquer une édition exactement à la bonne portée

- Vérifier le remplacement d’un seul mot, la reformulation d’une sélection, le raccourcissement, le changement de ton, l’ajout d’emoji et la correction après une première dictée. Placer du texte témoin de chaque côté. Tester aussi insertion, suppression, proposition vide et résultat contenant tout l’historique.
- Exercer les politiques existantes `Strict`, `AsrOnly` et `S3ClientCompatible`. Couvrir plusieurs hypothèses, plusieurs finales, un aperçu remplacé, un statut de sécurité inconnu et un résultat ambigu. Une consigne transcrite littéralement doit conserver cette provenance. Les commandes d’envoi et d’effacement demandent une décision explicite de l’hôte.
- Faire évoluer le vérificateur d’éditions, aujourd’hui centré sur les propositions strictes, pour mesurer séparément les réécritures compatibles. Vérifier plage UTF-16, sélection résultante et graphèmes avec accents combinés, drapeaux et emoji composés. Changer le document, la révision ou l’ancre entre capture et application.
- **Vérification.** Tous les cas déterministes préservent le texte témoin. Un contexte périmé donne un conflit et laisse le document intact. Pour les cas réels, rapporter séparément l’intention comprise, la justesse du texte et la portée de la proposition. [Contrat d’édition](research/edit-acceptance-contract.md) · [API du SDK](research/sdk-integration-guide.md) · [Tests de graphèmes](../crates/rambler-engine/tests/splice.rs)

## 6. Mesurer le délai que ressent la personne qui dicte

- Horodater avec une horloge monotone le début de connexion, la connexion prête, le premier et le dernier échantillon fournis, la fin d’envoi, le premier texte exploitable éventuel et le résultat final validé. Le canal peut recevoir un message de contrôle bien avant tout texte. Le SDK doit accepter le repère temporel de la source pour la future mesure depuis le micro.
- Mesurer **dernier échantillon → résultat final validé** en connexion froide puis réutilisée, sur bureau et sur téléphone. Comparer réseau local contrôlé, Wi-Fi et réseau mobile. Donner l’effectif, les valeurs dispersées et les échecs avec la médiane et, lorsque la série le permet, le 95ᵉ percentile.
- Profiler CPU, mémoire, démarrage, volume transmis et chauffe pendant des sessions successives. Pour l’autonomie, répéter un scénario de même durée dans des conditions d’écran, de charge et de réseau comparables. L’encodage de cinq secondes de PCM à 16 kHz prenait déjà 22 à 48 ms sur le Xiaomi. Les médianes historiques approximatives de fin de dictée atteignaient 373 ms en français et 388 ms en anglais sur Windows.
- **Vérification.** Pour chaque coût dominant identifié, comparer une seule modification à la fois, sur le même audio. Les premiers candidats sont la réutilisation de connexion, la durée des pages Ogg et les copies sur le chemin audio. Retenir un changement quand le gain sur la session complète est reproductible et que qualité, consommation et délai de fin restent acceptables. [Mesures du Xiaomi](evidence/audit-moteur-20261118.json) · [Profil CPU Windows](evidence/rust-native-pcm48-session-cpu-windows-20261118.md) · [Méthode de mesure](performance.md)

## 7. Rendre l’entrée audio prévisible sur les quatre plateformes

- Consolider les chemins existants de PCM16 mono à 16 et 48 kHz et d’Ogg/Opus continu. Varier taille des blocs, dernier bloc incomplet, absence de parole et pression sur la file. Vérifier la durée décodée, l’ordre des échantillons et la fin audible avec un décodeur indépendant.
- Inventorier les formats fournis par les futurs hôtes. Si un format courant manque, ajouter l’adaptateur correspondant en priorité, par exemple 44,1 kHz, virgule flottante ou stéréo. Tester alors saturation, conversion en mono, filtrage et fidélité sur voix naturelles. Si les enregistrements révèlent un problème de bruit ou de fin de parole, évaluer un traitement ciblé sur les consonnes finales et les pauses.
- **Vérification.** Le même signal découpé différemment conserve son contenu et sa durée. L’app reçoit un diagnostic précis pour un format incompatible ou une capture abandonnée. Une différence de taille des paquets entre architectures se juge par décodage et par effet sur le résultat. [Chaîne audio](../crates/rambler-audio/BUILDING.md) · [Kit de qualification](research/native-qualification-kit.md)

## 8. Livrer un SDK utilisable et détecter les régressions

- Compiler en Release et exécuter le même exemple de consommation sur Windows x64, Linux x64, macOS Intel et Apple Silicon, Android ARM64. Sur Android, charger la bibliothèque dans le petit APK et couvrir les pages mémoire de 4 et 16 Kio. Après les essais locaux et TLS, comparer une dictée et un remplacement sur le service avec le même audio public depuis chaque plateforme disponible. Indiquer pour chaque cible la dernière étape réellement vérifiée.
- Stabiliser le parcours public « ouvrir → fournir l’audio → arrêter ou annuler → lire le résultat → fermer ». Vérifier la réutilisation après erreur, préciser qui libère chaque ressource et préserver la compatibilité des sorties CLI. Si le langage de l’app exige un pont natif, exposer ce parcours et tester la durée de vie des tampons audio et des fonctions de rappel depuis ce langage.
- Faire tourner en CI les scénarios courts de bout en bout et la comparaison Rust/Python sur les réponses connues. Conserver des fixtures expurgées pour les nouveaux formats observés, notamment champs inconnus, résultats multiples et finales tardives. Diagnostiquer les évolutions du service par version et provenance.
- Produire par défaut des diagnostics de durée, de volumes, d’étape et de catégorie d’erreur. Tester avec des marqueurs fictifs que texte dicté, sélection, audio et métadonnées privées restent hors des journaux et rapports d’incident. Vérifier de nouveau dépendances et bibliothèque Opus à chaque mise à jour.
- **Vérification.** Chaque cible livre un artefact chargeable et termine sur serveur local la dictée, l’édition, l’annulation et une nouvelle session après coupure. Un échec de CI indique la cible et l’étape précises. [Exemple d’intégration](../crates/rambler-cli/examples/minimal_app.rs) · [Matrice CI](../.github/workflows/rust.yml) · [Couverture de robustesse](research/native-robustness-coverage.md)

## Fonctions à explorer après le socle

- **Composition, suite de brouillon et historique.** Définir pour `COMPOSE`, `RAMBLE` et `RAMBLE_HISTORY` des cas avec consigne et effet attendu. Sur les essais publiés de composition et de continuation, le service a rendu la consigne littéralement. N’exposer une capacité que lorsqu’une réponse observée et son effet sur le texte permettent de la distinguer de la dictée ordinaire. D’ici là, rendre son état « non confirmé » accessible à l’hôte. [Matrice de fidélité](research/rust-feature-parity.md) · [Essais de composition](research/composition.md)
- **Hors connexion.** Livrer dès le socle un état réseau indisponible et une reprise propre de la prochaine dictée. La reconnaissance, la ponctuation et le nettoyage locaux demanderaient un composant embarqué distinct. Si cette capacité devient souhaitée, évaluer séparément qualité FR/EN, poids, CPU et autonomie avant de l’intégrer.
- **Autres langues et architectures.** Étendre la matrice en fonction des utilisateurs visés. Chaque ajout reprend le même corpus adapté, les tests d’écriture propres à la langue et une exécution sur la cible annoncée.

**Pour commencer l’app**, je viserais le même appel SDK fonctionnel sur les quatre plateformes, une dictée testée jusqu’à la durée annoncée, une reprise propre après coupure et des éditions qui préservent le reste du document. Le premier bilan FR/EN et la mesure du délai de fin donnent alors une base concrète pour juger l’expérience dans l’application.

## Travaux déjà validés

`main` reste le point stable. Le [journal des validations](STATUS.md) conserve les résultats et les incidents.

**Validation du lot PCM/réception :** formatage, Clippy strict et tests du
workspace hors ligne sur Windows ([461 réussites, 2 ignorées, 1 test TLS
filtré](evidence/rust-streamed-sdk-wave-windows-20261118.json)). Pour
`ac5d22d`, la [CI initiale](evidence/rust-streamed-sdk-wave-ci-20261118.json)
a passé 12 jobs sur 14 : le contrôle VS2022 attendait l’ancienne liste de
tests et le test Ogg macOS supposait une réception que rien ne synchronisait.
Le [lot Ogg/édition et les corrections de
qualification](evidence/rust-ogg-edit-wave-windows-20261118.json) passent
465 tests Rust, 42 tests Python, le formatage et Clippy strict hors ligne
sur Windows (2 tests ignorés, 1 test TLS filtré). Sur `40da48e`, les
[10 jobs Rust](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/44262188795)
et les [4 jobs Python](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/44262188873)
sont passés au premier essai, dont VS2022 et macOS ARM/Rust 1.88. Le
[contrôle d’édition synthétique](evidence/rust-native-edit-acceptance-seam-windows-20261118.json)
relie en plus le SDK Rust au vérificateur hors ligne. Le [lot PCM 48 kHz et
édition ciblée](evidence/rust-sdk-input-edits-windows-20261118.json) passe
244 tests et doctests hors ligne, dont les suites CLI du flux PCM, avec un
test PyAV optionnel ignoré. Aucun appel au vrai service.

Le filtre 48 → 16 kHz économise 33 à 51 % de CPU dans son profil local avec une sortie identique octet pour octet. La réduction des allocations à la réception gRPC n’a pas donné de gain stable sur une session locale complète. PixelBoard 1.3.5 apporte également ses propres corrections côté éditeur Android, notamment pour les remplacements. [Filtre audio](evidence/rust-pcm48-fir-symmetry-windows-20261118.json) · [Réception gRPC](research/raw-h2-receive-cost.md) · [Audit du client PixelBoard](research/pixelboard-135-client-result-audit.md)

## Cadence

Un propriétaire par thème actif, un worktree et un cache de compilation propres.
Le propriétaire implémente, compile, corrige et valide son lot jusqu’à un
livrable utilisable. Le coordinateur règle les interfaces et les dépendances,
intègre les lots qui passent ensemble et observe la CI en arrière-plan.

On communique quand un résultat, un choix ou un blocage change le plan, pas
après chaque compilation. On ne maintient pas un agent occupé si sa prochaine
étape ne change aucune décision. Un worktree isole le code, pas les secrets :
les travaux délégués restent sans données privées ni appels au fournisseur.
