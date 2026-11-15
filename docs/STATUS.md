# État du moteur — 2026-11-15

## Qualification CI native — code `1561f87`

La matrice distante Windows/Linux/macOS × Rust 1.88/stable a réellement terminé
avec succès, ainsi que le lint Rust et les quatre jobs Python. Les comptes Rust
ci-dessous sont **par job**, pas une somme de répétitions :

| Plateforme | Workspace | Exemple reuse | CLI sans audio | CLI release finalisation |
|---|---:|---:|---:|---:|
| Windows, 1.88 et stable |382 réussis, 0 échoué, 1 ignoré|10|31|1|
| Ubuntu, 1.88 et stable |383 réussis, 0 échoué, 1 ignoré|10|31|1|
| macOS, 1.88 et stable |383 réussis, 0 échoué, 1 ignoré|10|31|1|

Le contrôle positif supplémentaire de descripteur explique le test Unix de plus.
Le smoke PCM release passe aussi dans les six jobs (1017→3051 échantillons à
48 kHz, un EOS, zéro appel service). PyAV reste ignoré dans cette CI. Python
3.11 passe 640 tests avec 71 ignorés par job ; Python 3.14 en passe 710 avec un
ignoré. Les 259 sous-tests rapportés séparément ne s'ajoutent pas à ces totaux.
[Résultats, versions et exclusions](research/rust-build-validation.md) ·
[Preuve CI](evidence/rust-remote-ci-20261115.json).

Cette exécution lève le blocage des builds/tests natifs macOS et de la CI pour
ce commit, pas les limites de TLS réel/Keychain, parité Android, intentions non
établies, provisionnement/révocation ou performance WAN. Les notices antérieures
« non exécuté » restent des instantanés historiques, pas le statut courant.

Un seul appel Rambler synthétique supplémentaire termine sainement : la sélection
`🙂` est remplacée par `😎`, avec avant/après conservés et source intégrale nettoyée.
Le résultat ne reproduit pas le mot « eleven » obtenu avec cet audio dans un autre
contexte : c'est une observation structurelle Unicode, pas une validation de
qualité sémantique ou de déterminisme. Les plages UTF-16 publiées sont dérivées
offline ; la CLI n'exerce pas l'API de proposition ni une insertion dans un éditeur.
[Observation et limites](evidence/rust-native-non-bmp-selection-public.json).

## Point de fiabilité R4 — code `b0591f5`

Le snapshot production R4, conservé comme référence historique, passe **382 tests workspace,
0 échoué, 1 ignoré** sur Windows stable 1.99.1, Windows/MSVC 1.88 et Linux/WSL
1.88. Les dix tests d'exemple, 31 tests CLI sans audio et le test du véritable
CLI release de finalisation passent sous Windows stable et Linux. Windows passe
ses 26 étapes, Linux ses 16 phases ; les exclusions Linux restent explicites.
Clippy/fmt/rustdoc stricts, 716 tests Python et l'oracle PyAV séparé (57 conteneurs)
sont qualifiés sous Windows, pas déduits pour Linux.

Ce lot durcit l'admission avant lecture audio, les erreurs persistantes et de
cleanup, le pacing TCP, la finalisation et le commit de succès. Le setup des
racines TLS est déporté hors du runtime async ; un loader déjà démarré reste
non interruptible. Les contrats, limites et preuves sont dans
[la validation R4](research/rust-build-validation.md#setup-tls-déporté--r4) et
[le cycle de vie](research/rust-reliability-lifecycle.md).

Aucun appel service/appareil supplémentaire ni gain WAN n'est déduit de ce lot.
macOS et CI distante restent non exécutés ; l'inspection macOS statique n'est
pas une qualification native. Parité Android, intentions non établies et
provisionnement/récupération après révocation restent ouverts. Les sections
suivantes conservent leurs **snapshots historiques** : leurs comptes et empreintes
ne désignent pas le code courant.

## Suivi ciblé tests / CI — 15 novembre

Le helper de cleanup remplace le no-op macOS par un audit device/inode ; Linux
ajoute le contrôle positif 0→1→0, Windows conserve l'ouverture exclusive.
Windows initial passe 3 tests sur stable et 3 sur MSRV, Clippy audio strict,
1 test du vrai CLI release et le smoke PCM, mais reste **FAILED** : fmt échoue
seul parmi 13 étapes. Linux passe son périmètre partiel (11 phases, 4 tests cleanup,
1 CLI release, smoke PCM ; Clippy **NOT_RUN**, non installé). Après le seul échange
d'ordre de deux imports cfg, Windows passe 6 étapes ciblées, dont fmt, 3/3 tests
et Clippy ; aucun rebuild release ni rerun Linux post-format n'est déduit.
Les échecs initiaux et deux préparations de harness échouées avant commandes sont
conservés, pas requalifiés. Les 382 tests workspace R4 ci-dessus restent historiques :
pas de suite complète courante à 383 tests revendiquée.

La matrice OS/MSRV écrite ajoute build audio CLI release, finalisation loopback
avec le vrai exécutable et smoke PCM release. **CI distante, macOS et Linux 32-bit
natif NOT_RUN**. Production/manifests/lock inchangés ; aucune fuite production
prouvée/corrigée, aucun gain moteur/WAN ni parité/provisionnement déduit.
[Validation ciblée et exclusions](research/rust-build-validation.md#focused-platform-gate-follow-up--2026-11-15) ·
[Preuve expurgée](evidence/rust-platform-gates.json).

## Canal PCM jusqu'au moteur : cinq tests loopback qualifiés

Publication du15 novembre, **exécutions du14 novembre2026** : les deux fichiers
CLI `pcm_channel_engine.rs` (2 tests) et `pcm_engine_failure.rs` (3 tests) composent
le vrai canal PCM, encodeur, moteur et transport face à tonic. Le premier contrôle
les octets Ogg reçus par le pair, CRC/durée décodée, égalité des paquets Opus et
ordre init/audio/contexte/END ; le second couvre abandon typé, réveil d'une
réservation réellement bloquée et END/OK précoce refusé comme succès.
**Dans le cas d'abandon, seule l'arrivée d'une page/en-tête Ogg est prouvée,
pas celle d'échantillons audio.** Les réponses S3 restent synthétiques.

**310 tests workspace passent,0 échoué,1 ignoré**, sur chacun de Windows stable,
Windows/MSVC1.88 et Linux/WSL1.88. Les10 tests d'exemple et30 tests CLI feature-off
passent sur Windows et Linux. Clippy strict, formatage et builds release CLI
avec/sans audio passent sur Windows ; Linux couvre le build release CLI audio.
L'exemple PCM release est construit et son smoke passe sur les deux OS
(1017→3051 échantillons après trim,1 EOS). Les96 entrées des manifestes sources
Windows/Linux concordent exactement avec les fichiers actuels ; les noms finaux
des deux tests sont vérifiés dans les trois logs workspace. Aucun nouveau
comportement production ni dépendance, zéro appel service/appareil.

Deux contre-revues statiques finales ne trouvent aucun défaut concret bloquant ;
les limites complémentaires de chaque fichier restent explicites. La première
revue, bloquée par un mauvais nom de cible, n'est pas comptée comme réussie.
[Preuve et provenance](evidence/rust-pcm-engine-loopback.json) ·
[Contrats et limites](research/rust-pcm-channel.md).
Ni dictée distante supplémentaire, ni gain WAN, ni perfection ou parité Gboard
déduite de ces tests. Les sections suivantes conservent leurs comptes historiques.

## Deux frontières HTTP/2 corrigées et reproduites localement

Un workflow couvre10 périmètres en parallèle, avec2 contre-vérifications et une
synthèse (13 agents Sol). Deux suites séparées établissent ensuite RED/GREEN et
contre-revue : type MIME gRPC valide en casse mixte désormais accepté, cinq noms
de métadonnées interdits en HTTP/2 refusés explicitement dès leur validation.
Les frontières de sous-type, doublons, limites, encodages, safety et credentials
valides restent préservés ; aucun en-tête n'est retiré silencieusement.
**305 tests workspace passent** sur Windows stable1.99.1, Windows/MSVC1.88 et
Linux1.88, avec un oracle ignoré et sources hashées identiques.10 tests d'exemple,
30 tests CLI feature-off, Clippy strict/formatage Windows, builds release et
smokes PCM passent. Zéro nouvel appel Rambler/appareil, aucun gain WAN revendiqué.
[Reproductions, contrats et limites](research/rust-http2-boundaries.md).

## Entrée PCM producteur et arrêt normal explicite

`pcm_channel()` ajoute un producteur unique avec une file d'un seul chunk et des
réservations annulables sans remise prématurée des bytes. `finish()` annonce
l'EOF normal après drainage ; Drop sans finish est un abandon en erreur. Close
réveille les producteurs bloqués, même si son futur n'est pas pollé. La capture
microphone reste hors périmètre ; le profil codec et le service sont inchangés.
L'acceptation d'un chunk n'est jamais présentée comme un acquittement de dictée.
**300 tests workspace passent** sur Windows stable1.99.1, Windows/MSVC1.88 et
Linux1.88 (un oracle ignoré), dont19 nouveaux cas PCM. Les10 tests de l'observateur
et30 tests CLI feature-off passent aussi ; Clippy strict/formatage Windows et
builds release natifs passent. Le smoke offline vérifie1017→3051 échantillons
après trim, un EOS, zéro service/appareil. L'échec de formatage initial est conservé.
[Contrat, exemple offline et limites](research/rust-pcm-channel.md).

## Métadonnées natives : snapshots par session

Deux nouveaux tests couvrent le fournisseur partagé avec rotation puis retrait
pendant trois RPCs simultanément ouverts sur un seul TCP loopback : doublons et
binaire exacts, aucune ancienne valeur sur la session vide, un appel fournisseur
par session jusqu'à fermeture. La sensibilité est contrôlée après conversion des
en-têtes **avant envoi**, pas inférée du décodeur HPACK du pair.
La suite transport passe **63 tests** sur Windows stable1.99.1, Windows/MSVC1.88
et Linux1.88 avec sources hashées identiques ; Clippy strict et formatage Windows
passent. Ce n'est pas une nouvelle exécution de tout le workspace.
Aucun changement de comportement production, aucun appel service/appareil et
aucun renouvellement ou accès après révocation démontré.
[Contrat et validation ciblée](research/rust-credential-snapshots.md).

## Référence Android de développement : préparation, pas parité acquise

PixelBoard officiel est installé côte à côte avec Gboard sur le téléphone de test
explicitement autorisé. L'[éditeur inerte original](../tools/reference/android_editor/README.md)
à zéro permission passe7 contrôles de cas/sélections et4 de suivi/reset sur appareil.
Aucun envoi réel, aucune donnée personnelle ni dépendance Android du moteur ajoutée.
Le préflight initial reste inconclusif. Un second préflight avec IME actif et un
contrôle minimal constructeur/lecture reproduisent un **SIGSEGV ART avant tout
armement audio**, avec restauration réussie. Lecture SDK seule, hook d'une méthode
de laboratoire et hook de lecture seul tiennent chacun20s ; la cause exacte du
crash reste non établie. Les3 phrases restent non injectées. L'alternative qui
neutraliserait `startRecording` est rejetée statiquement, sans falsifier l'état.
Les sources expérimentales audio/UI sont conservées avec29 tests Node et5 tests
d'assembleur Python passants, pas présentées comme injecteur fonctionnel.
[État, sources reproductibles et limites](research/android-reference-validation.md).

## Observation native des partiels

Le validateur expose `--observe-partials` : compteurs par session et hashes cadrés
stable/provisoire/rendu, distinction avant/première state1, sans texte ni promotion
en finale.10 tests ciblés passent sur Windows stable, Windows/MSVC1.88 et
Linux1.88 ; Clippy strict Windows stable passe. Le premier essai MSRV Windows
échoue en configuration CMake dans un target profondément imbriqué, puis passe
avec le target Cargo ordinaire, sans modification de source ; échec préservé.
Aucune API production/dépendance modifiée. [Méthode](research/rust-native-partials.md).
Un unique essai synthétique natif à débit temps réel termine sainement avec audio
intégral vérifié, mais20 réponses et **0 field4** : rendu partiel réel/progression
pré-state1 restent **inconclusifs**, sans second appel ni retry. La première réponse
n'est pas assimilée à un partiel. [Preuve](evidence/rust-native-partials-public.json).

## Décodage : allocations défensives

**279 tests workspace passent** sur Windows stable1.99.1 et1.88, ainsi que
Linux1.88 (R9), avec un oracle audio ignoré. Les4 tests d'exemple et30 tests CLI
sans audio passent sur Windows stable et Linux ; Clippy strict et formatage
Windows passent, ainsi que les builds release Windows avec/sans audio et Linux
audio. [Détail des validations](research/rust-build-validation.md).

Le [raffinement du décodeur](research/rust-backend-decode-defensive-capacity.md)
réduit les allocations mesurées de39 à24 par réponse, sans changement d'API.
Le premier enfant est validé avant allocation de la liste ; l'indication initiale
inspecte au plus quatre champs, avant filtrage. La variante V2 préallouant depuis
un compte non validé a été rejetée, avec ses mesures conservées séparément.
Les médianes CPU locales sont favorables, mais certains pics temporaires sparse
augmentent d'environ18,5% : ni optimum universel de mémoire ni gain WAN revendiqué.

## Suite de consolidation : mémoire, entrées fichier et options S3

**274 tests workspace passent** sur Windows stable1.99.1 et Rust1.88, ainsi que
Linux1.88 (snapshot R6). L'exemple de réutilisation passe4 tests, la CLI sans audio
30 ; Clippy strict, formatage et builds release avec/sans audio passent. macOS et
la matrice CI distante restent non exécutés. Les preuves réseau antérieures ne
sont pas réattribuées au nouveau binaire. Après les tests offline, trois appels
synthétiques bornés ont accepté champs7/40 omis, faux puis vrais, sur un même
moteur : PCM intégraux hashés, zéro retry et trois hashes de texte identiques.
[Preuve](evidence/rust-recognizer-options-public.json) : acceptation de requête,
pas efficacité du filtrage/ponctuation. Le routage secondaire n'a pas été essayé.

- [Raw partagé](research/rust-backend-profiling.md) : payloads identiques et
  **−70,82% de rétention logique** (12 635 008→3 686 912octets), texte/inconnus
  conservés. Revue indépendante sans finding concret, exécutée par Sol selon les
  métadonnées (ancienne attribution Opus corrigée) ; ce n'est pas une
  baisse RSS ou un gain WAN garanti. API Rust0.1 : `raw`/`active_raw` deviennent
  `Bytes`, migration et enfant retenant sa racine documentés.
- [Fichiers réguliers](research/rust-file-inputs.md) : FIFO Unix et pipe Windows
  pouvaient bloquer avant le démarrage du timeout RPC. Ouverture Unix non bloquante
  et contrôle du type du descripteur ; contrôle `GetFileType` sur Windows.
  RED conservé, GREEN sur les deux OS, zéro TCP/RPC sur entrées refusées.
- [Trois options RecognizerInfo](research/rust-recognizer-options.md) : préférence
  de mots offensants field7=0/2, auto-punctuation field40, routage secondaire field64.
  SDK et CLI, défauts `None` laissant les goldens identiques ; effet serveur non
  testé, aucun switch COMPOSE ni changement de contrôle safety supposé.
- La sélection legacy seule était déjà encodable via le champ texte plat ;
  aucune proposition ne confond cette projection avec la fenêtre originale.

Les autres frontières restent celles de la matrice : intentions non établies,
services/modèles Android locaux, accès après révocation et macOS non exécuté.
Aucun autre moteur ni API Gemini publique substitué. Le code n'est pas déclaré
mathématiquement parfait et l'incident long historique n'est pas déclaré résolu.

## Consolidation backend native — jalon fdc9fd9

**262 tests Rust passent** sous Windows stable1.99.1 et1.88.0, ainsi que Linux
x86_64/WSL Ubuntu24.04 avec Rust1.88. L'exemple `session_reuse` passe4 tests et la
CLI sans audio27 tests ; Clippy strict et formatage passent, builds release avec
et sans audio compris. L'oracle PyAV indépendant passe57 conteneurs séparément,
et la référence Python/outils passe656 tests en13,82s. La matrice CI distante
n'a pas été exécutée ; macOS reste non exécuté. [Builds](research/rust-build-validation.md).

- Résultats : champs `language`/`spoken_language` corrigés ; conteneurs3/5/7 et
  fallback complet décodés avec fusion/budgets/inconnus préservés. La sécurité
  couvre tous ces candidats, sans les sélectionner arbitrairement.
- API : politiques de rendu explicites avec provenance, partiels stables/provisoires,
  collecte bornée de candidats avec ordinal, classification SEND/CLEAR sans effet.
  Propositions toujours contrôlées par contexte/révision/UTF-16 ; aucun COMPOSE inventé.
- Fiabilité : GOAWAY/reset/driver indépendants préservés. Deux pertes d'erreurs
  moteur reproduites RED puis corrigées : driver écrasé par erreur session, et
  cleanup secondaire perdu après erreur audio primaire. Treize nouveaux filets
  couvrent annulation/concurrence/reaping ; les erreurs restent à relever par
  `close()` successifs jusqu'à `Ok`, sans replay d'audio.
- CLI Ogg : CRC/en-têtes/continuation/séquence/granules/EOS progressifs et bornés,
  sans décodage ni modification des octets. EOS vide valide vérifié aussi avec
  PyAV ; OpusTags doit commencer deuxième page. Limites applicatives explicites.
- Performance : une allocation exacte pour sérialiser l'audio, octets identiques ;
  [96 mesures CPU et40 sessions loopback](research/rust-backend-profiling.md).
  Pas de gain global pipeline/WAN robuste déduit des micro-mesures.
- Service réel : [trois cas synthétiques sur un même moteur](evidence/rust-native-session-reuse-public.json),
  puis [trois extraits naturels EN/FR,18,885s au total](evidence/rust-native-natural-public.json),
  PCM consommés/EOF/fermeture/hash vérifiés. Les trois textes naturels hashés
  correspondent aux textes Python historiques ; comparaison rétrospective, pas
  nouvelle garantie de qualité ou benchmark de vitesse entre implémentations.

[Validation détaillée et provenance](research/rust-backend-validation-2026-11-14.md).
Le premier lot de réutilisation reste inconclusif (`eleven`/`11`) ; l'incident
historique282s n'est pas déclaré résolu. Le backend S3 vérifié est opérationnel,
**pas toute l'application Gboard ni un moteur déclaré parfait** : COMPOSE/intentions
non établies, services/modèles Android locaux, récupération après révocation et
exécution macOS nécessitent des preuves/accès distincts. Microphone/GUI/insertion
restent hors périmètre initial. Aucun autre ASR/API Gemini substitué.

## Parité, robustesse, profilage et MSRV — jalon e427fb1 (13 novembre)

**186 tests Rust passent** sur l'arbre intégré avec stable1.99.1 et Rust1.88.0,
Windows x86_64/MSVC ; un oracle audio indépendant est ignoré par défaut, passé
séparément sur57 conteneurs. Formatage et Clippy tous targets/features passent.
La matrice CI Windows/Linux/macOS ×1.88/stable est écrite mais **pas exécutée à distance**.
[Validation builds](research/rust-build-validation.md).

La [matrice de parité](research/rust-feature-parity.md) relie les symboles APK,
API Rust et preuves réseau sans confondre enums et capacités validées. La revue
de parité a trouvé un marqueur `hypothesis.field30` sécurité ignoré : décodage0/1,
préservation des inconnus et refus de rendu/proposition sont maintenant testés.
La CLI n'affiche aucun candidat ni contexte pour `safety_blocked` ou
`unrecognized_safety`. Revue ciblée finale sans constat ; aucun cas bloqué réel
provoqué, aucune nouvelle requête Google ni interaction appareil dans ce lot.
Les refus hors VoiceEdit et sur valeurs inconnues sont un durcissement explicite.

Les nouveaux tests génératifs exercent128 séquences proto2 et343 traces safety,
les bornes exactes, occurrences tronquées, partitions gRPC et fermetures
concurrentes interrompues. `Presentation` distingue les types de résultats sans
simplifier leur texte ni prouver la fin globale. `EncodedAudioProgress` sépare
chunks/octets encodés, EOF audio et nettoyage des flags du RequestSource ; ce ne
sont ni des compteurs PCM ni des acquittements serveur.

Le [profilage offline release ABBA](research/rust-audio-profiling.md) mesure
encodeur, mux et pipeline sur signaux synthétiques5,001s. Total médian groupé
45–134ms, mux14–81µs ; **aucun gain robuste du candidat CRC**, rejeté. Codec,
qualité, bitrate et sources audio de production restent inchangés. Ce banc
n'établit ni optimum global ni accélération réseau.

La suite Python/outils passe656 tests en11,20s, en relance bornée avec diagnostic
de stack. Comme lors d'un précédent lot, une exécution silencieuse avait stagné
vers605 et a été arrêtée après120s ; la relance verbose ne reproduit pas le blocage.
Cette observation n'est ni cachée ni attribuée à une cause non démontrée.
La référence Python0.3 reste gelée : la nouvelle politique safety est portée
explicitement dans Rust, pas rétroactivement revendiquée pour Python.

## Consolidation Rust : diagnostic sans faux succès — jalon4fdf29e

Le cœur Rust comprend protocole/transport, sessions/résultats, WAV/PCM natif et CLI.
**163 tests Rust intégrés et 654 tests stricts de référence/outils Python passent**,
formatage/Clippy tous targets/features et build release audio compris. L'oracle
indépendant57 conteneurs est passé séparément ; la matrice OS/MSRV n'est pas déduite
de ces exécutions Windows.

Après le lot long arrêté, un nouveau binaire ajoute seulement des observations
structurelles typées d'`IncompleteSession`, sans changer les conditions de succès,
le codec ou introduire un rejeu. Le
[palier de qualification120s](evidence/rust-diagnostic-endurance-120-public.json)
réussit, puis [un unique essai300s](evidence/rust-diagnostic-endurance-300-public.json)
termine en290,904s avec14 422 requêtes et les mêmes finales connues ordonnées que
le premier essai long sain. **L'incident282s n'est pas reproduit, pas déclaré résolu.**

La revue indépendante, avec neuf contrôles loopback ciblés, ne démontre aucun
défaut client compatible avec cet incident et recommande de ne pas multiplier les
appels service. Il manque les flags/timeline de l'échec original, impossibles à
récupérer après coup ; cela interdit d'en attribuer la cause au client ou au service.
[Observations, sémantique des flags et limites](evidence/rust-incomplete-diagnostic-followup.json).
Le prochain incident d'usage autorisé disposera du diagnostic, sans accepter des
résultats tronqués ni les rejouer automatiquement.

## Audio Rust natif et endurance mesurée — jalon eb87cc5

L'entrée PCM est portée dans `rambler-audio` : WAV strict PCM16LE mono16k,
libopus 1.6.1 statique, VoIP/VBR24kbps, trames/pages20ms par défaut, flush et EOS
sans perdre les derniers échantillons. La CLI `dictate-wav` est optionnelle
(`--features audio`). **156 tests Rust passent**, plus un oracle ignoré par défaut
exécuté séparément sur **57 conteneurs Ogg** : durée décodée exacte à48k et16k.
Le build Ogg minimal n'a pas de dépendance normale sur Opus/CMake.
La suite de référence/outils Python étendue passe **653 tests stricts** dans la
relance bornée. Une première exécution était restée bloquée et a été arrêtée ;
la relance avec diagnostic de stack activé ne reproduit pas le blocage.

[Quatre appels supplémentaires réussis](evidence/rust-pcm-functional-public.json) :
PCM anglais accéléré puis progressif au même texte, réécriture intention3 et
sélection `ten`→`eleven` avec emoji préservé. Ces preuves s'ajoutent aux trois
appels Ogg initiaux, sans devenir une mesure de qualité générale.

Le [palier Rust118,682s](evidence/rust-endurance-120-public.json) termine en119,929s,
avec tous les marqueurs rendus, couverture RSS/CPU saine et maximum RSS
échantillonné11 898 880octets (~11,35Mio). Au
[palier288,352s](evidence/rust-endurance-300-public.json), le premier essai termine
en290,905s (max RSS11 862 016octets ; CPU enfant dernier échantillon12,984s),
mais **le deuxième échoue `incomplete` après282,202s** ; le troisième n'est pas
lancé. La cause précise de cette fin précoce n'est pas établie par l'ancienne
sortie d'erreur. Aucun résultat partiel n'est annoncé comme réussi et aucun rejeu
automatique ne masque l'échec. L'analyse diagnostique est distincte de ce bilan.

Chaque session du banc Rust crée un processus et un canal neufs, contrairement
aux trois sessions Python sur canal réutilisé. RSS/CPU sont échantillonnés, pas des
pics/exécutions exactes ; aucune comparaison de vitesse ou preuve d'absence de fuite
n'est déduite de ces chiffres.

Le [Ctrl-C console Windows réel](evidence/rust-windows-ctrl-c.json) est vérifié sur
un RPC **loopback**, dans une console isolée : exit130, RPC annulé, pas de résultat.
Le premier pilote de test a temporisé ; son isolation/héritage console a été
corrigé sans changer le binaire. Le pilote reproductible est versionné sous
`tools/validate_rust_ctrl_c.py`. [Guide Rust](rust.md).

## Première tranche Rust validée

Le workspace natif contient `rambler-protocol`, `rambler-transport`,
`rambler-engine` et `rambler-cli`. **121 tests Rust intégrés passent**, ainsi que
le formatage, Clippy tous targets avec warnings bloquants et la compilation
release. Les **643 tests de la référence Python** passent aussi (623 du gel 0.3
et 20 de l'oracle de politique de résultat).

**Trois appels réels Rust réussis sur Windows** : dictée anglaise, instruction
sur contexte `ten`→`eleven`, dictée française. Le processus Rust a été lancé avec
un PATH limité aux répertoires système Windows, sans Python/FFmpeg ; l'orchestration
Python de développement n'est pas une dépendance du binaire. TLS vérifié, service
S3Bidi/routage Rambler inchangés, plateforme déclarée `Rust`, fin S3 et gRPC OK,
aucun rejeu automatique. [Preuve inspectée](evidence/rust-first-slice-public.json)
avec hashes du binaire, du lock et des sources mesurées ; pas de payload opaque
ou d'identifiant de service publié.

Les tests loopback couvrent notamment annulation d'un upload bloqué par le
flow-control avec reset du seul flux, sessions voisines conservées sur une même
connexion, erreurs tardives, deadline, framing, mapping HTTP/RST comparé à tonic,
nettoyage exactement une fois et conservation de plusieurs erreurs background.
La fermeture explicite reste nécessaire pour attendre le nettoyage asynchrone.

Cette preuve utilise **de l'Ogg/Opus déjà encodé** : le portage PCM natif, les
mesures Rust CPU/RSS/endurance et l'exécution réelle sur d'autres OS sont des lots
distincts, pas des succès déduits des essais Python. Aucun gain de vitesse global
Rust n'est revendiqué. Voir [le guide Rust](rust.md) pour construire, exécuter et
intégrer le moteur ; GUI/microphone/insertion restent hors périmètre.

## Continuation SDK 0.3 et référence Rust

Le SDK 0.3 intègre les options de contexte initial et **623 tests locaux stricts**
passent. Les goldens de protocole par défaut restent identiques. Python est la
référence de conformité du portage demandé : bibliothèque Rust indépendante et CLI
d'abord ; Tauri, microphone et intégrations système restent ultérieurs et hors lot.
Voir le [contrat de portage Rust](architecture/portage-rust.md), la
[recommandation desktop](architecture/application-desktop-multiplateforme.md) et le
[raffinement](refinement.md). La [roue de référence 0.3](evidence/reference-0.3-wheel-smoke.json)
est construite et vérifiée dans un venv vierge sans PyAV : imports, égalité des
modules, fixture réécriture et wire des hints/langues secondaires passent offline.

**35 transports supplémentaires réussis** : 13 naturels, huit essais composition,
quatre d'endurance et dix essais de hints. Ce compte ne signifie pas 35 fonctions
validées. Les dix clips naturels supplémentaires (7 EN, 3 FR ; 72,825 s) donnent
3/109 et 3/79 erreurs WER normalisées, avec différences de format et de temps verbal ;
trois essais progressifs retrouvent exactement leur texte accéléré. La qualité générale
reste ouverte. [Provenance](evidence/natural-expansion-inputs.json),
[qualité](evidence/natural-expansion-quality.json), [progressif](evidence/natural-expansion-realtime-public.json).

L'endurance synthétique a réussi une session de 118,682 s dans un premier processus,
puis trois de 288,352 s sur un même canal dans un second processus, en cadence
simulée, avec source intégrale, marqueurs et finales ordonnées identiques entre ces
trois sessions longues. Les rapports montrent RSS échantillonné, CPU et durée, sans
prouver l'absence de fuite ; ces mesures sont sur base **0.2 avant hints**, avec
hashes de modules conservés. [120](evidence/endurance-120-public.json) ·
[300](evidence/endurance-300-public.json).

Les dix essais [hints](evidence/input-hints-public.json) n'établissent pas de gain
général : un nom change avec changement de ponctuation/conjonction ; la préférence
britannique et la langue secondaire n'ont pas de bénéfice isolé démontré. Les
invocations vocales testées restent littérales ; COMPOSE 4 n'est pas observé.

## Raffinement 0.2.0 — jalon historique du 2026-11-13

Branche locale : `feat/engine-refinement`, construite sur `1f52c94` sans modifier
la branche publiée. **438 tests locaux passent** avec asyncio debug et avertissements
bloquants. La [roue 0.2.0 finale](evidence/refinement-wheel-smoke.json) est construite
et installée dans un venv vierge sans PyAV : imports publics, modules identiques au
source et rendu de la fixture dérivée vérifiés offline. Les mesures initiales
portent le checkpoint `4b96696` ; le suivi
réécriture enregistre aussi les hashes du nouveau code de rendu.

- Deux défauts corrigés : fusion des messages protobuf singuliers répétés (y compris
  même membre oneof) et nettoyage producteur asynchrone interrompu par la fin RPC.
  Aucun des deux n'a été observé dans les 42 essais historiques ; reproductions
  offline/loopback et vérification canonique ajoutées.
- API de résultat final bornée, ambiguïté explicite, proposition de remplacement
  sans effet de bord et contrôle snapshot/révision : [guide](refinement.md).
- **20/20 sessions de la grille** 20/40/60/100 ms réussies, PCM décodé et toutes les
  mises à jour finales ordonnées identiques. Défaut 20 ms conservé ; 60 ms économise
  environ 26 % d'Ogg avec environ 17 ms de médiane finale supplémentaire sur ce banc.
  Aucun nouvel optimum de vitesse établi ; [distributions et limites](performance.md).
- Cinq cas fonctionnels ont terminé leur transport ; quatre rendus uniques au
  premier passage. La réécriture réelle, intention 3/champ 3, a révélé une limite de
  l'API puis a réussi dans un nouvel essai après correction du rendu. Le rapport
  initial 4/5 est conservé, pas réécrit.
- Clip naturel anglais de 3,505 s : WER 0/8 mots, sans généralisation. Dictée de
  40,56 s intégrale en accéléré puis à débit simulé (collecte 41,454 s), même texte
  corrigé et référence finale conservée. Sélection avec emoji réussie. La demande
  de composition est restée une dictée de l'instruction, pas une composition validée.

Les **27 nouveaux transports complets** sont les 20 mesures, cinq cas fonctionnels,
une dictée longue progressive et le suivi réécriture ; ils ne constituent pas
27 succès de rendu du premier coup. Les rapports publics sont expurgés, les captures
avec identifiants opaques restent hors git. Aucune nouvelle CI distante ou exécution
Google/Linux n'est revendiquée pour ce raffinement local.

## Jalon réel du 2026-11-12

**Des réponses du véritable chemin Rambler ont été obtenues par un client natif
Windows sans Android.** Ce n'est plus seulement une piste statique ou une connexion
gRPC. Aucun téléphone, compte Google, OAuth ou Zwieback utilisé pour ces essais.

Chemin exercé : `speechs3proto2-pa.googleapis.com` / `S3Bidi`, service
`beyond-a2a-recognizer`, routage `gboard_gemini_v3_streaming_voice_edit_mul`,
Ogg/Opus16k mono, contexte tardif, fin S3 puis statut gRPC OK.

## Capacités et niveau de validation

| Capacité | Preuve actuelle |
|---|---|
| Dictée anglaise et française | Service réel, corpus synthétique contrôlé |
| Correction orale « ten… no, eleven » | Service réel, texte corrigé |
| Édition de texte existant par instruction | Service réel, contexte avant curseur et historique édité renvoyé |
| Intentions et résultats d'édition structurés | `VoiceEditingResult` réel, intentions 2 et 3 ; cas historiques et réécriture décodés indépendamment |
| Réécriture polie en contexte | Service réel, intention 3/champ 3 rendue par le SDK ; pas de proposition de remplacement pour cette intention |
| Voix naturelles EN/FR | Premier clip EN puis dix extraits supplémentaires (7 EN, 3 FR), WER et limites documentés ; trois cas aussi progressifs |
| Dictées longues | 40,56 s puis palier 118,68 s et trois sessions de 288,35 s ; source intégrale, marqueurs et finales longues identiques, endurance limitée |
| Hints et langues secondaires | Champs initiaux exposés et dix transports réussis ; pas de gain global ou d'effet universel des préférences démontré |
| Sélection avec emoji | Service réel, contexte non-BMP conservé et sélection `ten`→`eleven` |
| Contexte en trois parties / sélection | Service réel : `ten`→`eleven`, contexte différé appelé après EOF audio, retour avant/sélection/après |
| Sessions successives / canal réutilisé |21/21 sessions du banc accéléré réussies ; audio et texte identiques |
| Alimentation progressive à débit simulé |12/12 sessions réussies : six FFmpeg, six encodeur natif ; PCM décodé identique entre pages dans chaque expérience |
| Annulation, isolation concurrente | Service réel : source fermée après annulation, puis sessions anglaise/française concurrentes sur même canal sans mélange |
| Erreurs tardives et deadlines | Tests gRPC loopback via transport et moteur ; pas d'injection d'erreur dans le vrai service |
| Protobuf inconnu / nouveaux champs | Préservés, dont un champ5 observé dans le service mais absent du schéma optimisé de l'APK |
| Entrée PCM16 avec encodeur natif optionnel | Validée au service réel via CLI : PyAV/libopus, aucun exécutable FFmpeg |

Au jalon précédent, **356 tests locaux passaient** en mode asyncio debug, avertissements traités comme
erreurs ; roue Python construite et installée dans un environnement vierge sans
PyAV. La [CI du jalon0814813](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43304009641)
a réussi sur Windows/Linux et Python3.11/3.14 (tests réseau local uniquement).

Le premier probe, trois cas fonctionnels,21 sessions accélérées,12 progressives,
une CLI PCM, trois cas sélection/concurrence et un smoke de roue installée
représentent **42 sessions complètes réussies**, plus **une annulation volontaire
réussie**. Le diagnostic anonyme initial a été refusé ; il n'est pas compté comme
une session réussie.
Ne pas extrapoler ce corpus à toutes les intentions, langues, qualités de voix ou
conditions réseau. Les réponses de champ `transcript` peuvent déjà être corrigées
ou contenir l'historique complet : ce n'est pas une garantie de verbatim audio.

## Preuves et performance

- [Observations et conditions](evidence/README.md)
- [Premier appel complet](evidence/reconstructed-default-session.json)
- [Correction, contexte, français](evidence/functional-cases.json)
- [Banc accéléré](evidence/performance-baseline.json)
- [Tamponnement source FFmpeg](evidence/realtime-paging.json)
- [Encodeur natif progressif, source et commit hashés](evidence/native-realtime.json)
- [Sélection, contexte différé, annulation et concurrence](evidence/lifecycle.json)
- [Roue installée, configuration locale, sans APK à l'exécution](evidence/installed-wheel-smoke.json)
- [Trois payloads réels publics inspectés](../tests/fixtures/live_synthetic_responses.json)
- [Grille native 20/40/60/100 ms du raffinement](evidence/refinement-page-grid.json)
- [Corpus fonctionnel initial expurgé, bilan historique 4/5](evidence/refinement-capabilities-public.json)
- [Dictée de 40,56 s progressive, copie expurgée](evidence/refinement-long-realtime-public.json)
- [Réécriture après extension du rendu, copie expurgée](evidence/refinement-rewrite-public.json)
- [Roue 0.2.0 finale, installation et smoke offline sans PyAV](evidence/refinement-wheel-smoke.json)
- [Fixture réécriture dérivée et expurgée, non originale](../tests/fixtures/live_rewrite_response.json)

Les **33 réponses des trois premiers cas fonctionnels** ont été redécodées
indépendamment de notre codec Python ; les fixtures publiques correspondantes ne
contiennent que textes synthétiques contrôlés et champs numériques inspectés.
Les preuves ultérieures (sélection, cycle de vie et banc natif) proviennent du
codec moteur et ne sont pas présentées comme une nouvelle inspection binaire
indépendante. Le reste des captures reste local, hors git. Pas de metadata
applicative dans les observations publiques.

Constats préliminaires : batch8192 réduit les messages67→5 et les payloads protobuf
d'environ6,5%, mais pas de gain de latence finale net dans le petit banc. Pages
Ogg20ms au lieu de1000ms : délai médian premier texte après épuisement de source
935→325ms, avec PCM décodé identique et davantage d'octets Ogg. Le banc natif sur
le commit0814813 reproduit le gain : **990→328ms** (fin S3 :991→330ms), trois
répétitions par réglage, aucun processus FFmpeg. Ces durées sont observées côté
client, pas du temps de calcul serveur ; les deux expériences utilisent une
source simulée, pas un microphone réel. Ne pas mélanger les horloges EOF des deux
producteurs (stdout encodé FFmpeg contre lecteur PCM natif).

## Dépendances et limites

- Python>=3.11, grpcio ; le codec d'entrée natif optionnel nécessite Python>=3.12
  et PyAV. Le flux déjà encodé n'en dépend pas.
- Trois métadonnées applicatives doivent être configurées avec une utilisation
  autorisée ; valeurs jamais embarquées dans le dépôt. Pas de provisionnement
  automatique de credentials. Aucun jeton de session temporaire copié à la main
  n'a été nécessaire pendant ces essais.
- Les metadata statiques n'ont pas de refresh observé. Une révocation ou une
  restriction future doit être traitée explicitement, pas contournée ou retryée.
- Zwieback : branche conditionnée par Jetson et `jetson_send_zb`, défautfalse.
  La première décompilation donnait une condition supplémentaire erronée sur
  l'application séparée ; le bytecode a permis de la corriger. Voir la note auth.
- Service interne non contractuel : schéma/routage/disponibilité peuvent changer.
- Pas de PCM brut étiqueté Opus, pas d'API Gemini alternative, pas d'application
  graphique ni d'intégration microphone/environnement dans ce périmètre.
- Nettoyage asyncio suppose des producteurs coopératifs. Les résultats partiels
  ne valent pas une fin de session. Erreurs S3 et statut final gRPC sont séparés.
- Au-delà des corpus naturels limités et des sessions synthétiques de 288 s, qualité
  représentative, endurance plus longue, transport réellement perdu et renouvellement
  de credentials après révocation restent non validés.

## Historique utile

Dépôt initialement vide ; premier lot documentation/wire poussé dans
`4745be3`, branche `feat/rambler-engine`. Analyse statique APK PixelBoard1.3.4
(SHA dans notes recherche), vérification bytecode ciblée, puis preuves serveur
réelles. Le Xiaomi proposé n'a pas été utilisé. La documentation technique et les
mesures restent versionnées avec le code pour permettre les contributions.
