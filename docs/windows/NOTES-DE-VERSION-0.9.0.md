# Sambler 0.9.0 pour Windows — version candidate V1

**Dictez au curseur dans l'application active, avec Rambler/Jetson de Google comme unique moteur.** Maintenez `Ctrl+Maj+F2`, parlez, relâchez : le texte s'insère une seule fois dans le champ où vous avez commencé. Si vous avez changé de champ entre-temps, il vous attend dans Sambler.

Deux téléchargements, mêmes fonctions :

| Fichier | Usage |
| --- | --- |
| `Sambler-0.9.0-windows-x64-setup.exe` | Installation pour votre compte Windows, sans droits administrateur. Menu Démarrer, désinstallation Windows. L'accès Rambler peut être mémorisé, chiffré pour votre compte. |
| `Sambler-0.9.0-windows-x64-portable.zip` | Extraire le dossier `Sambler` et lancer `Sambler.exe`. Les réglages restent dans `Data\` à côté du programme ; l'accès Rambler n'y est jamais enregistré. |

Prérequis : Windows 11 x64 avec Microsoft Edge WebView2 Runtime (inclus dans Windows 11), une connexion Internet et un accès Rambler autorisé (trois valeurs, ou les variables `RAMBLER_*`). Aucun autre composant n'est nécessaire.

## Premier lancement

1. Sambler explique ce que reçoit Google (votre voix pendant la dictée, la langue, les données d'accès) et ce qui reste sur le PC.
2. Choisissez et testez votre micro (le test ne transmet rien).
3. Renseignez l'accès Rambler s'il n'est pas déjà fourni par l'environnement.
4. Cliquez sur « Activer la dictée avec Rambler ». Rien n'est envoyé avant ce choix, réversible dans Réglages.
5. Essayez une phrase.

## Utilisation au quotidien

- **Maintenir pour parler** (défaut) : maintenez `Ctrl+Maj+F2`, attendez « Écoute » dans la capsule en bas de l'écran, parlez, relâchez.
- **Bascule (mains libres)** : Réglages → Bascule ; un appui pour commencer, un second pour terminer.
- **Annuler** : `Échap` pendant la dictée (y compris en maintenant le raccourci), ou « Annuler la dictée » dans l'icône près de l'horloge.
- **Texte qui n'a pas pu être inséré** : rubrique Brouillon, avec Copier et Effacer, gardé 10 minutes en mémoire seulement (1, 5 ou 10 min au choix).
- **Rendu** : Naturel (ponctuation, espaces et majuscules légèrement corrigés, sans changer vos mots) ou Fidèle (transcription telle quelle).
- **Mots et expressions** : remplacez une graphie (« projet Mélusine ») ou associez un déclencheur à un texte (signature sur plusieurs lignes).
- **Historique** : désactivé par défaut ; une fois activé, textes seulement, chiffrés pour votre compte, 7 jours par défaut (1, 7 ou 30).
- **Démarrer avec Windows** : option de l'édition installée.

Une phrase dictée n'appuie jamais sur Entrée et n'envoie jamais un message ou une requête.

## Où l'insertion est vérifiée

| Application | Méthode | Plusieurs lignes |
| --- | --- | --- |
| Bloc-notes, champs Windows classiques | Insertion en une opération, annulable par Ctrl+Z | Oui |
| Word (corps du document) | Insertion en une opération, curseur après le texte, un Ctrl+Z | Oui |
| Edge, Chrome, Brave (champs de page, barre d'adresse et de recherche) | Frappe Unicode cadencée (≈ 0,9 s pour 100 caractères) | Non : le texte reste dans le brouillon |
| Chat de VS Code | Frappe Unicode cadencée, sans envoi | Non : le texte reste dans le brouillon |
| Terminal, fenêtre administrateur, champ inconnu | Pas d'insertion : « Dicter dans Sambler » | — |
| Champ de mot de passe, bureau sécurisé | Le micro ne s'ouvre pas | — |

Dans une barre d'adresse ou de recherche, le texte sélectionné est remplacé, comme au clavier. Dans un document sur plusieurs lignes, une sélection empêche le démarrage.

## Confidentialité

Aucun audio n'est enregistré sur le disque. Le brouillon reste en mémoire et s'efface à l'expiration, au verrouillage de Windows ou à la fermeture de Sambler. Les journaux ne contiennent ni voix, ni texte, ni titres de fenêtres, ni données d'accès. « Copier » exclut le texte de l'historique du presse-papiers (Win+V) mais remplace le contenu actuel du presse-papiers.

## Mesures de cette version

- Recette réelle le 20 novembre 2026 sur Windows 11 (i5-12450H) : la personne a dicté avec sa propre voix dans le Bloc-notes, Word, Edge, Chrome, Brave et le chat de VS Code, en maintien et en bascule, avec annulation et changement de fenêtre. Les deux défauts trouvés (curseur Word, barre d'adresse) sont corrigés dans cette version.
- Campagne synthétique de 60 phrases (voix Windows, vrai service) : 59/60 insérées exactement, médiane 457 ms et p95 594 ms entre l'arrêt et le texte visible. Détails : [campagne synthétique](../evidence/windows-app-synthetic-campaign-20261120.md).
- Tests automatisés : 92 tests natifs et 149 tests d'interface, hors ligne.

## Limites connues

- Texte sur plusieurs lignes dans les navigateurs et dans VS Code : gardé dans le brouillon (pas de méthode sûre sans simuler Entrée). Piste V1.1 : extension compagnon pour VS Code.
- Outlook classique non qualifié (aucun profil sur le poste de test) ; nouvel Outlook non qualifié : utilisez le brouillon si l'insertion ne se fait pas.
- Le service peut refuser un passage (« filtre du service ») : rien n'est inséré, reformulez.
- Prises de 5 minutes, Narrateur, contraste élevé, verrouillage et débranchement du micro pendant une prise : à vérifier sur votre poste.
- Exécutables non signés : Windows SmartScreen peut afficher un avertissement. Vérifiez l'empreinte SHA-256 dans `SHA256SUMS.txt`.
- La disponibilité de la dictée dépend de votre accès Rambler ; sans accès valide, Sambler affiche « Dictée indisponible » et le reste de l'application fonctionne.

## Vérifier les fichiers

```powershell
Get-FileHash -Algorithm SHA256 .\Sambler-0.9.0-windows-x64-portable.zip
Get-FileHash -Algorithm SHA256 .\Sambler-0.9.0-windows-x64-setup.exe
```

Comparez avec les lignes de `SHA256SUMS.txt`. Code source sous licence MIT ; Opus et les autres composants tiers gardent leurs licences.
