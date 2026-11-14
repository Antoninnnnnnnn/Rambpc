# Référence Android de développement : état des validations

## Rôle et limites

Ce téléphone de laboratoire est utilisé **uniquement comme référence de client**,
avec autorisation explicite d'installation root et de tests isolés. Le moteur
Rust continue de communiquer directement avec Rambler sans Android, téléphone ou
relais pendant son utilisation. Cet outil n'ajoute aucun backend ASR de substitution.

L'état ci-dessous qualifie une préparation et des diagnostics, **pas une dictée
Android ni une intention supplémentaire du service**. Les trois phrases prévues
n'ont pas encore été armées/injectées dans les observations publiées ici.

## Préparation contrôlée

- PixelBoard release officielle1.3.4, APK SHA256
  `a4628c2e42d3f62028742ee1880c33ce5d96e2a034fcb72ad16a586320cc8fa8`,
  digest GitHub et signature vérifiés avant installation. Sa version manifeste est
  `18.6.3.1002251336-release-arm64-v8a`, distincte du tag de release.
- Paquet `com.akshaykadam.pixelboard`, installé côte à côte avec Gboard via le
  gestionnaire de paquets root normal : aucune suppression de Gboard/données,
  aucun changement de Magisk, de signature, d'authentification ou d'attestation.
- Android API34 / arm64-v8a. Aucun numéro de série, identifiant de compte, token,
  clé API ou valeur de certificat d'authentification publié.
- Préférence Rambler et sélection Jetson lues comme vraies, sans les modifier.
  English(US) a été ajouté au profil PixelBoard seulement. Les deux premiers
  diagnostics SDK conservaient le clavier d'origine ; leur sous-type framework
  n'est donc **pas** une mesure de la langue d'une session vocale Rambler.

## Éditeur inerte reproductible

Le [fixture original et son builder](../../tools/reference/android_editor/README.md)
a zéro permission (réseau, micro et stockage compris). Il ne lit que son propre
éditeur ; SEND incrémente un compteur local sans envoyer ni effacer quoi que ce soit.
Les snapshots privés portent un run ID, une séquence, les sélections UTF-16 et
les compteurs afin de rejeter les captures périmées. Pas d'intent texte arbitraire.

L'APK initial `c7e507…b912` a passé sept contrôles sur appareil : cas fixes,
sélection entière, curseur final, relance simple/ID invalide sans reset, nouveaux
runs et zéro callback d'envoi. Le nouvel APK
`354e1c4363e399ea1978d21544983843b5a423eb6cee8f11f171b641960fb8fe`
a passé ces mêmes contrôles, puis une saisie **non vocale** contrôlée `X` a vérifié
le TextWatcher, la sélection en fin de tour UI et le reset sans écriture périmée.
Il maintient seulement sa fenêtre de premier plan éveillée, sans réglage global.
L'ancien APK mesuré a été conservé ; ces preuves ne sont pas réattribuées entre versions.

Un transfert `adb push` vers un répertoire root privé a d'abord été refusé ; un
transfert via shell/tee a ensuite échoué à la vérification SHA avant installation.
Le transfert binaire `adb push` vers un staging shell, puis installation **root**
avec contrôle SHA, a réussi sans contourner la vérification normale de signature.
Sur cet appareil, la lecture imbriquée `su -c run-as …` rendait une sortie vide ;
la lecture directe `adb exec-out run-as` des seuls fichiers du fixture fonctionne.

## Préflight initial : échec/inconclusif préservé

[Projection publique](../evidence/android-reference-preflight1-public.json).
Le paquet, l'APK, les signatures de classe et l'installation de trois hooks ciblés
ont passé les contrôles. Mais la session n'était plus disponible au premier RPC
ultérieur en lecture seule (`InvalidOperationError`). Seuls `loaded` et `ready`
ont été observés : **0 arm, 0 constructeur de capture, 0 EOF PCM**.

La cause demeure inconnue : l'observation Android `UNKNOWN` ne démontre pas un
crash natif. Settings et IME déclarent le même process par défaut ; le manifeste
ne soutient pas l'hypothèse d'un mauvais process Settings distinct. Le contrôleur
initial ne capturait pas le détachement et son erreur RPC empêchait l'arrêt HTTP.
Le clavier initial, la permission micro et AppOps ont été restaurés ; le serveur
propre au test et son forward ont été retirés avant arrêt du contrôleur hôte.

## Diagnostics séparés : réussites limitées

[Deux observations publiques](../evidence/android-reference-sdk-diagnostics-public.json) :

1. SDK en lecture seule, aucun hook : package/Java, préférence Rambler/Jetson et
   getter de sous-type répondent ; pas de détachement inattendu pendant20s.
2. Installation des trois hooks, **aucun arm** : même lecture immédiate disponible,
   aucun constructeur appelé/refusé, pas de détachement inattendu pendant20s.

Chaque diagnostic a restauré l'état initial et retiré son serveur/forward. Ces
réussites **ne résolvent pas rétroactivement** le premier échec. La construction
ART avec initialisation du seul superclass InputStream n'a pas été exercée.
Le contrôleur durci a passé53 tests hôte mockés (détachement, arrêt malgré RPC
bloqué/échoué, délais, confidentialité, cap persistant et commandes UI bornées),
sans validation appareil attribuée à ces tests. Les diagnostics/fixtures sont du
travail Sol ; l'analyse statique complémentaire est Kimi, pas une revue Opus.

## Préflight actif durci et contrôles discriminants sans audio

Un second préflight, PixelBoard sélectionné comme IME devant l'éditeur inerte,
atteint `ready` à2,282s puis observe `script.destroyed` et un détachement
`process-terminated` avec objet crash à2,810s. Le contrôleur restaure automatiquement
l'appareil et sort en erreur : **0 arm, 0 départ vocal, 0 constructeur, 0 EOF**.
L'inspection filtrée du journal Android du seul PID cible a identifié SIGSEGV ;
le journal brut n'a pas été conservé. Le signal n'était pas renseigné dans l'objet
lifecycle publié : cette distinction de source reste explicite. Cela ne démontre
ni la cause du premier échec, ni qu'un événement `script.destroyed` cause le crash.

Des études distinctes, devant le même éditeur et sans audio, passent ensuite :

- IME actif **sans Frida**, permission micro révoquée et AppOp `ignore` : même
  processus vivant après20s ; restauration réussie.
- IME actif, permission micro accordée mais AppOp `ignore`, SDK en lecture seule
  sans remplacement de méthode : getters Rambler/Jetson vrais, même processus
  vivant après20s ; détachement demandé pendant notre nettoyage seulement.
- Même confinement, création d'une classe de laboratoire inerte dans le process,
  puis remplacement de sa seule méthode jamais appelée : aucun remplacement de
  méthode PixelBoard, même processus vivant après20s, zéro appel du marqueur.
- Même confinement, remplacement du seul `aaqe.read(byte[],int,int)` par un refus
  sans lecture : même processus vivant après20s, zéro appel, aucun constructeur
  remplacé ou exercé.

Un cinquième contrôle ajoute au hook de lecture le seul constructeur sans
argument, tous deux remplacés par un refus : SIGSEGV après0,426s. L'objet crash
permet d'en conserver uniquement le module `libart.so` et le PC relatif
`0x270fd8`, sans rapport brut, mémoire, registres, chemin, ID ni symbole. La
restauration réussit. Les compteurs d'entrée dans les handlers sont perdus à la
mort du process : ce contrôle ne prouve pas zéro invocation de remplacement.
L'initialisation ELF du module système a été lue comme données pour une recherche
de symbole ; les symboles exportés ne résolvent pas ce PC. Aucun code de cette
bibliothèque n'a été exécuté sur l'hôte.

[Preuve d'isolation](../evidence/android-reference-active-ime-isolation-public.json).
Ces contrôles réduisent les hypothèses mais n'établissent pas encore la cause du
crash à trois hooks. Les corps et la portée des scripts minimaux diffèrent du
préflight complet ; un succès de20s n'est pas une preuve de stabilité générale.
La revue statique supplémentaire Sol ne trouve pas de mismatch de signature et
rappelle que les mécanismes internes ART du bridge ont une portée plus large que
les trois méthodes applicatives remplacées. Elle n'est pas une revue Opus.

## Approches écartées avant tout nouvel audio

- Conserver le constructeur original et neutraliser `AudioRecord.startRecording`
  n'est pas viable avec un état vrai STOPPED : `aaqe` exige immédiatement l'état3
  puis jette une IOException sans libération déterministe sur cet échec. La
  proposition est rejetée **avant implémentation et exécution** ; ni état simulé
  ni postcondition modifiée. Deux lectures statiques indépendantes concordent.
- `kmy.l` est explicitement `connectOnly`, pas un sélecteur d'audio externe.
  `kmy.audioInputStream` est figé à null dans ce build. `knt.l` fournit seulement
  `new aaqe()` ou null ; `iho.j(null, …)` initialise puis termine le réseau sans
  upload audio. Cette branche modifie aussi le traitement du focus audio et
  d'autres chemins : elle n'est ni activée ni présentée comme injection externe.
  Source : `kmy.java:11,27–42,65–68`, `kmw.java:25–47,90–93`,
  `kla.java:183–257`, `knt.java:318–344`, `iho.java:250–308` du même APK.

Les [sources originales du prototype audio/UI](../../tools/reference/android_audio/README.md)
sont conservées avec29 tests Node offline passants et un assembleur local qui
passe5 tests Python. Son build réel passe avec le bridge installé exactement
haché, sans action appareil. L'assembleur ne fournit pas de contrôleur de dictée
qualifié ; son nouveau SDK composé n'est pas réattribué aux essais passés.
Le fragment source audio retenu est égal à celui du SDK de préflight observé.
Le code tiers du bridge, les binaires/APK, clés et fichiers privés ne sont pas livrés.

## Prochain essai borné, sans résultat anticipé

**Bloqué à ce stade sur la construction instrumentée compatible ART et confinée.**
Aucun nouvel armement n'est justifié par les contrôles actuels. Une autre méthode
nécessite sa propre revue et son préflight sans audio ; ne pas contourner cet
échec en démarrant le vrai micro, en falsifiant un état ou en appelant directement
un backend à la place de la chaîne normale.

Au plus trois phrases synthétiques locales, PCM16LE mono16kHz, hashes fixés avant
armement, aucune reprise automatique. Le remplacement audio est limité au client
PixelBoard exact et à `aaqe` ; aucun changement auth/TLS, sécurité, réponse ou routage.
AppOp `RECORD_AUDIO=ignore` doit rester imposé avant attache, pendant lecture et
jusqu'au force-stop avant restauration, pour empêcher une capture ambiante non prévue.
Une absence de hook attendu, erreur ART/Java, mismatch de hash/EOF ou perte de
confinement termine l'essai, sans passer à un autre moteur ni au vrai microphone.

Le dispatch clavier normal a été établi statiquement : `kla.g("jetson", consumer)`
vers l'instance LatinIME active, `dG(oak)` sur thread UI ; STOP emploie le même
`oak.d(kla.c)` que `kla.h`. Le DEX confirme `Consumer.accept(Object):void` ; le
`e(...)` de JADX est un artefact. Cela **ne qualifie pas** les branches non
décompilées `ffh.m`/`klb.g`, l'onboarding/quota `ffh.z/y`, ni l'executor `kla.n`.
Aucun flag ou gate n'est modifié. Toute proposition de parité doit garder ces limites.
