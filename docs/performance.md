# Mesures de performance et choix retenus

Mesures initiales du 2026-11-12. Il ne s'agit ni d'un classement de langages ni
d'une garantie de latence du service. Chaque réponse comptée réussie a un texte
exploitable, une fin S3, un gRPC OK et une entrée audio complète.

## Conditions et horloges

Windows11, Python3.14.5, grpcio1.85.1, 12 processeurs logiques ; détails et limites
[dans l'environnement](evidence/environment.json). Internet ordinaire, charge
machine et réseau non contrôlés, aucune instrumentation du serveur. Horloge
monotone côté client. Le corpus est la même voix synthétique Zira, phrase anglaise
de6,658s (WAV), conteneur Ogg/Opus de6,665s, entrée compressée14 136 octets dans le
premier banc. Ce n'est pas un corpus de voix naturelles.

Les premiers rapports proviennent d'un arbre de travail expérimental avant le
commit de consolidation : ils ne portent pas rétroactivement un hash de code
exact. Cette limite est conservée, pas dissimulée. Les outils publics ajoutent
maintenant provenance du code, versions et configuration aux nouveaux rapports.
Les payloads fonctionnels ont été vérifiés indépendamment ; leurs hashes et trois
captures originales inspectées sont versionnés.

Mesures distinctes :

- `first_any_response` peut être un heartbeat, pas du texte utilisable.
- `first_text` est le premier candidat textuel ; une erreur terminale ultérieure
  invaliderait la réussite de la session.
- Fin S3 et EOF gRPC sont séparés. Le résultat final ne se déduit pas d'un partiel.
- `last_audio_yield` / épuisement de source sont des timestamps **producteur**,
  pas des preuves de dernière écriture réseau terminée ni d'acquittement serveur.
- Les octets comptés sont des payloads protobuf ; pas le trafic TLS/HTTP2 exact.
- Sans instrumentation distante, temps réseau et calcul serveur restent mêlés.

## 1. Fichier préenregistré, envoi accéléré

[21 sessions originales](evidence/performance-baseline.json) : un warmup exclu des
agrégats, puis quatre groupes de5 essais, ordre alterné, concurrence1. Tous ont
réussi ; SHA-256 de l'audio consommé identique, texte et intention identiques.

| Canal API | Bloc Ogg | Premier texte médian | EOF médian | Payload requête médian | Messages |
|---|---:|---:|---:|---:|---:|
| Neuf | 224o | 3114ms | 3117ms | 15 317o | 67 |
| Neuf | 8192o | 3153ms | 3155ms | 14 327o | 5 |
| Réutilisé | 224o | 3171ms | 3174ms | 15 317o | 67 |
| Réutilisé | 8192o | 3117ms | 3120ms | 14 327o | 5 |

**Gain démontré :** 62 messages en moins et990 octets protobuf économisés,
soit6,46% des payloads requête sur cet audio. Le surcoût protobuf hors audio passe
de1181 à191 octets, environ84% de moins. Le cadrage gRPC minimal de5octets/message
économiserait310octets supplémentaires, mais HTTP2/TLS n'a pas été capturé.

**Gain non démontré :** accélération de la latence finale. Les écarts changent de
sens selon le groupe ; les distributions se recouvrent. Le p95 nearest-rank à n=5
est simplement le maximum du groupe, pas une estimation fiable de longue traîne.

**Neuf ne signifie pas nouvelle connexion physique.** Le canal réutilisé reste
ouvert ; gRPC C-core peut partager ses sous-canaux avec les canaux API neufs.
L'ouverture initiale du warmup vaut environ51ms, puis les ouvertures API médianes
environ3ms. Ces3ms ne sont donc pas un handshake TLS froid. Processus froid,
DNS froid et serveur froid ne sont pas comparés ici.

CPU du processus de mesure : médianes15,625–31,25ms par groupe, en marches visibles
de15,625ms sur Windows. Cela inclut lecture/hash et capture du banc, ainsi que le
client natif. Aucun gain CPU comparatif n'est établi avec cette granularité.
High-water RSS du processus environ47,3→49,0MiB sur21 sessions : mesure globale,
pas pic par session ni preuve d'absence de fuite en endurance longue.

**Choix :** exposer le réglage batch8192 pour les fichiers disponibles. Ne jamais
attendre artificiellement8192octets d'une source en direct : cela ajouterait une
latence d'accumulation. Réutiliser le moteur/canal sans partager l'état des streams.

## 2. Source progressive et tamponnement Ogg

[Six sessions originales](evidence/realtime-paging.json), trois par configuration,
ordre alterné. Même WAV, encodeur FFmpeg/libopus24kbps, mono16k, trames20ms,
`-re`, `-flush_packets 1`. Seule durée de page comparée :1000ms contre20ms. Un canal
ouvert, aucune relance. Toutes les sessions ont réussi avec le **même texte**.

| Réglage page Ogg | Premier événement de parole médian | Texte après épuisement source, médiane | Fin S3 après épuisement source, médiane | Ogg produit |
|---|---:|---:|---:|---:|
| 1000ms | 1663ms | 935ms | 937ms | 14 136o |
| 20ms | 216ms | 325ms | 325ms | 22 965o |

**Gain observé :** environ610ms, soit65% de moins pour le premier texte après
épuisement de la source. La fin S3 évolue de façon cohérente. **Fidélité :** les
six flux réenregistrés donnent exactement le même SHA-256 du PCM redécodé ; aucune
modification audio attribuable au changement de pages.

**Contrepartie :** environ62% d'octets Ogg en plus à cause des pages courtes.
Ce n'est pas une optimisation de bande passante. Le gain vient du tamponnement de
l'entrée, pas d'un changement du modèle distant ou d'une accélération démontrée
de son calcul. Aucun texte exploitable avant la fin n'a été observé dans ce petit
corpus, même si les marqueurs de parole arrivent plus tôt.

**Limite de simulation :** la source FFmpeg `-re` s'épuise vers6,15s pour6,66s
d'audio encodé ; ce n'est pas un microphone ni une simulation parfaite du temps
d'arrivée de chaque échantillon. Les deltas après épuisement sont comparables dans
l'expérience ; ne pas les appeler automatiquement « après la fin acoustique ».
Ils incluent réseau, tamponnement restant et service. Le CPU de l'encodeur externe
n'est pas le CPU du seul moteur ; aucune conclusion CPU n'est tirée ici.

**Choix :** profil natif optionnel PCM→Ogg/libopus avec pages20ms, mémoire bornée
et flush final explicite. La preuve FFmpeg ne valide pas cette implémentation
native à elle seule : elle doit avoir ses propres tests et sessions réelles.

## 3. Profil natif réellement exercé

[Six sessions natives](evidence/native-realtime.json) ont été exécutées avec le
code du commit `0814813` : les hashes exacts des modules et du banc sont enregistrés.
Le hash du banc a été vérifié contre le contenu du commit. Le flag `git_dirty`
inclut le nouveau fichier d'observation ; il ne signifie pas que les modules
mesurés diffèrent de ces hashes.

Aucun processus FFmpeg : lecteur WAVE PCM16 mono16k par20ms, encodeur PyAV/libopus,
source progressive, un canal prêt, trois paires alternées. Les six sessions ont
consommé exactement213 070octets PCM avec le hash de l'entrée ; leurs PCM redécodés
ont tous la même longueur et le même hash. Opus est lossy : le hash décodé est
comparé **entre configurations**, pas déclaré identique au PCM original.

| Pages Ogg natives | Événement de parole médian | Premier texte après EOF lecteur PCM | Fin S3 après EOF lecteur PCM | Ogg produit |
|---|---:|---:|---:|---:|
| 1000ms | 2150ms | 990ms | 991ms | 14 126o |
| 20ms | 344ms | 328ms | 330ms | 22 955o |

Les trois délais texte après EOF valent981/1036/990ms pour1000ms, contre
328/407/324ms pour20ms (arrondis). Le gain médian est environ**67%**, toujours sur
petit corpus et réseau non contrôlé. Les textes/intention sont identiques. Ce
résultat confirme le choix20ms dans l'implémentation livrée, pas seulement dans un
producteur FFmpeg de test. Le compromis réseau reste réel : Ogg environ63% plus
gros,75→339 messages dans ce banc. Le banc subdivise les écritures du muxer en
fragments d'au plus224octets ; il n'attend pas de remplir224octets sur le direct.

L'EOF natif est l'épuisement du **lecteur PCM**, suivi du flush de l'encodeur ;
l'EOF du banc FFmpeg était celui du **stdout déjà encodé**. Ne pas fusionner ces
deux populations ni présenter les deltas comme un temps serveur. Le PCM source
natif s'épuise vers6,64s pour6,66s audio : pacing de début de chunk, pas microphone.
Aucun texte exploitable avant fin n'a été observé sur ces phrases.

Le processus du banc natif20ms consomme171,875ms CPU en médiane pour une phrase
de6,66s, avec93,75–390,625ms observés : encodage, moteur, hash et redécodage de
vérification inclus. Granularité Windows et variabilité empêchent un gain CPU
comparatif fiable. La file de sortie Python de l'encodeur a une limite64KiB,
les chunks PCM entrants une limite8MiB ; ces bornes ne décrivent pas toute la
mémoire native de PyAV/gRPC ni un pic RAM complet par session.

## 4. Démarrage et distribution

[Une CLI PCM réelle](evidence/native-pcm-cli.json) a fonctionné dans un processus
neuf : total5,725s, dont environ3,198s dans le stream moteur à l'arrivée du texte.
L'écart comprend démarrage Python, imports, préparation CLI et connexion ; aucun
découpage n'en isole un responsable. Ce sample n'est pas une médiane de démarrage.

Trois imports PyAV ultérieurs en processus neufs, **caches OS non purgés**, ont
pris115,169 /97,754 /99,964ms hors réseau. Le banc natif séparait aussi sa préparation
(66ms dans ce processus) et l'ouverture du canal (48ms). Ces observations ne
justifient pas d'attribuer les2,5s d'écart de la première CLI au seul import PyAV.
L'encodeur est facultatif/lazy ; un processus long-vivant peut le préparer avant
capture. Ce n'est pas une accélération du serveur.

[Une roue installée dans un venv vierge](evidence/installed-wheel-smoke.json), sans
PyAV et sans import depuis le checkout, a effectué une dictée réelle via la CLI
Ogg : sortie0 et résultat attendu, total3,533s. Elle utilise seulement la configuration
locale déjà provisionnée ; aucun APK, JADX ou Android n'est consulté à l'exécution.
Ce sample isolé n'est pas directement comparable à la CLI PCM d'une autre session.

## 5. Grille native affinée — 2026-11-13

[20 sessions supplémentaires](evidence/refinement-page-grid.json), cinq par durée de
page 20/40/60/100 ms, ordre tournant puis inversé, concurrence 1. Même phrase et
même PCM source que le banc natif historique. Code du checkpoint `4b96696`, hashes
exacts enregistrés ; `git_dirty` inclut les nouveaux fichiers de preuve. La politique
`muxer` transmet immédiatement les écritures natives, comme la CLI/SDK, sans les
subdiviser systématiquement à 224 octets comme le banc historique. **Ne pas fusionner
les deux populations.**

Les 20 sources ont été intégralement consommées, les PCM redécodés ont le même hash
et toutes les sessions terminent S3/gRPC avec succès. Le garde-fou
`ordered_all_recognition_state_1_sets_v2` compare toutes les listes d'hypothèses
finales ordonnées, y compris listes vides et répétitions : pas seulement le dernier
texte, pas de fusion/déduplication. Il n'a constaté aucune divergence.

| Page Ogg | Texte après EOF PCM, médiane (min–max) | Fin S3 médiane | Ogg produit | CPU pipeline médian |
|---|---:|---:|---:|---:|
| 20 ms | 348 ms (297–858) | 349 ms | 22 955 o | 469 ms |
| 40 ms | 417 ms (407–489) | 421 ms | 18 446 o | 281 ms |
| 60 ms | 365 ms (348–428) | 366 ms | 16 961 o | 328 ms |
| 100 ms | 422 ms (354–485) | 423 ms | 15 746 o | 203 ms |

**Décision : conserver 20 ms par défaut.** Ce réglage a la plus faible médiane texte
observée et le marqueur de parole médian le plus précoce (322 ms contre 366/413/483 ms).
60 ms économise **26,1 % d'octets Ogg**, avec environ **17 ms** de médiane finale
supplémentaire sur ce seul banc : option de compromis réseau, pas nouveau gain de
vitesse ni preuve d'équivalence. Les octets Ogg ne sont pas le trafic TLS total.

L'outlier 20 ms à **858 ms** n'est pas exclu. Avec n=5, la longue traîne n'est pas
estimée fiablement ; le réseau et la charge ne sont pas contrôlés. Une médiane
favorable ne prouve pas une meilleure latence dans toutes les conditions. Aucune
configuration n'est établie comme optimum global.

Le CPU pipeline sépare désormais vérification/redécodage et écriture des artefacts,
mais inclut toujours encodage, transport, hashing et instrumentation du pipeline.
Ses plages restent larges (20 ms : 297–547 ms ; 60 ms : 188–766 ms), avec granularité
Windows. Pas de gain CPU du seul moteur établi. En-tête Ogg et première page audio
utile sont distingués ; le setup de l'encodeur reste **non instrumenté**, pas estimé
par différence.

Une [dictée progressive distincte](evidence/refinement-long-realtime-public.json)
de 40,564 s PCM a terminé en 41,454 s de collecte complète, source intégrale et même
texte que son essai accéléré. Ce sample isolé ne mesure ni latence microphone
après arrêt ni endurance prolongée ; il n'entre pas dans la grille ci-dessus.

## Reproduire

Préparer un WAV/Opus synthétique avec `tools/make_fixture.ps1` ou un corpus autorisé,
puis configurer les trois variables de metadata applicatives sans les journaliser.

```sh
# Windows / Git Bash, venv installé et .env local configuré
set -a; source .env; set +a
mkdir -p local
.venv/Scripts/python tools/benchmark_engine.py --live --audio artifacts/synthetic-v1/plain-en.ogg --language en-US --samples 5 --chunk-sizes 224,8192 --output local/batch-run.json
# Préparation explicite du WAV16k si le corpus SAPI d'origine est22,05k
ffmpeg -n -i artifacts/synthetic-v1/plain-en.wav -ac 1 -ar 16000 -c:a pcm_s16le artifacts/synthetic-v1/plain-en-16k.wav
# Extra audio requis ; aucun binaire FFmpeg utilisé dans ce banc natif
.venv/Scripts/python tools/benchmark_realtime.py --source native --live --wav artifacts/synthetic-v1/plain-en-16k.wav --language en-US --output local/native-run.json --encoded-dir artifacts/new-native-run --pairs 3
# Grille affinée : 20 sessions, politique fidèle aux écritures SDK/CLI
.venv/Scripts/python tools/benchmark_realtime.py --source native --live --native-chunk-policy muxer --rounds 5 --page-durations 20000,40000,60000,100000 --wav artifacts/synthetic-v1/plain-en-16k.wav --language en-US --output local/grid-run.json --encoded-dir artifacts/new-grid-run
```

Linux/macOS : remplacer `.venv/Scripts/python` par `.venv/bin/python`. Les voix
SAPI du générateur sont spécifiques à Windows et doivent être installées ; sinon
utiliser un corpus autorisé propre, avec son hash et ses conditions. Le fichier
WAV16k et les chemins de sortie doivent être nouveaux, pas écrasés.

Les outils refusent d'écraser une observation existante. Les fichiers bruts et les
audios restent locaux. `--include-results` enregistre du texte connu dans les bancs ;
dans `validate_corpus.py`, il conserve aussi les réponses complètes et peut donc
capturer des identifiants opaques du service. Ne pas publier ces rapports bruts,
même avec une phrase synthétique : créer une copie avec
`tools/sanitize_corpus_report.py`, puis l'inspecter. Sans ce flag, les résultats du
banc sont hachés. Les tests
locaux sont indépendants du service ; les tests FFmpeg optionnels ne prouvent que
le producteur de test.

## Ce qui reste à mesurer

Démarrage de processus réellement froid, microphone/PCM réel, endurance au-delà
des sessions de 40 s, réseaux dégradés, ressources par session isolée, trafic réseau
exact et qualité représentative sur voix naturelles (un seul clip anglais essayé).
Aucun langage concurrent n'a été benchmarké ; on ne prétend pas
que Python soit le plus rapide possible. Les mesures montrent à ce stade que le
levier important identifié est le pipeline d'entrée ; une réécriture du transport
n'aurait pas, à elle seule, démontré le gain de finalisation observé.
