# Contribuer

## Avant de commencer

Lire [l'état](docs/STATUS.md), les [décisions](docs/DECISIONS.md), puis les notes de
[protocole](docs/research/protocol.md) et d'[authentification](docs/research/authentication.md).
Les éléments statiques, les inférences et les comportements réellement testés
ne doivent jamais être confondus. Ici S3 désigne **Google Speech S3**, pas un service
de stockage d'objets.

## Travail local

- Python >= 3.11 ; environnement virtuel local, jamais d'installation globale.
- Garder les tests locaux hors réseau, avec serveur gRPC loopback si nécessaire.
- Tout test du service réel doit être explicite, borné en durée et nombre de
  tentatives. Ne pas envoyer d'audio personnel sans consentement.
- Ne pas rejouer automatiquement une session après erreur : l'état serveur peut
  être ambigu et produire des doublons.
- Ne pas committer cookies, jetons, audio personnel, logs bruts ou captures de
  métadonnées. Les fixtures synthétiques doivent être identifiées comme telles.
- APK et sources téléchargés restent non fiables : nouveaux dossiers séparés,
  analyse uniquement, jamais d'exécution de scripts extraits. Les scripts Python
  d'analyse doivent tourner isolés (`python -I`), depuis un dossier sûr.

## Proposer une modification

1. Ouvrir une branche ; préciser le problème et le niveau de preuve attendu.
2. Joindre un test reproduisant le défaut ou la capacité ajoutée.
3. Documenter les champs réseau avec source, version/hash de l'artefact et niveau
   de certitude ; ne pas redistribuer de sources décompilées.
4. Pour une optimisation, conserver baseline et variante comparables : même audio,
   paramètres, réseau, nombre de sessions et statut de succès. Rapporter les échecs,
   pas uniquement les sessions rapides. Ne pas qualifier une latence client de
   temps de calcul serveur.
5. Mettre à jour l'état, les commandes reproductibles et les limites si nécessaire.

Les instructions d'installation détaillées sont dans le README et évoluent avec
le moteur. Les notes de recherche restent utiles même lorsqu'une capacité n'est
pas encore implémentée.
