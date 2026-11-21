# Sambler 0.12.1 pour Windows

**Dictez au curseur dans l'application active, avec Rambler/Jetson de Google comme unique moteur.** Maintenez `Ctrl+Maj+F2`, parlez, relâchez : le texte arrive d'un bloc là où vous écrivez.

| Fichier | Usage |
| --- | --- |
| `Sambler-0.12.1-windows-x64-setup.exe` | Installation pour votre compte Windows, sans droits administrateur. |
| `Sambler-0.12.1-windows-x64-portable.zip` | Extraire le dossier `Sambler` et lancer `Sambler.exe`. Les réglages restent dans `Data\`. |

Prérequis : Windows 11 x64 avec WebView2 (inclus dans Windows 11) et une connexion Internet. **Rien à saisir au premier lancement** : un accès Rambler est intégré à l'application.

## Nouveautés de 0.12.1

- **Mise à jour sans question** : l'installateur remplace une version précédente sur place, en gardant vos réglages, sans passer par l'ancien désinstalleur. Il ferme Sambler lui-même s'il est ouvert (« Lancer Sambler » le relance à la fin). Fini « Impossible de désinstaller le programme ! » quand une fenêtre intermédiaire était annulée, et fini la fenêtre anglaise « Please select a language » à la désinstallation.

## Nouveautés de 0.12.0

- **Langue automatique (par défaut)** : parlez français, anglais, ou les deux dans la même dictée, sans rien régler. Rambler reconnaît les deux langues ; Sambler applique la typographie française (espaces fines, guillemets « ») au texte français et la ponctuation anglaise au texte anglais. « Français uniquement » et « Anglais uniquement » restent disponibles dans Réglages › Langue.
- **Guillemets français** : les guillemets droits que Rambler écrit lui-même deviennent « » dans le texte français (rendu Naturel, espaces typographiques activés ; désactivable par application).
- **Tout tient dans la fenêtre** : à la taille par défaut, l'Accueil, la prise en main et le Carnet s'affichent sans défiler (textes raccourcis, espacements resserrés, fenêtre inchangée). Dans l'Accueil, la liste Récents défile seule.

## Nouveautés depuis 0.10.0


- **Thèmes** : « Verre et aurore » (par défaut) et « Doux et rond », en mode Système, Clair ou Sombre (Réglages › Apparence). La fenêtre et la barre flottante changent en direct. Polices embarquées : même rendu sur tous les systèmes.
- **Carnet** : notes dictées (« Dicter dans la note », ou le raccourci habituel quand Sambler a le focus), protégées pour votre compte Windows. « Copier » exclut le texte de l'historique Win+V.
- **Transcrire un fichier audio** (Carnet) : WAV, Ogg Opus, MP3, M4A/AAC, WMA et FLAC, jusqu'à 2 heures. Choisissez le fichier ou déposez-le sur la fenêtre ; le texte devient une note « Transcription · nom du fichier ». La dictée reste disponible pendant la transcription.
- **Activité** : mots par jour sur 16 semaines, séries de jours, temps de dictée et mots par application.
- **Style › Par application** : un rendu Naturel ou Fidèle, et des règles, propres à chaque application (par exemple Fidèle dans VS Code).
- **Style › Commandes de dictée** : « à la ligne », « nouveau paragraphe », « ouvrez/fermez les guillemets », « ouvrez/fermez la parenthèse ». Rambler en applique déjà une partie lui-même ; Sambler complète quand les mots restent écrits.
- **Accès intégré** : plus d'étape d'accès au premier lancement. Réglages › Accès à Rambler permet toujours d'utiliser un autre accès, et les variables `RAMBLER_*` restent prioritaires.

## Toujours là

Démarrage instantané (« Écoute » dès l'appui), insertion instantanée par collage avec presse-papiers restauré (ou frappe caractère par caractère), « Où insérer le texte » (champ de départ, même après un aller-retour vers une autre fenêtre, ou curseur à la fin), Récents (5 textes, 10 minutes, en mémoire), Dictionnaire, Raccourcis vocaux, Historique facultatif chiffré, barre flottante qui ne prend jamais le focus.

## À savoir

- **L'accès Rambler intégré est lisible dans le code et dans les fichiers de l'application.** Si Google le révoque ou le restreint, la dictée s'arrête pour toutes les installations : saisissez alors un autre accès dans Réglages, sans réinstaller. Gardez-le en tête avant de publier ces fichiers.
- Brave, juste après y être passé, peut ne pas exposer son champ à temps : le texte va dans Récents. Contournement : lancer Brave avec `--force-renderer-accessibility`.
- Rambler met parfois en forme le texte long (listes à puces `*`, nombres) ; ces mises en forme apparaissent telles quelles.
- Outlook (classique et nouveau) n'est pas qualifié sur le poste de test ; si rien n'est collé, le texte est dans Récents.
- Exécutables non signés : SmartScreen peut afficher un avertissement (« Informations complémentaires » → « Exécuter quand même »). Vérifiez `SHA256SUMS.txt`.

## Vérifié sur le poste de test (Windows 11, i5-12450H)

- Mise à jour depuis 0.11.1 ouverte, sans langue enregistrée (le cas qui affichait « Impossible de désinstaller le programme ! ») : Bienvenue → Dossier → Installation terminée, sans autre fenêtre ; Sambler fermé puis relancé par « Lancer Sambler » ; réglages, activation et compteurs conservés.
- Même version, choix « Désinstaller Sambler » avec Sambler ouvert : Sambler fermé sans question, désinstalleur directement sur sa page (ni « Please select a language », ni « en cours d'exécution »), puis l'installation reprend.
- Réinstallation silencieuse (`/S`) par-dessus une version ouverte : code de sortie 0.
- Prise en main, Accueil au lancement et Carnet affichés en entier à 1040×720 dans les deux thèmes ; version portable lancée sans variable `RAMBLER_*`.
- Tests automatisés : 141 tests natifs, 417 tests d'interface.

Code source sous licence MIT ; Opus, les polices et les autres composants tiers gardent leurs licences.
