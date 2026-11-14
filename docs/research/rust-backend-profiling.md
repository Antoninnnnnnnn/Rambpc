# Profilage du backend Rust hors codec audio

## Périmètre et méthode

Baseline : `e427fb1`. Données exclusivement synthétiques, exécution locale sans
identifiants, `.env`, corpus privé ni connexion à un service externe. Le codec et
les sources de `rambler-audio` ne sont pas modifiés. Le CRC précédemment rejeté
n'est pas réévalué.

Deux exécutables indépendants dans `crates/rambler-engine/examples` :

- `backend_cpu_profile` mesure les requêtes initiale/contexte/audio, le décodage de
  réponses imbriquées et le décodage avec rétention ordonnée par lots de 128.
  Les fixtures contiennent du texte synthétique, un VoiceEdit, le statut de
  sécurité reconnu et des champs inconnus au niveau racine et hypothèse.
- `engine_loopback_profile` utilise le vrai `Engine`, le transport tonic/h2 et le
  vrai `collect_session`, avec un serveur TCP IPv4 local sur port éphémère. Il
  réutilise le serveur brut des tests engine, sans dépendance de développement
  vers `rambler-audio` (aucun cycle). Son allocateur n'est pas instrumenté.

### CPU et allocations

Chaque cas CPU effectue 128 échauffements, puis cinq passes wall de 20 000
opérations (156 lots de 128 pour la rétention), puis une passe de comptage
séparée. Pour l'audio, référence et production alternent leur ordre à chaque
paire, avec échauffement et comptage séparés à chaque passe. La référence est
l'assemblage figé de `e427fb1`, temporaires imbriqués compris ; les résultats
sont comparés octet par octet avant les mesures. `black_box` observe les payloads
produits et empêche leur élimination. Les allocations des fixtures, le formatage
JSON et la sortie console sont hors des mesures.

L'allocateur global CPU délègue à `System`. Pendant la passe de comptage,
`alloc`/`alloc_zeroed`, `realloc`, les octets demandés et le pic d'octets vivants
supplémentaires sont comptés exactement, après réussite de l'allocation. Un
`realloc` charge **toute sa nouvelle taille**, pas seulement sa croissance. Ce
volume n'est donc ni le trafic de copies effectif, ni un RSS. Le pic compte les
tailles logiques au contrat `GlobalAlloc`, pas les réserves internes de l'OS ni
un éventuel chevauchement interne à `System::realloc`. Toutes les
allocations de chaque opération naissent et meurent dans la passe ; les fixtures
préexistantes ne sont ni modifiées ni libérées. La valeur vivante de fin doit
retrouver exactement sa valeur de départ. Exécution synchrone monothread,
sans runtime réseau concurrent. Les passes wall désactivent ces compteurs mais
conservent le branchement de l'allocateur : ce n'est pas un profil CPU sans aucun
surcoût d'instrumentation.

La rétention CPU est une opération de mouvement vers `Vec<S3Response>` avec borne
de 8 MiB, pas une seconde implémentation autoritative du collecteur. La collecte
réelle et ses invariants de fin sont exercés par l'exécutable réseau.

### Pipeline et backpressure

Par défaut, chaque session envoie 16 384 chunks opaques de 640 octets, soit
10 485 760 octets, puis un contexte Unicode tardif et le marqueur END. Chaque
chunk contient son numéro de séquence ; ce n'est **pas** de l'Ogg/Opus valide et
le serveur local ne prétend pas faire de reconnaissance. Le moteur doit bien
traiter le flux encodé comme opaque. Le serveur renvoie une réponse par chunk,
dont 128 finales uniformément espacées, puis un unique S3 END.

Quatre modes, dans un ordre tournant entre les cinq répétitions :

1. `collect_active` : vrai collecteur actif sans délai volontaire ;
2. `observe_active` : consommateur d'événements actif sans rétention ;
3. `collect_reader_stall` : vrai collecteur actif ; lecture serveur suspendue
   pendant au moins 1 ms avant un chunk sur 64 ;
4. `observe_consumer_stall` : lecture serveur active ; consommateur d'événements
   suspendu pendant au moins 1 ms après une réponse sur 64.

La file de réponses du serveur a une capacité de **1** ; les files de production
restent inchangées. Il ne s'agit jamais d'un producteur laissé tourner sans
consommateur. Les délais utilisent le timer Tokio, ne garantissent pas exactement
1 ms et n'imitent pas le rythme temps réel d'une capture audio.

Le temps wall commence juste avant `Engine::start` et finit après fermeture et
jonction de session, engine, transport et serveur. Construction des fixtures,
validation préalable et connexion initiale sont exclues. Il comprend les
validations du serveur : égalité exacte de l'init, des chunks audio réassemblés
avec leur elapsed réellement observé, du contexte et d'END ; ordre et EOF
upload réels ; égalité des réponses brutes et ordre des finales côté client.
La vérification audio serveur utilise le parseur public et l'assemblage de
requête figé de `e427fb1`, même dans le binaire candidat : un gain du sérialiseur
client n'est ainsi pas compté une seconde fois côté serveur. Cette vérification
alloue : ce coût est intentionnellement inclus et interdit d'interpréter ces
mesures comme le coût du seul transport ou du seul moteur. Le mode observation
vérifie chaque réponse, le collecteur seulement les finales retenues ; leur
écart n'isole donc pas mathématiquement le coût de collecte.

Chaque exécution vérifie les volumes protobuf dans les deux sens, le nombre de
chunks et de finales, un seul EOF engine, l'EOF upload, les trois drapeaux de
cleanup et exactement un appel de fermeture de la source. Le collecteur conserve
ses bornes par défaut (128 finales / 8 MiB) ; le protocole garde ses bornes de
8 MiB / 100 000 champs, la sécurité et les sémantiques proto2/unknowns.

## Environnement de mesure

Windows 11 Pro `10.0.26300`, cible `x86_64-pc-windows-msvc`,
`rustc 1.99.1 (58cf8f9c3 2026-10-13)`, Cargo `1.99.1`, LLVM `23.1.1`.
Processeur annoncé par l'environnement : `Intel64 Family 6 Model 154 Stepping 3,
GenuineIntel`, 12 processeurs logiques. Profil Cargo release par défaut ;
`RUSTFLAGS`, `CARGO_BUILD_RUSTFLAGS` et `CARGO_PROFILE_RELEASE_LTO` non définis.
Runtime pipeline Tokio monothread. Pas d'affinité CPU ni de verrouillage de
fréquence ; on conserve la dispersion observée, sans promesse d'absence de bruit
Windows. Chaque fenêtre a été annoncée au coordinateur puis réservée sans autre
compilation ni benchmark des agents du mandat.

## Reproduction

Depuis la racine du dépôt, dans une fenêtre sans compilation ni autre benchmark :

```sh
cargo build --release -p rambler-engine --examples
# Exécuter directement les binaires afin d'exclure Cargo/compilation du wall.
./target/release/examples/backend_cpu_profile --iterations 20000 --repeats 5
./target/release/examples/engine_loopback_profile --chunks 16384 --repeats 5
```

Sous Windows, les exécutables portent le suffixe `.exe`. Chaque ligne est un
objet JSON autonome. `wall_ns` est le temps écoulé monotone ; `ns_per_unit`
normalise par requête ou réponse, y compris les lots de 128. Les tailles CPU
128/4096 décrivent le texte de remplissage, non la taille totale de la réponse.
Les volumes réseau sont des payloads protobuf, sans enveloppes gRPC/h2/TCP.
`elapsed_ms` des requêtes dépend du déroulement réel et peut changer légèrement
leur taille entre répétitions ; leur contenu audio et leur ordre ne changent pas.

Ce profil offline ne donne ni latence du service distant, ni qualité de
reconnaissance, ni consommation mémoire d'un codec, ni performance sur Android.

## Résultats retenus (consolidés le 2026-11-14)

[Preuve structurée, 96 lignes CPU et40 sessions pipeline](../evidence/rust-backend-offline-profile.json).
La dernière passe utilise la référence avec les temporaires imbriqués et
`Vec::extend(field)` de `e427fb1`. Les passes intermédiaires sont marquées
`superseded` en privé et **exclues** des chiffres ci-dessous. La finalisation
de l'agent a rencontré son quota ; le parent a vérifié les40 sessions et
consolidé directement les données déjà produites, sans inventer de nouveau run.

### Requêtes audio

| Audio opaque | Référence médiane ns (min–max) | Candidat médiane ns (min–max) | Alloc/realloc avant→après | Octets demandés avant→après | Pic logique avant→après |
|---:|---:|---:|---|---:|---:|
| 640 | 1399.605 (1332.790–1548.935) | 48.445 (47.130–50.765) | 8+3→1+0 | 2030→658 | 1950→658 |
| 4096 | 1416.995 (1378.970–1577.565) | 74.660 (70.290–78.555) | 8+3→1+0 | 12398→4114 | 12318→4114 |
| 65536 | 6925.355 (6137.865–15918.405) | 1653.345 (1607.680–1753.890) | 8+3→1+0 | 196723→65556 | 196643→65556 |

**Optimisation locale retenue** : tailles pré-calculées et bornées avant
allocation, écriture directe des mêmes varints/extensions, copie audio unique.
Aucune baisse de qualité ni réduction des validations. L'oracle indépendant
couvre220 frontières et2000 vecteurs déterministes, le message racine exact8MiB
et le rejet de l'octet suivant. Le gain wall est celui de ce microbenchmark
Windows avec allocateur instrumenté désactivé mais présent, pas une promesse
d'accélération de la dictée distante ni une comparaison de langages.

### Pipeline complet, cinq paires alternées

| Mode | Baseline médiane ms (min–max) | Candidat médiane ms (min–max) |
|---|---:|---:|
| collect_active | 924.1896 (852.0531–1019.7183) | 823.8450 (794.5236–1060.5637) |
| observe_active | 916.9192 (899.9580–1025.5379) | 890.5094 (790.2901–993.2578) |
| collect_reader_stall | 3916.1529 (3644.0743–3961.6783) | 3910.3259 (3400.9980–3958.1498) |
| observe_consumer_stall | 3976.8663 (3972.0477–4003.1131) | 3981.2580 (3974.9819–4006.4920) |

Les distributions se recouvrent ; **aucun gain global robuste retenu**. Toutes
les40 sessions vérifient16 384 chunks/10MiB,128 finales, ordre, payloads exacts,
EOF upload/engine et cleanup unique. Le serveur vérifie aussi chaque requête
avec le constructeur figé ; cette charge participe aux timings. Les modes
stall incluent la granularité des timers Windows, pas exactement256ms ajoutées.

Le lot de128 grosses réponses retient1 591 040octets protobuf mais atteint
12 463 808octets de heap supplémentaire logique sur ce modèle. Il n'y a pas
de fuite dans la passe (vivant final égal au départ), mais la borne8MiB du
collecteur porte sur les payloads, **pas** sur les allocations hiérarchiques.
Les raw/inconnus ne sont pas supprimés pour améliorer artificiellement ce chiffre.

Les SHA des sources/binaire réellement mesurés et des benchmarks livrés figurent
dans le rapport. Seuls des attributs/commentaires Clippy ont ensuite été ajoutés
aux deux `main` pour conserver le rejet runtime du mode debug. Ils ne changent
pas le workload ; ce n'est pas présenté comme un nouveau benchmark. Le lot
source mesuré précède les nouvelles politiques de rendu/segments et corrections
transport ultérieures, donc ne représente pas rétroactivement tout le code final.
