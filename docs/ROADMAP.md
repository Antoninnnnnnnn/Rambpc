# Feuille de route du moteur natif Rambler

**But :** un client Rust du vrai service Rambler, autonome à l’exécution (sans
relais Android), fidèle et rapide. Pas de service de remplacement. Microphone,
interface et insertion dans l’OS ne font pas partie du moteur dans cette phase.

`main` reste le point stable. Le [journal des validations](STATUS.md) conserve
les résultats et incidents ; cette page sert à décider du prochain travail.

**Dernière intégration locale :** les trois livrables ci-dessous passent le
formatage et la validation offline du workspace sur Windows : 453 réussites
comptées par cible, 2 ignorées et 1 smoke TLS filtré. Le nouveau contrôle Ubuntu
reste à confirmer dans la CI groupée ; aucun appel au vrai service.

| Thème | État | Critère de livraison |
|---|---|---|
| **SDK et intégration** | Livré et testé dans son worktree : [observateur SDK et guide](research/sdk-integration-guide.md), de l’audio fourni à une proposition d’édition vérifiable. | Guide utilisable sans les notes de rétro-ingénierie ; exemples et tests ciblés exécutés par le propriétaire. Limites, refus et cycle de vie préservés. |
| **Robustesse du client** | Livré et testé dans son worktree : [fin de flux et frontières wire](research/native-robustness-coverage.md), avec correction d’un faux succès gRPC. | Cas locaux reproductibles, erreurs et nettoyage explicitement contrôlés, pas de faux succès ni de rejeu. Corriger seulement les écarts établis et éviter les doublons. |
| **Livraison et fidélité** | Livré dans son worktree : contrôle du [kit natif](research/native-qualification-kit.md) sur un seul job Ubuntu stable. Analyseur testé localement ; exécution Linux à confirmer dans la CI groupée. | Le kit exerce un flux complet et le rapport est vérifiable ; il n’alourdit pas les huit configurations natives avec un nouveau build de profilage. |
| **Accès et parité sur le service** | En attente d’éléments externes. Les succès natifs historiques ne qualifient pas une méthode durable d’accès. | Périmètre autorisé, émission/renouvellement/révocation et échange de référence apparié pour les [modes avancés](research/rust-feature-parity.md#acceptation-actuelle-et-reste-à-faire). Ne pas inventer un mode ou une activation pour lever le blocage. |
| **Optimisation et mesure** | Le kit local fournit un point de comparaison ; pas de nouveau micro-banc systématique. | Partir d’un coût observé, comparer les mêmes entrées et résultats, puis mesurer qualité et latence représentatives lorsque la référence réelle est qualifiée. Publier les limites, pas un classement universel. |

## Cadence

Un propriétaire par thème actif, un worktree et un cache de compilation propres.
Le propriétaire implémente, compile, corrige et valide son lot jusqu’à un
livrable utilisable. Le coordinateur règle les interfaces et les dépendances,
intègre les lots qui passent ensemble et observe la CI en arrière-plan.

On communique quand un résultat, un choix ou un blocage change le plan, pas
après chaque compilation. On ne maintient pas un agent occupé si sa prochaine
étape ne change aucune décision. Un worktree isole le code, pas les secrets :
les travaux délégués restent sans données privées ni appels au fournisseur.
