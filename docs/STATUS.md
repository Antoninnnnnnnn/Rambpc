# État et journal

## Objectif et règles de preuve

Moteur natif réutilisable pour le véritable Rambler ; aucune dépendance Android
à l'exécution ; authentification et cycle de vie font partie du périmètre.
La réussite nécessite de vraies réponses audio et non seulement une connexion
TLS/gRPC. Chaque résultat doit être qualifié : **observé**, **inféré**, **non testé**.

## 2026-11-12 — démarrage

- Dépôt local vide, aucun historique de code à reprendre.
- Branche de travail : `feat/rambler-engine`.
- Trois missions d'analyse parallèles :
  - Sol / effort élevé : APK PixelBoard 1.3.4, schémas et séquence S3/Rambler.
  - Kimi Pro / effort élevé : authentification, Zwieback, dépendances d'accès.
  - Luna / effort moyen : inventaire outils et protocole de validation/mesures.
- Le coordinateur conserve l'intégration, les décisions et les validations croisées.
- Aucun accès utilisateur, jeton ou audio fourni. Aucun appel Rambler validé.

## 2026-11-12 — premier lot

- Hôte joignable depuis Windows, TLS 1.3 et ALPN h2 validés (un échantillon).
- Appel S3Bidi anonyme avec protobuf vide : `PERMISSION_DENIED`, sans payload.
  Ce test ne prouve pas l'impossibilité d'une requête conforme ; aucun audio envoyé.
- Codec wire sans schéma : 25 tests locaux passants. Ce n'est pas un codec Rambler
  complet. Lecture PCM et transport streaming en cours d'intégration.
- Première mise en dépôt des notes et observations expurgées, sur demande utilisateur.
- Affectation transport reprise par Sol après arrêt d'Astra par l'utilisateur.
  Ne plus lancer d'agents Astra ; privilégier Sol ou plus léger.
- Téléphone Xiaomi proposé pour analyse de développement si nécessaire ; aucune
  manipulation effectuée, téléphone encore utilisé. Il ne sera pas requis comme
  relais à l'exécution.

## Jalons

1. Retrouver et documenter les éléments réseau et protobuf avec leurs sources.
2. Déterminer une requête minimale et les conditions d'accès réelles.
3. Tester à faible volume le service réel, sans contourner ses contrôles d'accès.
4. Construire l'interface du moteur et les tests de cycle de vie.
5. Valider audio, résultats, contexte/corrections selon capacités observées.
6. Mesurer puis optimiser les parties démontrées, sans perte de fidélité.
7. Documenter les instructions reproductibles, limites, preuves ; committer code
   et documentation ensemble. Pas de publication distante sans demande explicite.

## Reprise

Lire ce fichier, `DECISIONS.md` et les notes `research/`. Les archives et sorties
intermédiaires restent dans le scratchpad de session, hors git. Les notes doivent
indiquer leurs références, hashes, commandes et inconnues pour permettre une
nouvelle collecte si ces artefacts disparaissent.
