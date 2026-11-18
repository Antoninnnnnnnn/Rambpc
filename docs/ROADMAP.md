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
relie en plus le SDK Rust au vérificateur hors ligne. Le [lot PCM 48 kHz et
édition ciblée](evidence/rust-sdk-input-edits-windows-20261118.json) passe
244 tests et doctests hors ligne, dont les suites CLI du flux PCM, avec un
test PyAV optionnel ignoré. Aucun appel au vrai service.

| Thème | État | Critère de livraison |
|---|---|---|
| **SDK et intégration** | Le [guide](research/sdk-integration-guide.md) couvre les aperçus, l’entrée PCM bornée, `OggAudio::open` sans codec natif et la préparation d’une édition UTF-16. Le [nouvel adaptateur](../crates/rambler-audio/BUILDING.md) accepte le PCM mono à 48 kHz en option. | Les parcours 16 kHz et Ogg existants restent disponibles. L’app fournit l’audio et décide si elle applique une édition, avec contrôles de révision et d’ancre. |
| **Robustesse du client** | [Fin de flux et frontières wire](research/native-robustness-coverage.md) : les cas synthétiques vérifient les erreurs, le nettoyage et l’isolation des sessions. Le test d’erreur Ogg tardive attend maintenant la réception d’un paquet audio par le pair. | Maintenir l’ordre et les limites de réception, sans faux succès ni rejeu. Vérifier les corrections dans la CI groupée. |
| **Livraison et fidélité** | Le [kit natif](research/native-qualification-kit.md) teste un flux complet sur Ubuntu stable. Le [contrat d’édition hors ligne](research/edit-acceptance-contract.md) compare un résultat de proposition strict à un attendu préétabli ; les rapports historiques ne contiennent pas ces attendus. | Qualifier séparément la provenance et le producteur des observations avant de conclure sur un corpus réel. Ni le WER, ni une session réussie ne prouvent un effet d’édition conforme. |
| **Accès et parité sur le service** | Le 18 novembre, le [moteur natif sur le vrai service](evidence/pixelboard-vs-native-real-20261118.json) a reçu une réécriture polie (intention 3, champ 7) que PixelBoard ne faisait pas sur le Xiaomi de test ; la CLI l’accepte avec `--result-policy s3-client-compatible`. Les succès natifs historiques ne qualifient pas une méthode durable d’accès. L’[audit PixelBoard 1.3.5](research/pixelboard-135-client-result-audit.md) situe ses corrections dans le client Android ; un [cas observable dans Gboard](research/advanced-voice-observable-reference.md) est prêt mais non exécuté. | Vérifier l’effet d’édition via une référence appariée pour les [modes avancés](research/rust-feature-parity.md#acceptation-actuelle-et-reste-à-faire). Traiter séparément le périmètre autorisé et le cycle de vie de l’accès. Ne pas inventer un mode ou une activation. |
| **Optimisation et mesure** | La [réception gRPC](research/raw-h2-receive-cost.md) réduit les allocations ; la comparaison appariée ne montre pas de gain stable sur une session locale complète. Le [profil PCM](research/native-audio-hotpath.md) ne justifie pas de changement de production. Sur une session PCM 48 kHz complète, l’[encodage Opus/Ogg domine le CPU local](evidence/rust-native-pcm48-session-cpu-windows-20261118.md) (environ 56 %) ; un CRC Ogg par table n’a pas montré de gain assez stable. Le [filtre 48 → 16 kHz](evidence/rust-pcm48-fir-symmetry-windows-20261118.json) exploite la symétrie de ses coefficients : 33 à 51 % de CPU en moins localement, sortie identique octet pour octet. | Préserver les mêmes entrées/résultats, publier les limites, puis mesurer la qualité et la latence réelles seulement avec une référence qualifiée. |

## Cadence

Un propriétaire par thème actif, un worktree et un cache de compilation propres.
Le propriétaire implémente, compile, corrige et valide son lot jusqu’à un
livrable utilisable. Le coordinateur règle les interfaces et les dépendances,
intègre les lots qui passent ensemble et observe la CI en arrière-plan.

On communique quand un résultat, un choix ou un blocage change le plan, pas
après chaque compilation. On ne maintient pas un agent occupé si sa prochaine
étape ne change aucune décision. Un worktree isole le code, pas les secrets :
les travaux délégués restent sans données privées ni appels au fournisseur.
