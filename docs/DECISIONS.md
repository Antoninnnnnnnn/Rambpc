# Décisions

## D001 — preuves avant optimisation (2026-11-12)

**Décidé.** Séparer quatre niveaux : transport accessible, requête S3 comprise,
reconnaissance vocale S3 exploitable, comportement Rambler identifié. Aucun niveau
ne prouve automatiquement le suivant. Pas de chiffre de performance distante
sans requête réussie et conditions enregistrées.

## D002 — confinement des artefacts tiers (2026-11-12)

**Décidé.** APK et sources tiers téléchargés dans de nouveaux dossiers du
scratchpad ; aucun code extrait exécuté. Analyses Python isolées (`-I`). Seuls
notes, schémas reconstruits nécessaires à l'interopérabilité, scripts propres et
résultats expurgés entrent dans git. Ne pas redistribuer de code décompilé.

## D003 — authentification explicite (2026-11-12)

**Décidé.** Ne pas prétendre à une authentification autonome si elle repose sur
un jeton copié. Documenter émission, expiration, renouvellement et dépendances
observées. Ne pas emprunter des identifiants tiers ni contourner un refus d'accès.

## D004 — langage et architecture (2026-11-12)

**Décidé pour le premier client de preuve.** Python >= 3.11 et `grpc.aio`
(gRPC C-core natif), sans Android. Les numéros de champs restent à reconstruire ;
la possibilité de modifier et inspecter rapidement les messages prime à ce stade.
Rust est disponible et reste une possibilité si un coût local mesuré le justifie,
mais aucun langage n'est déclaré optimal sans comparaison. Le transport de bytes
est séparé du codec, de l'authentification et de l'API audio/résultats. Une connexion
peut être réutilisée ; les streams et leur état ne sont pas partagés. Pas de retry
qui rejouerait automatiquement une dictée.

## D005 — sécurité et confidentialité des diagnostics (2026-11-12)

**Décidé.** Ne pas journaliser de secrets ni de contenu utilisateur par défaut.
Les tests de service réel sont explicites, limités, et distincts des tests locaux.
Aucun rejeu automatique d'audio après erreur ambiguë : éviter les doublons et
la confusion entre sessions.
