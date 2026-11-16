# Composition Rambler : preuves et limites au 2026-11-13

## Conclusion bornée

**La composition n'est pas démontrée sur le profil S3 natif testé.** Cinq sessions
réelles du coordinateur ont terminé correctement : question directe et demande de
message restent dictées littéralement, tandis que le contrôle de réécriture produit
bien une phrase polie. Aucun de ces résultats ne contient `INTENT_COMPOSE=4`.
Trois essais supplémentaires avec préfixe d'invocation restent également
littéraux, mais le nom «Gboard» est mal reconnu («Hey board», «Hagbard») : ils
**ne réfutent pas proprement** un éventuel déclencheur phonétique.
Le suivi SDK0.3.0 ajoute quatre observations appariées : «Gboard» est cette fois
reconnu dans les quatre sorties, et un préfixe complet «Hey Gboard» ou
«Okay, Gboard» dans trois d'entre elles, mais les résultats restent littéraux,
intention2 / oneof3. Ce suivi lève donc **partiellement** le confound antérieur,
sans établir de composition ni d'intention4.
Cela ne prouve ni l'absence universelle de composition dans Rambler, ni son
impossibilité dans un autre profil ou une autre version du service.

Trois éléments doivent rester distincts :

1. L'enum `INTENT_COMPOSE=4` et un test **simulé** PixelBoard existent.
2. Le sélecteur de backends de cet APK reçoit une **carte vide** pour les
   alternatives MUSE/MUSE_LITE : changer leur enum ne suffit pas à les activer.
3. Un véritable chemin **Jetson Lite sur Android** existe séparément comme
   retombée, avec ASR lié à Speech Services et génération liée à AICore. Ses
   prompts prévoient certaines générations, mais cela ne valide ni leur exécution
   effective sur un appareil, ni une réponse wire S3 d'intention4. Ce chemin n'est
   pas une API publique de remplacement pour le moteur natif.

Aucune modification SDK, aucun changement de backend et aucun accès réseau n'ont
été réalisés par l'analyste de cette passe. Les sessions réseau ci-dessous sont
celles du coordinateur, relues dans leur rapport expurgé.

## Provenance et conventions des références

Autorité APK : PixelBoard1.3.4, SHA-256
`a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`.
Source PixelBoard consultée : tag extrait sous
`Akshayykadam-PixelBoard-9a81f8b`. Aucun code APK ou source PixelBoard n'a été
exécuté : lecture textuelle avec Python isolé, sans import des fichiers examinés.

Les alias de sources suivants désignent les artefacts locaux de recherche déjà
identifiés dans [protocol.md](protocol.md) :

- `APK` : `scratchpad/pixelboard-jadx/sources`, notamment `defpackage`.
- `IHM` : `scratchpad/pixelboard-jadx-ihm/sources/defpackage/ihm.java`.
- `FFH-SIMPLE` : `scratchpad/pixelboard-jadx-ffh-simple/sources/defpackage/ffh.java`.
- `TAG` : `scratchpad/pixelboard-source/Akshayykadam-PixelBoard-9a81f8b`.

Le scratchpad de cette analyse est
`C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad`.
Les numéros ci-dessous sont ceux des sorties JADX ou des sources citées, pas des
lignes du code original Google. Les conditions manquantes des décompilations
incomplètes ne sont pas considérées prouvées.

## Enum, sortie S3 et filtre PixelBoard

- `APK/defpackage/aamp.java:6-12,21-38` établit notamment
  `2=INTENT_EDIT_HISTORY`, `3=INTENT_REWRITE`, `4=INTENT_COMPOSE`.
- `IHM:192-219` accepte seulement les intentions2/3 dans le helper récupéré
  `maybeGetVoiceEditedText`. Il sélectionne le texte édité field3 ou le contexte
  field7 ; les autres intentions ne produisent pas ce candidat au callsite.
  `IHM:224` revient alors au texte ASR et `IHM:255-285` ne construit une
  `VoiceAction` que si ce candidat est présent. Une enum4 ne démontre donc pas
  son exposition par ce processeur S3.
- `APK/defpackage/ffh.java:670-695` appelle le filtre PixelBoard **avant** de
  vérifier le span final et la présence d'une `VoiceAction`.
- `TAG/pixelboard-patches/extensions/extension/src/main/java/com/akshaykadam/pixelboard/extension/rambler/GboardRamblerDictationRuntime.java:174-183`
  remplace, pour une action locale4 avec transcript non vide, le texte produit par
  ce transcript, remet l'intention à0 et efface le diff. C'est une transformation
  **locale après résultat**, pas un déclencheur de composition transmis au serveur.
  Le corps principal de cette méthode est manquant dans la décompilation générale
  APK ; le détail de cette branche vient de la source taguée, et son appel effectif
  est visible dans `ffh` de l'APK.
- Le test tagué adjacent
  `src/test/java/com/akshaykadam/pixelboard/extension/rambler/GboardRamblerDictationRuntimeTest.java:74-88`
  injecte explicitement une **fausse action4**, la question « What is the capital
  of France? » et une réponse sur Paris. Il vérifie le filtre client : ce n'est
  **pas** une capture du backend.

Le statut IME `ABORT_COMPOSING` rencontré dans le cycle de vie de la dictée ne doit
pas être confondu avec l'intention sémantique `INTENT_COMPOSE`.

## Backends : câblage réellement présent

### Sélecteur MUSE/MUSE_LITE : alternatives non enregistrées ici

`APK/defpackage/mup.java:6-10` définit UNDEFINED0, S3=1, MUSE=2, MUSE_LITE=3.
`APK/defpackage/muq.java:31` donne1 comme défaut déclaré du backend.
`APK/defpackage/ffi.java:21-30` consulte d'abord
`pref_key_ad_backend_override`, sinon ce flag, puis cherche le fournisseur dans la
carte injectée. Une entrée absente revient au fournisseur S3 par défaut.

Le point décisif est l'injection, pas le nom des enums :

- `APK/defpackage/erz.java:1895` enregistre `new ffi(this.cM, wmr.b)`.
- `APK/defpackage/erz.java:1088` construit `cM` avec `ffk`.
- `APK/defpackage/ffk.java:16-17` construit l'extension `ffh`.
- `APK/defpackage/wmr.java:11` définit `b` comme une carte de zéro entrée.

La recherche des références MUSE et de la construction de `ffi` a couvert les
19704 fichiers Java disponibles dans `APK/defpackage`, en excluant `iha.java` ;
elle n'a trouvé qu'une construction de `ffi`. Les deux racines source de patchs
principales examinées (107 fichiers Java/Kotlin/XML) ne contiennent pas de
référence aux prefs de sélection de backend ni au flag de backend.

**Conséquence pour ce câblage** : une préférence2/3 n'active pas magiquement un
client MUSE/MUSE_LITE ; l'entrée manque et le fallback reste `ffh`. Aucun endpoint,
schéma d'authentification ou client de composition distant alternatif n'est ainsi
établi. Cela ne signifie pas que tout code portant le nom Muse soit inutilisé :
il existe des modules et un véritable chemin Lite distinct ci-dessous.

### Jetson Lite : retombée Android réelle, pas endpoint portable démontré

- `APK/defpackage/kkj.java:23-46` sélectionne S3 lorsqu'il est disponible et peut
  sélectionner Jetson Lite lors d'une retombée hors ligne ou d'une indisponibilité.
- `APK/defpackage/fgu.java:13-23` installe la factory `fgo` lors de la création du
  module ; `fgo.java:14-23` exige une session Jetson et
  `muq.K == a.ao(3)`. `a.java:771-772` résout cette valeur à1, alors que le défaut
  déclaré `jetson_no_network_fallback_strategy` est0 (`muq.java:45`). Les valeurs
  effectives, le chargement du module et les modèles disponibles sur un appareil
  ne sont pas établis ici ; aucune de ces gates n'a été modifiée.
- ASR : `erz.java:1534-1538,1823-1825` fournit le client `uxz` via
  `sgq.java:91-97` et `ohq.java:69-80`. Le channel est construit vers le composant
  **Android Binder** `com.google.android.tts` /
  `com.google.android.libraries.speech.transcription.recognition.grpc.GoogleAsrService`.
  Le package vient de `GboardSpeechServicesDetector.PACKAGE_NAME`.
  `uxz.java:15-45` décrit `RecognitionSession` et `DownloadModel` sur ce service.
  Ce n'est pas la découverte d'un hôte HTTPS distant.
- La construction du channel inclut une policy `abfi`, un `PackageManager` et une
  allowlist contenant le package Speech Services (`sgq.java:94-97`). Les contrôles
  Android de liaison/identité et l'installation du vrai service restent des
  dépendances ; aucun header cloud autonome équivalent n'est établi ni proposé.
- Génération : `fgn.java:187-202` crée un client AICore et demande un feature ;
  `fgn.java:82-87` appelle le client LLM `eog`. `enm.java:38-40` cible les services
  Android `com.google.android.aicore` /
  `AiCoreMultiUserService` ou `AiCoreService`, via Binder. Disponibilité du feature,
  autorisations effectives, modèle et résultats réels ne sont pas vérifiés ici.

`APK/defpackage/igs.java:6` déclare exactement les expressions «Okay Gboard» et
«Hey Gboard». `APK/defpackage/fgn.java:63` contient une comparaison exacte d'un ancien prompt
et son remplacement par un autre prompt, sous condition d'égalité. L'ancien
texte prévoit un mode explicite avec «Hey Gboard» et génération demandée ; le
texte de remplacement prévoit aussi l'exécution de demandes telles que
« Write me a sick leave email », plutôt que leur dictée littérale. Ce sont des
**instructions locales embarquées** montrant une intention de fonctionnalité,
non la preuve d'une inférence réussie, non un prédicat du backend S3, et non la
preuve de l'émission d'une intention wire4. Si le prompt effectif diffère du
littéral comparé, cette branche de remplacement ne s'applique pas nécessairement.

## Préférences : pas de switch compose démontré

`muq.java:58-60` déclare app-context (`X`, défautfalse), custom-instruction (`Y`,
défautfalse) et des règles rewrite/compose (`Z`). La présence de ce dernier nom
ne prouve pas son émission. Aucune référence `muq.Z` n'a été trouvée dans les
19704 fichiers `defpackage` examinés.

`FFH-SIMPLE:2010-2077` ajoute l'app-context seulement avec la gateX et un
`EditorInfo` adapté, et une instruction personnalisée seulement avec la gateY et
une préférence non vide. Le wrapper de cette dernière limite explicitement les
préférences au style de dictée et exclut les réponses aux questions et tâches
non-dictée. Il ne doit pas être réinterprété comme une méthode pour activer la
composition. Le détail de transport/timing des hints est une analyse séparée.

## Cinq observations réelles du profil S3

Preuve expurgée du coordinateur :
[compose-matrix-public.json](../evidence/compose-matrix-public.json), code observé
au commit `a5fa05c2a0492e6f6feb186c9175c3253c918624`.
Résumé :5 sessions complètes,5 transports/rendus uniques réussis. Les contenus
sont des phrases contrôlées ; aucun payload opaque n'est reproduit ici.

| Cas | Contexte / variable | Résultat observé | Intention / oneof |
|---|---|---|---|
| `compose-question-en` | Question directe, contexte vide | La question sur la capitale reste une question ; pas de réponse générée |2 /3|
| `compose-message-empty-en` | Demande d'un message à Alex, contexte vide | L'instruction est dictée, avec «20» normalisé |2 /3|
| `compose-message-context-en` | **Même fichier audio** que le précédent, précédé de «Message to Alex:» | Scaffold + instruction dictée ; pas de message composé |2 /3|
| `compose-question-literal-control-en` | Question présentée comme texte à inclure | La question citée est conservée comme texte |2 /3|
| `compose-rewrite-control-en` | Demande de politesse, contexte «Send me the report now.» | «Could you please send me the report at your earliest convenience?» |3 /3|

Les deux cas message partagent le même SHA-256 audio dans la preuve : la variation
porte bien sur le contexte, pas sur deux synthèses distinctes. Le contrôle de
réécriture écarte ici l'explication simple «aucune commande contextuelle ne
fonctionne». Il ne prouve pas la composition, ni la portée de remplacement de
l'historique pour l'intention3.

Plans reproductibles :
[compose-validation.json](../../tools/fixtures/compose-validation.json) et
[compose-speech.json](../../tools/fixtures/compose-speech.json). Ils bornent les
observations à cinq appels, sans retries automatiques ni ajout de flags/auth,
sans changement de backend et sans autre API générative.

## Suivi : invocation explicite et confusion ASR

Preuve expurgée :
[compose-invocation-public.json](../evidence/compose-invocation-public.json).
Les trois sessions supplémentaires du coordinateur terminent correctement, sans
changement de flags, de prompt ou de backend ; leurs résultats restent
intention2 / oneof3 :

| Cas | Texte retourné pertinent | Conséquence bornée |
|---|---|---|
| `compose-invocation-question-en` | «Hey board, what is the capital of France?» | Pas de réponse générée, mais le nom d'invocation n'est pas fidèlement reconnu |
| `compose-invocation-message-en` | «Hey board, write a short message…» | Demande encore littérale ; même confusion du nom |
| `compose-invocation-literal-en` | Citation contenant «Hagbard, what is the capital of France?» | Le contrôle reste du texte cité, avec une autre confusion phonétique |

**Ces négatifs sont confondus par la reconnaissance du nom.** Ils ne permettent
pas de réfuter un éventuel déclencheur serveur dépendant du signal phonétique ou
d'une reconnaissance correcte de «Gboard». Inversement, le client Lite ne prouve
pas qu'un tel déclencheur existe côté S3. L'hypothèse d'une articulation différente
et de «Okay Gboard» a ensuite été testée dans le suivi ci-dessous, sans activation
d'un flag compose découvert. Aucune instruction locale Lite n'a été transplantée
dans les requêtes S3.

## Suivi SDK0.3.0 : nom reconnu avec et sans biais initial

Preuve expurgée :
[input-hints-public.json](../evidence/input-hints-public.json), version SDK
rapportée **0.3.0**. Le lot complet comporte10 sessions réussies sur10 ; les
**quatre cas d'invocation** ci-dessous en sont un sous-ensemble, pas dix nouveaux
essais de composition. Les intentions et membres actifs ont été vérifiés
directement dans les `final_updates` du rapport public : **2 /3 dans les quatre
cas**, avec un candidat par cas.

Les textes de synthèse emploient explicitement l'orthographe phonétique
«Hey, G board» et «Okay, G board». Chaque paire baseline/biais partage exactement
le même SHA-256 WAV, la langue en-US et le contexte tardif vide ; seul le biais
initial `bias_terms=["Gboard"]` varie. Plans et textes contrôlés :
[input-hints-validation.json](../../tools/fixtures/input-hints-validation.json) et
[input-hints-speech.json](../../tools/fixtures/input-hints-speech.json).

| Cas | Biais initial | Texte retourné | Intention / oneof |
|---|---|---|---|
| `invocation-hey-baseline-en` | Aucun | «Hey Gboard, write a short message to Alex saying I will arrive 20 minutes late.» |2 /3|
| `invocation-hey-biased-en` | `Gboard` | «Hey Gboard, write a short message to Alex saying I will arrive 20 minutes late.» |2 /3|
| `invocation-okay-baseline-en` | Aucun | «Gboard, what is the capital of France?» |2 /3|
| `invocation-okay-biased-en` | `Gboard` | «Okay, Gboard, what is the capital of France?» |2 /3|

Le nom **Gboard est effectivement reconnu** : la confusion «board/Hagbard» des
anciens essais n'explique donc plus à elle seule ces nouveaux résultats. Les deux
cas Hey et le cas Okay avec biais conservent même le préfixe complet dans le
texte retourné ; malgré cela, aucune réponse sur Paris ni aucun message composé
n'est produit. Le cas Okay sans biais omet toujours «Okay» : la fidélité du
préfixe n'est pas complète dans toutes les sorties.

**Conclusion bornée** : le confound ASR antérieur est partiellement levé, mais la
composition reste non observée dans ces phrases et ce profil S3. La présence
textuelle du préfixe ne prouve pas que le serveur l'a classé comme invocation,
ni qu'elle suffirait à déclencher une génération. Une observation par bras ne
permet pas non plus d'attribuer avec certitude le maintien de «Okay» au biais,
ni de généraliser un gain de reconnaissance. Aucun `INTENT_COMPOSE=4` n'est
observé ; aucune impossibilité générale de composition n'est établie. Aucun prompt
Lite, flag d'activation compose, changement d'authentification ou backend alternatif n'a
été utilisé dans ce suivi.

## Ce qui reste ouvert

- Le prédicat serveur exact déclenchant une composition, s'il existe pour un
  autre profil S3, n'est pas récupéré dans le client.
- Une génération de contenu pourrait être observée sous une intention autre que4 :
  comportement du contenu et valeur numérique doivent être rapportés séparément.
- Les gates et prompts Android Lite constituent des sources réelles, mais pas une
  validation de disponibilité sur un appareil ni un contrat natif portable.
- Ni une enum, ni un mock, ni ces petites matrices d'observations ne justifient l'affirmation
  universelle «Rambler ne sait jamais composer».
- Les essais ne changent rien à l'authentification, aux quotas, à l'attestation,
  au consentement ou à la politique d'édition. Aucun contournement n'est proposé.
