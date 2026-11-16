# Protocole S3 / Rambler — PixelBoard 1.3.4

État : analyse statique vérifiée sur l'APK publié, sans exécution de l'APK ni des sources tierces. Aucun test réseau effectué par cet analyste. Les noms descriptifs ci-dessous sont des alias de travail, pas des noms `.proto` retrouvés. **Le protocole ne se réduit pas à une API de transcription : Rambler choisit un service/routage Agentic et transporte du contexte d'édition.**

## 1. Provenance et reproductibilité

- Release : <https://github.com/Akshayykadam/PixelBoard/releases/tag/1.3.4>.
- APK : `PixelBoard-1.3.4.apk`, 127650866 octets, SHA-256 **`a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`**.
- Notes release : base Gboard `18.6.3.1002251336-release-arm64-v8a`.
- Archive sources tag : racine `Akshayykadam-PixelBoard-9a81f8b` ; l'APK, et non les patches d'une autre version, est l'autorité sur les numéros wire.
- Outil : JADX 1.5.3, Java 17, `--no-res`, 26 erreurs de méthodes lors de la passe générale ; récupération ciblée des méthodes importantes. ZIP JADX SHA-256 `8280f3799c0273fe797a2bcd90258c943e451fd195f13d05400de5e6451d15ec` ; archive source SHA-256 `156146ba76c27fac8b8717e54a5ef5a2772bc414c6e21906e3791515f6c86dff`.
- Vérification indépendante des métadonnées générées : `C:/Users/FSOS/Documents/project/Rambpc/tools/research/jadx_metadata.py`. Exécuter `python -I` depuis le dépôt avec les chemins Java en arguments. Les schémas centraux présentés ci-dessous ont été décodés sans reste de métadonnées ; ce script ne charge ni n'exécute leur code.
- Dossier artefacts partagé : `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad`.
- APK : `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-apk-download/PixelBoard-1.3.4.apk`.
- Décompilation générale : `C:/Users/FSOS/AppData/Local/Temp/claude/C--Users-FSOS-Documents-project-Rambpc/6a88b7b9-7237-4fb1-9c07-58a4c0a8ca3e/scratchpad/pixelboard-jadx/sources`.
- Récupérations ciblées : même scratchpad, `pixelboard-jadx-ihv/sources/defpackage/ihv.java` (init), `pixelboard-jadx-knv/sources/defpackage/knv.java` (mutateur), `pixelboard-jadx-ihm/sources/defpackage/ihm.java` (réponses). Contrôle de flux bas niveau : `pixelboard-jadx-knv-simple/sources/defpackage/knv.java` et `pixelboard-jadx-ffh-simple/sources/defpackage/ffh.java` (`--decompilation-mode simple --no-inline-methods --no-inline-anonymous`).

Les références `classe.méthode` ci-dessous visent `sources/defpackage`, sauf précision. `aaej.T(extendee, default, default, N, MESSAGE)` établit directement le numéro d'extension. Les chaînes de métadonnées `new aafx(...)` établissent numéros et types des champs ; **les masques `b |= 524288`, etc. sont des bits de présence Java, pas des numéros protobuf**.

Avertissement : les sorties ciblées `--show-bad-code` ont des conditions manifestement incorrectes/inversées. Les champs/valeurs des affectations sont exploitables, mais une condition douteuse n'est pas considérée vérifiée. Lignes JADX = lignes d'un artefact généré, pas lignes du source Google.

## 2. Transport et métadonnées

**Vérifié statiquement** :

- Service : `speech.s3.S3StubbyClientConnectorService`.
- Méthode bidirectionnelle : `S3Bidi`, chemin gRPC `/speech.s3.S3StubbyClientConnectorService/S3Bidi`.
- `aamk.b()` lignes 22–30 : type bidi, marshaller requête **`aama`**, réponse **`aamb`**.
- Domaine présent dans l'APK : `speechs3proto2-pa.googleapis.com`, port 443. Le chemin est du gRPC binaire, pas REST Speech-to-Text ni une API Gemini publique.
- `iha.a()` / `iha.b()` (`S3ConnectionUtil`) attachent effectivement au stub les headers **`X-Goog-Api-Key`**, **`X-Android-Package`**, **`X-Android-Cert`**. Le package déclaré est `com.google.android.googlequicksearchbox`, et non le package PixelBoard. **Valeurs de clé et empreinte non reproduites ici, aucun secret trouvé utilisé.**
- Ces métadonnées applicatives ne prouvent pas à elles seules OAuth/compte utilisateur/attestation. Leur acceptation générale et leurs restrictions serveur restent inconnues ; les essais natifs des sections9–10 établissent leur acceptation dans la configuration testée.
- `knv.a()` peut ajouter séparément des informations utilisateur Zwieback ; acquisition par `tjj` (`ZwiebackFetcher`) via GMS. Les flags de base `muq.C` (`agentic_dictation_use_separate_s3_application_id`) et `muq.D` (`jetson_send_zb`) ont respectivement défaut vide et false. Cela ne prouve pas leur valeur effective après configuration/patch.

**Inféré du gRPC standard, non capturé sur appareil** : HTTP/2 + TLS, POST, `content-type: application/grpc`, `te: trailers`, enveloppe message 1 octet de compression + longueur big-endian32 + protobuf. Ne pas confondre ce cadrage avec un préfixe protobuf varint.

## 3. Enveloppes S3

### S3Request = `aama`

Métadonnées `aama.a(int,Object)` ligne 31 :

| Champ | Type wire | Membre Java | Usage établi |
|---|---|---|---|
| 1 | string / 2 | c | service choisi par `ihv.a()` |
| 2 | bool / 0 | d, défaut **true** | explicitement true dans init et fin ; nom sémantique original non retrouvé |
| 3 | bool / 0 | e | marqueur final/end-of-data dans `iho.q` |
| extensions | essentiellement messages / 2 | inherited `aaeh.w` | audio, paramètres, contexte |

La requête terminale statique `iho.q` contient **field2=true et field3=true**. Son protobuf est exactement **`10 01 18 01`** ; `iho.g()` l'envoie puis appelle `onCompleted()` côté client (half-close). Ce n'est ni un message vide ni uniquement field3=true.

### S3Response = `aamb`

Métadonnées `aamb.a(...)` ligne 32 :

| Champ | Type | Membre Java | Usage |
|---|---|---|---|
| 1 | enum / varint | c | statut S3, distinct du statut gRPC |
| 2 | int32 / varint | d | code d'erreur S3 |
| 3 | string | e | détail erreur S3 |
| extensions | messages | inherited `aaeh.w` | reconnaissance, heartbeat, etc. |

Valeurs statut **vérifiées** par `a.aw(int)` lignes825–835 et `ihm.c()` ciblé lignes86–108 : **0=en cours/normal, 1=fin de reconnaissance, 2=erreur, 3=NOT_STARTED**. Le dispatcher traite séparément ces branches. Une réponse protobuf peut être valide malgré un statut S3 non réussi ; inversement `grpc-status != 0` ne doit pas être traité comme une réponse S3.

## 4. Extensions de requête et config initiale

### AudioInfo = `aalt`, extension S3Request **293100**

`aalt` static ligne 18 ; métadonnées ligne 28 :

| Champ | Type | Usage établi |
|---|---|---|
| 2 | **float32**, wire5 | sample rate ; init **16000.0f** |
| 3 | enum, wire0 | encoding ; chemin observé **10** |
| 4 | int32, wire0, défaut1 | nombre de canaux ; appel NewS3 **1** |
| 9 | int64, wire0 | temps écoulé en millisecondes (`wba.c().toMillis()`) |

`ihv.a()` ciblé lignes 106–125/147–166 écrit ces valeurs. **Encoding10 correspond au chemin Ogg/Opus**, pas à PCM brut : `iho.j()` utilise `ihp` (`OggOpusInputStream`), qui appelle `com.google.audio.hearing.common.OggOpusEncoder`.

Une enum d'encodage peut admettre d'autres valeurs sans que le backend Rambler les accepte. Aucune valeur PCM utilisable n'est encore établie par ce rapport ; ne pas inventer `LINEAR16=1` à partir de Cloud Speech.

### AudioData = `aals`, extension S3Request **293101**

`aals` static ligne 15 et métadonnées ligne25 : **field1 bytes**. Les octets sont des morceaux d'un **flux Ogg/Opus continu**. `iho.j()` lit normalement jusqu'à **224 octets** par requête ; ce découpage n'est pas une frontière de paquet Opus et n'implique pas un fichier Ogg séparé à chaque message.

Les messages audio réels ajoutent aussi extension293100 avec **uniquement field9 elapsed_ms**, et non toute la config audio (`iho.j` lignes 342–360). `iho.i()` découpe le chemin batch en blocs8192, donc 224 n'est pas une constante normative du protocole.

### RecognizerInfo = `aapf`, extension S3Request **294500**

`aapf` static ligne29 ; métadonnées ligne44. Config du mutateur `knv.a()` :

| Champ | Type | Valeur/usage |
|---|---|---|
| 1 | message `aakn` | sous-config, **field4 bool=false** (`knv` lignes95–116) |
| 5 | bool | true |
| 7 | int32, défaut2 | **blockOffensiveWords** (`kmy.h`) : true→2, false→0 |
| 13 | bool | true |
| 20 | bool | true |
| 40 | bool | flag `sba.s` **enable_s3_auto_punctuation**, défautfalse |
| 41 | bool | true |
| **50** | **string** | routing key principal |
| **64** | string | routing secondaire optionnel |
| 91 | message `aapa` | field2 bool = absence du composant **AdultAccount**, via `!qfl.e(mtd.d)` ; field4 bool existe mais n'est pas écrit ici |

**Rambler / Jetson** : `knv.a()` lignes117–135 met field50 au flag `muq.A`, dont la valeur de base vérifiée est **`gboard_gemini_v3_streaming_voice_edit_mul`** (`muq.java` ligne35). Le flag est bien `jetson_experiment_recognizer_routing_key`. Ce n'est pas un header HTTP. Les autres reconnaisseurs utilisent `sba.r` ou omettent ce champ.

### ClientInfo = `aaly`, extension S3Request **294000**

`aaly` static et métadonnées :

| Champ | Type | Usage mutateur |
|---|---|---|
| 2 | string | application id : `gboard`, ou flag Agentic distinct non vide |
| 8 | string | plateforme `Android` |
| 9 | string | `Build.DISPLAY` |
| 10 | string | version majeure.mineure de l'application, fallback `-1` |
| 11 | string | `Build.MODEL` |
| 16 | string | trigger application id (`kmy.d`) |
| 17 | string | usage non établi |
| 22 | message `aalw`, avec présence | field1 BOOL, également avec présence : `kmy.m` / `supervisionEnabled` ; signal d'état de compte, voir ci-dessous |
| 26 | enum | valeur1 affectée par le mutateur |

Ces valeurs décrivent le client Android observé ; aucun besoin établi de falsifier modèle/appareil pour un port natif.

### UserInfo = `aamc`, extension S3Request **293000**

- field2 message `aalx` = locale principale ; `aalx.field1 string` (tag langue), field2 int32.
- field4 repeated `aalx` = locales secondaires.
- field9 repeated `aalv` = tokens/informations utilisateur.
- field20 bool = absence du composant **UnmanagedAccount**, via `!qfl.e(mtd.a)` (`mtd` lignes20–23, `knv-simple` lignes188–192). Cela encode une restriction/absence de composant local, pas un consentement à la donation.
- `aalv` : field1 string, **field2 string auth_token**, **field4 int32 type** (mutateur Zwieback écrit2), field7 bool.

`knv.a()` n'ajoute le token que pour **jetsonVoiceInput=true ET muq.D=true**, avec futur Zwieback déjà terminé et réussi. **C non vide n'est pas nécessaire** : `C=""` garde seulement application id `gboard`. Preuve de contrôle de flux résolue par la passe `knv-simple` (lignes132–137,193–195,229–258,316–324), qui corrige les branches inversées du premier pseudo-Java. Le hash est **hex minuscule de SHA-256(UTF-8(identifiant Zwieback))**, placé dans `aalv.field2`, avec `aalv.field4=2` ; `wuh.a -> wul()` choisit SHA-256 et `wue.toString()` écrit l'hex minuscule. Futur non terminé ou exception : token omis. Aucune valeur d'identifiant n'a été utilisée ou conservée dans ce rapport. Voir aussi `authentication.md`.

### Autres extensions init

- **29735924** -> `aamh` : field1 int32 = **500** dans `knv.a()` ; cadence/timeout vraisemblable mais nom sémantique non établi.
- **47096292** -> `aalz` : conteneur extensible de journalisation/expériences.
- Extension interne de `aalz` **229601060** -> `aamg`, repeated int64 field1. Une branche de `knv` ajoute un identifiant d'expérience. Ce n'est pas nécessairement une exigence d'interopérabilité ; ne pas reproduire une expérience de quota pour modifier des contrôles serveur.
- Donation : extension **307792803** -> `aame`, ajoutée par `saz.a()` seulement si opt-in et conditions locales. **Le chemin Jetson du mutateur passe false**, donc `saz.a()` retourne sans ajouter cette extension (`knv-simple` lignes196–228 ; `saz` lignes25–35). Ne pas ajouter une déclaration de consentement par supposition.

Les champs RecognizerInfo7/40/91, ClientInfo22/26 et UserInfo20 sont explicitement écrits par le chemin observé ; certains dépendent de l'état local. Leur omission n'est pas une reproduction statique exacte. `kmy.toString()` lignes67–68 confirme les propriétés `blockOffensiveWords`, `supervisionEnabled` et `jetsonVoiceInput`.

### Signaux d'état de compte : non-parité native volontaire

Précision statique du 2026-11-16, limitée à l'APK identifié en §1 :

- **ClientInfo22 → `aalw.field1`** : message et BOOL ont chacun une présence
  (`aaly.java:25,43`, `aalw.java:23`). `kla.s` initialise le paramètre à false
  (`kla.java:240`), puis le met à true si le composant **UnsupervisedAccount**
  n'est pas positivement prêt (`kla.java:264–265`, `mtd.java:21`). Le contrôle
  `hky.java:17–36`, étiqueté **MinorsChecker**, lie ce signal à l'état de compte
  mineur/supervisé ; ce n'est pas un commutateur de fonctionnalité.
- Le paramètre passe par `kmw.i` → `kmw.l` → `kmy.m`
  (`kmw.java:47,117–119`, `kmy.java:68`). Pour l'init Rambler sans ClientInfo
  préexistant, le mutateur écrit la présence et la valeur, **y compris false**
  (`knv-simple:128–129,344–355`, appels `ihv.java:125,166`). La sortie `ihv`
  porte un avertissement JADX ; le contrôle bas niveau `knv-simple` est la
  référence pour cette branche.
- **RecognizerInfo91.2** = absence de **AdultAccount** et **UserInfo20** =
  absence de **UnmanagedAccount** sont deux autres signaux d'état de compte
  (`knv-simple:115–125,188–192`), pas des consentements ni des options libres.

Le client Rust sans fournisseur légitime d'état de compte **omet volontairement
ces trois signaux**. Aucune API true/false ni option CLI n'est ajoutée. Android
émet explicitement true pour la supervision lorsque son état est inconnu ;
l'omission native est donc une divergence, **pas une reproduction de ce défaut**.
Une absence protobuf ne doit pas être décrite comme une assertion false.
Le traitement serveur de l'absence, de true et de false reste inconnu, notamment
sans compte/Zwieback. Cette analyse ne prouve ni son comportement de sécurité ni
une activation COMPOSE ; aucun essai serveur deviné ne la complète.


## 5. Contexte d'édition : extension **1203**

InputBoxInfo = `aamo`, extension S3Request **1203**, static ligne22. Ce n'est **pas** un champ JSON libre.

| Champ | Type | Usage |
|---|---|---|
| 1 | string, oneof | texte précédant le curseur (ou sélection selon flag) |
| 3 | repeated string | mots de biais / contacts |
| 4 | message `aalu`, même oneof que1 | contexte avec curseur |
| 5 | repeated string | préférences utilisateur / consignes |

`aalu` = texte en trois parties : **field1 string=avant**, **field2 string=sélection**, **field3 string=après**. Preuve directe `iia.a()` ; les chaînes ne sont pas des indices de curseur.

`ihv.a()` peut mettre le contexte dans l'init pour certains modes. **Chemin Rambler observé : contexte transmis tardivement après l'audio** :

1. `ffh.fK()` / `AgenticDictationExtension.onListeningStop` récupère le contexte au moment de l'arrêt micro (lignes1186–1213).
2. `knt.b()` installe un fournisseur `knr` pour Jetson ; `kno.call()` récupère les textes courants.
3. `iho.a()` attend le fournisseur jusqu'à2s ; `gwp.apply`, case12, appelle `iho.b(context,cursorAware)`.
4. `iho.b()` construit une nouvelle requête contenant **uniquement extension1203** ; `gwp` l'envoie puis `iho.g()` envoie la fin et half-close.

Donc **ne pas fermer le flux juste après le dernier audio si du contexte reste à envoyer**. Le choix `aamo.field1` contre `.field4` dépend de l'activation cursor-aware (`muq.P`, défautfalse) ; quand false, `muq.Q` peut choisir seulement le texte sélectionné.

**SDK 0.3 — hints init.** `ProtocolConfig.bias_terms` et `.preferences` écrivent
respectivement les champs répétés 3 et 5 de 1203 dans la requête initiale, et
`.secondary_languages` écrit les locales répétées `293000.field4`. Le texte
avant/sélection/après reste envoyé séparément après l'audio. L'examen statique du
fallback `ihv.java` lignes 153–211, de `knv-simple/knv.java` lignes 259–309 et
`pixelboard-jadx/sources/defpackage/zxh.java` lignes 243–255 appuie ce placement et
la construction des locales ; les chemins complets des sorties sont donnés §1.
Leur présence n'établit pas leur efficacité générale. Les requêtes des dix essais
[hints](../evidence/input-hints-public.json) ont abouti ; effets et limites sont
rapportés dans [raffinement](../refinement.md). Les bornes sont celles du SDK, pas
des limites serveur connues ; les goldens init sans options restent inchangés.

## 6. Séquence minimale fidèle

```text
TLS/HTTP2 + metadata applicative légitime
S3Bidi(
  S3Request {
    1: "beyond-a2a-recognizer", 2:true,
    [293100]: {2:float(16000), 3:10, 4:1, 9:elapsed_ms},
    [294500]: {..., 50:"gboard_gemini_v3_streaming_voice_edit_mul"},
    [294000]: ClientInfo, [293000]: UserInfo,
    [29735924]: {1:500}, autres paramètres selon chemin
  }
  S3Request {[293101]:{1:ogg_chunk}, [293100]:{9:elapsed_ms}} ...
  S3Request {[1203]:{1:preceding_text}}  # OU {4:{1:before,2:selected,3:after}}
  S3Request {2:true,3:true}             # protobuf 10 01 18 01
)
client half-close ; continuer lecture réponses/trailers
```

**Vérifié** : `ihv.a()` choisit service **`beyond-a2a-recognizer`** quand `iic.d` (Jetson) ; sinon **`multi-recognizer`** si multilingue, sinon **`recognizer`**. Cette sélection + le routage field50 différencient Rambler d'une simple transcription S3. L'init n'est donc pas un `RecognitionConfig` de Google Cloud Speech.

Ce squelette était initialement une reconstruction statique. **Le sous-ensemble
implémenté dans `src/rambler/protocol.py` est désormais accepté par le service**
(voir sections9–11 et preuves natives). Les champs marqués `...` ne sont pas tous
émis ni déclarés indispensables ; le besoin exact de chaque option n'a pas été
établi par ablation. Une réussite dans ce profil ne garantit pas les autres
consentements, flags ou configurations serveur.


## 7. Réponses et résultats Agentic

### Extension principale S3Response **1253625** = `aapd`

`aapd` static, métadonnées `a(int,Object)` : **un seul oneof** contient les alternatives messages suivantes : `1:aaln`, `2:aalf`, `5:aalc`, `6:aale`, `7:aalk`, `8:aapc`, `9:aald`, `10:aapb`, `11:aahy`, `12:aalr`, `13:aaph`, `15:aalh`, `16:aahx`, `18:aall`, `19:aalq`, `20:aapg`, `21:aalb`. Les fields **3 string `d`** et **4 string `e`** sont indépendants, hors du oneof. Une alternative inconnue peut donc remplacer la variante1/2 : respecter la sémantique protobuf, ne pas garder toutes les variantes simultanément.

- field1 message **`aaln`** = résultats de reconnaissance.
- field2 message **`aalf`** = événement de parole.
- field3 string `d` = identifiant associé au résultat (nom exact inconnu).
- field4 string `e` ; autres alternatives oneof non exhaustivement analysées.

**Résultat final texte** :

```text
S3Response[1253625]
  .field1 (aaln)
  .field3 (aalp)
  .field3 repeated (aalg)
  .field1 string = texte hypothèse
  .field2 float32 = score/confiance
```

`aaln.field1 enum` représente l'état de reconnaissance (**0 intermédiaire, 1 final** dans les branches du dispatcher). `aaln.field3` (résultat segment final) et `aaln.field4` (partiel) sont **indépendants**, hors oneof. `aaln.field4 (aalj).field1 repeated(aali)` fournit les partiels : `aali.field1 string` texte, `aali.field2 double` stabilité/confiance. `ihm` concatène les parties et utilise seuil0.9 pour les distinguer. Un final de segment peut arriver avant la fin globale : ne pas assimiler présence field3 à half-close.

Champs supplémentaires vérifiés, même si pas nécessaires au premier parseur :

- `aaln` : field5 `aalp`, field7 `aalp`, field10 `aalm`.
- `aalp` : field3 repeated `aalg`, field5 `aalo`.
- `aalg` : field1 string, field2 float32, fields3/4/10/21 `aala`, field6 `aakl`, field30 enum.
- `aalj` : field1 repeated `aali`, fields4/5/10 `aala`.

Les messages `aala`/`aakl` et l'enum field30 ne sont pas sémantiquement reconstruits ici. Absence de score sur une hypothèse valide est possible ; préserver présence séparément du défaut0. De même, statut S3 absent signifie valeur protobuf par défaut0, tout en restant absent sur le wire.

`aalf.field1 enum` porte **0=start-of-speech, 1=end-of-speech, 3=end-of-utterance** (`ihm.c` lignes124–139 + `a.aw`). La valeur2 est admise par le validateur mais n'entraîne pas de callback dans cette branche.

### VoiceEditingResult = extension **74956224** de chaque hypothèse `aalg`

Ce n'est **pas** une extension directe de S3Response. `aamq` static ligne24, métadonnées ligne36 :

| Champ | Type | Usage |
|---|---|---|
| 1 | string | transcript nommé par le client ; **pas nécessairement verbatim audio**, voir section10 |
| 2 | string, oneof | alternative texte, signification exacte à confirmer |
| 3 | string, même oneof | edited history text |
| 4 | enum | intention utilisateur |
| 7 | `aalu`, même oneof | edited history text avec avant/sélection/après |

`ihm.c()` ciblé contient `VoiceEditResultHelper.maybeGetVoiceEditedText` : pour certaines intentions, utilise field3 ou concaténation des trois parties field7 au lieu du texte ASR. Le transcript avec marqueurs `<cursor>`/`</cursor>` peut être normalisé. Cette branche comporte des défauts de contrôle JADX ; la cartographie wire, elle, provient de `aamq.a(...)`.

Enum **`aamp`**, valeurs **vérifiées** :

```text
0 DEFAULT_INTENT_DICTATION
1 INTENT_EDIT
2 INTENT_EDIT_HISTORY
3 INTENT_REWRITE
4 INTENT_COMPOSE
5 INTENT_RAMBLE
6 INTENT_RAMBLE_HISTORY
```

### Types locaux de l'extension Agentic (ne pas les confondre avec le wire S3)

`aaks` = VoiceText local, repeated **field1 `aakw`** (spans).

`aakw` : field1 string texte, **field3 bool finalisé**, field4 string id, field5 repeated annotations `aaku`, field6 `aakr`, field7 bool résultat d'édition, **field8 `aakv` VoiceAction**, field9 double confiance, field10 bool.

`aakv` : **field1 enum intention**, **field4 string transcript**, **field5 `aalu` texte produit avec curseur** (les diffs sont calculés localement).

`ffh.j()` (`AgenticDictationExtension.onVoiceText`, lignes670–804) vérifie **exactement un span**, finalisé et avec VoiceAction ; les partiels ne sont pas appliqués par ce chemin. Puis traite sécurité, actions simples, contenu édité et contexte. Une réimplémentation doit distinguer texte produit, transcript brut et action d'édition ; ne pas coller aveuglément chaque hypothèse.

**Modification PixelBoard vérifiée** : `ffh.j()` ligne676 appelle `GboardRamblerDictationRuntime.filterVoiceText(this, aaks)` **avant** les validations. Le runtime source préserve notamment des transcripts, réécrit certaines intentions COMPOSE en dictation et supprime certains diffs. Ce filtre client modifie le rendu ; ce n'est pas le comportement brut du backend Rambler.

### Autres extensions réponse

- **29737259** -> `aami`, message vide : heartbeat reconnu par `ihm.c`.
- **171993426** -> `aape`, field2 string et field4 string : journalisation/diagnostic.
- **443278602** -> `aamf`, field1 bool : consentement/donation réponse (`aamf` static ligne15).

### Texte intégral, diffs locaux et unités de curseur

**Vérifié** : `aamq.field3` / `.field7` décrivent du **texte d'historique produit** ; `VoiceAction.field5` contient trois chaînes, pas un patch numérique distant. `ffh.E()` recompose `before+selected+after` puis `AgenticDictationTextEditor` génère localement une liste de diffs entre texte courant et texte produit (`rfw.b`, `ffh-simple` lignes141–145,261–301). Des corrections locales de blancs/retours à la ligne et de contexte ont lieu (`ffj.e`, `ffq.b/g`). Ne pas traiter ce texte comme un simple delta à ajouter.

**Unités vérifiées : UTF-16 Java.** `ffq.d()` lignes64–74 calcule la sélection à partir de `aalu.c.length()` et `aalu.d.length()`, puis `owj.F(start,end)` ; `ffq.g()` ligne94 utilise `before.length()`/`after.length()` pour le remplacement. `ffh-simple` emploie `String.length`, `charAt`, `subSequence` et des offsets de même unité. Ni UTF-8 bytes ni indices Python `str`. Exemple : `"A\U0001F600B"` (notation Python) a longueur Java4, codepoints Python3. Aucun offset numérique wire n'a été établi dans les messages contexte/VoiceEditingResult ci-dessus ; le port doit garder les trois chaînes et ne convertir en ranges qu'avec une convention explicite.

## 8. Vérifié, inféré, inconnu — conséquences architecture

**Vérifié statiquement** : service bidi, enveloppes et extensions ci-dessus ; service Jetson distinct ; routage protobuf field50 ; Ogg/Opus16k mono sur le chemin NewS3 ; contexte tardif ; fin+half-close ; modèle d'intentions et filtre PixelBoard.

**Inféré** : certains noms descriptifs de paramètres, sémantique des booléens non nommés et du500 ; absence d'une contrainte Android dans le wire lui-même. La présence de dépendances Android dans le client n'établit pas une exigence du backend.

**Inconnu / à valider avant moteur produit** :

1. Valeurs effectives des flags après patches/config distante ; noms exacts de tous les champs.
2. Durabilité/restrictions serveur des metadata et besoin éventuel de Zwieback dans d'autres profils ; le profil testé fonctionne effectivement avec les trois metadata, sans Zwieback.
3. Encodage PCM accepté : pas établi ; le chemin réel est Ogg/Opus,10.
4. Mapping sémantique complet des alternatives de résultat et des types d'édition ; couverture des différentes intentions au-delà du helper partiel récupéré. Les statuts0/1/2/3 sont, eux, établis.
5. Liste minimale de paramètres/consentements indispensable au serveur, erreurs applicatives/quotas et stratégies de reconnexion.
6. Il manque une capture Android pour comparer la parité avec l'application et une couverture fonctionnelle large. Les sélections en trois parties, y compris dans un contexte non-BMP, sont maintenant exercées nativement (sections11–12), sans comparaison sur appareil. La sélection d'un caractère non-BMP lui-même n'a pas été testée au service.

**Décision recommandée** : garder transport bidi, codecs protobuf et politique d'édition séparés ; parser/conserver les extensions inconnues ; ne pas appeler « Rambler fonctionnel » une simple transcription S3. Aucun moteur n'a été implémenté par l'analyste statique.

## 9. Première validation réseau native du coordinateur

**Observation fournie par le coordinateur, JSON relu indépendamment. Les octets de cette première session seule n'ont pas été conservés ; le redécodage indépendant porte sur les33 réponses suivantes, section10.** Preuve : `C:/Users/FSOS/Documents/project/Rambpc/docs/evidence/reconstructed-default-session.json`, horodatage `2026-11-12T20:10:49.907227+00:00`.

- Cible/méthode ci-dessus ; service **beyond-a2a-recognizer**, routage **gboard_gemini_v3_streaming_voice_edit_mul**.
- Metadata applicative observée injectée en mémoire par le coordinateur, aucune valeur consignée ; **pas d'OAuth, pas de Zwieback** sur cette session.
- Client natif, plateforme déclarée **Python**, application id `gboard`, langue en-US.
- Parole synthétique contrôlée, Ogg/Opus16k mono24kbit/s,14136 octets, SHA-256 `5f9e53bb05e954f16517c618c63eb52da16e5289cc454936815354e9af397bc6`.
- Audio224octets/morceau ; contexte vide transmis après audio ;67 requêtes,13 réponses ; gRPC **OK**, statut S3 terminal **1**, état reconnaissance **1**, événement **3** (end-of-utterance).
- Hypothèse produite : `Hello. This is a test of intelligent dictation. The meeting is tomorrow at 11.`
- VoiceEditingResult : **intent2**, transcript et edited_text non vides/identiques au texte ci-dessus ; aucun edited_context. Ce2 est une valeur réelle, **ne pas la renommer automatiquement DICTATION0** : dans l'enum client analysé2=INTENT_EDIT_HISTORY.
- Le JSON de preuve conserve présence des statuts absents (`null`) séparément du défaut protobuf0 ; c'est correct. Score absent n'est pas une erreur.
- Durée totale3301.479ms, dernier audio fourni148.022ms ; ceci est un envoi accéléré d'un échantillon, **pas** une mesure de latence de bout en bout en dictée temps réel.

**Conclusion bornée de cette première session seule** : ceci valide une session du pipeline S3 service/routage Agentic identifié statiquement et une réponse structurée VoiceEditingResult hors Android, sans token de compte/Zwieback pour cette configuration. La validation fonctionnelle supplémentaire de correction/contexte est décrite en section10. Aucun de ces essais ne démontre toutes les langues, accès durable, absence de quota ou parité complète avec Rambler/PixelBoard.

## 10. Correction et contexte : vérification indépendante de 33 réponses brutes

Le coordinateur a effectué trois sessions supplémentaires via `RamblerEngine`/`GrpcTransport`. **L'analyste a ensuite redécodé les33 protobuf réponses avec un parseur autonome, sans importer `rambler.protocol`, `rambler.wire` ni le décodeur du coordinateur.** Outil : `C:/Users/FSOS/Documents/project/Rambpc/tools/research/inspect_s3_capture.py`, exécuté avec `python -I`.

- Captures réponse uniquement, locales/ignorées par Git : `C:/Users/FSOS/Documents/project/Rambpc/artifacts/live-functional-cases`.
- Preuve expurgée : `C:/Users/FSOS/Documents/project/Rambpc/docs/evidence/functional-cases.json`, horodatage `2026-11-12T20:14:44.714250+00:00`.
- Les33 hashes de captures, statuts et textes correspondent au JSON. Aucun header/secret présent dans ces captures réponse.
- **Corpus réutilisable offline** : `C:/Users/FSOS/Documents/project/Rambpc/tests/fixtures/live_synthetic_responses.json` conserve les trois résultats finaux **octet pour octet originaux**, avec hex, SHA-256, hash de l'audio d'entrée, date/provenance et attendus. Inspection récursive avant publication : les seuls champs texte sont les sorties synthétiques contrôlées dupliquées ; les autres feuilles sont états/intention et petits scalaires numériques. Aucun identifiant de compte, token, pseudonyme, clé, header ou chaîne opaque constaté. Les scalaires inconnus sont conservés inchangés sans leur inventer une sémantique.
- `C:/Users/FSOS/Documents/project/Rambpc/tests/test_live_fixtures.py` vérifie les hashes des originaux et leur décodage par le client, **3 tests passés**. Les résultats finaux n'ont pas de statut S3 explicite ; ce corpus ne les présente pas comme les messages terminaux de session.

### Résultats vérifiés dans les octets bruts

| Cas | Entrée contrôlée rapportée par le coordinateur | Texte final relu directement dans la capture |
|---|---|---|
| `correction-en` | phrase suivie de « at ten. No, make that eleven. », contexte vide | `The meeting is tomorrow at 11.` |
| `context-edit-en` | contexte avant `The meeting is tomorrow at ten.` ; audio `Change ten to eleven.` | `The meeting is tomorrow at eleven.` |
| `plain-fr` | parole synthétique française ; contexte vide | `Bonjour. Ceci est un essai de dictée intelligente. La réunion aura lieu demain à 11h.` |

Pour chaque résultat, chemin exact constaté : **S3Response.field1253625 → field1 → field3 → repeated field3 → field1 texte** ; dans la même hypothèse, **extension74956224**, **field1 string**, **field3 string**, **field4 varint=2**. Les fields1 et3 de VoiceEditingResult sont identiques au texte final ci-dessus ; **field7 absent**, confiance absente. Reconnaissance `.1253625.1.1=1`, événement `.1253625.2.1=3` et statut terminal `.1=1` constatés dans les captures respectives. Le coordinateur rapporte gRPC OK et consommation audio intégrale avec hash identique pour les trois sessions ; ces faits de transport/entrée ne sont pas déductibles des seules réponses.

Captures finales et SHA-256 :

- `C:/Users/FSOS/Documents/project/Rambpc/artifacts/live-functional-cases/correction-en/response-008.pb`,243 octets : `43fd638823e88612faa6ee8a50614c0ba30ac38bef51f1623fee1178013b9c1e`.
- `C:/Users/FSOS/Documents/project/Rambpc/artifacts/live-functional-cases/context-edit-en/response-006.pb`,267 octets : `788d7342768e01bc529ef1799182641f10ff954b72b2e21099884672d22f922c`.
- `C:/Users/FSOS/Documents/project/Rambpc/artifacts/live-functional-cases/plain-fr/response-010.pb`,597 octets : `b8c4e7ce9a84b34f27fd1bebf1201cd1ecd538940962a4b8b2cb243380b70d15`.

### Conséquences établies et limites

1. **Preuve forte de comportement Agentic au-delà de l'ASR simple** : l'autocorrection orale est résolue ; la commande contextuelle réécrit le texte fourni au lieu de transcrire littéralement « Change ten to eleven. ». Cela correspond au service/routage Rambler statiquement identifié, sans dépendance runtime Android pour ces cas.
2. **Le champ nommé transcript n'est pas une garantie de verbatim.** Dans le cas contexte, son contenu est l'historique produit, pas la commande prononcée ; dans le cas correction il ne conserve pas la disfluence. Ne pas promettre récupération d'un transcript audio brut à partir de ce seul champ.
3. **Texte intégral, pas delta** : le contexte est inclus dans le texte final retourné. Une application qui ajoute ce résultat à la fin du contexte dupliquerait la phrase ; politique d'édition/ranges reste nécessaire.
4. **Champ d'enveloppe inconnu observé : field5 varint**. Il apparaît dans les33 réponses, y compris les messages terminaux, mais n'a pas de champ nommé dans le `aamb` optimisé de l'APK. Le conserver/ignorer comme inconnu ; aucune sémantique de séquence/temps n'est démontrée.
5. Ces trois échantillons seuls ne couvrent ni microphone, ni sélection, tous les intents, quotas ou parité générale PixelBoard. Les validations ultérieures de sélection/concurrence et de l'encodeur natif sont séparées ci-dessous et dans l'état du projet.

## 11. Validation ultérieure de sélection et contexte différé

Preuve du coordinateur : [lifecycle.json](../evidence/lifecycle.json), code moteur,
transport et protocole hashés. Entrée contrôlée `Change ten to eleven.`, contexte
fourni par callback **après EOF audio** : avant=`The meeting is tomorrow at `,
sélection=`ten`, après=`.` ; `cursor_aware=true`.

Le service renvoie VoiceEditingResult intention2 avec `edited_context` :

```text
before:   "The meeting is tomorrow at "
selected: "eleven"
after:    "."
```

Le texte/transcript contient aussi `The meeting is tomorrow at <cursor>eleven</cursor>.`.
Les marqueurs appartiennent à cette représentation : pour l'intégration, préserver
le contexte structuré plutôt que coller ce transcript littéralement. Le champ3
`edited_text` est absent, remplacé par le oneof7. Fin S3, gRPC OK et intégrité audio
ont été observés. Cette preuve est issue du codec du moteur ; elle n'est pas
présentée comme une nouvelle inspection indépendante de payload brut.

Le même rapport valide ensuite annulation d'une entrée bloquée, fermeture source,
puis deux sessions anglaise/française concurrentes sur le même canal, sans mélange
observé. L'[encodeur natif progressif](../evidence/native-realtime.json) a en outre
réussi six sessions avec PCM intégral, hashes source/code et profils Ogg comparés.
Cela ne valide toujours pas toutes les intentions, voix naturelles, longues
durées ou contraintes futures du service.

## 12. Réécriture et contexte non-BMP — 2026-11-13

Un nouveau cas contrôlé fournit le contexte `Send me the report now.` puis l'audio
`Make this sentence more polite, but keep the same meaning.`. La réponse finale a
été inspectée avec un parseur autonome, sans importer `rambler.*` :

- Réponse source : **1055 octets**, SHA-256
  `7a8c1217362395a706d0626aa69860413a82bed82cfc868f369750f185f4ff92`.
- Reconnaissance `.1253625.1.1=1`, hypothèse sous `.1253625.1.3.3` ; statut S3 absent
  de cette réponse finale de reconnaissance, qui n'est pas le terminal de session.
- Hypothèse `.field1` et `VoiceEditingResult.field1` : contexte suivi de la commande.
- `VoiceEditingResult.field4=3`, oneof actif **field3**, fields2/7 absents :
  `Could you please send me the report at your earliest convenience?`.
- `aamp.java` lignes9/29–30 confirme `INTENT_REWRITE(3)`. Le callsite `ihm.java`
  lignes192–212 choisit le texte édité3 ou contexte7 pour les ordinals2/3, puis
  ligne224 le privilégie au texte ASR. Cette lecture corrobore le rendu du cas
  observé, pas un contrat général de portée de remplacement.

Le SDK rend désormais l'intention3/champ3, mais les propositions de remplacement
restent limitées à l'intention2. Une [nouvelle session de suivi](../evidence/refinement-rewrite-public.json)
a confirmé ce rendu après modification. Le premier
[rapport expurgé](../evidence/refinement-capabilities-public.json) conserve son état
historique ambigu pour cette session ; il n'est pas corrigé rétroactivement.

La réponse source contient aussi **extension74956225** avec des labels internes de
traitement texte et un identifiant opaque du service, présent dans deux branches.
Son schéma et sa sémantique complète ne sont pas établis ; le SDK le préserve comme
inconnu et n'exécute aucune action à partir de ces données. Le corps original reste
privé. La [fixture](../../tests/fixtures/live_rewrite_response.json) est **dérivée et
expurgée**, avec wrapper reconstruit, composants sûrs conservés et hashés, suppression
explicite de74956225. Elle n'est pas une quatrième capture originale publique.

Le même corpus a exercé le contexte avant=`🙂 The meeting is tomorrow at `,
sélection=`ten`, après=`.`. Retour intention2/champ7 : même avant, sélection=`eleven`,
même après. Une sélection dans un contexte contenant un caractère non-BMP est donc
réellement exercée hors Android ; l'emoji n'est pas lui-même sélectionné. Les ranges
numériques restent calculés localement en UTF-16, pas transmis par le wire.
Le premier clip naturel anglais et la dictée de40,56s sont décrits avec leurs limites
dans [le raffinement](../refinement.md), ainsi que les corpus naturels supplémentaires
et les essais d'endurance ultérieurs.
