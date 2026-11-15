# Qualification native macOS Intel

## Matrice ajoutée — exécution en attente

La matrice Rust ajoute le runner standard explicite `macos-26-intel`, pour
Rust 1.88.0 et stable, sans retirer `macos-latest` (ARM64 lors des qualifications
précédentes). Aucun label `-large` payant n'est sélectionné. Les étapes existantes
restent identiques : tests synthétiques/loopback, configuration des racines OS
publiques sans connexion, réutilisation de session, CLI release et PCM release,
feature audio désactivée. Aucun identifiant ou appel Rambler n'est nécessaire.

Ce changement de CI ne qualifie pas encore Intel : attendre les conclusions des
deux jobs et leurs journaux sur le commit exact. Les réussites ARM64 antérieures
ne les remplacent pas. Il ne fixe pas non plus la version minimale de macOS
supportée par un binaire distribué, ni la parité Android ou un handshake/service
réel sur macOS. Les résultats locaux et limites du lot PCM/admission précédent
restent dans [sa preuve](../evidence/rust-pcm-progress-admission.json).

## Sources des labels consultées le 15 novembre 2026

- [Runners standard GitHub pour les dépôts publics](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) : ligne macOS Intel, labels `macos-15-intel` et `macos-26-intel`.
- [Images disponibles et inventaires](https://github.com/actions/runner-images#available-images) : ligne macOS 26 x64 et inventaire associé. Aucun statut GA/Beta n'est déduit de l'absence de badge.
