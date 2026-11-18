# Feuille de route du moteur natif Rambler

**But :** un client Rust du vrai service Rambler, autonome à l’exécution (sans
relais Android), fidèle et rapide. Pas de service de remplacement. Microphone,
interface et insertion dans l’OS ne font pas partie du moteur dans cette phase.

`main` reste le point stable. Le [journal des validations](STATUS.md) conserve
les résultats et incidents ; cette page sert à décider du prochain travail.

**Validation du lot PCM/réception :** formatage, Clippy strict et tests du
workspace hors ligne sur Windows ([461 réussites, 2 ignorées, 1 test TLS
filtré](evidence/rust-streamed-sdk-wave-windows-20261118.json)). Le jalon
précédent `4cf9c4d` a passé les 14 jobs de CI Rust/Python, dont le kit natif
sur Ubuntu stable. Aucun appel au vrai service dans ces validations.

| Thème | État | Critère de livraison |
|---|---|---|
| **SDK et intégration** | Le [guide](research/sdk-integration-guide.md) couvre l’observation des aperçus et le démarrage PCM borné, sans seconde collecte. | Le fournisseur d’audio reste maître de la capture ; l’API préserve les erreurs, la fin normale et l’annulation. Prochaine étape : rendre la source Ogg/Opus réutilisable hors CLI sans codec natif. |
| **Robustesse du client** | [Fin de flux et frontières wire](research/native-robustness-coverage.md) : les cas synthétiques vérifient les erreurs, le nettoyage et l’isolation des sessions. | Maintenir l’ordre et les limites de réception, sans faux succès ni rejeu. N’ajouter des cas que lorsqu’ils établissent un invariant distinct. |
| **Livraison et fidélité** | Le [kit natif](research/native-qualification-kit.md) teste un flux complet sur Ubuntu stable. Les rapports de corpus existants ne contiennent pas d’attendus d’édition par cas ; un WER ou une proposition valide ne suffit pas à juger la fidélité. | Définir des attentes indépendantes et appariées avant d’évaluer le texte, l’intention et l’effet d’édition ; les valider hors ligne sur des données inventées. |
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
