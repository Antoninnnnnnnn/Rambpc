# Application desktop multiplateforme — recommandation d’architecture

Date : 13 novembre 2026.

**Statut : recommandation, pas décision de migration ni engagement de réalisation.**

## 1. Objectif

Faire évoluer Sambler vers une application de dictée pour **Windows d’abord, puis macOS et Linux**, avec une majorité de code partagée.

Les usages envisagés sont :

- application résidente avec indicateur d’état ;
- capture microphone déclenchée depuis l’interface ou un raccourci global ;
- reconnaissance et édition via le moteur Rambler ;
- affichage, copie et, lorsque cela est possible, insertion du résultat dans une autre application ;
- installation et utilisation sans environnement Python à configurer.

Ces usages sont des objectifs de conception, pas des capacités déjà implémentées ou validées dans le dépôt.

## 2. Recommandation

**Rust pour le moteur, Tauri 2 pour l’application desktop, TypeScript + React pour l’interface.**

| Couche | Choix recommandé | Responsabilité |
|---|---|---|
| Moteur indépendant | Rust | Protocole, sessions, résultats, annulation et limites de ressources |
| Concurrence et réseau | Tokio + tonic, à valider sur le protocole actuel | Transport gRPC bidirectionnel et orchestration |
| Audio natif | CPAL, conversion explicite, libopus et muxage Ogg | Microphone, adaptation du format et encodage progressif |
| Enveloppe desktop | Tauri 2 | Fenêtres, commandes natives, distribution et intégration de l’interface |
| Interface | TypeScript + React, avec Vite | Paramètres, état de dictée, erreurs et historique facultatif |
| Adaptateurs système | Rust et API propres à chaque OS | Raccourcis, permissions, application active et insertion |
| Recherche et conformité | Python existant | Fixtures, exploration du service et comparaison des implémentations |

Les bibliothèques précises restent à confirmer par un prototype. Le protocole reconstruit peut nécessiter un codec ou un traitement des champs inconnus adapté : ne pas supposer qu’une génération protobuf standard préservera automatiquement tous les comportements utiles.

React est un choix pratique d’écosystème, pas une contrainte fondamentale. Svelte serait également envisageable. Le choix structurant est la séparation entre moteur natif, adaptateurs système et interface.

## 3. Pourquoi cette stack

### Un moteur natif adapté à une application résidente

Rust permet de contrôler les ressources, les buffers et la durée de vie des tâches. Il convient à un pipeline qui coordonne capture, encodage, émission réseau, réception et annulation.

Les bénéfices attendus concernent surtout la distribution, la consommation de ressources et la robustesse du moteur. Ils devront être mesurés ; le langage ne garantit pas à lui seul une application légère ou correcte.

### Une interface productive sans tout développer en Rust

TypeScript et React facilitent la construction et l’évolution des écrans. Tauri relie cette interface au moteur natif et s’appuie sur les WebViews système.

Cela évite d’embarquer systématiquement une distribution complète de Chromium, mais impose des tests entre WebViews et plateformes. La taille finale dépendra aussi des bibliothèques natives et du packaging.

### Un produit sans Python installé par la personne utilisatrice

L’application finale peut fonctionner sans runtime Python embarqué. Python resterait utile dans le dépôt pour la recherche et les outils de validation.

Ce choix n’implique pas nécessairement un unique exécutable autonome : codecs, composants système et formats d’installation doivent être traités par plateforme.

## 4. Ce que Rust ne résout pas

### Pas de gain de reconnaissance garanti

Sambler est un client d’un service distant. Rust n’accélérera pas le modèle exécuté sur le serveur.

Le moteur Python utilise déjà des composants natifs pour le transport gRPC et l’encodage audio. Les mesures existantes ont surtout identifié un levier de tamponnement du pipeline audio, pas un goulot d’étranglement démontré dans le langage Python.

Voir [les mesures et leurs limites](../performance.md).

### Pas de correction automatique du protocole

Le portage devra préserver notamment :

- les messages et champs utiles du protocole ;
- l’envoi du contexte au bon moment ;
- le flush des derniers échantillons ;
- la distinction entre arrêt normal et annulation ;
- les erreurs tardives après réception de résultats ;
- la validation de la fin applicative et de la fin gRPC ;
- l’interprétation prudente des résultats d’édition.

La sécurité mémoire et le typage ne garantissent pas ces propriétés fonctionnelles.

### Pas de garantie de pérennité du service

Le service Rambler interne reste susceptible d’évoluer ou de devenir inaccessible. Prévoir une interface de fournisseur remplaçable dès le départ, sans construire prématurément plusieurs intégrations.

Le choix technique n’autorise pas la redistribution de métadonnées d’accès. Les conditions d’accès et de distribution doivent être vérifiées séparément.

## 5. Architecture proposée

### Cœur portable, indépendant de Tauri

- Modèle de session et événements.
- Protocole et transport.
- Encodage et traitement des résultats.
- Délais, annulation et erreurs.
- Interface de fournisseur de reconnaissance.

Le cœur doit pouvoir être exercé depuis une CLI de diagnostic et des tests, sans interface graphique.

### Adaptateurs système

- Acquisition microphone et changements de périphérique.
- Permissions.
- Raccourcis globaux.
- Identification de l’application cible.
- Lecture éventuelle de sélection et de contexte.
- Insertion ou proposition de remplacement.

Ces capacités doivent être exposées explicitement : une fonction indisponible sur un système ne doit pas être présentée comme universelle.

### Interface Tauri

L’interface affiche l’état et transmet les commandes. Le microphone, l’encodage, le réseau et l’insertion restent côté natif.

Les échanges avec l’interface doivent être limités aux commandes et données nécessaires. Les credentials ne doivent pas être exposés au frontend ; le stockage natif sécurisé ne rend toutefois pas un secret partagé inviolable sur la machine cliente.

## 6. Contraintes propres aux systèmes

| Système | Points à anticiper |
|---|---|
| Windows | Raccourcis, zone de notification, capture, applications élevées, restrictions de simulation clavier et accessibilité |
| macOS | Permissions microphone/accessibilité, signature, notarisation et architectures matérielles ciblées |
| Linux | Dépendances de distribution, différences entre bureaux, X11 et Wayland, portails et capacités du compositeur |

**La fenêtre est multiplateforme ; l’interaction avec les autres applications ne l’est pas automatiquement.**

Sous Wayland notamment, ne pas promettre une insertion ou une lecture de contexte universelle. Prévoir une copie dans le presse-papiers ou une copie manuelle lorsque l’intégration automatique est indisponible.

Chaque système nécessite ses propres builds et tests. Prévoir signature et vérification des mises à jour ; la notarisation est spécifique à macOS.

## 7. Insertion et édition : deux niveaux distincts

### Insertion simple

Insérer une nouvelle dictée au curseur est une première cible raisonnable. Les mécanismes possibles, comme le presse-papiers ou la simulation clavier, ont leurs propres limites et effets de bord.

Une solution utilisant temporairement le presse-papiers doit respecter son contenu autant que possible et gérer les modifications concurrentes, plutôt que restaurer aveuglément une ancienne valeur.

### Modification d’un texte existant

Un résultat Rambler peut contenir une fenêtre de contexte entière. Il ne doit pas être ajouté systématiquement au texte existant.

Préserver les contrôles de contexte et de révision déjà prévus dans le moteur. Une vérification avant écriture ne rend pas nécessairement atomique une modification dans une application externe.

**En cas d’ambiguïté ou de contexte modifié : prévisualisation ou copie, pas remplacement automatique.**

Voir [l’API de résultat et ses limites](../refinement.md).

## 8. Alternatives considérées

| Alternative | Quand elle serait préférable | Compromis pour Sambler |
|---|---|---|
| C# + Avalonia | Forte maîtrise de .NET ou priorité Windows durable | Très crédible ; les intégrations macOS/Linux restent spécifiques |
| Flutter + Dart | Interface homogène et éventuelle extension mobile | Plugins et code natif toujours nécessaires pour les fonctions système avancées |
| Electron + TypeScript | Priorité à la vitesse de développement d’une interface riche | Chromium embarqué, distribution généralement plus volumineuse et intégrations natives à maintenir |
| Python + PySide6 | Ajouter rapidement une interface au moteur existant | Empaquetage du runtime et des dépendances natives à maintenir |
| C++ + Qt | Forte expérience Qt et besoin de son écosystème mature | Complexité de maintenance et conditions de licence/distribution à examiner |
| Interface entièrement Rust | Besoin particulier ou expérience de l’équipe | Pas nécessairement le meilleur compromis pour une interface soignée et accessible |

Il n’existe pas de meilleur langage universel. À compétences comparables, Rust/Tauri est la recommandation pour l’utilitaire de dictée multiplateforme envisagé. Une forte expérience préalable dans une autre stack peut modifier ce choix.

## 9. Transition progressive

### Étape 1 — Établir la référence de conformité

Conserver Python et ses tests. Définir les comportements à préserver, notamment protocole, Unicode, positions UTF-16, résultats ambigus, erreurs tardives et annulation.

Les fixtures et tests loopback doivent permettre de vérifier le portage sans dépendre constamment du service réel.

### Étape 2 — Porter une tranche minimale en Rust

Valider d’abord : **fichier audio → service → résultat final confirmé**, depuis une CLI.

Comparer les comportements à Python avant d’ajouter le microphone. Mesurer démarrage, RAM, CPU et latence avec les mêmes entrées, sans attribuer au langage des variations réseau.

### Étape 3 — Valider le pipeline microphone Windows

Construire la chaîne complète : **raccourci → microphone → adaptation audio → Opus/Ogg → service → résultat confirmé → insertion contrôlée**.

Ne pas supposer que tous les périphériques produisent directement du PCM mono 16 kHz. Tester les formats disponibles, le rééchantillonnage et le changement de périphérique.

Inclure les scénarios de débranchement, veille, perte réseau, arrêt immédiat et changement de fenêtre active. Ne pas rejouer automatiquement un audio après erreur.

### Étape 4 — Construire l’interface et la distribution

Ajouter paramètres, choix du microphone, indicateur d’état, raccourcis, erreurs compréhensibles et installateur. Prévoir les permissions, la signature et les mises à jour.

Ne conserver ni audio ni historique textuel par défaut sans besoin explicite ; traiter ces données comme sensibles.

### Étape 5 — Compléter macOS et Linux

Prévoir les interfaces système dès le début et compiler/tester tôt sur les trois OS. Finaliser ensuite les adaptateurs macOS, puis Linux avec une matrice de capacités explicite X11/Wayland.

Ne pas attendre la fin du développement Windows pour vérifier la portabilité des dépendances.

## 10. Avis indépendant et conclusion

Un agent Opus a été sollicité avec une demande neutre de recommandation, sans lui transmettre le choix proposé initialement. Après examen du dépôt et consultation de documentations officielles, il a également recommandé **Rust + Tauri 2 + TypeScript/React**.

Les avis convergent sur le moteur indépendant, le maintien de Python comme référence, les adaptateurs propres à chaque OS et l’absence de promesse de gain de latence par simple changement de langage.

Cette convergence constitue un avis architectural, pas un benchmark comparatif ni une validation de faisabilité de toutes les intégrations système.

**Conclusion : privilégier cette stack pour le produit envisagé, mais valider une petite tranche fonctionnelle avant d’engager une réécriture complète.** Le budget, les compétences disponibles et les fonctionnalités exactes de la première version restent à préciser.

## Références

- [README du projet](../../README.md)
- [Mesures de performance](../performance.md)
- [Résultats et propositions d’édition](../refinement.md)
- [Documentation Tauri 2](https://v2.tauri.app/start/)
- [Restrictions de SendInput sous Windows](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [Portail RemoteDesktop sous Linux](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
