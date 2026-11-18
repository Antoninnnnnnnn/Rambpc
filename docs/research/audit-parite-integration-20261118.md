# Audit du moteur Rambpc avant intégration

18 novembre 2026 · Windows, Android et comparaison avec la Dictée améliorée de Gboard.

**Je prendrais le cœur Rust de `e52e568` comme base de l’application.** La dictée courte, le traitement audio et la réutilisation des sessions sont suffisamment éprouvés pour ce choix. Les risques les plus concrets concernent la parole qui arrive pendant une coupure de segment, la récupération du texte déjà reconnu et la fidélité des éditions. Les mesures donnent peu de raisons de réoptimiser Opus en premier. [Mesures de cet audit][mesures]

Le dossier ouvert est sur `9a29ab8`. J’ai testé cette version, la révision intermédiaire `7ceb590`, puis la révision publiée `e52e568`, quatorze commits devant le dossier. Les essais ci-dessous portent sur cette dernière sauf mention contraire. Les dictées envoyées à Rambler viennent des campagnes enregistrées dans le projet. Mes nouveaux essais de dictée utilisaient des pairs locaux, avec des résultats attendus connus. Les nouvelles connexions à Google ont servi à tester le transport HTTPS public. [Révision auditée][revision] · [Relevé des essais][mesures]

## Quatre comportements à traiter avant l’intégration

| Priorité | Observation reproduite sur `e52e568` | Conséquence et test d’acceptation |
| --- | --- | --- |
| Haute pour le micro | Pendant le passage au segment suivant, le moteur attend la finale du précédent. Avec 90 blocs de 20 ms et une finale retardée artificiellement de 600 ms, la remise d’un bloc a attendu **581 ms sur le Xiaomi**, **599 ms sur Windows**. Les trois segments ont abouti. Dans le contrôle sans retard ajouté, aucune attente n’a dépassé 20 ms. | La source de test conservait ses échantillons. Un micro matériel continue de les produire pendant l’attente. Prévoir un tampon borné en amont du canal à une place et un signal de débordement. Vérifier la continuité par indices d’échantillons avec des retards variables, y compris un retard supérieur à la capacité du tampon. [Canal PCM][canal] · [Transition entre segments][transition] |
| Haute pour la récupération | Avec la CLI interactive, j’ai attendu que le deuxième appel commence après la finale du premier, puis envoyé Ctrl-C. Sortie **130**, avec `completed_segments: []`. Dans le contrôle où le serveur interrompt le deuxième appel, la CLI rend bien le premier segment et sa plage d’échantillons. | L’annulation explicite a une sémantique d’abandon. Pour une action « arrêter et garder ma dictée », terminer normalement la source. Pour récupérer le texte aussi après interruption, exposer ou conserver chaque résultat validé dès sa fin. Aujourd’hui, l’événement de fin de segment donne sa plage. Tester séparément arrêt normal, abandon et panne. [CLI][interruption] · [Événement de fin][evenement] |
| Moyenne pour la dictée multilingue | Le SDK et la CLI ajoutent systématiquement une espace entre segments. Avec deux finales synthétiques, la CLI rend `L’ équipe arrive.` Le SDK rend aussi `明天 开会。` et double l’espace si la seconde finale commence déjà par une espace. Les textes individuels sont intacts. | Mutualiser la règle du SDK et de la CLI, préserver les segments originaux et définir le rendu des apostrophes, de la ponctuation, des paragraphes et des langues sans espaces entre mots. Rejouer ces trois cas sur les deux entrées publiques. [Assemblage SDK][raccord] · [Assemblage CLI][raccord-cli] |
| Moyenne pour un éditeur riche | Dans `oui oui`, la sélection initiale est le premier `oui `, en UTF-16 `0..4`. Une réponse structurée le supprime et conserve le second `oui`. Le calcul par graphèmes propose d’effacer `3..7`. Les deux opérations donnent le texte brut attendu. | Les occurrences peuvent porter des liens ou des styles différents. Si l’édition porte sur la sélection et que le contexte structuré permet de la localiser sans ambiguïté, utiliser cette ancre. Sinon, présenter le remplacement pour revue. Vérifier les attributs de l’occurrence conservée en plus du texte final. [Calcul de la plage][splice] |

Le collecteur long conserve également les réponses de tous les segments. Les limites de collecte s’appliquent à chaque segment. Pour une utilisation continue sur mobile, une API livrant ensemble le résultat validé et sa plage permettrait à l’hôte de le sauvegarder puis de libérer sa mémoire. [Contrat du collecteur][transition]

## Fidélité à la Dictée améliorée

La référence produit est le comportement décrit par Gboard et le texte finalement obtenu dans l’éditeur. Le projet a été reconstruit depuis PixelBoard 1.3.4. Sa comparaison sur téléphone utilise PixelBoard 1.3.5 sur Xiaomi. Google documente entre autres le nettoyage de la parole, les modifications ciblées, le ton, les emoji, l’alternance de langues et une dictée de base hors connexion. Il indique aussi que le texte apparaît après la fin de la saisie dans ce mode. [Aide officielle Gboard][gboard] · [PixelBoard 1.3.5][pixelboard]

| Fonction | Niveau atteint |
| --- | --- |
| Dictée FR/EN, ponctuation et autocorrection orale | Le service a produit des phrases ponctuées et corrigé l’heure d’une réunion après une rectification orale. Le taux de conservation des noms, montants, négations et répétitions voulues en parole spontanée reste à mesurer. [Essais de dictée][dictee] |
| Modifier un mot, reformuler une sélection, raccourcir et ajouter un emoji | Des réponses réelles couvrent ces parcours. Pour certaines reformulations de sélection, le SDK exige la politique explicite `S3ClientCompatible`. Sur le Xiaomi, PixelBoard a réussi le remplacement bleu → vert et a inséré littéralement la demande de reformulation polie. Les voix utilisées par les deux clients étaient différentes. [Campagne native][campagne] · [Comparaison sur téléphone][comparaison] |
| Langues et changements au milieu d’une phrase | Une phrase synthétique par langue a produit une finale pour 13 langues additionnelles. Le hindi et le tamoul sont revenus en caractères latins. Un clip anglais puis français a conservé les deux langues. Ces essais laissent ouverte la reconnaissance d’un locuteur qui alterne spontanément dans une phrase. [Mesures multilingues][langues] |
| Composer, poursuivre ou restructurer un brouillon | Les essais ciblés ont transcrit ou ajouté la consigne. Le client sait décoder les intentions `COMPOSE`, `RAMBLE`, `RAMBLE_HISTORY`. Leur activation et le résultat attendu sur ce parcours restent à établir. [Essais de composition][composition] |
| Hors connexion | Gboard décrit une reconnaissance avec ponctuation et corrections de base. Rambpc encode sur l’appareil et délègue la reconnaissance au service. Ajouter cette capacité impliquerait un moteur de reconnaissance local. [Aide Gboard][gboard] · [Architecture Rust][architecture] |

Un cas mérite une place dans le corpus de qualité. Sur 122,6 s de répétitions d’une phrase, une seule occurrence est revenue. Sur 46,8 s avec douze phrases distinctes, les douze sont revenues. La cause de la différence reste ouverte. Les chiffres souvent cités de 3 erreurs sur 109 mots anglais et 3 sur 79 mots français concernent dix extraits de livres lus avec la référence **Python**. Une campagne native distincte donne six écarts sur un passage de 520 mots. Aucun de ces chiffres ne mesure la fidélité d’une réécriture sur une conversation spontanée. [Répétitions et passage long][fiabilite] · [Évaluation Python][wer]

## Vitesse et stabilité

Pour l’audio local, j’ai chronométré l’encodage complet de **10 s** de signal synthétique mono, présenté par blocs de 20 ms. Les nombres sont des médianes de neuf passages après chauffe. La colonne 48 kHz comprend la conversion vers 16 kHz. Les entrées étaient préparées en mémoire. [Données par passage][mesures]

| Appareil | PCM 16 kHz → Opus/Ogg | PCM 48 kHz → 16 kHz → Opus/Ogg |
| --- | ---: | ---: |
| Windows 11, Core i5-12450H | 36,1 ms | 52,6 ms |
| Xiaomi, Android 14, ARM64 | 44,3 ms | 53,0 ms |

- **Délai ressenti.** Une campagne réelle conservée de 100 dictées courtes sur deux phrases synthétiques donne **351 ms de médiane**, **447 ms au 95e percentile**, maximum **1,143 s**, entre fin audio estimée et fin de session. Le calcul soustrait la durée nominale du fichier cadencé. Pour la future app, horodater le dernier échantillon réellement acquis et le résultat utilisable. Le SDK expose déjà `validated_eof_at()` pour une session simple. [Campagne de fiabilité][fiabilite] · [Horodatage du SDK][horloge]
- **Endurance.** Sur Windows et sur le Xiaomi, j’ai enchaîné **1 001 sessions par plateforme** sur une connexion locale. Les 901 succès ont donné le texte attendu, les 100 pannes injectées ont donné l’erreur prévue et la session suivante a réussi. Sur le Xiaomi, la mémoire résidente est passée de 6 536 Kio au premier point à 6 588 Kio au centième et à chacun des quatre points suivants, jusqu’à la dernière session. La mise en veille, la bascule Wi-Fi/mobile, les sessions d’une journée et la consommation de batterie restent à caractériser. [Relevé d’endurance][mesures]
- **Durée de parole.** Le profil générique garde un délai de 30 s. Le profil interactif publié utilise des segments de 120 s, un budget total de 135 s par session et jusqu’à 5 s de finalisation dans ce budget. Mon test cadencé de 31 s a terminé en 30,993 s sur Windows avec ce profil. Le dépôt contient aussi un essai cadencé de 300 s complet sur serveur local. Deux anciens envois réels de 320 et 675 s en un seul flux ont échoué autour de 300 s. La préservation du sens de part et d’autre d’une coupure reste à évaluer sur parole naturelle. [Profil interactif][profil] · [Essai de cinq minutes][cinqminutes] · [Observations sur le service][fiabilite]

## Portabilité et intégration

**Android.** Dans une application de test lancée sous Android 16/API 36, l’ancienne révision échoue à joindre le serveur public Google alors que le client HTTPS natif reçoit HTTP 204. Sur `e52e568`, après `init_android_tls`, le moteur reçoit bien le PONG HTTP/2. J’ai ensuite limité le domaine à une autorité de test dans la seule application. Le moteur a renvoyé `CertificateRejected` et le client Java a également refusé le certificat. Cette mesure confirme que la politique de confiance du domaine est prise en compte. [Contrat Android][tls] · [Mesures de l’application][mesures]

Le Xiaomi a refusé l’installation ADB ordinaire de mon APK. Les essais applicatifs de cet audit ont donc tourné sur un émulateur Android 16 x86-64 exécutant la bibliothèque ARM64 par traduction. Les mesures audio et d’endurance viennent du Xiaomi physique. Le projet possède en plus une preuve de vérification TLS dans une application sur ce Xiaomi, avec le même code de transport. Les deux environnements de cet audit ont des pages mémoire de 4 Kio. La bibliothèque de test compilée avec le NDK r27 sans réglage d’alignement sort en 4 Kio. Le projet a contrôlé l’alignement de son APK témoin pour 16 Kio. Son chargement sur un système à pages de 16 Kio reste à tester. [Essai physique du projet][xiaomi] · [Pages mémoire Android][pagesandroid]

**Windows, Linux et macOS.** Sur Windows, `e52e568` passe **588 tests Rust** avec quatre contrôles optionnels ignorés par la commande générale. J’ai exécuté ces quatre contrôles séparément avec succès, ainsi que les six tests de documentation, le formatage et Clippy strict. La référence Python passe **931 tests**, avec quatre tests sautés. La CI du même commit a terminé ses dix jobs Rust, y compris Ubuntu, Windows et macOS Intel et Apple Silicon. Pour Linux et macOS, la preuve utilisée ici porte sur la construction et les essais locaux de la CI. [CI Rust][ci] · [CI Python][cipy] · [Résultats Windows][mesures]

**Entrées et données.** Le SDK accepte notamment du PCM16 mono à 16 ou 48 kHz, et un flux Ogg/Opus continu conforme au profil. Une source à 44,1 kHz ou stéréo nécessite une conversion en amont. L’app peut ainsi conserver le même cœur Rust avec un adaptateur de capture par plateforme. Le résultat structuré et la sortie JSON peuvent contenir la dictée et le contexte d’édition, y compris les segments antérieurs en cas d’échec. Pour diagnostiquer les incidents, journaliser les durées, catégories d’erreur, volumes et compteurs. La recherche OSV sur les 150 versions de registre du verrou Rust et les versions installées de `grpcio` et `av` a renvoyé zéro avis indexé. [API audio][architecture] · [Sortie CLI][interruption] · [Relevé des versions][mesures]

## Ordre de travail conseillé

1. **Protéger une dictée en cours.** Qualifier la remise des blocs pendant la finalisation, la politique de débordement et la livraison des segments déjà validés. Simuler ensuite la perte du réseau suivie de l’ouverture d’une nouvelle dictée.
2. **Fixer les règles de restitution du texte.** Rejouer les raccords multilingues sur le SDK et la CLI, puis le cas des deux mots identiques dans un éditeur avec deux styles. Conserver la distinction entre aperçu, texte validé et proposition d’édition.
3. **Mesurer la qualité perçue avant de retoucher le codec.** Le projet a déjà un plan d’annotation FR/EN. L’exécuter sur plusieurs voix spontanées, avec les mêmes sons et le même état d’éditeur sur le moteur et sur un Pixel équipé de Gboard officiel. Compter séparément erreurs de faits, répétitions supprimées, modifications hors sélection et délai après la fin de parole. [Plan d’annotation existant][qualite]

[mesures]: ../evidence/audit-parite-integration-20261118.json
[revision]: https://github.com/Antoninnnnnnnn/Rambpc/tree/e52e568ec8a602a224b249970e69f749ffa9ab18
[canal]: ../../crates/rambler-audio/src/input.rs
[transition]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-audio/src/long_dictation.rs#L174
[evenement]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-audio/src/long_dictation.rs#L146
[interruption]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-cli/src/main.rs#L483
[raccord]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-audio/src/long_dictation.rs#L114
[raccord-cli]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-cli/src/main.rs#L499
[splice]: ../../crates/rambler-engine/src/splice.rs
[gboard]: https://support.google.com/gboard/answer/17468539?hl=fr
[pixelboard]: https://github.com/Akshayykadam/PixelBoard/releases/tag/1.3.5
[dictee]: ../../README.md
[campagne]: ../evidence/rust-native-real-campaign-20261118.json
[comparaison]: ../evidence/pixelboard-vs-native-real-20261118.json
[langues]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/evidence/rust-native-multilingual-real-20261118.json
[composition]: composition.md
[architecture]: ../../crates/rambler-audio/BUILDING.md
[fiabilite]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/evidence/rust-native-real-reliability-20261118.json
[wer]: ../evidence/natural-expansion-quality.json
[horloge]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-engine/src/engine.rs#L247
[profil]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/research/interactive-pcm-dictation.md
[cinqminutes]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/evidence/rust-interactive-wall-cadence-windows-20261118.md
[tls]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/crates/rambler-transport/ANDROID_TLS.md
[xiaomi]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/evidence/android-tls-host-app-20261118.md
[pagesandroid]: https://developer.android.com/guide/practices/page-sizes
[ci]: https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/44391305891
[cipy]: https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/44391305956
[qualite]: https://github.com/Antoninnnnnnnn/Rambpc/blob/e52e568ec8a602a224b249970e69f749ffa9ab18/docs/research/quality-annotation-plan.md
