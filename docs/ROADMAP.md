# Feuille de route du moteur natif Rambler

**Objectif avant l’app.** Livrer un SDK Rust qui prend un flux audio et un contexte d’édition, puis rend un texte fiable ou une erreur exploitable sur Windows, macOS, Linux et Android. La future app fournira le microphone, le document actif et l’interface. Un petit programme de test suffit pour exercer aujourd’hui les mêmes entrées du SDK.

Plan établi le 18 novembre 2026 sur la révision `9a29ab8`. Le Xiaomi disponible a terminé 40 sessions locales avec les mêmes textes que Windows. Deux problèmes étaient reproductibles sans service de reconnaissance : le transport Android ne trouvait pas de racines par défaut, et la limite globale de 30 s interrompait un WAV de 31 s à cadence réelle. [Audit historique](research/audit-moteur-20261118.md) · [Mesures et versions](evidence/audit-moteur-20261118.json)

Depuis l’audit, `7ceb590` a passé les [10 jobs Rust](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/44373609858) et les [4 jobs Python](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/44373609771) avant le passage sur `main`. Le collecteur segmente l’audio sur des RPC successifs et indépendants ; [609,03 s sur localhost](evidence/rust-long-pcm-loopback-windows-20261118.md) sont couverts en quatre segments sans perte ni doublon, avec rétention des seuls segments terminés sur une erreur injectée. Ce test est accéléré, avec des réponses prédéfinies. Il ne qualifie ni la cadence réelle sur cinq minutes, ni la reconnaissance distante.

Les [diagnostics de fin et l’endurance](evidence/rust-native-real-reliability-20261118.json) séparent ce qui est observé par le client d’une livraison au pair. Un `DeadlineExceeded` historique et deux arrêts vers 300 s sont documentés ; leurs causes ne sont pas établies. Aucun succès local ne démontre un accès autorisé au service pour une application distribuée.

Le lot correctif (`7d9af7b` pour le code validé) passe sous Windows avec Rust stable et 1.88 : **526 tests réussis, 2 ignorés**, plus **24 tests CLI sans audio** par toolchain ; formatage et Clippy strict au vert. La suite Python intégrée a passé **931 tests, 4 sautés** sur trois passages instrumentés. Le TLS de l’APK témoin et la dictée locale à cadence réelle sont détaillés ci-dessous. La CI multiplateforme de ce nouveau lot reste distincte de celle de `7ceb590`.

Le [second audit](research/audit-parite-integration-20261118.md) a repéré quatre points d’intégration : la capture ne doit pas attendre la fin du segment précédent ; les segments validés doivent rester récupérables après une annulation ; le texte ne doit pas être raccordé par une espace systématique ; une édition riche doit savoir préserver la bonne occurrence d’un texte répété. Les API et la preuve de chaque correction sont détaillées ci-dessous. Les mesures d’endurance de l’audit sont locales : 1 001 sessions par plateforme (901 succès attendus et 100 pannes injectées), pas une mesure WAN ni d’autonomie.

| Quand | Travail à engager | Résultat attendu |
| --- | --- | --- |
| Lot correctif | TLS Android, dictée temps réel, horodatage de fin et acceptation d’édition compatible. | APK vérifié sur API 34, plage intégrale sur cinq minutes de fichier local, séparation résultat/diagnostic et tests d’édition hors ligne. |
| Avant d’intégrer le moteur | Qualifier les coupures, les éditions, l’endurance et le même parcours SDK sur les quatre systèmes. | L’app peut distinguer un texte final, une proposition à relire, une interruption et une indisponibilité réseau. |
| À partir des mesures | Optimiser le coût qui pèse sur l’utilisateur. Étendre les formats audio, les langues et les modes avancés selon les résultats. | Chaque extension documente les plateformes couvertes, son effet observable et son coût. |

## 1. Faire fonctionner la connexion Android avec les réglages du téléphone

- Le [transport Android](../crates/rambler-transport/ANDROID_TLS.md) initialise la politique de confiance de l’application via les API publiques Android. Il conserve le nom d’origine pour cette politique, la vérification DNS/IP du certificat et ALPN `h2`. Le SDK distingue confiance indisponible, refus du certificat, échec de résolution et expiration ; les catégories CLI existantes restent stables.
- **Vérifié sur le Xiaomi/API 34.** L’APK ordinaire a obtenu un PONG h2 depuis un serveur public et un serveur local avec CA limitée à l’application. Le même pair local au mauvais nom a été refusé. L’installation autorisée a utilisé le root ; le processus de test était non-root et a été désinstallé. [Résultats et limites](evidence/android-tls-host-app-20261118.md)
- **Vérifié aussi sur API 36/16 Kio.** L’APK x86_64 a chargé dans l’émulateur officiel à pages de 16 384 octets. Il a obtenu un PONG h2 avec la CA limitée à l’app et rejeté le même pair au mauvais nom. [Résultats et limites](evidence/android-tls-host-app-20261118.md)
- **Vérifié aussi par le second audit : politique d’application plus restrictive.** Sur API 36 x86_64 avec traduction ARM64 et des pages de 4 Kio, le SDK et le client Java ont tous deux refusé le certificat lorsque la politique de domaine n’acceptait que l’autorité de test. Ce contrôle est distinct de l’essai x86_64 à 16 Kio. [Mesures du second audit](evidence/audit-parite-integration-20261118.json)
- **Reste.** Exercer un serveur qui exige les SCT fournis uniquement par TLS et la combinaison ARM64 + pages 16 Kio. Le Xiaomi couvre ARM64 à 4 Kio ; le test chargé à 16 Kio couvre x86_64.

## 2. Donner à la dictée longue un contrat de durée utilisable

- Le [profil opt-in](research/interactive-pcm-dictation.md) segmente par défaut à 120 s. Il réserve un délai distinct pour la finalisation après EOF et nettoyage, sans changer les délais explicites ni la CLI classique.
- La [validation locale à cadence réelle](evidence/rust-interactive-wall-cadence-windows-20261118.md) couvre 31 s et 300 s : les plages d’échantillons concordent avec les durées déduites des granules Ogg. Un pair qui retient la réponse fait remonter `finalize_timeout`. La matrice accélérée couvre 29, 30, 31, 120 et 300 s avec un témoin audio de fin.
- Le [raccord partagé par le SDK et la CLI](research/sdk-integration-guide.md#opt-in-to-long-dictation) conserve les textes individuels, les espaces déjà fournis et les sauts de paragraphe. Il n’insère pas d’espace dans `L’équipe` ou `明天开会。`. Cette règle d’affichage ne reconstitue pas les mots coupés ni le contexte reconnu entre deux RPC.
- **Reste.** Vérifier les derniers mots et le délai perçu sur des voix naturelles, en condition de capture et avec un accès autorisé au service. Le pair local ne transcrit pas l’audio. [Options et fin de session](../crates/rambler-engine/src/engine.rs)

## 3. Survivre aux coupures et libérer les ressources

- **Vérifié localement.** Une coupure TCP contrôlée pendant l’audio, pendant l’attente du contexte ou après la demi-fermeture avant le statut gRPC ne valide aucun texte de cet appel. L’ancien moteur ne consomme pas la prochaine source et ne se reconnecte pas. Un nouveau moteur ouvert explicitement traite une nouvelle dictée isolée. [Régression TCP](../crates/rambler-engine/tests/app_tcp_recovery.rs)
- L’[API de livraison](research/recoverable-segment-delivery.md) transfère chaque résultat et sa plage d’échantillons après la fin vérifiée du RPC. Un arrêt normal traite la fin de capture ; un abandon ou une panne ne révoquent pas les résultats déjà remis. Ctrl-C conserve ces segments dans la sortie JSON privée, sans valider l’appel actif ni produire de texte global. Une tentative manuelle distincte exige un opt-in borné, un EOF normal et un nettoyage réussi.
- Enchaîner ensuite Wi-Fi coupé puis rétabli, sortie de veille et nouvelle dictée sur la cible applicative. Utiliser la reconnexion explicite pour la **prochaine** session.
- Distinguer une coupure réseau, un refus du service et une limitation temporaire de débit. Transmettre à l’hôte un délai de nouvelle tentative lorsque le service en fournit un. Vérifier qu’une série d’échecs ne lance aucune boucle de connexions en arrière-plan.
- Tester un producteur rapide, un consommateur lent, deux sessions simultanées et la fermeture de l’engine pendant une capture. Relever octets, échantillons, tâches, descripteurs et mémoire pendant une série de sessions sur un même processus. Vérifier la borne des files et le retour des ressources au niveau de repos après échauffement.
- Garder séparés les deux incidents historiques. Pour la session native incomplète à 282 s : relever le dernier échantillon, la fin d’envoi, la fin S3 et le statut gRPC lors d’une nouvelle observation autorisée. Côté Python, deux défauts distincts d’annulation ont été reproduits et corrigés, et 200 répétitions bornées du scénario audité ont terminé. La cause de l’attente historique n’a pas été établie. [Validation Python](evidence/python-transport-cancelled-close-windows-20261118.md)
- **Vérification.** Un succès exige l’envoi complet de la source, sa fermeture, la fin S3 et le statut gRPC OK. Dans la matrice de coupures, zéro texte partiel présenté comme final, zéro audio renvoyé automatiquement et zéro résultat attribué à la dictée suivante. La nouvelle session fonctionne après rétablissement du réseau. Le bilan d’endurance inclut les échecs et les ressources encore ouvertes. [Cycle de connexion](research/rust-connection-lifecycle.md) · [Incident natif](evidence/rust-incomplete-diagnostic-followup.json) · [Test Python](../tests/test_transport.py)

## 4. Mesurer et améliorer la qualité sur de vraies voix

- Commencer par un lot proposé de **40 énoncés spontanés**, majoritairement en français, complété par de l’anglais et des phrases bilingues. Varier locuteurs, accents, débit, bruit et silences. Inclure négations, dates, heures, noms propres, hésitations, autocorrections et répétitions voulues. Le corpus naturel déjà passé dans le moteur Rust représente 18,885 secondes. [Corpus existant](evidence/rust-native-natural-public.json)
- Le [plan d’annotation FR/EN](research/quality-annotation-plan.md) fixe les critères avant enregistrement : texte acceptable, informations à préserver, action attendue, scènes de bruit et de bilinguisme. Les exemples inventés ne sont pas des résultats de reconnaissance. Réserver des phrases et des locuteurs pour la validation ultérieure.
- Inclure deux cas où la même opération de nettoyage aurait des effets opposés. Dans « mardi à quinze heures… non, jeudi à quinze heures », le jour attendu est jeudi. Dans « c’est très, très important », les deux « très » font partie du message attendu.
- Mesurer le taux d’erreur sur les mots pour la dictée littérale, la ponctuation, les erreurs sur les nombres et négations, les répétitions perdues et la réussite des éditions. Tester le même énoncé avec et sans vocabulaire contextuel ou langue secondaire lorsque ce réglage est en question.
- **Vérification.** Publier pour chaque catégorie le nombre de cas, les réussites, les erreurs et un exemple explicatif. Retester en particulier « Merci de envoyer », le changement de langue au milieu d’une phrase et une séquence avec une répétition intentionnelle. Comparer le rendu Rust à PixelBoard 1.3.5 sur le Xiaomi avec texte initial, sélection et énoncé appariés. Consigner toute différence de prise de son. [Campagne réelle](evidence/rust-native-real-campaign-20261118.json) · [Observations PixelBoard](evidence/pixelboard-vs-native-real-20261118.json)

## 5. Appliquer une édition exactement à la bonne portée

- Vérifier le remplacement d’un seul mot, la reformulation d’une sélection, le raccourcissement, le changement de ton, l’ajout d’emoji et la correction après une première dictée. Placer du texte témoin de chaque côté. Tester aussi insertion, suppression, proposition vide et résultat contenant tout l’historique.
- Exercer les politiques existantes `Strict`, `AsrOnly` et `S3ClientCompatible`. Couvrir plusieurs hypothèses, plusieurs finales, un aperçu remplacé, un statut de sécurité inconnu et un résultat ambigu. Une consigne transcrite littéralement doit conserver cette provenance. Les commandes d’envoi et d’effacement demandent une décision explicite de l’hôte.
- Le [vérificateur d’éditions](research/edit-acceptance-contract.md) sépare maintenant les propositions compatibles, sur opt-in, des résultats stricts. Les tests hors ligne couvrent fenêtre complète, sélection UTF-16, graphèmes, doublons de finales, sécurité inconnue et conflit de document/révision/ancre. La validation appariée sur parole reste à faire.
- `derive_selection_splice` est une option distincte du diff textuel général. Avec une fenêtre et une révision toujours exactes et une réponse structurée qui prouve l’ancre, la suppression du premier `oui ` dans `oui oui` préserve le lien porté par le second. Sans cette preuve, le SDK refuse la proposition ciblée au lieu de deviner la plage. [Guide SDK](research/sdk-integration-guide.md) · [Test de styles](../crates/rambler-engine/tests/splice.rs)
- **Vérification.** Tous les cas déterministes préservent le texte témoin. Un contexte périmé donne un conflit et laisse le document intact. Pour les cas réels, rapporter séparément l’intention comprise, la justesse du texte et la portée de la proposition. [Contrat d’édition](research/edit-acceptance-contract.md) · [API du SDK](research/sdk-integration-guide.md) · [Tests de graphèmes](../crates/rambler-engine/tests/splice.rs)

## 6. Mesurer le délai que ressent la personne qui dicte

- L’hôte peut horodater son dernier échantillon sur l’horloge monotone et le comparer à [`SessionDiagnostics::validated_eof_at`](research/interactive-pcm-dictation.md#independent-sdk-timing-seam) pour une session qu’il possède. Les diagnostics distinguent l’envoi et la fin vérifiée ; le premier événement protocolaire n’est pas forcément du texte. Horodater séparément connexion prête, premier aperçu utilisable et rendu par l’app.
- Mesurer **dernier échantillon → résultat final validé** en connexion froide puis réutilisée, sur bureau et sur téléphone. Comparer réseau local contrôlé, Wi-Fi et réseau mobile. Donner l’effectif, les valeurs dispersées et les échecs avec la médiane et, lorsque la série le permet, le 95ᵉ percentile.
- Profiler CPU, mémoire, démarrage, volume transmis et chauffe pendant des sessions successives. Pour l’autonomie, répéter un scénario de même durée dans des conditions d’écran, de charge et de réseau comparables. L’encodage de cinq secondes de PCM à 16 kHz prenait déjà 22 à 48 ms sur le Xiaomi. Les médianes historiques approximatives de fin de dictée atteignaient 373 ms en français et 388 ms en anglais sur Windows.
- **Vérification.** Pour chaque coût dominant identifié, comparer une seule modification à la fois, sur le même audio. Les premiers candidats sont la réutilisation de connexion, la durée des pages Ogg et les copies sur le chemin audio. Retenir un changement quand le gain sur la session complète est reproductible et que qualité, consommation et délai de fin restent acceptables. [Mesures du Xiaomi](evidence/audit-moteur-20261118.json) · [Profil CPU Windows](evidence/rust-native-pcm48-session-cpu-windows-20261118.md) · [Méthode de mesure](performance.md)

## 7. Rendre l’entrée audio prévisible sur les quatre plateformes

- Le [tampon de capture optionnel](research/rust-pcm-capture.md) accepte des blocs PCM depuis un callback sans attendre le lecteur. Il réserve sa mémoire à l’avance ; une capacité dépassée est un échec explicite. Les tests retiennent artificiellement une finale pendant que des blocs représentant 60, 600 ou 900 ms sont offerts ; ils vérifient les paquets, les granules et le cas de débordement, ainsi que le 48 → 16 kHz continu. Ce ne sont pas des mesures de délai de callback matériel.
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
