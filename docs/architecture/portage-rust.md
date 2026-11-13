# Portage du moteur Rambler en Rust

Décision utilisateur du **13 novembre 2026** : porter le moteur en Rust. Clôturer
le lot Python déjà engagé, puis utiliser Python comme référence de conformité et
outil de recherche, **sans dépendance Python à l'exécution du moteur final**.

La [recommandation desktop](application-desktop-multiplateforme.md) reste pertinente,
mais Tauri, React, microphone, raccourcis et insertion système sont des étapes
ultérieures. Le portage du cœur n'autorise ni moteur de reconnaissance alternatif,
ni API Gemini publique, ni relais Android.

## Ordre de réalisation

1. **Référence Python 0.3.0** : résultats et limites documentés, fixtures, tests
   offline/loopback, preuves du service réel et commit de clôture. Ne pas attendre
   une perfection universelle du service pour fixer ce contrat.
2. **Tranche Rust indépendante** : CLI fichier Ogg/Opus → vrai S3Bidi Rambler →
   résultat confirmé après fin S3 **et** gRPC réussie. Pas de microphone ni GUI.
3. **Parité de robustesse et audio** : sessions, contexte différé, annulation,
   erreurs tardives, collecte bornée, encodeur PCM natif optionnel et métriques.
4. **Comparaisons contrôlées** : mêmes sources et configurations, temps client et
   CPU/RSS séparés ; ne pas attribuer à Rust les variations du serveur/réseau.
5. **Intégrations desktop** seulement après validation du moteur autonome.

## Contrat à préserver

- Endpoint/méthode et routage du véritable service documentés dans
  [le protocole](../research/protocol.md), TLS vérifié et credentials fournis
  localement, jamais embarqués dans un binaire ou exposés à une interface.
- Init → flux Ogg/Opus continu → contexte tardif → `10 01 18 01` → half-close →
  drainage jusqu'au statut terminal. Aucun rejeu automatique d'audio.
- Hints optionnels : biais/préférences dans `InputBoxInfo` **à l'init**, langues
  secondaires dans `UserInfo`. Ne pas recopier ces listes dans le contexte tardif,
  inventer des flags ou prétendre que toute préférence est respectée par le service.
- Protobuf : présence distincte des défauts, ordre des répétitions, fusion proto2
  des messages singuliers et du même membre oneof, reset au changement de membre.
  Conserver enums/champs inconnus et octets bruts ; ne pas concaténer des fragments
  malformés pour les rendre artificiellement valides. Bornes de taille/champs.
- Une génération protobuf standard n'est pas présumée conserver les extensions
  inconnues. Valider le codec avant de choisir un modèle généré qui les perdrait.
- Producteur/encodeur/nettoyage supervisés et bornés. Arrêt normal avec flush
  complet distinct d'une annulation ; aucune réussite sur upload incomplet, END
  manquant, erreur S3 ou erreur gRPC tardive.
- Toutes les finales ordonnées conservées, sans concaténation/déduplication choisie
  arbitrairement. Distinguer hypothèse, transcript et texte édité. Conserver les
  refus explicites d'ambiguïté et la politique de rendu des intentions vérifiées.
- Propositions d'édition sans effet de bord, snapshot/révision et unités UTF-16.
  Rendre une réécriture ne suffit pas à autoriser son remplacement automatique.
- Aucun import d'Android, aucun processus FFmpeg pour le pipeline normal. Les
  outils de préparation de corpus restent des outils de développement distincts.

## Référence et vérification

- [`tests/fixtures/live_synthetic_responses.json`](../../tests/fixtures/live_synthetic_responses.json) :
  trois payloads originaux inspectés.
- [`tests/fixtures/live_rewrite_response.json`](../../tests/fixtures/live_rewrite_response.json) :
  fixture **dérivée/expurgée**, jamais présentée comme quatrième original.
- [`tests/test_protocol_hints.py`](../../tests/test_protocol_hints.py) : goldens
  init préexistants et émission exacte des champs optionnels.
- [`tests/fixtures/result_policy_conformance.json`](../../tests/fixtures/result_policy_conformance.json) :
  19 cas d'oracle Python pour le rendu, les ambiguïtés, suppressions vides, conflits,
  révisions typées de taille arbitraire et UTF-16. Les 20 tests Python associés
  vérifient ce corpus ; ce n'est pas une preuve de session S3 complète.
- Suites Python protocole, transport, moteur, résultats et encodage : cas à
  traduire, pas seulement happy-path. Tests Rust offline/loopback avant accès réel.
- [Corpus naturel](../evidence/natural-expansion-public.json),
  [endurance](../evidence/endurance-300-public.json) et
  [hints](../evidence/input-hints-public.json) : mesures de référence avec leurs
  hashes, conditions et limites. Ne pas confondre les versions de code mesurées.

Pour comparer les requêtes byte à byte, fournir les mêmes valeurs explicites de
configuration. Le client Rust réel devra déclarer une plateforme honnête `Rust`,
pas `Python` ou un appareil Android fictif ; cette différence attendue ne doit pas
être confondue avec une régression wire.

Les hashes de modules des rapports décrivent les fichiers locaux réellement
mesurés. Git peut normaliser leurs fins de ligne CRLF/LF : vérifier cette seule
différence avant de conclure à un changement de code. Le smoke de roue vérifie
l'égalité exacte des modules installés avec les fichiers locaux ; cette question
n'affecte pas les goldens des requêtes protobuf binaires.

## Première tranche obtenue le 13 novembre 2026

La CLI release Rust a terminé trois sessions réelles Windows : anglais, édition
sur contexte et français, sans Python/FFmpeg dans le PATH du processus. Les
[observations inspectées](../evidence/rust-first-slice-public.json) fixent les
hashes du code/binaire et les limites. 121 tests Rust intégrés et 643 tests de la
référence Python passent ; aucun gain de latence remote n'est déduit de ces essais.

Le transport réel est `h2` + `tokio-rustls`/racines système, avec types et tests
`tonic`. Ce choix remplace le Channel tonic/hyper du prototype, pas le service :
le test d'upload bloqué exigeait de pouvoir réinitialiser une session sans couper
les autres ou laisser son upload reprendre après annulation. Les cas HTTP/RST ont
été comparés au client tonic. Le portage PCM natif et les mesures Rust prolongées
restent distincts de cette preuve fichier Ogg. [Guide d'intégration](../rust.md).

## Critère de réussite du premier lot Rust

Une CLI Rust autonome reçoit une réponse du vrai Rambler, sans Python/Android à
l'exécution, et les tests de conformité retenus passent. Les lacunes restantes
sont identifiées précisément ; aucun gain de vitesse, de mémoire ou de portabilité
non mesuré n'est revendiqué. Python reste dans le dépôt pour la recherche et les
comparaisons, pas comme implémentation cachée appelée par le binaire Rust.
