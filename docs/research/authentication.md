# Authentification et dépendances d'accès Rambler / PixelBoard

Date de l'analyse : 2026-11-12. Portée : client PixelBoard 1.3.4, transport S3 utilisé par la dictée agentique/Jetson. Cette note ne documente pas une API Gemini publique et ne propose pas de la substituer au service observé.

## Conclusions déterminantes

1. **L'accès anonyme n'est pas suffisant pour caractériser l'authentification.** Un appel gRPC sans metadata, avec un message vide et half-close, a reçu `PERMISSION_DENIED` (« Permission denied on the service. ») en environ 80 ms. Cela établit seulement le refus de cet appel incomplet, pas qu'un compte Google est obligatoire ni qu'une requête conforme serait refusée (`docs/evidence/anonymous-empty-rpc.json`).
2. **Le client attache systématiquement trois métadonnées d'application au canal S3** : `X-Goog-Api-Key`, `X-Android-Package` et `X-Android-Cert`. La valeur du package est `com.google.android.googlequicksearchbox`. La clé et l'empreinte sont des valeurs applicatives embarquées ; leurs valeurs ne sont volontairement ni reproduites ici ni enregistrées dans les logs. Ces éléments identifient/restringent potentiellement le client et son quota ; **ce ne sont pas, à eux seuls, une preuve d'authentification d'un compte utilisateur**.
3. **Le `auth_token` lié à Zwieback est un champ protobuf conditionnel, pas le header gRPC `Authorization`.** Dans le mutator observé, le gate est `jetsonVoiceInput` **et** `jetson_send_zb=true`. L'identifiant applicatif S3 séparé (`agentic_dictation_use_separate_s3_application_id`) ne conditionne pas ce gate : lorsqu'il est vide, le client utilise `gboard` tout en conservant la possibilité d'ajouter Zwieback. Les défauts dans cet APK sont `jetson_send_zb=false` et identifiant séparé vide.
4. **Le mécanisme Zwieback observé n'est ni OAuth ni un cookie.** Gboard demande à GMS Core un `PseudonymousIdToken`, puis calcule localement `hex_minuscule(SHA-256(identifiant_en_UTF-8))` et place ce résultat dans le champ d'authentification. Aucun endpoint Web d'émission de jeton, expiration, refresh token ou délai de renouvellement n'apparaît dans ce chemin.
5. **PixelBoard ne fournit pas de mécanisme d'authentification autonome.** Ses patches publics activent des capacités et des routes de l'application ; ils n'implémentent ni émission OAuth, ni cookies, ni compte, ni attestation, et ne modifient ni `jetson_send_zb` ni les flags de routing S3 examinés.
6. **Android n'est pas démontré comme obligatoire pour toute requête Rambler.** Le chemin par défaut observé n'exige pas le Zwieback. En revanche, si une configuration serveur légitime active la branche Zwieback et que le service l'exige, l'acquisition initiale observée dépend de GMS Core sur l'appareil de l'utilisateur. Aucun moyen public documenté et équivalent d'émettre cet identifiant hors Android/GMS n'a été trouvé. Il ne faut ni fabriquer un identifiant, ni emprunter celui d'un tiers.
7. **Aucune preuve de compte, cookie, OAuth ou Play Integrity n'a été trouvée dans le chemin S3 concret examiné.** Cette absence de preuve est limitée à ce chemin et à cette version ; elle ne prouve pas que le service n'imposera jamais un contrôle supplémentaire.

## Artefacts et références reproductibles

### Sources publiques PixelBoard

- Dépôt : <https://github.com/Akshayykadam/PixelBoard>
- Révision de travail inspectée : `80974b4614d15b4e80398f5400867aed366998bc`.
- Tag `1.3.4` : `9a81f8b03d1d26b2ebac07355362865f72452137` (« Bump version to 1.3.4 and update changelog », 2026-11-10).
- APK analysé : <https://github.com/Akshayykadam/PixelBoard/releases/download/1.3.4/PixelBoard-1.3.4.apk>
- SHA-256 de l'APK : `a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`.
- Politique Rambler publique : [`GboardRambler1803StockPolicy.java`](https://github.com/Akshayykadam/PixelBoard/blob/9a81f8b03d1d26b2ebac07355362865f72452137/pixelboard-patches/extensions/extension/src/main/java/com/akshaykadam/pixelboard/extension/rambler/GboardRambler1803StockPolicy.java), notamment lignes 38-41 et 52-110.
- Sélection Rambler publique : [`GboardRambler1803OfficialSelectionRuntime.java`](https://github.com/Akshayykadam/PixelBoard/blob/9a81f8b03d1d26b2ebac07355362865f72452137/pixelboard-patches/extensions/extension/src/main/java/com/akshaykadam/pixelboard/extension/rambler/GboardRambler1803OfficialSelectionRuntime.java).

Commandes de rattachement, sans exécution de code du dépôt :

```bash
git clone --no-tags https://github.com/Akshayykadam/PixelBoard.git
git ls-remote --tags https://github.com/Akshayykadam/PixelBoard.git refs/tags/1.3.4
sha256sum PixelBoard-1.3.4.apk
```

Sous Windows, le checkout du dépôt a nécessité `core.longpaths=true` dans la copie locale du scratchpad. Aucun script tiers n'a été exécuté.

### Sorties JADX partagées par l'analyse protocole

L'APK n'a pas été retéléchargé ni redécompilé pour cette note. Les chemins ci-dessous désignent la décompilation existante fournie par l'agent protocole ; les noms non obfusqués proviennent des chaînes de source conservées dans le bytecode.

- `pixelboard-jadx/sources/defpackage/iha.java` — `S3ConnectionUtil`, lignes 8-20 : création des trois headers et attachement au stub avec `abtq`.
- `pixelboard-jadx/sources/defpackage/abtq.java`, lignes 17-22, et `abtp.java`, lignes 17-21 : insertion des metadata dans les appels.
- `pixelboard-jadx/sources/defpackage/aams.java`, lignes 5-16 et 182-214 : hôte de production, méthodes et scopes déclarés.
- `pixelboard-jadx/sources/defpackage/aamk.java`, lignes 16-35 : méthode bidirectionnelle `S3Bidi`.
- `pixelboard-jadx/sources/defpackage/aamr.java`, lignes 25-29 : association de `S3Bidi` aux scopes déclarés.
- `pixelboard-jadx-knv-simple/sources/defpackage/knv.java` — reconstruction simple vérifiée : `L36`/`L40-L43` (lignes 132-137 et 316-324) établissent le gate Jetson, y compris lorsque l'identifiant séparé est vide et que l'application vaut `gboard` ; `L95` (lignes 229-254) vérifie `muq.D`, exige un future terminé avec succès, puis dérive et ajoute le jeton. La sortie `pixelboard-jadx-knv/sources/defpackage/knv.java` avec `--show-bad-code`, dont les branches étaient contradictoires, n'est plus utilisée pour déduire les conditions.
- `pixelboard-jadx/sources/defpackage/muq.java`, lignes 35-39 : flags et valeurs par défaut.
- `pixelboard-jadx/sources/defpackage/tjj.java`, lignes 8-58 : `ZwiebackFetcher`, client GMS, cache du future et timeout d'une seconde.
- `pixelboard-jadx/sources/defpackage/lwa.java`, lignes 10-16 : `PseudonymousId.API`.
- `pixelboard-jadx/sources/defpackage/lwc.java`, lignes 17-22 et 77-83 : appel GMS feature `3901`.
- `pixelboard-jadx/sources/defpackage/lfo.java`, lignes 72-82 : Binder `IPseudonymousIdService`.
- `pixelboard-jadx/sources/defpackage/lwd.java`, lignes 9-42 : `PseudonymousIdToken` contenant une chaîne.
- `pixelboard-jadx/sources/defpackage/tji.java`, lignes 7-24 : wrapper dont `toString()` vaut `Zwieback`.
- `pixelboard-jadx/sources/defpackage/wuh.java`, `wul.java` lignes 71-77 et `wue.java` lignes 45-53 : SHA-256 et encodage hexadécimal minuscule.
- `pixelboard-jadx/sources/defpackage/aalv.java`, lignes 25-33 : champs protobuf du jeton ; `aamc.java`, lignes 15-20 et 28-36 : liste d'authentifications dans l'extension de requête.
- `pixelboard-jadx/sources/defpackage/igz.java`, lignes 7-10 : hôte par défaut et flag `s3_auth_token_for_debugging` vide par défaut.

Préfixe absolu des sorties dans cette session :

```text
C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/
```

### Source publique indépendante sur Zwieback

Le proto Chromium antérieur au commit `7b9af2142151fbb4f38f3abd23ec87541492cf81` définit Zwieback comme « the signed-out session identifier » et précise que certains clients le transportent dans la bande plutôt que dans un cookie HTTP :

- Fichier à la révision `f9f096edfd9a3020626bc2fac9a351c91c150f20` : <https://chromium.googlesource.com/chromium/src/+/f9f096edfd9a3020626bc2fac9a351c91c150f20/components/feed/core/proto/v2/wire/chrome_client_info.proto>
- Diff qui retire ce commentaire : <https://chromium.googlesource.com/chromium/src/+/7b9af2142151fbb4f38f3abd23ec87541492cf81%5E%21/>

Cette source définit la nature générale de l'identifiant ; elle ne documente pas le serveur S3, son autorisation ou son acquisition par GMS.

## Chaîne d'accès observée

### 1. Transport et identification applicative

Cible de production observée :

```text
speechs3proto2-pa.googleapis.com:443
/speech.s3.S3StubbyClientConnectorService/S3Bidi
```

`iha.a()` construit les trois headers suivants, puis `iha.b()` les attache au stub :

| Élément | Observation | Ce qui n'est pas démontré |
|---|---|---|
| `X-Goog-Api-Key` | Valeur applicative embarquée, présente dans le canal S3 | Identité d'un utilisateur, consentement OAuth, accès illimité |
| `X-Android-Package` | `com.google.android.googlequicksearchbox` | Package réel de PixelBoard ou compte connecté |
| `X-Android-Cert` | Empreinte applicative embarquée | Attestation Play Integrity ou preuve fraîche de l'appareil |

La présence conjointe package/certificat est compatible avec une restriction d'API Android classique. Ce n'est pas une attestation générée à chaque session : les valeurs sont statiques dans le client analysé. Pour une interopérabilité autorisée, un client fidèle doit pouvoir fournir ces metadata sans les journaliser. Leur injection doit rester une configuration locale secrète, jamais une valeur copiée dans git. Si leur réutilisation n'est pas autorisée, il faut obtenir des credentials officiels propres à l'utilisateur plutôt que contourner la restriction.

### 2. `auth_token` Zwieback : émission locale conditionnelle

Le code récupéré effectue conceptuellement :

```text
si jetsonVoiceInput
et si jetson_send_zb == true
et si le future Zwieback est terminé avec succès :
    zb = GMS_Core.PseudonymousIdToken       # acquisition Android/GMS
    token = lower_hex(SHA-256(UTF8(zb)))     # dérivation locale
    authentication.type = 2                  # sémantique du nombre non établie
    authentication.auth_token = token
    request.user_info.auth.append(authentication)
```

Les noms `user_info` et `type` ci-dessus sont descriptifs, pas des noms proto confirmés. Le champ 2 (`aalv.f`) reçoit le hash ; le champ 4 (`aalv.c`) reçoit `2` ; l'objet est ajouté au champ répété 9 (`aamc.c`) de l'extension `UserInfo` numéro 293000 (`aamc.g`). Si le future n'est pas terminé, le client journalise « Zwieback ID not ready yet, skipping auth_token » et poursuit sans ce champ ; une exception mène au même comportement. Le code ne construit pas `Authorization: Bearer ...` et ne place pas ce hash dans un cookie.

**Point de configuration important :** lorsque l'identifiant S3 séparé est vide, le mutator utilise l'identifiant applicatif `gboard`, mais le gate Zwieback reste `jetsonVoiceInput && jetson_send_zb`. La condition supplémentaire « Jetson et identifiant séparé non vide et `muq.E` » concerne l'expérience de quota, pas l'ajout du jeton. Les patches PixelBoard ne forcent ni `agentic_dictation_use_separate_s3_application_id` ni `jetson_send_zb`. Les valeurs Phenotype/serveur effectivement reçues au runtime restent toutefois inconnues : les défauts locaux ne suffisent pas à exclure une configuration distante différente.

Le flag `s3_auth_token_for_debugging` existe, vide par défaut, mais la recherche JADX n'a trouvé aucun consommateur explicite hors sa déclaration. Il ne faut pas en déduire qu'il permet de remplacer le jeton ni l'utiliser comme mécanisme d'accès sans preuve.

### 3. Renouvellement et durée de vie

- La clé API et l'empreinte sont statiques : aucun renouvellement n'est associé dans le code observé.
- `ZwiebackFetcher` conserve un future de résultat et applique un timeout d'une seconde à l'appel GMS. Ce timeout est une borne d'attente locale, **pas une durée de validité du jeton**.
- La dérivation SHA-256 est déterministe : pour un même Zwieback, le hash est identique. Aucun timestamp, nonce, `expires_in`, refresh token ou rotation planifiée n'apparaît dans `knv`/`tjj`.
- La fréquence exacte de réacquisition GMS et les événements qui changent le Zwieback restent inconnus. Le hash et l'identifiant source doivent tous deux être traités comme des identifiants pseudonymes sensibles et ne doivent pas être journalisés.

### 4. Compte, OAuth, cookies et attestation : observations négatives bornées

`S3Bidi` déclare quatre scopes génériques : `assistant`, `assistant-sdk-prototype`, `googlenow` et `googleit`. Cette déclaration ne prouve pas qu'un flux OAuth ou un `AccountManager` est exécuté. Dans les classes du chemin concret (`iha`, `abtq`, `aamj`, `aamk`, `aamr`, `knv`, `tjj`), aucune acquisition de compte, cookie, bearer token ou attestation n'a été identifiée. Les occurrences génériques `authorization`/`cookie` dans des tables HTTP partagées ne sont pas des preuves d'usage.

Il n'a été trouvé ni endpoint d'émission/renouvellement Web, ni callback OAuth, ni appel Play Integrity/SafetyNet dans ce chemin. Une autre couche de GMS Core ou du serveur peut avoir ses propres contrôles, invisibles dans ce code client.

## Hypothèses encore ouvertes

| Question | État | Preuve nécessaire |
|---|---|---|
| Les trois metadata applicatives suffisent-elles avec une requête Rambler conforme ? | Non testé | Une session valide à faible volume, avec schéma confirmé et statut serveur |
| Un compte Google connecté est-il nécessaire ? | Aucune preuve | Comparaison de sessions légitimes ou documentation/serveur explicite ; ne pas l'inférer du seul `PERMISSION_DENIED` anonyme |
| Les flags distants activent-ils Zwieback ? | Inconnu | Valeur runtime de `jetsonVoiceInput` (`kmy.n`) et valeur expurgée de `muq.D`, ou trace du mutator sans valeur d'identifiant ; `muq.C` renseigne seulement l'identifiant applicatif |
| Le serveur exige-t-il `auth_token` lorsque la branche est active ? | Inconnu | Réponse à une requête valide dans cette configuration, sans fabriquer de jeton |
| Le champ d'authentification est-il accepté hors GMS après provisionnement légitime ? | Inconnu | Réutilisation contrôlée du seul identifiant de l'utilisateur, si autorisée, sans relais Android à l'exécution |
| Une attestation supplémentaire est-elle exigée côté serveur ? | Non observée côté client | Erreur serveur explicite ou capture autorisée d'une session légitime ; aucune tentative de contournement |
| Quelle est la sémantique de `aalv.c = 2` ? | Inconnue | Proto source ou enum correspondant ; ne pas inventer « OAuth » ou « Zwieback » sans preuve |

## Accès utilisateur potentiellement nécessaire

- **Pour le premier test fidèle :** autorisation explicite d'effectuer une requête réelle unique et d'utiliser un court échantillon audio synthétique ou fourni avec consentement. Aucun compte n'est demandé à ce stade sur la seule base du refus anonyme.
- **Pour les credentials applicatifs :** une configuration locale autorisée, injectée sans affichage ni commit. Ne pas copier de jeton privé ou de credentials d'un tiers.
- **Seulement si Zwieback est réellement actif :** le PseudonymousIdToken du propre appareil GMS de l'utilisateur, obtenu par le mécanisme normal du client, potentiellement comme provisionnement initial ponctuel. Le besoin d'un appareil Android dans ce cas ne doit pas être généralisé au mode par défaut. Ne pas extraire la base de comptes Android et ne pas utiliser l'identifiant d'une autre personne.
- **Si une session PixelBoard de référence devient nécessaire :** utiliser le Xiaomi proposé uniquement lorsqu'il est libre, avec action et preuve attendue définies à l'avance. Le téléphone servirait à observer une session de développement, jamais de relais obligatoire pour le moteur final.

## Mini-expériences réseau non destructives

Ces étapes sont des propositions ; elles n'ont pas été exécutées par cette analyse. Le coordinateur doit les séquencer après confirmation du schéma, à très faible volume, sans rafales ni variantes d'authentification au hasard.

### Étape 0 — test local sans réseau

Avec une valeur factice publique telle que `zwieback-test-fixture`, vérifier que l'implémentation produit exactement le SHA-256 UTF-8 hexadécimal minuscule et sérialise `aalv.c = 2`, `aalv.f = hash` dans le bon champ. Ne jamais utiliser une vraie valeur dans une fixture ou un test commité.

### Étape 1 — une requête valide dans la configuration par défaut

Construire la requête Rambler minimale complète selon le schéma confirmé, avec :

- les trois metadata applicatives légitimement configurées ;
- l'identifiant applicatif `gboard` si l'identifiant séparé est vide ;
- les routing keys et paramètres réellement observés pour Jetson ;
- aucun `auth_token` si `jetson_send_zb` est faux ;
- un court audio de test autorisé, puis half-close ; aucun retry automatique.

Consigner uniquement statut, détail non sensible, latence, nombres de messages/octets et hash de configuration expurgée. **Preuve attendue :** distinguer un refus d'accès d'une erreur de schéma et déterminer si le chemin par défaut fonctionne sans compte/Zwieback. Un nouveau `PERMISSION_DENIED` après requête valide serait significativement plus informatif que le test anonyme vide, mais ne justifierait toujours pas une tentative de contournement.

### Étape 2 — référence PixelBoard seulement si l'étape 1 échoue

Sur l'appareil utilisateur libre et avec son accord, exécuter une seule dictée Rambler connue dans PixelBoard, sans proxy TLS ni affaiblissement de sécurité. Collecter les événements `GboardS3RMutator`/`NewS3Recognizer` expurgés et le résultat fonctionnel ; ne pas enregistrer audio, texte, clé API, certificat, Zwieback ou hash. **Preuve attendue :** établir si cette version/config précise fonctionne et si la branche « skipping auth_token » est empruntée. Cela ne remplace pas une capture autorisée des metadata si le désaccord persiste.

### Étape 3 — branche Zwieback uniquement sur configuration légitimement observée

Ne pas activer artificiellement `jetson_send_zb` pour explorer le serveur. Si la configuration normale de l'utilisateur montre déjà `jetsonVoiceInput=true` et `jetson_send_zb=true`, reproduire une requête valide avec son propre jeton dérivé, injecté en mémoire, que l'identifiant applicatif soit `gboard` ou un identifiant séparé. Comparer au plus une session à la référence légitime et arrêter après un refus explicite. **Preuve attendue :** validité de la dérivation et caractère suffisant/insuffisant du champ ; aucune rotation ou expiration ne doit être supposée sans réponse ou observation temporelle.

## Critères d'arrêt et règles de confidentialité

- Ne pas tester des clés, comptes, cookies, tokens ou empreintes provenant de logs publics ou d'autres utilisateurs.
- Ne pas substituer l'API Gemini publique au protocole S3/Rambler observé.
- Ne pas transformer les flags PixelBoard qui court-circuitent des vérifications client de quota en prétendue autorisation serveur.
- Ne pas désactiver de vérification TLS, attestation ou contrôle d'accès.
- Ne pas journaliser les trois valeurs de credentials, le Zwieback source, son hash, l'audio ou le texte utilisateur. Les erreurs doivent être expurgées avant stockage.
- Après un refus d'accès sur requête conforme, privilégier une voie d'accès officielle/autorisée et l'analyse de configuration ; ne pas lancer de fuzzing d'authentification.
