# Préparer l’acceptation de la qualité FR/EN

Les cas ci-dessous sont **des scénarios écrits de référence**, pas un corpus enregistré, de la parole spontanée ni des résultats d’un fournisseur. Ils servent à répéter l’annotation et à préparer une étude autorisée. Garder distincts les résultats des fixtures protocolaires, de la parole lue ou synthétisée et des voix spontanées.

## Dictée : référence et faits à vérifier

Dans les cas ci-dessous, le texte cité est le stimulus inventé. Annoter deux références avant l’essai quand elles diffèrent : **littérale** (mots réellement dits, avec répétitions) et **nettoyée** (texte visé, si ce mode est l’objet du test). Les hésitations, réparations et insertions de ponctuation sont annotées, pas inférées après avoir vu une sortie.

| ID | Langue et scénario | Référence ou invariant prédéclaré |
| --- | --- | --- |
| D01 | FR, « mardi à quinze heures… non, jeudi à quinze heures » | Littéral : les deux jours et les deux heures. Nettoyé : **jeudi** à **15 h**, sans maintenir le rendez-vous de mardi. `quinze heures` est une variante de forme acceptée pour le fait. |
| D02 | FR, « c’est très, très important » | Les **deux** occurrences de `très` sont voulues dans les deux modes ; `important` seul ne suffit pas. |
| D03 | FR, « Merci d’envoyer le devis à Maëlle. » | Élision `d’envoyer` requise ; noter `de envoyer` séparément. Nom `Maëlle`, document `devis`. |
| D04 | FR, « N’envoie pas le dossier avant le 9 mai à 8 h 30. » | Dictée, aucune action. Négation + borne `avant`, date `9 mai`, heure `08:30`. |
| D05 | FR, « Euh, on se retrouve à Varenne, je veux dire, Varennes-sur-Lac. » | Littéral : hésitation et autocorrection présentes. Nettoyé : lieu `Varennes-sur-Lac` uniquement. Tester le même stimulus avec et sans vocabulaire contextuel (toponyme inventé). |
| D06 | FR, « Il a dit : “On part ?” » | Attribution à `il`, question à conserver ; annoter aussi le découpage citation/question, sans imposer un style de guillemets. |
| D07 | FR, « Le total est de douze euros cinquante, pas vingt euros cinquante. » | Montant `12,50 €` ; `20,50 €` est explicitement exclu. Ne pas réduire à un seul montant dans le mode littéral. |
| D08 | FR, deux groupes séparés par une pause : « Prends les trois clés. [pause] Elles sont dans le tiroir du haut. » | Deux propositions, pause sans ajout ni perte de mots, `trois`, `tiroir du haut`. La notation `[pause]` n’est pas du texte attendu. |
| D09 | EN, “Don’t ship box B until June eighteenth, at four thirty.” | Negative + `until`, identifier `B`, June 18 at 4:30; no AM/PM inferred. No host action. |
| D10 | EN, “We need ten—no, eleven—blue labels.” | Literal: both numbers and correction. Clean: `eleven` blue labels, not `ten`. |
| D11 | EN, “No, no, I really meant Lake Mira.” | Both intentional `no`s; `Lake Mira` is a fictional name. |
| D12 | FR→EN au milieu d’une phrase, « Je te retrouve after the last train près du hall B. » | Conserver les deux langues dans le même énoncé, `last` et `hall B` ; ne pas traduire le passage anglais. Même entrée avec et sans langue secondaire. |
| D13 | EN→FR au milieu d’une phrase, “For the sign, write rendez-vous à midi next to the arrow.” | Conserver l’îlot français et `midi` ; garder le sens de `next to`, sans action automatique. |

## Édition : portée et intention indépendantes

Dans cette notation, `⟦…⟧` est la sélection initiale (vide = curseur). Les délimiteurs ne font **pas** partie du document ; `L | ` et ` | R` sont des sentinelles littérales, avant/après la cible. Pour une nouvelle observation, figer séparément le document complet, le découpage exact de la fenêtre, la plage locale UTF-16 initiale et la sélection résultante attendue si elle est définie. Ne pas générer le document attendu depuis la proposition. Les commandes figurent ici comme stimuli, pas comme indications sur l’intention brute que renverrait un fournisseur.

| ID | Fenêtre initiale et consigne | Intention, texte acceptable et limite |
| --- | --- | --- |
| E01 | `L \| La veste est ⟦bleue⟧. \| R` — « Remplace bleue par verte. » | Remplacer le seul mot sélectionné par `verte`, conserver la phrase et les deux côtés. Une transcription de la consigne n’est pas une édition. |
| E02 | `L \| ⟦Donne-moi le devis avant jeudi, 15 h.⟧ \| R` — « Rends ça plus poli. » | Ton poli, même document et échéance (`avant jeudi, 15 h`) ; par exemple `Pourriez-vous m’envoyer le devis avant jeudi à 15 h ?` ou `Merci de me transmettre le devis avant jeudi à 15 h.` Sans nouvelle promesse ni modification hors sélection. |
| E03 | `L \| ⟦It is important to note that the museum closes at 6 p.m. and does not open on Mondays.⟧ \| R` — “Make this shorter.” | Shorten the selection, preserving both closing time and Monday closure. `The museum closes at 6 p.m. and is closed Mondays.` is one acceptable variant. |
| E04 | `L \| Bravo⟦⟧ ! \| R` — « Ajoute l’emoji confettis après Bravo. » | Insertion locale de `🎉` après `Bravo`, séparé par un espace si déclaré ; ne pas dupliquer la phrase entière ni changer les sentinelles. |
| E05 | `L \| Send ⟦a draft copy of ⟧the form. \| R` — “Remove ‘a draft copy of’.” | Suppression dans la sélection, résultat `L \| Send the form. \| R` ; ne rien envoyer. |
| E06 | Après une première dictée : `L \| Départ à ⟦dix heures⟧. \| R` — « Corrige : onze heures. » | Historique déjà présent ; produire `L \| Départ à onze heures. \| R`. Si la proposition contient toute la fenêtre, ne pas la concaténer. |
| E07 | `⟦Brouillon⟧` (fenêtre entière par hypothèse), texte extérieur à la fenêtre inchangé — « Supprime la sélection. » | Une proposition de remplacement vide peut constituer un edit (et non un `no_edit`). Sans autorisation explicite de l’hôte, aucun effet. |
| E08 | `L \| ⟦é🇫🇷👩‍💻⟧ \| R` — édition test, `è🇫🇮👨‍💻` attendu | Vérifier le texte Unicode exact, la sélection UTF-16 `[4,15]` dans la fenêtre retournée et des limites de splice compatibles avec les graphèmes. Les deux accents sont décomposés. Changer révision, ancre ou document dans des essais distincts : conflit, aucun changement. |
| E09 | `L \| ⟦Draft⟧ \| R` — “Send.” et “Clear all.” (essais distincts) | Consigner séparément la transcription, toute classification et toute décision de l’hôte. Aucune autorisation de test pour envoyer/effacer ; document inchangé. |

Pour les fixtures de sécurité, ne pas annoter des finales identiques comme une seule hypothèse, ni une réponse au statut de sécurité inconnu comme une proposition rejetée qualifiée. Noter une prévisualisation comme provisoire et remplaçable ; une finale ambiguë ne valide ni le texte ni une mutation. Pour `Strict`, `AsrOnly` et `S3ClientCompatible`, étiqueter le rendu par sa provenance observée. Une chaîne littérale, même identique à une consigne canonique, ne suffit jamais à déduire une réécriture.

## Rubrique à geler avant collecte

1. **Identité et appariement.** Préenregistrer ID, langue, version de l’énoncé et de la référence, répartition développement/réserve, configuration (dont politique, contexte et vocabulaire), catégories et critères ci-dessous. Pour une paire de configurations, conserver la même entrée et changer une seule dimension ; annoter les écarts de prise de son au lieu de les masquer. Anonymiser les étiquettes et garder les captures autorisées hors dépôt.
2. **Mots littéraux.** Aligner la sortie littérale sur la référence littérale, sensible aux accents mais insensible à la casse et à la ponctuation de surface ; fixer à l’avance les règles de tokenisation (apostrophe droite/courbe équivalente, apostrophe interne maintenue dans le mot ; aucune fusion des nombres en lettres et en chiffres pour le WER principal). Les hésitations et les répétitions effectivement dites restent dans la référence. Publier `S`, `D`, `I`, `N` et `(S+D+I)/N` par strate et au total sur les cas évaluables ; `N` = mots de référence. Publier le nombre non évaluable à part. Ne pas appliquer ce WER à une réécriture libre ni le présenter comme un score de contenu nettoyé.
3. **Texte nettoyé et caractéristiques.** Préannoter les limites de phrase et marques attendues, entités/orthographes, faits numériques (valeur, unité, rôle), négations et leur portée, répétitions voulues (nombre) et réparations (valeur retenue/rejetée). Une variante de forme peut préserver un fait sans réussir le WER littéral. Noter ponctuation manquante/ajoutée, erreur de nombre ou de négation, répétition perdue/dupliquée, résidu d’une réparation, changement de langue maintenu et erreurs de nom/élision. Un label peut s’appliquer à plusieurs catégories.
4. **Édition.** Noter indépendamment **intention comprise** (action/cible correspondant à la demande), **texte acceptable** (faits préservés et consigne respectée, variantes acceptées), **portée** (fenêtre prête à substituer, sentinelles et contenu extérieur préservés, sélection/plage correctes). Étiqueter chaque axe `réussi`, `erreur`, `non évaluable` et, si elle était prédéclarée sans application, `sans objet`. Rapporter séparément la provenance et le statut de sûreté de la proposition, le résultat de la validation de l’ancre/révision et tout effet hôte effectivement observé. Une bonne reformulation au mauvais endroit est un échec de portée. Le [vérificateur exact](edit-acceptance-contract.md) ne remplace pas cette annotation : il compare une seule fenêtre attendue, sans juger les variantes sémantiques.
5. **Publication.** Pour chaque catégorie, donner le nombre prévu, évaluable, réussi/non, non évaluable et un exemple explicatif autorisé, sans compléter les données manquantes. Faire annoter indépendamment au moins les cas d’édition et de réparations ambiguës, masquer les noms de candidats si possible et consigner la résolution des désaccords avant agrégation. Reporter la répartition par langue, conditions et mode, sans fusionner les résultats `Strict` et `S3ClientCompatible`.

## En attente de références réelles

Le lot proposé de **40 énoncés spontanés** n’a pas été collecté. Proposition de stratification : 28 FR, 8 EN, 4 mixtes ; conserver en réserve 7 FR, 2 EN, 1 mixte, avec énoncés **et locuteurs** distincts du lot de développement. Utiliser les thèmes ci-dessus comme guide, sans transformer une lecture de ces phrases en « spontané ». À autorisation et recrutement effectifs, varier accents, débit, bruit et silences, et consigner les conditions, les consentements et la rétention. Les taux d’erreur réels, l’effet du vocabulaire ou de la langue secondaire et les comportements réels d’intentions non établies restent à mesurer.

Comparaisons également **en attente** : Rust et PixelBoard 1.3.5 sur le Xiaomi (mêmes versions/configurations consignées, texte initial, sélection et énoncé appariés avec écarts de capture documentés), et Gboard officiel sur matériel éligible. Aucun essai service, micro, téléphone ni résultat de référence n’est requis pour reproduire les tests de l’acceptation hors ligne.
