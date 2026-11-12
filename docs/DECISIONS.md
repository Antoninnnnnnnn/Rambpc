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

## D006 — préserver la sémantique des éditions (2026-11-12)

**Décidé et validé sur trois réponses réelles inspectées.** Les résultats peuvent
contenir l'historique complet modifié ; même le champ nommé `transcript` n'est pas
nécessairement le verbatim de la commande. Exposer texte, intention et contexte
édité séparément. Ne pas convertir automatiquement une réponse en insertion,
ni appliquer des ranges Java UTF-16 à des indices de caractères Python. Le moteur
ne remplace pas silencieusement Rambler par une politique de rendu PixelBoard.

## D007 — fin explicite et erreurs tardives (2026-11-12)

**Décidé.** Un résultat partiel ne constitue pas une réussite. Attendre fin S3 et
statut terminal gRPC ; conserver la priorité d'une erreur gRPC tardive. Une fin
serveur avant épuisement de l'upload ne valide pas le contenu tronqué. Annuler et
fermer les ressources de la session, sans fermer un canal injecté appartenant à
un autre composant. Les producteurs doivent coopérer avec l'annulation asyncio.

## D008 — optimisations distinctes batch et direct (2026-11-12)

**Mesuré, portée limitée au corpus synthétique.** Blocs8192 au lieu de224 pour
un fichier disponible : 67→5 requêtes et environ6,5% de payload protobuf économisé,
aucun gain net de latence finale établi. Ne pas accumuler8192 octets en attendant
sur une source en direct : cela ajouterait du délai.

Pages Ogg20ms au lieu de1000ms, source FFmpeg `-re` identique : délai médian texte
après épuisement de source935→325ms sur trois essais par réglage. PCM redécodé
identique, mais volume Ogg accru. Ce gain de tamponnement d'entrée n'est pas un
gain de calcul serveur. Retenir un profil faible latence20ms pour l'encodeur natif
optionnel ; conserver les comparaisons, limites et contreparties dans la doc.

## D009 — encodeur PCM facultatif et natif (2026-11-12)

**Implémenté et validé.** PyAV/libopus embarqué en dépendance optionnelle,
importé à la demande, pour PCM16LE mono16k→Ogg/Opus24kbps. Pas de processus FFmpeg
à l'exécution du moteur. Python>=3.12 pour l'extra PyAV19 ; le chemin déjà encodé
reste Python>=3.11. Les tests contrôlent les derniers échantillons et la fin Ogg,
les bornes mémoire et la fermeture après annulation. Une CLI PCM réelle a obtenu
le texte attendu, VoiceEditingResult, fin S3 puis gRPC OK ; la preuve se trouve
dans `evidence/native-pcm-cli.json`. La mesure FFmpeg et cette validation native
restent distinctes ; elles ne sont pas interchangeables.
