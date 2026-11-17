# État du moteur — 2026-11-16

## Objectif global : backend non terminé

Les jalons et les CI ci-dessous qualifient des périmètres précis, **pas un backend
complet ni « parfait »**. Le jalon Opus et ses 14 jobs verts ne ferment pas les
écarts fonctionnels suivants.

| Domaine | État vérifiable | Ce qui manque pour fermer le point |
|---|---|---|
| Accès durable | Snapshots, remplacement par génération et refus serveur gérés côté client | Contrat légitime de provisionnement, validité/rotation et révocation côté fournisseur ; ne pas inventer un renouvellement OAuth |
| COMPOSE / RAMBLE / RAMBLE_HISTORY | Valeurs décodées et conservées ; pas de sémantique serveur implémentée | Échanges autorisés démontrant leur activation et leur contrat ; un fallback ASR ne les remplace pas |
| Parité Android | Traces statiques et plusieurs comportements natifs observés | Référence appariée service → client → éditeur, sans méthode de hooking/tampering refusée |
| Linux réel avec identifiants | Compilation/SDK loopback et nouveau contrôle public positif/protégé des cores qualifiés dans leurs périmètres | Liaison de la protection au vrai moteur et canal de secrets non qualifiés ; accès réel Linux toujours NO_GO |
| Performance et fiabilité réelles | Gains locaux synthétiques bornés et invariants offline documentés | Pas de preuve de voix naturelle/WAN représentative ni de vitesse optimale universelle |

Sources : [matrice fonctionnelle](research/rust-feature-parity.md),
[authentification](research/authentication.md),
[qualification Linux et limites](research/rust-linux-guarded-loopback.md),
[Opus Release et 14 jobs CI](research/rust-opus-release-build.md).

Les deux audits ciblés du 16 novembre (cycle des credentials et résultats
structurés) ne trouvent pas de manque client corrigeable démontré dans leurs
périmètres ; ce n’est pas une preuve d’absence de défaut. Ils ne fournissent pas de
contrat serveur. L’utilisateur a ensuite renouvelé le 16 novembre l’autorisation
d’effectuer les appels utiles sans demande répétitive par essai, et proposé des
hôtes Linux natifs pour la qualification. Les compteurs et résultats des anciennes
campagnes restent historiques et intacts ; les nouveaux essais doivent être
tracés séparément, sans rejeu automatique d’un RPC commencé ni contournement
auth/TLS/attestation/sécurité. L’étude Opus conserve exactement ses quatre runs.
Aucun gain ou accès durable ne doit être déduit d’une nouvelle série de mocks.
Microphone, GUI et insertion OS restent hors du périmètre moteur initial, pas des
fonctions cloud manquantes.

## Qualification native et référence vocale supervisée — 16 novembre

Le snapshot `82e6394` passe sur un hôte Linux natif non privilégié : workspace
**430/0/2**, construction du vrai CLI audio Release, quatre contrôles de ce
binaire, SDK gardé 9, réutilisation 10 et smoke direct des racines publiques 1.
Les groupes se recouvrent ; ne pas les sommer comme tests distincts. Source
inchangée et nettoyage vérifiés ; aucun credential ou appel ASR réel.
[Preuve et limites](research/rust-native-linux-workspace.md).

Trois cas ont ensuite été observés dans un éditeur Android public distinct,
avec phrases prononcées manuellement : dictée reconnue mais contexte initial non
préservé, instruction de réécriture dictée littéralement, deux callbacks SEND au
total après plusieurs essais du dernier cas. Leur attribution par énoncé n’est
pas isolée et aucun message n’est envoyé par le fixture. Ni route S3Bidi ni parité
wire ne sont déduites de cette interface. Le clavier et les restrictions micro
initiaux sont restaurés. [Observations et réserves](evidence/android-supervised-public-voice-20261116.json).

Le nouvel exemple expérimental d’admission Linux passe **17 tests Fake sous
Windows** et Clippy ciblé ; sa couche native, le canal d’entrée des identifiants
et le mode réel restent non qualifiés. Il n’est pas inclus dans le snapshot Linux
ci-dessus. [Contrat et limites](research/linux-real-admission.md),
[preuve Windows](evidence/linux-real-admission-windows-20261116.json).
**Accès réel Linux toujours NO_GO ; backend toujours non terminé.**

Le premier CI du lot public `81f9a83` termine à **13 jobs réussis / 1 échec** :
les huit matrices Rust, VS17 et Python passent, mais Clippy Linux refuse la
signature FFI de `read`. Correction native ciblée vers `*mut c_void`, sans
suppression du lint ; la validation CI fraîche reste à collecter.
[Échec conservé et delta source](evidence/linux-real-admission-ci-20261117.json).

## Linux natif — contrôle public des cores réussi

Sur un nouvel hôte Linux x86_64, le témoin public produit un core borné et le bit
`WCOREDUMP` ; le cas protégé ne produit ni fichier ni bit et refuse l’ouverture
externe d’`environ`, sans lecture. Exactement deux cibles sont lancées puis
récupérées. Les 25 contrôles purs séparés, le nettoyage et une vérification
ultérieure en lecture seule passent. Aucun credential, appel service ou réglage
global. La protection testée combine dumpable=0 et core limit=0 ; ni effet isolé
de dumpability ni admission du vrai moteur avec secrets n’en sont déduits.
L’échec WSL historique reste inchangé ; **Linux réel avec identifiants NO_GO**.
[Protocole et limites](research/linux-public-core-control.md) ·
[Preuve expurgée](evidence/linux-public-core-control-20261116.json).

## Enveloppes gRPC — CI des huit configurations natives qualifiée

Les 13 jobs de la source exacte `8cb381e8d26d448db80481d4aab9472aee3b096c`
réussissent : [Rust](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43859489195)
et [Python](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43859489085).
Windows, Linux, macOS ARM et Intel passent sur Rust 1.88 et stable, y compris le
job Intel/1.88 précédemment en échec. Les 72 groupes Rust sont vérifiés séparément :
workspace **429/0/2** sous Windows, **430/0/2** ailleurs. Les sept nouveaux noms de
coalescence, dont deux `should_panic`, les quatre HPACK et les oracles antérieurs
sont confirmés dans les huit workspaces ; API/provenance et 1605020 octets de logs
audités. [Preuve CI distincte](evidence/rust-grpc-envelope-coalescing-ci-20261116.json).
Ces résultats ne reconstituent pas la cause du job ancien et ne mesurent aucun
optimum ou gain global de performance. G1 Linux et accès durable restent inchangés.

**Checkpoint local et échec historique distincts :**

La CI de `70bbf3b` reste **échouée : 12/13 jobs réussis**, oracle de framing sur
macOS Intel/Rust 1.88 en échec avec GOAWAY `ENHANCE_YOUR_CALM`. Les quatre tests
HPACK passent dans ce même job. [Échec conservé](evidence/rust-credential-hpack-wire-ci-failed-20261116.json).
La cause exacte de cette trace n'a pas été capturée ; les analyses statiques
soutiennent un mécanisme lié aux petites DATA séparées, pas une preuve wire historique.

Le candidat regroupe préfixe et payload dans un buffer chaîné privé, sans tampon
de concaténation du payload, batching entre messages ou modification des limites.
Les oracles antérieurs de 260 messages et fenêtre trois octets restent inchangés.
Windows stable/1.88 : transport **126/0/1** chacun, nouvelles intégrations **2/0/0**
chacune, workspace **429/0/2**, SDK9 et feature-off38 ; format/Clippy strict passent.
Les 14 commandes et jobs réussissent, 188 sources avant/après identiques. Source
qualifiée : candidat non committé sur `70bbf3b` ; nouvelle CI distante encore en
attente à ce checkpoint. Aucun gain global de temps/allocation ni accès durable,
parité complète ou Linux live déduit. [Contrat et limites](research/rust-grpc-envelope-coalescing.md) ·
[Preuve positive locale](evidence/rust-grpc-envelope-coalescing-windows-20261116.json).

## HPACK des métadonnées — qualification locale Windows S1

Un oracle indépendant décode les frames et HPACK/Huffman selon la RFC 7541 sur
une trace loopback native : trois RPC simultanées, credentials fictifs en rotation
puis retrait, un TCP par fixture. Il vérifie la liaison exacte aux streams, le
codage never-indexed, le nom statique 23, une table dynamique sans entrée sensible
et l'ordre des valeurs par nom. Vingt mutations de la capture réelle du test et
les préfixes négatifs contrôlent l'oracle ; aucun audio, service ou credential réel.

Les **quatre tests** ciblés passent sur Windows stable 1.99.1 et 1.88 ; transport
**119/0/1** sur chaque toolchain, workspace stable **422/0/2**, feature-off
**38/0/0**, fmt et Clippy transport strict toutes features passent. Ces groupes
ne sont pas à additionner. QA1 a échoué sur un lint Clippy du helper hexadécimal ;
sa correction limitée au test précède QA2 réussi, échec initial conservé. Les 187
sources sélectionnées restent inchangées avant/après QA2. Source qualifiée :
candidat mesuré non committé sur `cb7c707` ; CI distante encore en attente à ce
checkpoint local, aucune modification de production.
[Contrat et limites](research/rust-credential-snapshots.md#inspection-hpack-brute--qualification-locale-windows-s1) ·
[Preuve locale](evidence/rust-credential-hpack-wire-windows-20261116.json).

S1 est une observation finie : un reset ne prouve pas l'absence d'octets non lus.
S2/CONTINUATION, éviction, mises à jour de table, TLS, isolation universelle,
performance et accès durable restent hors qualification. G1 Linux reste non satisfait,
live NO_GO ; budget réel Windows MAX1 déjà consommé et téléphone inchangés.

## Préfixes gRPC — allocation retirée sur le chemin court mesuré

Une table immuable de **1280 octets de données source** remplace l'allocation du
préfixe de cinq octets pour les messages0–255 ; le payload était déjà zéro-copy,
le chemin de repli et le contrôle de flux sont inchangés. Deux runs sur le même
binaire confirment **1 → 0 allocation/libération** pour les onze tailles courtes
mesurées ; cinq contrôles de repli identiques en compteurs, sans dépassement du
seuil de régression temporelle répétée. Aucun gain global de dictée attribué.

Sur le vrai pipeline PCM/Opus synthétique + enveloppe S3, chaque signal avec pages
20ms donne **253/253 messages éligibles**, même à la largeur d'elapsed maximale
testée. Contrôle1000ms :3/8. Ce n'est ni une voix représentative ni un hit serveur.
Framing brut260messages et préfixe réellement coupé à3octets par fenêtre distante
passent sur stable/1.88. Transport **115/0/1**, banc **7/0/0** chacun, workspace
stable **418/0/2**, SDK9, feature-off38, format/Clippy/build passent. Échec lint du
premier lot conservé ; CLI historique et budgets inchangés. Ces mesures locales
restent un point de contrôle historique, où la CI était encore en attente.

CI distante hors ligne désormais réussie sur la source exacte `98d46590ec708c7e1ae6157323a07b8eaa41ddd7` : [Rust](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43832001772) et [Python](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43832001623), 13 jobs dont 8 natifs et 72 groupes de commandes Rust, non des tests distincts. Par toolchain, workspace Windows **418/0/2**, Linux et macOS **419/0/2** ; macOS ARM et Intel confirmés. Par job natif : banc préfixe **7/0/0**, SDK gardé **9/0/0**, banc wire **3/0/0** ; ne pas sommer les groupes rejoués. La CI vérifie les invariants, la matrice H2 brute, la fragmentation forcée et les tailles PCM/Opus synthétiques, pas les temps ni les allocations locales, la voix réelle ou la performance du service. G1 reste non satisfait, Linux live NO_GO ; aucune qualification nouvelle des credentials, téléphone, core ou provisioning.

[Contrat et limites](research/rust-grpc-prefix-allocation.md) ·
[Preuve locale historique](evidence/rust-grpc-prefix-allocation-windows-20261116.json) ·
[Preuve CI distante](evidence/rust-grpc-prefix-allocation-ci-20261116.json).

## Optimisation de sérialisation — CI des huit configurations natives qualifiée

Le commit `43e918d` passe les **13 jobs CI** : Windows, Ubuntu, macOS ARM64 et
Intel × 1.88/stable, lint et quatre Python. Workspace **411/0/2** sous Windows,
**412/0/2** ailleurs ; les cinq nouveaux noms fidélité/fixed32 et le gate de trois
tests du banc sont confirmés dans les huit jobs natifs. Les 64 groupes Rust,
1 545 494 octets de logs, métadonnées, étapes, hashes et hosts sont vérifiés.
[Preuve CI du commit exact](evidence/rust-wire-encoding-allocation-ci-20261116.json).

Les gains de compteurs restent ceux des deux mesures Windows locales : aucun
nouveau timing en CI ni gain WAN/moteur complet attribué. La qualification locale
« CI en attente » ci-dessous est historique ; accès durable, parité Android et
G1/Linux live ne sont pas résolus par cette optimisation.
[Mesure et limites](research/rust-wire-encoding-allocation.md).

## Sérialisation protobuf — gain d'allocations mesuré (checkpoint local historique)

Les encodeurs génériques écrivent sans vecteurs temporaires, avec octets, erreurs
et bornes préservés. Deux runs synthétiques sur le même binaire confirment les
149 identités et des compteurs identiques ; aucune régression d'appels/tailles
d'allocation, y compris dans les 11 assemblages complets init/contexte. Init par
défaut : **60 → 34 appels à l'allocateur** ; avec hints : **102 → 59**. Ce ne sont
ni des pics mémoire ni une mesure de vitesse du moteur complet/réseau.

Windows passe protocole **75/0/0** et banc **3/0/0** sur stable/1.88, workspace
stable **411/0/2**, feature-off **38/0/0**, fmt/Clippy strict/build. Le premier
échec de compilation d'une fixture du banc est conservé, corrigé avant les deux
mesures. Production modifiée seulement dans les encodeurs de champs ; décodeur,
requêtes, audio/codec et Cargo inchangés. CLI historique non reconstruite/invoquée,
budgets inchangés. CI distante non encore exécutée pour ce candidat ; les preuves
CI plus bas portent sur leurs commits exacts.
[Contrat et limites](research/rust-wire-encoding-allocation.md) ·
[Preuve locale et toutes les valeurs brutes](evidence/rust-wire-encoding-allocation-windows-20261116.json).

## SDK gardé — Linux natif offline et CI qualifiés

La source `aaf56df` passe les **13 jobs CI**, dont huit natifs Windows, Ubuntu,
macOS ARM64 et Intel × Rust 1.88/stable. Chaque gate de l'exemple passe **9/0/0** ;
workspace Windows **406/0/2**, autres **407/0/2**. Les 56 groupes de commandes et
1 516 223 octets de logs sont vérifiés avec provenance, sans sommer les rejeux.
[Preuve CI](evidence/rust-linux-guarded-loopback-ci-20261116.json).

Une exécution séparée sous Linux x86_64/Rust 1.88 passe les neuf tests et les
quatre cas du vrai exécutable : no-op, canary public avec SDK/codec/transport
loopback, entrée invalide et EOF retenu avec arrêt/récupération de l'enfant possédé.
Trois refus externes d'ouverture d'environ sans lecture, trois propriétés proc
root et neuf observations de cleanup sont confirmés ; job du pont Windows vérifié
séparément. Les 178 sources sélectionnées et les binaires restent inchangés.
[Contrat, historique et preuve native](research/rust-linux-guarded-loopback.md#completed-linux-and-ci-qualification--16-november-2026).

Aucun credential, service réel, téléphone, réservation, crash provoqué ou réglage
global. Il s'agit d'un exemple de développement à entrée publique fixe, pas d'un
receiver de secrets ni d'une qualification TLS/ASR : **G1 reste non satisfait et
Linux live NO_GO**. La qualification Windows ci-dessous est un checkpoint antérieur.

## Contexte applicatif — frontière wire précisée, pas nouvelle fonction inventée

Le chemin Android statique rejoint bien S3 via InputBox1203/champ5, déjà encodé
par `ProtocolConfig::preferences`. Aucun champ protobuf supplémentaire manquant
n'est établi. La collecte OS, ses gates et son formatage restent absents ; effet
serveur et activation runtime non observés. Aucun essai distant ni changement de
production. [Trace et limites](research/rust-application-context-boundary.md).

## SDK complet derrière admission simulée — Windows qualifié (checkpoint historique)

Le nouvel exemple compose réellement PCM/Opus, Engine et transport vers un pair
numérique loopback, avec ordre init/audio/contexte/END, CRC/décodage et nettoyage
vérifiés : 1017 échantillons, 2034 octets PCM, 3051 échantillons après trim à 48 kHz.
Ses **9 tests** passent sur Windows stable et Rust 1.88 ; workspace stable
**406/0/2**, feature-off 38, fmt, Clippy CLI strict et build de l'exemple passent.
Le binaire Windows vérifie no-op et refus natif unsupported avant toute lecture
ou connexion. Aucun durcissement du processus de test, ni appel service réel.

Seul l'exemple/sa registration et son gate CI sont ajoutés ; production et lock
inchangés. Le mode natif Linux de ce nouvel exemple reste à exécuter séparément,
la sonde précédente ne le qualifie pas. [Contrat et preuve Windows](research/rust-linux-guarded-loopback.md).
G1 reste non satisfait ; aucun budget ou accès réel supplémentaire déduit.

## Préprotection Linux — observation offline, pas admission live

Un [prototype Rust autonome à entrée publique fixe](../tools/reference/LINUX_PRECREDENTIAL_CANARY.md)
passe ses 12 tests simulés sous Windows stable/1.88 et Linux 1.88. Sur Linux,
no-op, acceptation littérale, refus et arrêt de l'enfant bloqué passent ; trois
ouvertures externes de son `environ` sont refusées à l'observateur non privilégié,
avec propriété proc root. Cleanup invité et job de pont Windows sont vérifiés.
Zéro credential, service, réservation, téléphone, crash core provoqué ou réglage
global modifié. Ce code de référence est hors workspace Cargo, pas une intégration
production ni une nouvelle qualification de la CI cœur.

Ces effets d'accès ne prouvent ni suppression réelle des cores ni absence de
lancement du collecteur : **G1 reste non satisfait et Linux live NO_GO**. Aucun
budget n'est réinitialisé. [Preuve et limites](evidence/linux-precredential-canary-offline-20261116.json).

## CI des refus précoces — source `1d66138` qualifiée

Les 13 jobs Rust/Python réussissent, dont huit natifs sur Windows, Ubuntu,
macOS ARM64 et Intel × Rust 1.88.0/stable. Les quatre scénarios sont confirmés
par nom dans chaque workspace : Windows **406/0/2**, autres OS **407/0/2**.
Les 48 groupes Rust et 1 494 689 octets de logs sont vérifiés par commande et
provenance, sans sommer les réexécutions. Lint, gates release/racines/PCM et
quatre jobs Python passent. [Preuve CI du commit exact](evidence/rust-credential-rejection-early-ci-20261116.json).
Les checkpoints locaux « CI non exécutée » ci-dessous sont historiques ; aucune
nouvelle dictée, authentification distante ou récupération après révocation déduite.

## Refus précoces — qualification locale, sans nouveau service

Deux tests supplémentaires couvrent un refus headers-only avant EOF pendant
qu'un voisin est encore en upload ; remplacement explicite puis drainage intégral
du voisin passent. Les quatre scénarios passent sous Windows : transport 109/0/1
sur stable 1.99.1 et Rust 1.88.0, workspace stable toutes features 406/0/2,
fmt et Clippy transport strict. Aucun changement de production ou de binaire release.
[Contrat, preuve et exclusions](research/rust-credential-snapshots.md#refus-headers-only-pendant-un-autre-upload--extension-locale).
La CI ci-dessous porte sur le commit précédent, pas cette extension locale.

## CI des refus après upload — source `0e5ce23`

Les 13 jobs Rust/Python ont réussi. Huit jobs natifs sur Windows, Ubuntu,
macOS ARM64 et Intel × 1.88.0/stable : workspace 404/0/2 sous Windows,
405/0/2 ailleurs ; les deux tests de refus après upload passent dans les huit.
Les 48 groupes Rust et 1 492 824 octets de logs sont vérifiés par commande et
provenance ; pas une somme de tests distincts. Les quatre jobs Python et le lint
passent aussi. [Preuve CI liée au commit exact](evidence/rust-credential-rejection-ci-20261116.json).
Ni nouveau TLS/service réel ni provisionnement/récupération après révocation déduit.

## Refus d'authentification — isolation offline

Deux nouveaux tests loopback couvrent les refus `UNAUTHENTICATED` et
`PERMISSION_DENIED`, le voisin actif, les métadonnées fictives remplacées
explicitement et la session suivante sur le même TCP. Listener surveillé,
stream IDs et messages exacts contrôlent l'absence observée de reconnexion/rejeu ;
sources nettoyées et transport fermé. Aucun changement de production.
Windows passe 107/0/1 au transport sur stable 1.99.1 et MSRV 1.88.0, puis
404/0/2 au workspace stable toutes features ; fmt et Clippy transport strict passent.
Comptes par commande, pas une somme de tests distincts. CI distante/Linux/macOS
non exécutés pour ce lot à ce checkpoint. Refus après upload complet uniquement :
ni provisionnement durable ni récupération réelle après révocation déduits.
[Contrat et limites](research/rust-credential-snapshots.md#refus-du-pair-remplacement-explicite-et-voisin-actif--16-novembre-2026) ·
[Preuve locale](evidence/rust-credential-rejection-offline-20261116.json).

## Régression native release — résultat observé, réservation immuable

Une unique opportunité et capture natives ont été consommées avec succès, sans
rejeu ni retry : contrôleur gelé `78ce29130fef8ac828d0dac58e7bc2183ddc8861a985ab01c2f9eb753a5bc1f4`,
`REGRESSION_HEALTHY_STRICT`, sortie processus 0 et qualification du candidat après
cette sortie. Gardes et provenance passent. Les 20 groupes offline, le préflight,
le sentinel renforcé et une génération locale SAPI supervisée ont précédé l'essai.
Le NO_GO historique reste correct pour son instant : la barrière de confidentialité
est depuis remédiée, sans administration, changement de réglages ou ancienne ACL.

La CLI release capturée correspond au HEAD `703f0eee82f6228044115f72be5744aba7c08d6f`
et à l'association de production `7327f51b89aeec537e8bf5ebaf6db3b0c25e5cef` ;
ce n'est ni le binaire du banc ni une attestation CI. Sur une entrée synthétique
de 4,34 s, la session client non cadencée prend 2,2043591 s : ni gain WAN ni
qualité représentative. Les 223 requêtes sont des messages protobuf, pas des RPC ;
leur nombre côté serveur demeure inconnu. Les flags terminaux sont inférés du
contrat de succès CLI vérifié, sans capture wire indépendante. Intent 2/champ actif 7
est une requête littérale stricte, pas COMPOSE/génération ; les contrôles de tokens
sont mécaniques et la sémantique de message généré reste inconnue.

La [réservation initiale](evidence/rust-native-release-regression-20261115-budget.json)
conserve `not_observed_at_reservation` ; le [résultat séparé](evidence/rust-native-release-regression-20261116.json)
ne la réécrit pas. [Méthode et limites](research/rust-pcm-channel.md#régression-native-release--résultat-distinct-de-la-réservation--16-novembre-2026).
Linux live, téléphone, autres intentions, révocation, incident long et limites
d'accès restent ouverts ; ni perfection, provisionnement durable ou parité Android.

## CI attribution codec — qualification de la source

Les 13 jobs Rust/Python du commit `72fd4affa561619745e9d642500467d41364458f`
ont réussi. Les huit jobs natifs passent chacun le nouveau gate de huit tests de
l'exemple ; aucune campagne de chronométrage n'est exécutée en CI. Cette
qualification couvre tests, builds et contrôles, sans changement des fonctionnalités
de production ni preuve de gain, d'ASR réel, de parité ou d'autorisation live.
La mesure Windows locale reste distincte et l'ISA native liée n'est pas attestée.
[Preuve CI](evidence/rust-codec-attribution-ci-20261115.json) ·
[protocole et limites](research/rust-audio-profiling.md#qualification-ci-distante-du-banc--2026-11-15).

## Opus Release Windows — gain local borné et contrôle VS17

Une nouvelle comparaison fraîche et indépendante de l'étude CRC rejetée mesure
`native_frames` 3,0–3,6× plus rapide et le pipeline audio 67,4–71,6 % plus court
sur trois signaux synthétiques Windows, à fidélité inter-build identique.
Quatre runs uniquement ; aucun gain WAN/ASR ou optimum global revendiqué.
Le Debug/MSRV local et les **14 jobs CI** du correctif `2334b76` passent, dont
le contrôle VS2022 explicite. Deux jobs ARM ont été relancés une seule fois après
un manque de capacité GitHub, avant toute exécution initiale ; incidents conservés.
[Protocole, incidents et limites](research/rust-opus-release-build.md) ·
[588 mesures brutes](evidence/rust-opus-release-build-windows-20261116.json).

## Répertoire privé de validation — remédiation locale

Un nouveau répertoire est créé et accepté par le contrôle strict d'origine,
sans modification des permissions existantes. Les 30 tests synthétiques passent ;
le contrôle réel confirme que le verrouillage empêche une ouverture avec droit de
suppression pendant sa tenue. Aucun renommage, suppression, identifiant chargé
ou appel Rambler. L'intégration du lanceur reste distincte de cette admission.
[Preuve et limites](evidence/rust-private-capture-root-20261115.json).

## CI budgets et fermeture — nouveau résultat

Sur `171ef084cc1b1e7f3b82daefdcd6bb0aadafe442`, les 13 jobs Rust/Python
ont réussi, dont huit natifs sur quatre familles et deux toolchains. Chaque
workspace rapporte 402/0/2 sous Windows, 403/0/2 ailleurs (réussis/échecs/
ignorés) ; les quatre autres groupes rapportent respectivement 10, 1, 3 et
38 réussites. Le groupe de trois vérifie le refus des budgets CLI zéro sur le
binaire release. Ces groupes rejoués ne sont pas des tests distincts à sommer.
La régression de fermeture passe dans les huit workspaces : le premier close
borné accepte uniquement succès ou erreur indépendante `Connect`, puis le
second doit réussir ; les assertions de propriété et de non-rejeu restent.

Le run Rust de `7327f51` a échoué sur trois jobs macOS à cause de l'ancienne
attente stricte sur close, et reste un échec historique, pas une requalification.
Aucun changement de production entre ces deux commits. Les limites de service
réel, téléphone et liaison noyau Linux restent inchangées.
[Preuve détaillée et provenance](evidence/rust-close-ledger-zero-budgets-ci-20261115.json).

## CI PCM et admission — matrice étendue (historique)

Sur `4264126852b2c03e25fc231c60f9582569a84ae9`, les 13 jobs Rust/Python
ont réussi : huit natifs, lint Rust et quatre Python. Windows rapporte 399
réussis, 0 échec, 2 ignorés au workspace par toolchain ; Linux et macOS ARM64
et Intel 400/0/2. Les deux jobs `macos-26-intel` sont natifs
`x86_64-apple-darwin`, Rust 1.88.0 et stable 1.100.0. Chaque job natif passe
également réutilisation 10, finalisation release exacte 1 et feature-off 35 ;
ces réexécutions ne sont pas additionnées comme tests distincts. PyAV Rust
reste ignoré par défaut. Les contrôles Python stdlib et pytest sont distincts.

Le lanceur de racines publiques rapporte son marqueur après sélection unique et
sortie zéro du test direct, sans exposer son stdout libtest ; aucun handshake
n'est revendiqué. Le PCM release synthétique rapporte 1017 échantillons en
entrée, 3051 décodés, un EOS et zéro appel service. Aucun nouvel appel réel,
clé privée, audio privé ou preuve de version minimale macOS. Le blocage du live
Linux reste ouvert sans nouvelle tentative ; téléphone inchangé à 0/3 phrases.
[Preuve, comptes par job et provenance](evidence/rust-pcm-admission-expanded-ci-20261115.json).
Le checkpoint local ci-dessous conserve ses échecs et limites historiques.

## PCM et admission stricte — qualification locale du 15 novembre

Sur le snapshot A (base `1813602` et changements locaux), le workspace passe
399 tests, 0 échec, 2 ignorés sur Windows stable et 1.88, et 400/0/2 sur
Linux 1.88. Le contrôle Windows R1 reste cependant **non qualifié** : Clippy
strict échoue sur `never_loop` dans un test, pas sur une régression runtime.
Sur B, seule cette boucle redondante de test est retirée : Windows repasse les
6 tests progress sur chaque toolchain, fmt et Clippy strict ; Linux repasse
400/0/2. Pas de relance complète Windows ni de rebuild release sur B ; les
preuves release restent celles de A, malgré des octets de production identiques.

La CLI exige un snapshot strict avant connexion et attend la fermeture de l'entrée
si elle le rejette ; bibliothèque permissive et loopback explicite restent inchangés.
Les compteurs PCM optionnels ne sont ni hashes, ni acquittements, ni garantie
contre les pertes ; l'EOF naturel reste distinct d'un close ou d'une erreur.
[Qualification et limites](research/rust-build-validation.md#pcm-progress-and-strict-admission--local-checkpoint-2026-11-15) ·
[Preuve](evidence/rust-pcm-progress-admission.json).

Aucun nouvel essai service ou appareil : le préflight Linux live est bloqué par
un collecteur de crash dumps observé, sans suppression vérifiée, credentials
chargés, réservation, RPC ou modification système. Ce blocage ne signifie pas
que le moteur Linux est unsupported. Le téléphone reste à 0 armement, 0 voix,
0 phrase sur 3. Le nouveau code n'est pas encore qualifié sur macOS, ni commité
ou poussé à ce point ; aucune CI distante de ce code n'est revendiquée.

## Qualification CI native et racines publiques — code `9069059`

Les deux runs CI3 du push `906905988c5c51d83b05b259cd642a165325b7d2`
sont **completed/success**, vérifiés par API et logs : six jobs natifs
Windows/Linux/macOS × Rust 1.88.0/stable, lint Rust et quatre jobs Python.
Stable est effectivement **1.100.0** ; macOS est **ARM64 uniquement**
(`aarch64-apple-darwin`), pas une qualification Intel.
Les comptes ci-dessous sont **par job et par commande**, sans somme des répétitions :

| Plateforme, chaque toolchain | Workspace | Exemple reuse | CLI sans audio | CLI release finalisation |
|---|---:|---:|---:|---:|
| Windows |382 réussis, 0 échoué, 2 ignorés|10|31|1|
| Ubuntu |383 réussis, 0 échoué, 2 ignorés|10|31|1|
| macOS ARM64 |383 réussis, 0 échoué, 2 ignorés|10|31|1|

Le workspace ignore **l'oracle Rust PyAV et le smoke racines**. Ce dernier
est ensuite exécuté explicitement dans chacun des six jobs par le helper direct :
une unique entrée de test ignoré est imposée, puis le processus du test exact
termine à zéro. Le marqueur exact est
`Rust native roots OK: exact ignored smoke passed via direct test executable`.
Le helper capture stdout libtest : aucun résumé Rust brut « 1 passed » n'est
revendiqué. L'oracle Rust PyAV reste **non exécuté** dans cette matrice.

Cela qualifie réellement le chargement des **racines publiques OS** et la
construction de la configuration TLS de production avec ALPN `h2` ; les assertions
Rust exigeant l'absence de `SSL_CERT_FILE` et `SSL_CERT_DIR` restent inchangées.
Le helper lance directement l'exécutable avec une copie d'environnement nettoyée,
sans faire lancer le test par Cargo. Ce n'est **pas** un handshake avec un pair,
une validation de chaîne/hostname, une rotation, une preuve d'exhaustivité des
racines, ni un test d'authentification, de clés privées ou du service.

Les **23 unités stdlib du launcher** passent dans chaque job natif (avec les
17 unités du checker audio). Elles sont aussi incluses dans pytest, pas à ajouter :
Python 3.11 passe **663 tests, 71 ignorés** par OS ; Python 3.14 **733 tests,
1 ignoré** par OS. Les **335 sous-tests** sont rapportés séparément.
Build CLI audio release et smoke PCM passent dans les six jobs
(1017→3051 échantillons à 48 kHz, un EOS, zéro appel service).

Les deux échecs Ubuntu CI2 et Linux local R1 sont antérieurs au loader :
précondition d'environnement, injecteur exact non établi, pas un défaut production
des racines démontré. Le correctif `9069059` ne change que launcher Python,
workflow et ses tests ; Rust TLS/assertions/Cargo restent inchangés.
Les preuves FAILED restent conservées ; les résultats locaux et leurs limites
sont distincts de cette CI3. Aucun nouvel appel natif/service/appareil ;
aucun gain WAN, parité Android ou provisionnement/révocation déduit.
L'observation Unicode `🙂`→`😎` ci-dessous reste sémantiquement non confirmée.
[Preuve CI3, comptes et hashes](evidence/rust-native-roots-ci-20261115.json) ·
[Historique du runner](evidence/rust-native-root-runner-history.json) ·
[Validation détaillée](research/rust-build-validation.md).

## Qualification CI native — code `1561f87` (historique)

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
