# Ce que l’on doit pouvoir dicter avec Sambler

Dans Outlook, « Je **ne** pourrai **pas** venir jeudi à 14 h » doit arriver dans le brouillon choisi, avec la négation et l’heure intactes. Dans Word, une personne doit pouvoir dicter deux paragraphes sans les recoller à la main. Dans un prompt d’IA, elle doit pouvoir dicter trois contraintes sur plusieurs lignes et les relire avant de les envoyer. Si elle change d’onglet pendant la transcription, elle retrouve le texte dans Sambler. Ces situations définissent la première version.

Wispr Flow fournit un point de comparaison pour le nettoyage du texte et les expressions personnelles. Superwhisper en fournit un pour le choix du style. Sambler mise sur la qualité du français avec Rambler, la fiabilité au curseur et la récupération d’une dictée interrompue. La saisie vocale intégrée à Windows sert de témoin sur le même poste. [Étude des produits et comparaison sur PC](01-produits-et-usages.md)

## Le premier quart d’heure

1. **Brancher son micro.** Montrer le périphérique choisi et une jauge à déclenchement volontaire qui fonctionne sur le PC. Expliquer avant le premier envoi que Google reçoit la voix pendant la dictée. La personne peut essayer le niveau puis choisir « Activer la dictée avec Rambler ». Elle peut revenir sur ce choix dans les réglages. [Flux de données](06-confidentialite-et-continuite.md)
2. **Essayer une phrase dans Sambler.** Proposer de maintenir le raccourci, de dire « Rendez-vous jeudi à 14 h » puis de relâcher. Le voyant et l’annonce accessible signalent le moment où le micro est prêt. Le même écran permet d’essayer la bascule, de changer la combinaison et d’annuler.
3. **Dicter là où l’on travaille.** Revenir à un brouillon Outlook, à un document Word, à un champ web ou à un prompt. L’indicateur laisse le curseur en place. L’insertion a lieu une fois la prise validée. La phrase « envoie ce message » reste du texte. La personne garde la main sur l’envoi.
4. **Retrouver ce qu’on vient de dire.** Si la cible a changé, afficher « Texte disponible » et les actions Copier et Effacer. Après une insertion incertaine, indiquer « Vérifiez le champ avant de recopier ». En cas de coupure, un brouillon distingue les passages reçus et les lacunes. Seules les parties déjà validées sont récupérables. [Cas d’interruption](06-confidentialite-et-continuite.md)

## Ce que l’on livre dès la V1

| Usage | Exigence |
|---|---|
| Dicter tous les jours | Français prioritaire et anglais sélectionnable. Répondre dans Outlook, rédiger dans Word, remplir un champ dans Edge et Chrome, dicter un prompt dans VS Code. Qualifier les contrôles et versions réellement pris en charge. Le Bloc-notes sert aux premiers essais. Une copie manuelle reste une récupération et se compte séparément du succès d’insertion. [Matrice Windows](05-integration-windows.md) |
| Garder le contrôle du micro | Maintien pour parler, bascule appui/appui, bouton et commandes accessibles de la zone de notification. Tester changement de périphérique, clavier AZERTY/QWERTY, permission refusée, verrouillage, veille et retour. Fermer la fenêtre des réglages laisse Sambler disponible, quitter libère le micro et le raccourci. |
| Produire un texte prêt à l’emploi | Deux profils, un lexique et des expressions personnelles. Tester noms, montants, négations, autocorrections volontaires et répétitions voulues. Les retours à la ligne utilisent une voie d’insertion qualifiée pour le champ. L’app ne simule jamais Entrée ou Tab pour écrire un paragraphe. [Conditions de rendu](04-integration-moteur.md) · [Insertion multiligne](05-integration-windows.md) |
| Dicter plusieurs paragraphes | Viser cinq minutes par prise. Prévenir avant la limite et terminer proprement. Si une prise se coupe, rendre les passages complets déjà admis dans leur ordre et matérialiser les lacunes. La mesure sur voix réelle fixe la durée annonçable. |
| Relire ou récupérer | Un brouillon temporaire et, sur activation, un historique de texte local avec recherche, copie et suppression. Prise sans historique accessible en un geste. L’interface s’utilise au clavier, avec Narrateur, en contraste élevé et sans son. |
| Utiliser son PC au choix | Windows 11 x64 sous compte standard. Un même tag fournit l’installateur et le ZIP portable. Les deux passent les mêmes parcours. Les données privées de l’utilisateur du ZIP conservent la protection de son compte. [Distribution](09-distribution-et-version-portable.md) |

## Réglages et valeurs de départ

Les réglages usuels doivent suffire à dicter dès le premier lancement. Une modification de langue, de micro, de profil ou de vocabulaire s’applique à la prochaine prise. Un retrait d’autorisation de transmission ou de conservation prend effet immédiatement. Les commandes Annuler, Copier et Effacer restent accessibles quel que soit le profil.

| Réglage | Valeur initiale | Options et effet |
|---|---|---|
| Langue et microphone | Français, périphérique de capture par défaut de Windows | Choix explicite de l’anglais ou d’un microphone précis. Si le périphérique choisi disparaît, signaler l’incident et proposer de sélectionner le suivant. |
| Geste et combinaison | Maintenir pour parler avec `Ctrl+Maj+F2`, première combinaison à qualifier sur PC réel. | Bascule appui/appui pour les personnes qui préfèrent garder les mains libres. Combinaison modifiable. En cas de conflit, le signaler et garder bouton et menu utilisables. Les deux gestes doivent passer la recette de sortie. |
| Rendu du texte | **Naturel** avec ponctuation et typographie légères | En Naturel, appliquer les règles activées sur le texte admis. La gestion des hésitations ou des autocorrections par le service exige de prouver que le texte renvoyé correspond aux seuls mots à insérer. **Fidèle** reproduit la transcription admise telle que rendue par Rambler, y compris sa ponctuation. |
| Lexique personnel | Vide. Aucune collecte automatique de vocabulaire. | Ajouter et tester une graphie, par exemple « projet Mélusine ». Règles sur des mots entiers, activables individuellement. Les entrées restent sur le PC. En Fidèle, les substitutions demandent une activation distincte. |
| Expressions personnelles | Aucune expression enregistrée | Associer un déclencheur explicite tel que « insère ma formule courte » à un texte choisi. Aperçu avant enregistrement, désactivation et suppression. Une expansion s’effectue une seule fois. Les expressions sont désactivées par défaut en Fidèle. |
| Destination | Insérer une seule fois dans le champ qualifié au début et à la fin de la prise | Option « Toujours garder le texte dans Sambler » et action **Copier**. Une cible perdue ou incertaine désarme l’insertion pour toute la prise, quel que soit le réglage. Le texte du document et le contenu du presse-papiers restent privés. |
| Indicateur et son | Voyant discret pendant la prise et état lisible au clavier. Son désactivé. | Activer un son de début/fin, choisir l’emplacement de l’indicateur. Conserver un état perceptible pendant la capture et tenir compte du contraste et de la réduction des animations Windows. |
| Historique et brouillon | Historique désactivé, dernier brouillon en mémoire jusqu’à **10 minutes**. Aucun audio sur disque. | Si l’historique est activé, **7 jours** par défaut, **1, 7 ou 30 jours** au choix. Exclure la prochaine prise ou effacer à la demande. Pour le brouillon, proposer **1, 5 ou 10 minutes** ainsi qu’un effacement immédiat. [Rétention et supports portables](06-confidentialite-et-continuite.md) |
| Démarrage avec Windows | Désactivé | Option activable dans l’édition installée. Le ZIP se lance depuis son dossier, sans inscrire de démarrage automatique sur le PC hôte. |

Dans les réglages du texte, laisser activer séparément ponctuation légère, espaces typographiques et majuscule de phrase. Les trois sont actifs en Naturel et inactifs en Fidèle. Montrer l’effet des réglages sur une phrase d’exemple sans ouvrir le micro. Si un traitement plus poussé de Rambler est qualifié, l’exposer comme option distincte et conserver le choix de rester en nettoyage léger. Toute règle portant sur des noms, URL ou nombres doit préserver le sens et la graphie technique.

Reconnaître les déclencheurs d’expression sur la dictée complète, puis protéger leur expansion littérale. Appliquer le lexique aux mots restants, puis la typographie à la prose hors de ces passages. Signaler deux règles actives ayant le même déclencheur à l’enregistrement. Tester une signature avec un retour à la ligne et un nom de projet partagé entre deux segments. Une lacune interrompt toute correspondance entre les deux passages voisins.

**Exemple de rendu.** « Je peux mardi, pardon jeudi à 14 h » sert à vérifier le traitement d’une autocorrection. Une sortie Naturel qui retient « jeudi à 14 h » n’est acceptable que si la correction est sans ambiguïté. « Je ne peux pas mardi, mais jeudi oui » doit garder les deux jours et la négation. Au lancement du développement, le chemin de référence est la reconnaissance admise suivie de règles déterministes de ponctuation, d’espacement et de casse. Les enrichissements qui touchent au sens passent d’abord la recette avec de vraies voix. Une réponse ambiguë, vide ou refusée suit la décision du SDK. [Politique de rendu et d’admission](04-integration-moteur.md)

## Ce qui peut attendre

| Fonction | Essai qui justifierait de la livrer |
|---|---|
| Profils de rédaction par application | Mesurer les retouches évitées pour du code, une adresse ou un texte formel. Commencer par un choix manuel, puis un réglage associé au logiciel sur le PC. |
| Aperçu pendant la parole et arrêt automatique sur silence | Vérifier que Rambler fournit un aperçu révisable, puis le montrer seulement dans Sambler. Pour l’arrêt automatique, mesurer les fins de prise prématurées chez des personnes qui font de longues pauses. |
| Relire et reformuler une sélection | Essayer d’abord dans un éditeur contrôlé par Sambler. Afficher avant/après et prouver que le champ, le texte et sa révision sont ceux de la sélection initiale. [Contrat d’édition](../research/sdk-integration-guide.md) |
| Annuler la dernière insertion depuis Sambler | En V1, vérifier l’annulation native de l’éditeur. Une commande globale future doit savoir retirer exactement la dictée même si la personne a retapé ailleurs entre-temps. |
| Épingler ou exporter des entrées, partager un dictionnaire | Vérifier les usages réels de conservation et expliquer la durée distincte d’une entrée épinglée ou d’un export en clair. |
| Traduction, passage FR/EN dans une même prise, fichiers audio | Vérifier les capacités effectives de Rambler sur chaque usage et l’intérêt au-delà des deux langues déjà disponibles. L’import demande un choix explicite du fichier et de sa destination. |
| Réunions, voix multiples, capture du son système ou historique d’équipe | Ces extensions impliquent d’autres personnes et d’autres données. Priorité à la dictée individuelle dans une application active. |

## Idées à mettre à l’épreuve

Les pages publiques de Flow et de Superwhisper laissent ouverte la question de la nouveauté de ces pistes. Chacune peut se juger sur un incident concret.

| Idée | Ce que verrait la personne | Expérience |
|---|---|---|
| Vérifier son éditeur en une phrase | Dicter « L’été à 9 h 05 » puis confirmer accents, espaces et emplacement. Sambler retient « insertion testée » ou « copie conseillée » pour ce type de champ. | Comparer les échecs sur trois éditeurs avant et après l’essai. Invalider le résultat si une mise à jour change le type de champ. |
| Relire ce qui change le sens | Après une future reformulation, faire ressortir « 15 » devenu « 50 » ou une négation disparue et permettre de reprendre le texte initial. | Comparer les deux sorties attribuables sur des paires FR/EN annotées. Compter erreurs détectées et fausses alertes. |
| Reprendre après la dernière phrase validée | Un paragraphe est suivi de « Passage interrompu à 2 min 06 ». Après rétablissement, la personne lance une nouvelle prise et assemble les deux brouillons. | Couper le réseau pendant le second segment. Mesurer ce qu’il faut redicter, vérifier l’ordre et la visibilité des lacunes. La récupération simple est déjà exigée en V1. |
| Dicter d’abord, placer le texte plus tard | Garder une note dans Sambler, ouvrir le bon formulaire puis demander explicitement à y placer le texte. | Comparer au brouillon et à la copie manuelle déjà disponibles. Compter les erreurs de cible. |
| Préparer plusieurs champs en une seule dictée | « Objet, décaler la réunion. Message, je suis disponible à quinze heures. » Un gabarit défini par la personne découpe le texte rendu par Rambler en deux blocs à relire. | Sur un formulaire fictif, comparer à deux prises distinctes. Chaque bloc reste à placer par la personne, une rubrique ambiguë reste à relire. |
| Ajuster le signal de départ à son casque | Trois phrases de démarrage à froid révèlent si le premier mot est tronqué. | Comparer micro intégré, USB et Bluetooth, avec une seule syllabe puis une phrase. Mesurer ce que change le moment du signal « prêt ». |

Le [plan de construction et de recette](08-plan-de-construction.md) donne les seuils de sortie, les logiciels à essayer et la première consigne à donner à l’agent de développement.
