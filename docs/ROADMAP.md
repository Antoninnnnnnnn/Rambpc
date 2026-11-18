# Feuille de route du moteur natif Rambler

**But :** un client Rust du vrai service Rambler, autonome à l’exécution (sans
relais Android), fidèle et rapide. Pas de service de remplacement. Microphone,
interface et insertion dans l’OS ne font pas partie du moteur dans cette phase.

`main` reste le point stable. Le [journal des validations](STATUS.md) conserve
les résultats et incidents ; cette page sert à décider du prochain travail.

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
relie en plus le SDK Rust au vérificateur hors ligne. Aucun appel au vrai service.

| Thème | État | Critère de livraison |
|---|---|---|
| **SDK et intégration** | Le [guide](research/sdk-integration-guide.md) couvre les aperçus, le démarrage PCM borné et `OggAudio::open` pour lire un fichier préencodé sans codec natif. | Le même lecteur sert au SDK et à la CLI, avec préflight borné, pages intactes, erreurs et annulation préservées. La capture reste à l’application. |
| **Robustesse du client** | [Fin de flux et frontières wire](research/native-robustness-coverage.md) : les cas synthétiques vérifient les erreurs, le nettoyage et l’isolation des sessions. Le test d’erreur Ogg tardive attend maintenant la réception d’un paquet audio par le pair. | Maintenir l’ordre et les limites de réception, sans faux succès ni rejeu. Vérifier les corrections dans la CI groupée. |
| **Livraison et fidélité** | Le [kit natif](research/native-qualification-kit.md) teste un flux complet sur Ubuntu stable. Le [contrat d’édition hors ligne](research/edit-acceptance-contract.md) compare un résultat de proposition strict à un attendu préétabli ; les rapports historiques ne contiennent pas ces attendus. | Qualifier séparément la provenance et le producteur des observations avant de conclure sur un corpus réel. Ni le WER, ni une session réussie ne prouvent un effet d’édition conforme. |
| **Accès et parité sur le service** | En attente d’éléments externes. Les succès natifs historiques ne qualifient pas une méthode durable d’accès. | Périmètre autorisé, émission/renouvellement/révocation et échange de référence apparié pour les [modes avancés](research/rust-feature-parity.md#acceptation-actuelle-et-reste-à-faire). Ne pas inventer un mode ou une activation pour lever le blocage. |
| **Optimisation et mesure** | La [réception gRPC](research/raw-h2-receive-cost.md) réduit les allocations dans le cas local mesuré. Le [profil PCM](research/native-audio-hotpath.md) ne justifie pas de changement de production. | Préserver les mêmes entrées/résultats, publier les limites, puis mesurer la qualité et la latence réelles seulement avec une référence qualifiée. |

## Cadence

Un propriétaire par thème actif, un worktree et un cache de compilation propres.
Le propriétaire implémente, compile, corrige et valide son lot jusqu’à un
livrable utilisable. Le coordinateur règle les interfaces et les dépendances,
intègre les lots qui passent ensemble et observe la CI en arrière-plan.

On communique quand un résultat, un choix ou un blocage change le plan, pas
après chaque compilation. On ne maintient pas un agent occupé si sa prochaine
étape ne change aucune décision. Un worktree isole le code, pas les secrets :
les travaux délégués restent sans données privées ni appels au fournisseur.
