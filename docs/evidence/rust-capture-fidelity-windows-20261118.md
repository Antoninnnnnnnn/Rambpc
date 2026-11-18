# Validation Windows des corrections de capture et d’intégration

18 novembre 2026 · Source testée : `72d79fc793411fa65d2703d61e0e65cef31a7420` · Windows/MSVC. Les tests utilisent des échantillons, textes et réponses de test inventés, avec des pairs HTTP/2 contrôlés sur loopback. Aucun appareil ni microphone, aucun appel fournisseur et aucune métadonnée d’accès.

| Cas | Observation vérifiée |
| --- | --- |
| [Tampon de capture](../../crates/rambler-cli/tests/capture_rollover.rs) | Le pair retient la finale pendant l’offre de blocs représentant 60, 600 ou 900 ms. Les paquets et granules correspondent au PCM attendu. Le chemin continu 48 → 16 kHz est également couvert. Le débordement au-delà de 54 places et l’abandon sont des échecs distincts, pas des EOF normaux : les segments antérieurs restent disponibles. |
| [Parcours capture et livraison ensemble](../../crates/rambler-cli/tests/capture_rollover_delivery.rs) | 90 blocs de 20 ms, deux transitions avec finale retenue, trois RPC sur un même TCP. Trois flux Ogg indépendants avec numéros de série distincts, paquets Opus et granules identiques à l’encodage séparé des trois tranches attendues. Le sink reçoit les plages `[0,9600)`, `[9600,19200)`, `[19200,28800)` dans l’ordre, après validation seulement. |
| [Interruption et fin normale](../../crates/rambler-cli/src/tests.rs) | La branche Ctrl-C est exercée via le même mécanisme de supervision que le binaire. Pendant le deuxième RPC, un signal injecté rend la première plage en JSON, aucun texte global ni texte non validé ou bloqué, et le code 130. Les [tests du SDK](../../crates/rambler-cli/tests/long_pcm_dictation.rs) couvrent aussi la fin normale avec un reliquat de cinq échantillons et une panne gRPC tardive, sans rejeu. |
| [Raccord CLI et SDK](../../crates/rambler-cli/tests/interactive_join_fidelity.rs) | Les deux entrées publiques conservent les finales individuelles et produisent le même rendu pour `L’` + `équipe arrive.`, `明天` + `开会。`, un saut de paragraphe ou un espace déjà fourni. Des tests d’unité couvrent aussi les guillemets, les accents et les graphèmes Unicode. Sur 4 096 raccords d’indicateurs régionaux, le témoin instrumenté examine 8 193 scalaires nouvellement ajoutés et n’interroge jamais le curseur sur le préfixe entier. |
| [Édition avec styles](../../crates/rambler-engine/tests/splice.rs) | Dans `oui oui`, le remplacement structuré du premier `oui ` produit la plage UTF-16 `0..4` et conserve le lien du deuxième `oui`. Une ancre non démontrée, une révision périmée ou une frontière de graphème invalide ne donnent pas de proposition ciblée. |
| [Trois vraies coupures TCP locales](../../crates/rambler-engine/tests/app_tcp_recovery.rs) | Pendant l’attente de PCM, l’attente de contexte ou l’attente du statut gRPC après demi-fermeture : aucun texte incomplet validé, aucun nouveau PCM lu par le moteur déconnecté, aucune reconnexion implicite. Après ouverture explicite, la dictée suivante traite uniquement sa nouvelle source. |

## Vérifications reproductibles

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-features --locked --offline
cargo test -p rambler-cli --no-default-features --locked --offline
```

| Toolchain | Workspace avec audio (doctests inclus) | CLI sans audio |
| --- | --- | --- |
| Stable | 559 réussis, 2 ignorés | 24 réussis |
| 1.88.0 | 559 réussis, 2 ignorés | 24 réussis |

Formatage, Clippy strict et contrôle du graphe de dépendances audio réussis. Les tests avec les deux toolchains ont tourné sous des Jobs Windows possédés et vides après chaque commande. Un passage Clippy préliminaire a signalé un `chunks_exact` à taille constante dans le nouveau témoin ; le remplacement par `as_chunks` vérifie aussi que la tranche n’a aucun reliquat.

Les intervalles de production simulés ne mesurent pas le matériel, le délai réel d’un callback ou la consommation sur batterie. La suite ne démontre ni l’exactitude d’une transcription de part et d’autre d’une coupure, ni la latence sur le WAN, ni la parité Gboard. Cette preuve Windows ne remplace pas la CI multiplateforme. Voir le [second audit](../research/audit-parite-integration-20261118.md), le [guide SDK](../research/sdk-integration-guide.md) et les [limites du tampon](../research/rust-pcm-capture.md).
