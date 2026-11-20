# Sambler pour Windows — campagne synthétique du 20 novembre 2026

Premières mesures de l'application Windows (branche `windows-v1`) avec le vrai service Rambler/Jetson. **Voix de synthèse uniquement** : ces chiffres qualifient la chaîne logicielle (contrôleur, adaptateur du SDK, rendu, insertion), pas la reconnaissance de voix humaines au micro. La recette des [critères de livraison](../windows/08-plan-de-construction.md#critères-de-livraison) reste à mener avec de vraies voix.

## Conditions

| Élément | Valeur |
| --- | --- |
| Poste | Windows 11 Professionnel build 26300, Intel Core i5-12450H, 32 Go, Ethernet |
| Application | Contrôleur et adaptateur réels, profil Naturel, geste maintien, moteur déjà connecté (« à chaud ») |
| Source audio | Phrases fictives synthétisées (voix Windows Hortense fr-FR, Zira en-US), WAV 16 kHz rejoué en temps réel à la place du micro par le harnais `recette_bloc_notes` |
| Cible | Fenêtre neuve du Bloc-notes 11.2607 (insertion `EM_REPLACESEL`) |
| Mesure | Du geste d'arrêt (relâchement simulé) à la première lecture non vide du champ via UI Automation, même horloge |

## Résultats (60 phrases : 40 FR, 20 EN)

| Mesure | FR | EN | Total |
| --- | --- | --- | --- |
| Texte inséré une fois, égal au brouillon rendu | 40/40 | 19/20 | 59/60 |
| Médiane arrêt → texte visible | 460 ms | 455 ms | 457 ms |
| p95 (rang `ceil(0,95 × n)`) | 556 ms | 607 ms | 594 ms |
| Maximum | 619 ms | 607 ms | 619 ms |

- **Échec** : « Can you add Sarah to the thread? » est classée `safety_blocked` par le service à chaque essai (2/2). Sambler n'insère rien et marque le passage « bloqué » dans le brouillon, conformément au contrat du SDK.
- **Écarts de contenu** (2) : « pull request » → « poll request » ; « Pensez à valider » → « Penser à valider ».
- **Écarts de forme seulement** (10) : heures et nombres normalisés par Rambler (« 14 heures » → « 14h », « 9 heures 30 » → « 9h30 », « 2 PM » → « 2:00 p.m. », « 450 dollars » → « $450 », « 20 pour cent » → « 20 % », « premier » → « 1er », « two » → « 2 ») et « main point rs » → « main.rs ». Le rendu Naturel a placé l'espace fine insécable avant « ? » en français.
- Taux d'erreur brut par mot, écarts de forme compris : 21/427 = 4,9 %.
- Aucun doublon, aucune écriture dans une autre fenêtre. Les essais interrompus par un déplacement de focus extérieur (essais d'interface lancés en parallèle) ont laissé le texte dans Sambler sans rien écrire ; ils ont été rejoués et ne sont pas comptés.

## Autres vérifications du même jour

- Application lancée, raccourci `Ctrl+Maj+F2` envoyé par `SendInput`, phrase jouée sur les haut-parleurs (volume 18 %) et captée par le micro intégré : insertion exacte en maintien (820 ms) et en bascule (621 ms). Échap en bascule : aucune écriture 5 s après. Silence seul : aucune insertion. Changement de fenêtre pendant le traitement : aucune écriture, « Texte disponible » et brouillon avec Copier (formats d'exclusion de l'historique Win+V présents).
- Hors ligne, avec du texte généré par le harnais : Word 365 (insertion atomique, multiligne, annulable en un Ctrl+Z), Edge et Chrome (input, textarea, contenteditable : 20/20 chacun, ≈ 0,85–0,93 s par 100 unités UTF-16), chat de VS Code (un essai exact, aucun envoi), terminal intégré classé incertain.

## Reste à qualifier

Voix humaines (100 intentions, 70 FR / 30 EN), latence p95 sur voix réelles, Outlook classique (aucun profil sur ce poste) et nouvel Outlook, Narrateur, verrouillage et débranchement du micro en cours de prise, prises de 30/121/300 s, et les deux paquets sur un poste vierge.
