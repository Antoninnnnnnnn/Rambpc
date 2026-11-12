# Plan de validation du moteur Rambler natif

## But et frontières

Le dépôt étant encore vide, ce document définit une validation à implémenter, pas des résultats. Aucun chiffre de performance n'est mesuré ni présumé ici. Tester le client et le moteur hors Android, puis séparer explicitement le coût du calcul local de celui du transport et d'un service distant éventuel. Distinguer la reconnaissance conventionnelle Google Speech S3 de la dictée agentique Rambler.

## Preuve d'identité fonctionnelle

Avant toute mesure, consigner le contrat observé et les éléments reproductibles : protocole et schéma, types de messages, ordre des événements, états de session, format audio, sémantique d'annulation, comportement de fin d'audio, et provenance des réponses. Classer les preuves :

- **Transport gRPC** : service/méthode identifiés, enveloppes protobuf décodables et échanges conformes au contrat observé. Cela prouve le transport, pas Rambler.
- **Google Speech S3 conventionnel** : identifier le protocole/service Google Speech S3 et le contrat observé; valider la reconnaissance et son résultat conventionnel sur le corpus. Cette preuve caractérise cette voie de reconnaissance, pas la dictée agentique Rambler.
- **Rambler** : comportement de dictée agentique propre au produit démontré par une trace/session valide et reproductible, avec transcription exploitable, corrections ou contexte observables selon contrat, et fermeture/finalisation cohérentes. Conserver preuves (requêtes/réponses assainies, horodatages, version/hash artefacts, configuration). Une simple signature de transport ou un endpoint ne suffit pas.

Ne pas inventer une API ni inférer une fonctionnalité de la seule présence d'une dépendance. Distinguer explicitement hypothèse, observation et preuve.

## Corpus minimal

Créer un corpus local versionné avec consentement/licence et manifest (ID opaque, durée, codec, fréquence, canaux, langue, hash audio et référence texte, annotations); aucune donnée personnelle inutile. Au minimum :

1. Trois segments de parole contrôlés : phrase nette et courte, parole continue plus longue avec ponctuation, et bruit/silence représentatif. Annoter référence texte et bornes temporelles si les sorties sont segmentées.
2. Une séquence de correction : énoncé initial puis correction explicite; vérifier si le système révise la transcription ou expose la correction conformément au contrat, sans postuler son mécanisme.
3. Une séquence contextuelle : terme/noms propres donnés dans le contexte pris en charge, puis contrôle identique sans contexte; comparer erreurs et fidélité au contexte.

Conserver une fixture identique dans toutes les variantes. Mesurer WER (normalisation publiée avant test), erreurs de ponctuation/casse séparément si pertinent; rapporter aussi texte final exact et statut de complétude. Ne pas mélanger qualité et latence.

## Protocole reproductible

Chaque résultat indique commit, version moteur/client, hash corpus, commande/config, OS/CPU/RAM, mode réseau, codecs et unités. Utiliser horloge monotone; journaliser timestamps audio capturé/lu, envoi/réception, premier résultat exploitable et final terminal. Signaler warm-up et nombre de répétitions; fournir toutes observations et médiane, p95, min/max, pas seulement une moyenne. Tester les cas dans un ordre randomisé ou alterné, avec mêmes entrées. Séparer processus froid (nouveau processus et cache déclaré) et chaud (processus déjà initialisé); définir si connexion neuve/réutilisée. Pas de seuil chiffré inventé : fixer SLO après mesures de référence et besoin produit.

### Mesures à rapporter

- **Moteur local** : durée calcul depuis entrée audio disponible jusqu'au premier résultat et au résultat final; temps CPU (ms), pic mémoire (MiB), ressources persistantes; préciser si décodage/rééchantillonnage inclus.
- **Service/réseau** : temps de connexion/handshake, octets aller-retour, RTT de référence, durée d'envoi, attente serveur visible, erreurs/retries. Sans instrumentation serveur, nommer la durée « latence client observée », jamais « temps moteur ».
- **Expérience complète** : depuis audio disponible (ou son temps d'arrivée simulé) jusqu'au premier texte exploitable, puis jusqu'au résultat final après signal de fin; unités ms et ratio temps réel (durée traitement / durée audio). Le timestamp final est distinct du premier résultat partiel.

### Variantes comparables

Exécuter, avec corpus et environnement fixes : (A) moteur directement en mémoire/fichier si disponible, (B) transport loopback avec service simulé séparant coût client/protocole, (C) service réel autorisé avec conditions réseau consignées. Comparer au minimum les tailles de chunks supportées (petit, médian, grand — valeurs exactes fixées et documentées avant exécution), lecture au débit temps réel vs envoi le plus rapide autorisé, connexion neuve vs connexion réutilisée. Contrôler débit, ordre et fin de flux. Ne conclure à une optimisation qu'après comparaison end-to-end; une amélioration du débit seul peut dégrader délai du premier texte ou mémoire.

## Tests de comportement et critères

- **Résultat** : sortie structurée parseable, texte final non vide pour parole du corpus, état explicitement final après end-of-audio; mesurer la qualité vs référence. Un partiel n'est jamais déclaré final.
- **Isolation** : envoyer deux sessions avec audio/contextes distincts en alternance et concurrence; aucune fuite de texte, contexte ou terminaison; réexécuter dans ordre inversé. Utiliser IDs distincts et vérifier association de toutes les réponses.
- **Annulation** : annuler pendant émission et pendant traitement; vérifier qu'aucun résultat ultérieur n'est attribué à la session annulée, que les ressources finissent par se libérer et que l'état est terminal explicite. Documenter délai d'observation sans seuil préétabli.
- **Backpressure** : producteur plus rapide que consommateur puis consommateur ralenti; vérifier limites de file/mémoire documentées, absence de perte silencieuse/réordonnancement, propagation de pression ou erreur explicite, reprise après drainage.
- **Défaillances** : fin prématurée, audio invalide, fermeture de connexion, timeouts et répétition contrôlée; pas de résultat final mensonger ni de duplication masquée.

Acceptation minimale : chacune de ces propriétés possède un test reproductible et un résultat explicite pass/fail; protocoles observés conformes au contrat documenté; toutes latences reportées avec périmètre et unités. Les seuils de performance sont approuvés après baseline, et non choisis pour rendre une implémentation passante.

## Décision d'architecture

Recommandation de départ : Rust est une hypothèse d'architecture pour le cœur natif, avec API interne indépendante du transport, adaptateur de protocole séparé et instrumentation par session. Python est également viable pour une probe rapide et l'orchestration des mesures; retenir le langage après avoir confirmé le contrat et les contraintes de déploiement. La disponibilité locale de Rust ne démontre pas qu'il est supérieur. Aucun crate gRPC/protobuf n'a été confirmé; choisir un client uniquement après identification du protocole et de la source de schéma. Java/JDK est disponible pour l'analyse éventuelle d'artefacts existants, sans preuve d'un besoin de moteur Java. Les frontières entre modules servent à séparer responsabilités et mesures, sans présumer ni proposer un moteur alternatif. Aucun code moteur/test n'est créé par ce plan.
