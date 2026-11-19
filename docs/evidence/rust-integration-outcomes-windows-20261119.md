# Corrections de l’audit indépendant d’intégration

19 novembre 2026 · révision auditée : `9120decab7df7b2ecb277445ef24ff8fb66aa1aa`.
Révision Rust testée : `fb29d02afa4ddf370e82e765abc30a41ac81c1a2`. Tous les essais correctifs ci-dessous utilisent des pairs loopback possédés, des réponses synthétiques et des éditeurs simulés. Aucun appel fournisseur, APK, appareil ou microphone.

| Cas | Contrat vérifié |
| --- | --- |
| Dictée longue : texte puis segment vide | Une finale avec forme d’édition non prise en charge et sans hypothèse ASR ne devient pas une dictée réussie. La CLI signale l’absence de résultat, garde exactement les deux plages `[0,96000)` et `[96000,116000)` et conserve « Première phrase. » comme résultat du premier segment. Pas de texte global utilisable ni de code 0. Le résumé strict SDK ne saute pas non plus un segment sans texte. |
| Aucune finale, ambiguïté, sécurité, suppression | Zéro finale n’établit pas que l’audio était silencieux. Plusieurs finales restent à revoir. Un statut de sécurité bloqué ou inconnu reste distinct et ne donne pas de texte. Une suppression demande un remplacement explicite et étayé de la fenêtre capturée ; une chaîne ASR vide n’est jamais une suppression. Un résultat interprété n’autorise toujours aucun effet sur le document. |
| Édition riche | `minimal_app` vérifie l’identité du document, la plage exacte, la révision et la sélection ou le caret avant de dériver une proposition. Dans `Paris Paris`, supprimer le premier `Paris ` laisse intact le lien porté par le deuxième. Si la portée d’une correction de sélection n’est pas prouvée, le modèle indique une revue sans plage à appliquer. |
| Télémétrie CLI | Les marqueurs de transcription et de contexte apparaissent dans la sortie privée attendue, mais sont absents sous `--diagnostics-only` sur le vrai binaire, y compris en cas d’ambiguïté. Une liste d’autorisation retient les catégories, durées, compteurs et plages ; les tests la confrontent aussi à la sortie imbriquée des segments. Le mode ne modifie ni le code de sortie ni les requêtes. |
| Référence Python | `secure=False` exige une autorité TCP numérique de boucle locale, avec adresse directe et proxy désactivé. Une configuration modifiée après construction est contrôlée de nouveau avant la création du canal. Le parcours TLS reste inchangé. Aucun test n’ouvre de connexion vers une destination distante. |

## Validation locale

- Python 3.14, hors ligne : **947 réussis, 5 sautés, 981 sous-tests réussis**. Sous-ensemble transport/moteur : **117 réussis**. Le contrôle d’admission prouve le rejet avant la création du canal et la suite exerce aussi un vrai pair IPv4 local.
- Rust stable : **569 réussis, 2 ignorés** dans le workspace, **28 réussis** côté CLI sans audio ; **6 réussis** pour `minimal_app`, formatage et Clippy strict.
- Rust 1.88 : **569 réussis, 2 ignorés** dans le workspace, **28 réussis** côté CLI sans audio.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-features --locked --offline
cargo test -p rambler-cli --no-default-features --locked --offline
cargo test -p rambler-cli --features audio --example minimal_app --locked --offline
python -m pytest -q tests
```

## Limites de mise en production

L’app garde le brouillon et les segments déjà validés ; une panne n’autorise pas à rejouer automatiquement la phrase en cours. La récupération d’audio avant EOF demande une rétention indépendante, explicite et bornée côté hôte. Un texte utilisable ne préjuge pas d’une décision d’édition ; la vérification du document et l’application annulable doivent se faire dans la même transaction côté hôte.

L’audit indépendant a exécuté `rambpc` sur le vrai service sans métadonnées d’authentification d’application : TLS et PING/PONG h2 réussis, dictée refusée par `PERMISSION_DENIED`, aucune nouvelle transcription. Ce constat n’établit ni l’expiration d’anciens paramètres ni un accès autorisé pour une nouvelle application. La reconnaissance réelle, le délai en conditions d’usage, le micro et la comparaison avec Gboard ne sont pas déduits de ces essais correctifs.
