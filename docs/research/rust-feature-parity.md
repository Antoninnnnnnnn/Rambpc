# Parité Rambler / Gboard : matrice vérifiable

État de l'analyse : 2026-11-13. Référence du moteur avant ce lot : `4fdf29e`.
Il s'agit d'un **sous-ensemble du client Rambler S3**, pas d'un clone de toute
l'application Gboard. Un enum décodé n'est pas une fonctionnalité distante validée,
et un résultat distant n'est pas nécessairement le texte appliqué par Android.

## Sources et niveaux de preuve

- APK analysé statiquement : SHA-256
  `a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`,
  rehaché pendant cette revue. Les noms obfusqués ci-dessous concernent **ce build**.
- Extraits JADX locaux : `pixelboard-jadx/sources/defpackage/`, et extraction ciblée
  `pixelboard-jadx-ihm/sources/defpackage/ihm.java`. Ils ne sont pas exécutés.
  La récupération des métadonnées est décrite dans [protocol.md](protocol.md) et
  utilise `tools/research/jadx_metadata.py` depuis un emplacement de confiance.
- Sources PixelBoard analysées : révision `9a81f8b`, fichier
  `GboardRamblerDictationRuntime.java`, lignes174–224 et232–264. Le filtre local
  travaille sur une VoiceAction déjà créée ; son mock ne prouve pas une réponse S3.
- **Rust réel** : [première tranche Ogg](../evidence/rust-first-slice-public.json),
  [PCM, réécriture, sélection](../evidence/rust-pcm-functional-public.json).
- **Python réel seulement** : corpus/hints et réutilisation de canal documentés dans
  [les observations](../evidence/README.md). Pas attribués rétrospectivement à Rust.
- **Statique/offline** : schéma, branche client, test synthétique. Pas une activation
  de fonction serveur ni une comparaison appariée avec un appareil.

## Intentions et politiques de résultat

Enum : `aamp.java:5`; VoiceEditingResult : `aamq.java:20`.
Le helper S3 `ihm.java:192–212` accepte les intentions2/3 avec oneof3/7 ; les autres
retombent dans son chemin ASR. Le moteur natif reste volontairement plus strict :
il n'invente pas la portée d'un remplacement ni un fallback métier.

| Valeur / fonction | API et politique Rust | Preuve et limite |
|---|---|---|
|0 DICTATION|`VoiceEditResult.intent` conservé ; rendu sans VoiceEdit possible, mais VoiceEdit0 ambigu|Enum statique. Pas de valeur wire0 démontrée dans les preuves publiées. Écart conservateur de façade avec le fallback Android.|
|1 EDIT|Décodable, rendu/proposition non interprétés|Nom statique seulement ; ne signifie pas automatiquement suppression, envoi ou delta.|
|2 EDIT_HISTORY|`DictationResult::text`, `replacement_proposal`, snapshot/révision ; champs3 et7|**Rust réel** : dictées EN/FR, correction, `ten→eleven`, sélection avec emoji dans le contexte. Fenêtre complète, pas ajout aveugle.|
|3 REWRITE|Rendu champ3 ; champ7 ambigu ; pas de proposition automatique|**Rust réel** champ3 : réécriture polie. Helper Android accepte aussi7 statiquement ; cette combinaison n'est pas validée à distance.|
|4 COMPOSE|Décodable ; rendu/proposition refusés|**Non démontré**. Huit essais historiques restent distincts des mocks et prompts locaux ; voir [composition](composition.md). Aucun switch S3 vérifié.|
|5 RAMBLE|Décodable ; pas de politique de rendu|Enum certaine, sémantique opérationnelle non établie par son nom ; non observé.|
|6 RAMBLE_HISTORY|Décodable ; pas de politique de rendu|Même limite ; aucune preuve de résumé ou nettoyage de notes.|
|VoiceEdit champ2|`alternative_text`, `active_field` ; ambigu|Même oneof que3/7. Ni synonyme d'ASR ni fallback édité prouvé.|
|Plusieurs finales/hypothèses|Conservées ordonnées ; pas de texte unique arbitraire|API structurée, collecte bornée ; aucune concaténation/déduplication inventée.|

Le filtre PixelBoard peut supprimer des diffs et convertir une action locale4
avec transcript vers0 ; il préserve5 dans une branche où d'autres deviennent0.
Cela **ne prouve pas** que le helper S3 crée une action4/5/6.

## Marqueur de sécurité : écart corrigé dans ce lot

Chaîne statique précise :

1. `aalg.java:37`, RawMessageInfo : **hypothesis.field30 enum**, présence21,
   membre Java `int e` (`aalg.java:11`). Pas un booléen wire.
2. `ywl.java:12,36–41`, validateur `r`, case17 vers `a.aY` ;
   `a.java:490–495,677–679` : seules les valeurs0/1 sont reconnues.
3. `ihm.java:192–212` : pour le candidat VoiceEdit accepté, valeur1 devient
   `iid.e=true`, puis `ihm.java:287–291` la copie dans `aakw.k`.
   `aakw.java:33` situe ce **booléen local** au champ10 de VoiceText ; ce n'est
   pas un second champ sécurité S3 à envoyer.
4. `fga.java:11–15` et `ffh.java:703–708` : `SAFETY_BLOCKAGE` et retour **avant**
   classification de commandes (`:767`), envoi IME (`:780`) et édition (`:804`).

Avant ce lot, Rust conservait le champ30 comme inconnu, mais `text()` et
`replacement_proposal()` l'ignoraient. Une finale intention2/champ3 avec valeur1
pouvait donc être rendue/proposée contrairement à cette branche Android.

Le portage décode maintenant `Hypothesis.safety_status` avec présence0/1.
Les autres valeurs et mauvais wire types restent dans `unknown_fields` ; une
occurrence inconnue ne supprime pas la dernière valeur reconnue (proto2).
Les helpers de rendu/proposition refusent tout candidat final bloqué et tout
champ30 non interprété. La CLI renvoie une erreur sans texte/contexte/finales.
Les réponses structurées restent explicitement disponibles aux intégrateurs.

**Durcissements délibérés, pas parité Android prétendue** : refus hors VoiceEdit
et sur valeurs inconnues ; valeurs numériques u64 autres que0/1 conservées
opaques, sans troncature int32 susceptible de les faire passer pour0.
Un varint long représentant effectivement0/1 reste reconnu. Aucun cas de blocage
sécurité n'a été observé sur le service ; validation synthétique/offline seulement.

## Autres capacités et frontières

| Capacité | Preuve statique / API Rust | Qualification |
|---|---|---|
|Session, audio, stop normal|`ihv.a`, `iho.java:40` ; `Engine`, `AudioSource`, init/audio/contexte/END puis half-close|**Rust réel** Ogg et PCM natif. Fin exige END S3 + upload complet + cleanup + gRPC OK ; pas une finale précoce.|
|Annulation / délais|`EngineSession::{request_cancel,close}`, transport h2 par flux|Loopback/backpressure/concurrence et vrai Ctrl-C Windows ; pas de rejeu automatique.|
|Contexte tardif|`iho.a/b/g`, `iho.java:96` ; `ContextInput`, `ContextProvider`|Édition réelle Rust ; ordre callback/cleanup borné testé offline. Pas de comparaison sur appareil.|
|Sélection UTF-16|`ffq.java:64–74,84–105` ; `selection_utf16_range`, `validate_current`|Sélection `ten` avec emoji dans le contexte réelle Rust ; sélection de l'emoji lui-même non validée.|
|Partiels|`ihm.java:310–355`, double/seuil0,9/span non final ; `partials()`, événements|Décodage offline. Pas de preuve dédiée publiée de rendu partiel Rust ; `first_text` ne suffit pas.|
|Segments vs fin globale|`aaln.field3/4` indépendants ; événements, `recognition_state`|Conteneurs conservés ; collecteur final seulement state1. Aucune règle d'assemblage déduite des seuls noms.|
|Autres résultats oneof|`aapd.java:34` :1/2,5–13,15,16,18–21 ; champs3/4 indépendants|`active_field`, `active_raw`, inconnus préservés ; **pas de prise en charge sémantique** des variantes opaques.|
|Extensions futures|Inconnus, enveloppe5 et extension74956225|Raw/inconnus et bornes préservés. Pas de garantie de sémantique d'un futur oneof non connu.|
|Biais, préférences, langues secondaires|InputBox1203 champs3/5, UserInfo4 ; `ProtocolConfig`|Encodage Rust testé ; dix transports **Python** acceptés, effets limités. Aucun gain général ni activation COMPOSE établi.|
|Contexte applicatif Android|`ffh-simple.java:2010`, gates X/Y ; wrapper app-info/préférences|Non reproduit comme collecte OS. Les champs de préférences sont exposés ; leur effet serveur n'est pas garanti.|
|Commandes SEND/CLEAR|`fgc.java:13–30` regex ; `ffh.java:767–781` prend seulement SEND avec IME_ACTION_SEND|Classification/effet **locaux**, hors insertion OS de cette phase. Détection CLEAR ne prouve pas suppression exécutée.|
|Diffs, espaces, curseur, application|`ffh.E`, AgenticDictationTextEditor|Proposition native sans effet ; pas de copie des heuristiques/fallbacks Android. Contrôle snapshot/révision plus prudent qu'un remplacement concurrent forcé.|
|MUSE/MUSE_LITE|`ffi.java:21–30`, injection `erz:1895`, `wmr.b` carte vide|Pas d'autre backend distant manquant établi ; changement d'enum peut revenir au fournisseur S3.|
|Jetson Lite / génération locale|`kkj.a`, `ohq.java:69` Binder Speech Services, `enm.java:38` Binder AICore|Chemin Android distinct, services/modèles/gates requis. Non porté ; prompts embarqués ≠ activation S3. Aucun autre ASR/API Gemini substitué.|
|Authentification / cycle de vie accès|Métadonnées applicatives injectées, branche Zwieback conditionnelle|Profil réel sans compte/Zwieback. Pas de provisionnement automatique, renouvellement/révocation ni garantie d'accès durable.|
|Microphone, GUI, insertion OS|Non implémentés|Hors périmètre moteur initial ; pas présentés comme fonctions cloud manquantes.|

## Suites utiles et véritables blocages

- Marqueur sécurité : correction et tests offline dans ce lot, pas d'audio sensible
  envoyé pour déclencher artificiellement un blocage.
- Présentation : projection sans effets de bord distinguant partiels, segments,
  finales et variantes opaques ; jamais autorisation d'appliquer ou preuve de fin.
- La comparaison exacte **service → helper S3 → filtre PixelBoard → IME** demanderait
  une référence Android appariée avec backend effectif et éditeur de test inerte.
  Au plus trois cas courts discriminants (répétition près d'un texte similaire,
  réécriture connue, SEND) seraient préférables à de nouveaux essais COMPOSE aveugles.
  **Aucun essai appareil ni nouvel appel service n'a été lancé pour cette matrice.**

Les traces absentes de l'incident long historique ne peuvent pas être recréées
par une série de succès. Sa cause reste indéterminée ; les nouveaux compteurs
audio améliorent un futur diagnostic sans prétendre résoudre cet ancien incident.
