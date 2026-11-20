# Sambler 0.10.0 pour Windows

**Dictez au curseur dans l'application active, avec Rambler/Jetson de Google comme unique moteur.** Maintenez `Ctrl+Maj+F2`, parlez, relâchez : le texte arrive d'un bloc là où vous écrivez.

| Fichier | Usage |
| --- | --- |
| `Sambler-0.10.0-windows-x64-setup.exe` | Installation pour votre compte Windows, sans droits administrateur. L'accès Rambler peut être mémorisé, chiffré pour votre compte. |
| `Sambler-0.10.0-windows-x64-portable.zip` | Extraire le dossier `Sambler` et lancer `Sambler.exe`. Les réglages restent dans `Data\` ; l'accès Rambler n'y est jamais enregistré. |

Prérequis : Windows 11 x64 avec WebView2 (inclus dans Windows 11), une connexion Internet et un accès Rambler autorisé (les trois valeurs `RAMBLER_*`).

## Nouveautés depuis 0.9.0

- **Nouvelle interface**, identique sur Windows, macOS et Linux : fenêtre sans cadre système, barre latérale (Accueil, Dictionnaire, Raccourcis vocaux, Style, Historique, Réglages), statistiques de mots et vitesse, liste des **Récents**.
- **Barre flottante** en bas de l'écran pendant la dictée : ondes de la voix, minuteur, puis le résultat en quelques mots. Elle ne prend jamais le focus.
- **Démarrage instantané** : « Écoute » dès l'appui. Le micro s'ouvre aussitôt ; l'audio reste sur le PC jusqu'à la vérification du champ et n'est jamais envoyé si c'est un champ de mot de passe.
- **Insertion instantanée partout** (Réglages → Insertion, « Instantanée ») : le texte est collé d'un bloc, sur plusieurs lignes si le champ le permet, puis votre presse-papiers est **remis à l'identique** (texte, image, fichiers ; une copie faite entre-temps est conservée ; exclu de l'historique Win+V). L'option « Frappe caractère par caractère » reste disponible sans presse-papiers.
- **Où insérer le texte** (Réglages → Destination) :
  - « Dans le champ de départ » (défaut) : vous pouvez changer de fenêtre en parlant ; si vous êtes revenu dans votre champ à la fin, le texte s'y insère, à l'endroit du curseur. Sinon il vous attend dans Récents.
  - « Là où se trouve le curseur à la fin » : le texte va dans le champ actif quand vous terminez.
  - Jamais dans un champ de mot de passe.
- **Récents** : les 5 derniers textes restent 10 minutes en mémoire avec Copier et Effacer. Un texte en attente ne bloque plus la dictée suivante.
- Un champ non reconnu (terminal, fenêtre administrateur…) n'empêche plus de dicter : le texte part dans Récents.

## Où l'insertion est vérifiée

| Application | Méthode | Plusieurs lignes | Mesures sur le poste de test |
| --- | --- | --- | --- |
| Bloc-notes, champs Windows classiques | Insertion directe, un Ctrl+Z | Oui | Exact à chaque essai |
| Word | Insertion directe, curseur après le texte, un Ctrl+Z | Oui | Exact à chaque essai |
| Chrome, Edge (champs, zones de texte, éditeurs riches, barre d'adresse) | Collage, presse-papiers restauré | Oui (sauf champ d'une ligne) | 120/120 chacun, texte visible en ≈ 40 ms |
| Chat de VS Code | Collage, sans envoi | Oui | 40/40 |
| Brave | Collage | Oui (sauf champ d'une ligne) | 100/100 une fois actif ; voir la limite ci-dessous |
| Champ de mot de passe | Jamais | — | Le micro se coupe |

## Limites connues

- **Brave juste après y être passé** : Brave peut mettre plusieurs secondes à rendre ses champs visibles à Windows. La dictée qui suit immédiatement un passage sur Brave peut alors finir dans Récents au lieu d'être collée (13/20 réussies sur des prises courtes). Contournement vérifié sur un petit échantillon (5/5) : lancer Brave avec l'option `--force-renderer-accessibility` (clic droit sur le raccourci de Brave → Propriétés → Cible, ajouter l'option après les guillemets).
- Outlook classique et le nouvel Outlook ne sont pas qualifiés sur le poste de test ; si rien n'est collé, le texte est dans Récents.
- Le service peut refuser un passage (« filtre du service ») : rien n'est inséré.
- Prises de 5 minutes, Narrateur, contraste élevé, verrouillage et débranchement du micro pendant une prise : à vérifier.
- Exécutables non signés : SmartScreen peut afficher un avertissement (« Informations complémentaires » → « Exécuter quand même »). Vérifiez `SHA256SUMS.txt`.

## Confidentialité

Aucun audio sur le disque. Récents en mémoire seulement, effacés au verrouillage de Windows et à la fermeture. Historique désactivé par défaut. Lors d'un collage, le texte passe brièvement par le presse-papiers (exclu de l'historique Win+V et de la synchronisation) puis votre contenu précédent est remis ; une application qui surveille le presse-papiers pourrait voir passer le texte pendant ce bref instant. Choisissez « Frappe caractère par caractère » pour l'éviter.

## Mesures de cette version

- Vraie voix (session de l'utilisateur sur 0.9.0) et voix de synthèse : texte exact inséré une fois, médiane ≈ 0,45 s entre l'arrêt et le texte visible.
- Recette réelle de 0.10.0 avec le vrai service : maintien, bascule, changement de fenêtre, annulation, anglais ; barre flottante visible 25/25 sans prise de focus.
- Tests automatisés : 106 tests natifs, 211 tests d'interface.

Code source sous licence MIT ; Opus, Inter et les autres composants tiers gardent leurs licences.
