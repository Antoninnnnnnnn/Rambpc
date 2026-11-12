# Rambpc — moteur expérimental Rambler

Objectif : un client réutilisable, sans environnement Android à l'exécution,
communiquant avec le véritable service de dictée Rambler utilisé par PixelBoard.
Pas d'interface graphique et pas de remplacement par Gemini public ou un autre
moteur de reconnaissance.

## État initial

Analyse et reconstruction en cours. **Aucune réponse Rambler obtenue par ce projet
n'est encore validée.** Les endpoints, champs et exigences d'authentification issus
d'une précédente analyse sont des pistes, pas un protocole fonctionnel établi.

## Navigation

- [État et journal de travail](docs/STATUS.md)
- [Décisions d'architecture et seuils de preuve](docs/DECISIONS.md)
- [Protocole](docs/research/protocol.md)
- [Authentification](docs/research/authentication.md)
- [Plan de validation](docs/validation-plan.md)
- [Outils disponibles](docs/research/toolchain.md)

## Exercer le premier lot (diagnostics uniquement)

```sh
python -m venv .venv
# Windows / Git Bash
.venv/Scripts/python -m pip install grpcio pytest
PYTHONPATH=src .venv/Scripts/python -m pytest tests/test_wire.py -q
# Réseau réel : une requête vide, aucun audio ni identifiant
.venv/Scripts/python tools/probe_empty_request.py --output local/probe.json
```

Sous Linux/macOS, remplacer `.venv/Scripts/python` par `.venv/bin/python`.
Le diagnostic réseau a reçu `PERMISSION_DENIED` ; ce n'est pas un test de dictée.
L'utilitaire wire ne contient pas encore le schéma complet Rambler.

Les instructions de construction et d'utilisation du moteur seront ajoutées avec
le parcours audio exécutable. Ne pas déposer de jetons, cookies, audio personnel,
APK ou sources décompilées dans git.
