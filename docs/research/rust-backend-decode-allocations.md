# Allocations privées du décodeur Rust : prototype compatible

> **Historique — variante V2 non bornée rejetée après revue défensive.**
> Les décisions, chiffres « finaux », capacités exactes des grandes listes et
> validations ci-dessous concernent uniquement cette ancienne variante mesurée.
> Ils ne décrivent pas le code finalement retenu. Les données A/B restent
> intégralement conservées, mais une erreur au premier enfant peut demander
> 33,84 Mo de stockage modèle spéculatif. Voir le
> [correctif final à hint borné avant filtrage](rust-backend-decode-defensive-capacity.md)
> et sa nouvelle preuve distincte. Aucun gain historique de V2 ne vaut mesure
> du correctif final.

## Périmètre et décision

Baseline **`67ac9a5`**, Windows x86_64, 2026-11-14. Prototype dans le seul
worktree de travail, **sans commit ni modification de `main`**. La précédente
optimisation `Bytes` est déjà dans cette baseline ; son gain historique de
70,82 % n'est pas réattribué à ce travail.

[Preuve structurée et sources mesurées](../evidence/rust-backend-decode-allocations-profile.json)
: **420 lignes A/B finales**, **630 lignes initiales trois-voies intégralement
conservées**, 42 lignes de diagnostic, SHA256 des trois binaires/sources,
patchs des variantes et du harness commun, résumés, rapports appariés et logs de
validation. Aucune suppression d'échantillon lent.

**Retenu :** capacités exactes des vecteurs répétés, lookup privés paresseux,
parts à occurrence unique sur la pile, réutilisation du premier vecteur produit
par `parse_ref`. Aucun nouveau type public, changement de `Bytes`, `String`,
`wire::Field`/`Value`, dépendance, `unsafe` ou code transport/engine de production.
L'allocateur instrumenté préexistant du profiler est réutilisé sans modification.

## Diagnostic et changements

Le recensement effectif de cette cible donne `size_of::<Hypothesis>() = 376` et
`size_of::<S3Response>() = 1984` octets, **avant et après**. Les estimations
initiales 384/1952 ne sont donc pas les valeurs de ces exécutables.

- Un seul candidat dans `HypothesisResult` retenait une capacité de quatre,
  soit **1504 octets**. Préallouer à partir du nombre de champs déjà parsés de
  bon numéro **et bon wire type** ramène cette allocation à **376 octets**.
  Deux candidats : capacité 4→2 ; 257 : 512→257. Zéro reste sans allocation.
  Même traitement pour `PartialResult`, indépendamment pour chaque conteneur.
- Le comptage parcourt une seconde fois **les références déjà validées**, pas
  les octets protobuf ; il ne charge jamais une seconde fois le budget.
  Travail linéaire borné par 100 000 champs, aucune arithmétique issue d'une
  longueur wire non validée. La préallocation peut précéder le rejet d'un enfant
  malformé : ce n'est pas une nouvelle garantie de borne heap.
- `Message::data` renvoie un itérateur clonable plutôt qu'un `Vec` temporaire.
  Toutes les occurrences des chaînes restent validées en ordre avant copie de
  la dernière. Les fixed32/64 ont déjà tous été validés par `parse_ref` ; le
  lookup final peut donc parcourir les références depuis la fin.
- Les parseurs privés empruntent une slice de parts. Racine et enfant répété
  n'allouent plus un `vec![part]`. Pour un sous-message singulier, zéro occurrence
  reste absent, une utilise une slice sur la pile, plusieurs conservent un
  vecteur temporaire puis la fusion existante.
- `Message::new` reprend directement le premier `Vec<RefField>` ; les autres
  occurrences sont toujours parsées séparément puis étendues. Pas de double
  vecteur/copieur de champs pour le cas commun à occurrence unique.

Les **1984 octets inline de racine** et les layouts publics restent inchangés.
Les vecteurs de runs oneof, les parts des vraies fusions et les capacités des
inconnus restent présents ; il ne s'agit pas d'une suppression de provenance.

## Méthode A/B

Même harness/fixtures dans les trois exécutables release, seule `decode.rs`
diffère. Première comparaison : cinq passes à 20 000 opérations demandées,
ordre tournant baseline / capacité seule / candidat complet. Elle isole le
fait que la capacité seule donne déjà la baisse de rétention, mais laisse
**39 allocations** par réponse contre **24** pour le candidat complet.
Elle n'est pas retenue comme variante finale, sans être un échec de fidélité.

Une paire oneof initiale était **+27,0 %**, une paire de rétention grosse
**+1,3 %**. Cela a déclenché **une seule répétition bornée plus longue**,
annoncée au coordinateur, sans compilation/benchmark concurrent : cinq paires
strictement alternées **AB / BA / AB / BA / AB**, 100 000 opérations demandées,
128 échauffements par cas. Les anciennes valeurs ne sont ni exclues ni mélangées
aux finales. Aucune nouvelle répétition après cette fenêtre.

Rétention : 781 lots de 128 réponses, mouvement réel vers un `Vec` croissant
inchangé, puis destruction. Formes répétées : nombre d'itérations adapté au
nombre de candidats ; 6250 pour partiels/fusions/oneof, 100 pour 100 000 champs.
Les nombres exacts sont dans chaque ligne. Compteurs d'allocation désactivés
pendant le wall, puis passe séparée ; `black_box` observe le modèle entier.
Construction des fixtures, assertions préalables, JSON et sorties sont exclus.
Le vivant retrouve exactement sa valeur initiale après chaque passe comptée.

Windows 11 Pro 10.0.26300, `rustc 1.99.1`, cible `x86_64-pc-windows-msvc`,
profil release Cargo, incrémental désactivé. Pas d'affinité CPU ni de verrouillage
de fréquence. L'allocateur délègue à `System` ; son branchement reste dans les
passes wall. Les reallocs chargent toute leur nouvelle taille. Ce sont des
octets logiques au contrat `GlobalAlloc`, **pas du RSS ni du trafic de copies**.

## Résultats finaux

Temps **ns par réponse**, médiane (minimum–maximum), cinq paires.

| Cas | Baseline | Candidat |
|---|---:|---:|
| decode petit (526 octets wire) | 2119,05 (2025,25–2834,80) | 1579,65 (1480,99–2097,11) |
| decode gros (12 430 octets wire) | 2996,11 (2819,39–4238,29) | 2376,04 (2318,86–3392,03) |
| rétention 128 × petit | 3786,18 (3583,75–5074,69) | 2884,33 (2813,37–3710,83) |
| rétention 128 × gros | 4173,90 (3835,01–4902,14) | 3175,88 (3090,30–5722,55) |
| 1 candidat court | 1271,24 (1245,53–1673,22) | 846,91 (807,51–1723,64) |
| 16 candidats courts | 6368,62 (6191,09–8900,00) | 3480,38 (3303,50–4548,43) |
| 257 candidats courts | 142757,07 (125493,83–178908,23) | 84091,52 (81762,72–113678,92) |
| 16 partiels | 5662,62 (5431,49–8945,06) | 2855,25 (2764,35–4059,18) |
| fusion de 16 conteneurs | 7647,62 (7453,30–12172,88) | 4482,21 (4321,57–6646,67) |
| 16 runs oneof | 4345,65 (3816,50–7770,80) | 3367,22 (3188,50–4527,97) |
| racine 100 000 champs | 3526607 (3395157–4285408) | 2652096 (2624928–3517055) |

Les 17 médianes, y compris vide/status/speech et listes de 0/2/4 éléments, sont
favorables. **Ce n'est pas une preuve d'absence de régression CPU** : une paire
finale oneof est encore +14,7 %, une paire de rétention grosse +42,4 % ; quatre
paires sur cinq sont meilleures pour ces deux cas, avec recouvrement des
intervalles. Les rapports appariés de tous les cas sont dans la preuve.

Analyse spécifique oneof : son regroupement et ses vecteurs sont inchangés,
aucune passe `count()` de liste répétée n'est exercée dans cette fixture.
Les lookup d'absence font toujours un scan ; les deux `next()` du singulier
partagent un itérateur, pas deux parcours complets. La copie du premier vecteur
de champs et des matérialisations de lookup sont supprimées, sans nouvelle
complexité asymptotique. On retient donc une **tendance wall favorable de ce
microbenchmark**, pas un gain CPU universel garanti, et aucune accélération
h2/service distant n'est revendiquée.

### Allocations déterministes

| Mesure | Baseline | Candidat |
|---|---:|---:|
| Allocations, réponse imbriquée isolée petit/gros | 39 | 24 |
| Octets demandés, petit | 5703 | 3135 |
| Octets demandés, gros | 29511 | 26943 |
| Vivant du lot 128 × petit | 639488 | 495104 |
| Pic logique, lot petit | 640320 | 496336 |
| Vivant du lot 128 × gros | 3686912 | 3542528 |
| Pic logique, lot gros | 3687744 | 3543760 |

Le lot gros transporte toujours **1 591 040 octets** protobuf et économise
**144 384 octets vivants, soit 3,92 %** de la baseline `Bytes` actuelle.
Le lot petit économise les mêmes 144 384 octets, soit **22,58 %**.
L'explication est exacte : **3 × 376 × 128** de capacité excédentaire supprimée.
Les chaînes (456/12360 octets par réponse), inconnus (502), raw exacts et
métadonnées de partage sont inchangés. Le vecteur de lot garde une allocation
initiale et sept reallocations ; les réponses isolées n'en font aucune.

À 257 candidats : allocations **1302→523**, reallocs **21→7**, octets demandés
**564256→188464**. À 100 000 champs racine : pic logique **9442896→5442904**.
Ces volumes temporaires ne sont pas assimilés à de la mémoire retenue ni au RSS.

## Invariants et validation réellement exécutée

- **154 tests protocol + engine `--all-targets`** passent sur Windows stable
  1.99.1 et sur **MSRV 1.88.0** ; **2 doctests** passent sur chacun.
- **Clippy stable `-D warnings` passe**. Clippy 1.88 a été tenté mais son
  composant est absent ; aucun téléchargement/install ni succès inventé.
- Trois tests ciblés ajoutés : capacités et ordre/duplicates pour 0/1/2/3/4/5/16/
  257 éléments dans les trois conteneurs et les partiels, wrong-wire/inconnus,
  fusions/vides avec présence, **100 000 champs exactement** et rejet du suivant.
- Les tests existants gardent leurs oracles : root exact 8 MiB/copied hors framer,
  raw partagé et enfants survivants, fragments qui ne guérissent pas, UTF-8
  écrasé toujours rejeté, proto2 merges, oneofs, sécurité fermée, noncanonique,
  inconnus opaques ordonnés, et chaque champ interprété compté une seule fois.

Aucune concaténation des textes candidats, application d'edit, changement
collecteur/limite, partage entre sessions ou perte de champ. Pas de nouveau
profil h2, Linux, codec/Opus/CMake, Android, réseau externe ni corpus privé.
Aucune revue indépendante n'est revendiquée.

## Options non retenues

- **Capacité seule** : mesurée séparément, conserve le gain mémoire mais garde
  les allocations temporaires ; dominée ici par la variante complète pour les
  objectifs communs. Ses sources/binaires/630 lignes restent dans la preuve.
- `shrink_to_fit` après collecte : non prototypé, pas de mesure inventée ; imposer
  potentiellement un realloc/copie évitable n'est pas nécessaire quand le compte
  valide est déjà disponible avant allocation.
- Boxing/changement de layout pour réduire les 1984 octets de racine : hors
  contrainte API publique ; aucun gain revendiqué.
- Suppression/compactage des inconnus, partage intersessions et réécriture des
  runs oneof : non appliqués. Les gains chiffrés ne reposent pas sur ces pistes.

## Reproduction

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=t cargo test --locked -p rambler-protocol -p rambler-engine --all-targets
# Refaire avec cargo +1.88.0 ; --doc séparément pour les deux toolchains.
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=t cargo clippy --locked -p rambler-protocol -p rambler-engine --all-targets -- -D warnings
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=t \
  cargo build --locked --release -p rambler-engine --example backend_cpu_profile
./t/release/examples/backend_cpu_profile.exe --decode-only --iterations 100000 --repeats 1
./t/release/examples/backend_cpu_profile.exe --decode-shapes --iterations 100000 --repeats 1
```

Construire baseline/candidat **avant** la fenêtre de timing, préserver chaque
exécutable puis alterner cinq paires ; jamais compiler dans les mesures. Le
patch baseline/harness et celui de la variante capacité seule sont inclus dans
la preuve pour reconstruire les trois sources sans toucher `main`.
Cible courte `t`, dev/test debug=0, incremental=0 ; maximum observé **1,1 Gio**.
Le cache debug stable a été supprimé avant le build MSRV pour rester sous la
borne de disque ; les preuves structurées sont indépendantes de ces caches.
Après auto-revue et vérification des hashes, le cache `t` restant (851 Mio) a été
supprimé. Seuls les trois petits binaires/sources mesurés et les scripts de
preuve sont préservés dans le scratchpad dédié (environ 1,7 Mio) pour revue.
