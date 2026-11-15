# Qualification native macOS Intel

## Matrice native exécutée — 15 novembre 2026

Sur le commit `4264126852b2c03e25fc231c60f9582569a84ae9`, les deux jobs
GitHub `macos-26-intel` ont réussi avec Rust 1.88.0 et stable (1.100.0),
hôte observé `x86_64-apple-darwin`, image `macos-26`, CMake 4.4.2. Chacun
rapporte 400 tests workspace réussis, 0 échec, 2 ignorés ; réutilisation 10,
finalisation release exacte 1 et feature désactivée 35. Le smoke de racines
publiques passe par le lanceur direct ; le PCM release synthétique rapporte
1017 échantillons en entrée, 3051 décodés à 48 kHz, un EOS et zéro appel
service. [Détails et provenance](../evidence/rust-pcm-admission-expanded-ci-20261115.json).
Cela qualifie ces exécutions, pas une version minimale de macOS, un handshake
réel ou une parité service/Android.

## Matrice ajoutée — checkpoint historique en attente

La matrice Rust ajoute le runner standard explicite `macos-26-intel`, pour
Rust 1.88.0 et stable, sans retirer `macos-latest` (ARM64 lors des qualifications
précédentes). Aucun label `-large` payant n'est sélectionné. Les étapes existantes
restent identiques : tests synthétiques/loopback, configuration des racines OS
publiques sans connexion, réutilisation de session, CLI release et PCM release,
feature audio désactivée. Aucun identifiant ou appel Rambler n'est nécessaire.

À ce checkpoint, le changement de CI ne qualifiait pas encore Intel : il fallait
attendre les conclusions des
deux jobs et leurs journaux sur le commit exact. Les réussites ARM64 antérieures
ne les remplacent pas. Il ne fixe pas non plus la version minimale de macOS
supportée par un binaire distribué, ni la parité Android ou un handshake/service
réel sur macOS. Les résultats locaux et limites du lot PCM/admission précédent
restent dans [sa preuve](../evidence/rust-pcm-progress-admission.json).

## Sources des labels consultées le 15 novembre 2026

- [Runners standard GitHub pour les dépôts publics](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) : ligne macOS Intel, labels `macos-15-intel` et `macos-26-intel`.
- [Images disponibles et inventaires](https://github.com/actions/runner-images#available-images) : ligne macOS 26 x64 et inventaire associé. Aucun statut GA/Beta n'est déduit de l'absence de badge.
