# Résultats partiels et WAV 48 kHz — validation du lot interactif

19 novembre 2026 · base auditée `debd06e3fb960eaf2254aa8b330b8918df9dff94` · source testée `a1017177ffca1602dbcc6532bfcb8322a7fa544c`.

## Ce qui change

- Un mélange de segments utilisables et de segments à revoir ou sans résultat, dans une capture dont les RPC sont terminés normalement, produit `type: "ambiguous"`, `outcome: "partial"`, `text: null` et le code de sortie `1`. L’app récupère les parties utilisables et les plages à revoir dans `completed_segments`, sans concaténer à travers les trous. Un segment sans résultat ne prouve pas que l’entrée était silencieuse. Un refus de sécurité, une erreur RPC/fermeture ou une annulation conservent leur catégorie et leur priorité ; le contenu refusé demeure masqué. [Contrat](../research/recoverable-segment-delivery.md)
- `dictate-wav` accepte maintenant le PCM16LE mono 16 ou 48 kHz, y compris en mode temps réel et en mode interactif. À 48 kHz, un seul rééchantillonneur continu précède l’encodeur ou le découpage ; les plages des segments sont toujours en échantillons à **16 kHz**. L’API `WavAudio::open` reste limitée à 16 kHz ; `open_16_or_48k` est un opt-in. Les WAV stéréo, flottants, à 44,1 kHz ou déjà tronqués sont refusés avant la connexion. [Formats et profil](../research/interactive-pcm-dictation.md)

## Témoins vérifiés

Les [tests de WAV](../../crates/rambler-cli/tests/wav.rs) comparent les paquets Opus et granules Ogg à des références encodées séparément et détectent un redémarrage du filtre au passage de segment. Les [tests de résultats partiels](../../crates/rambler-cli/tests/interactive_partial.rs) contrôlent l’ordre et les plages, y compris du texte reconnu avant **et** après une zone non utilisable, les politiques de rendu et les refus de sécurité. La supervision de l’annulation est exercée par injection du résultat de signal, pas par une nouvelle interruption du processus par l’OS.

Le [témoin combiné dans le vrai binaire](../../crates/rambler-cli/tests/interactive_48k_partial.rs) présente 67 200 échantillons WAV à 48 kHz. Il obtient deux plages contiguës couvrant exactement 22 400 échantillons après conversion, sur une seule connexion, avec `partial` et code 1. La variante `--diagnostics-only` garde les mêmes statuts et plages sans publier le texte du segment utilisable.

- Rust stable, Windows/MSVC : **573 tests workspace réussis, 2 ignorés**, **28 CLI sans audio réussis**, formatage et Clippy strict workspace au vert.
- Rust 1.88.0, Windows/MSVC : 5 tests ciblés WAV, résultat partiel et témoin combiné réussis.

La suite locale a utilisé des pairs loopback possédés et des entrées synthétiques, sans service réel, appareil ni micro. Ces résultats ne qualifient ni la transcription des répétitions ni la fidélité en bruit fort. Le [suivi séparé de la fermeture Python](python-transport-cancelled-close-windows-20261118.md#independent-audit-follow-up-19-november) n’a pas reproduit le blocage intermittent et ne lui attribue pas de cause.
