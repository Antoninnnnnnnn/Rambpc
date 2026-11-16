# Décodeur Rust : préallocation défensive, version finale

## Décision et traçabilité

Prototype isolé sur **`67ac9a5`**, sans commit ni changement de `main`.
[Preuve finale distincte](../evidence/rust-backend-decode-defensive-capacity-profile.json).

La variante précédente **V2 à capacité exacte pour toutes les listes est
rejetée** après mesure d'une amplification sur enfant malformé. Ses
[1050 lignes A/B et 42 lignes de diagnostic](../evidence/rust-backend-decode-allocations-profile.json)
restent inchangées et ne sont pas présentées comme mesures du correctif final.
Le nouveau JSON conserve séparément diagnostic erreur, V2 rejetée,
**V3 intermédiaire** à hint de trois matches, et **version finale à hint borné
avant filtrage**. Sources, patchs, binaires SHA256 et logs sont conservés.
Aucun dump de modèle ou de corpus privé.

## Régression mesurée puis correction

Fixture : 90 000 champs répétés de trois octets, chacun contenant l'enfant
`[0x00]`, soit environ **270 Ko wire**, plus trois wrappers. Le budget global
reste inférieur à 100 000. Le premier enfant doit échouer avec `FieldNumber`.
Un contrôle utilise un premier enfant vide valide puis les enfants invalides.
Les deux variantes hypothèses/partiels sont mesurées sans injection OOM.

| Cas, erreur au premier enfant | Baseline : max requête / pic vivant | V2 rejetée : max / pic | Finale : max / pic |
|---|---:|---:|---:|
| 90 000 hypothèses | 5 242 880 / 9 113 591 | **33 840 000 / 39 353 543** | 5 242 880 / 5 513 543 |
| 90 000 partiels | 5 242 880 / 9 113 591 | **8 640 000 / 14 153 543** | 5 242 880 / 5 513 543 |

V2 réservait le stockage de tous les modèles **avant validation du premier
enfant**, uniquement à partir de wrappers syntaxiquement parsés. Le risque
était réel ; des timings favorables de réponses valides ne le justifient pas.
Avec un enfant valide avant l'erreur, la version finale atteint un pic de
**5 515 046 octets** pour les hypothèses, **5 513 926** pour les partiels ;
le maximum demandé reste **5 242 880** dans les deux cas.

La version finale :

1. Cherche le premier enfant et le décode complètement **avant toute allocation
   du vecteur de modèles** ; zéro enfant retourne un vecteur vide.
2. Calcule ensuite un hint sur **au plus quatre `RefField` du préfixe, avant
   filtrage par numéro/wire type**, puis réserve entre un et quatre modèles.
   Ces références ont été parsées ; **les enfants auxquels elles renvoient ne
   sont pas pour autant validés**. Ce n'est pas un compte de tous les candidats.
3. Décode chaque enfant suivant avec succès **avant** son `push` et l'éventuelle
   croissance du vecteur. Jamais de réserve proportionnelle à des dizaines de
   milliers de wrappers dont les enfants n'ont pas encore été validés.

V3 intermédiaire réservait déjà au plus quatre modèles après le premier succès,
mais `take(3)` **après** un filtre pouvait rebalayer une longue queue d'inconnus
pour trouver trois matches. Elle est remplacée par la borne **avant filtre**.
Les 420 lignes A/B V3 restent séparées dans la nouvelle preuve, pas supprimées.

Les capacités usuelles restent 0/1/2/3/4 pour ces petites listes. Au-delà, la
croissance redevient géométrique sur succès : **257 candidats → capacité 512**,
comme baseline, **pas 257** comme V2. Une liste sparse de deux candidats séparés
par 20 000 inconnus peut garder capacité quatre plutôt que rebalayer cette
queue uniquement pour un hint. Aucun gain de capacité universel n'est promis.

## Autres optimisations privées conservées

`Message::data` est paresseux, les parts uniques sont sur la pile, le premier
vecteur `parse_ref` est réutilisé, et seuls les parseurs privés changent de
signature. `Bytes`, `String`, `wire::Field`/`Value`, les modèles/layouts publics
et les inconnus conservés restent inchangés. `Hypothesis` vaut **376 octets**,
`S3Response` **1984** sur cette cible, avant et après.

Les chaînes écrasées sont toujours toutes vérifiées UTF-8. Les fixed32/64 sont
validés par le parseur wire avant lookup depuis la fin. Chaque occurrence de
sous-message est parsée séparément avant fusion ; les fragments ne peuvent pas
se réparer. Runs oneof, présence/vides, sécurité fermée et ordre des candidats
restent identiques. Les hypothèses ne sont jamais concaténées ni auto-appliquées.
Budget global **100 000 champs**, racine **8 MiB**, copie exacte hors framer et
ownership `Bytes` sont inchangés ; le hint ne réinterprète aucun octet et ne
charge pas une seconde fois le budget. Aucun nouveau `unsafe` ni dépendance.

## A/B final réellement mesuré

Windows 11 Pro 10.0.26300, `x86_64-pc-windows-msvc`, Rust **1.99.1**, release,
incrémental désactivé. Même harness dans baseline et candidat final ; seule
`decode.rs` diffère. Cinq paires **AB/BA/AB/BA/AB**, binaires construits avant la
fenêtre annoncée au coordinateur, sans autre build/benchmark concurrent.

128 échauffements puis 100 000 opérations demandées pour les cas usuels,
781 lots de 128 pour la rétention ; formes adaptées au nombre de candidats.
Deux cas discriminants sparse, un/deux candidats et 20 000 champs inconnus,
64 opérations mesurées chacun. Erreurs : 12 formes, 32 opérations par forme.
La passe allocations est séparée du wall et tous les checks de vivant final
passent. `black_box` observe le modèle entier ; fixture/assertions préalables,
JSON et console restent hors mesure. Le compteur maximal ajouté est dans le
helper non-`unsafe` existant ; tout le forwarding `unsafe` à `System` est inchangé.

**460 lignes A/B finales**, 48 lignes d'erreur finales, 72 lignes d'erreur
V2/V3/baseline, 48 lignes de diagnostic initial et 420 lignes A/B V3 intermédiaires
sont conservées dans le nouveau JSON, en plus de l'ancien JSON non réécrit.

Temps **ns/réponse**, médiane (minimum–maximum), cinq paires finales uniquement.

| Cas | Baseline | Finale |
|---|---:|---:|
| decode petit | 2105,74 (2033,04–2450,57) | 1526,61 (1484,59–1844,57) |
| decode gros | 2985,56 (2884,12–3120,57) | 2400,46 (2296,68–2934,73) |
| rétention 128 × petit | 3952,00 (3690,94–4092,61) | 2833,42 (2790,39–3586,33) |
| rétention 128 × gros | 4348,19 (3939,14–4617,41) | 3466,14 (3307,90–3645,62) |
| 257 candidats courts | 143670,44 (130413,37–164493,06) | 101265,30 (77194,60–117576,35) |
| 16 runs oneof | 4017,70 (3900,96–4593,02) | 3373,79 (3209,89–4672,48) |
| sparse 1 + 20 000 inconnus | 2173270,31 (2091153,12–2327496,88) | 1917331,25 (1901301,56–2055765,62) |
| sparse 2 + 20 000 inconnus | 2098803,12 (1990204,69–2223492,19) | 1835901,56 (1787662,50–2146471,88) |

Les **19 médianes** sont favorables ; ce n'est pas une garantie CPU universelle.
Une paire oneof est **+17,2 %**, une paire sparse2 **+6,1 %**, conservées sans
exclusion ; quatre paires sur cinq sont meilleures pour ces deux cas. Pas
d'affinité/fréquence verrouillée, et dispersion réelle de l'OS/allocateur.
Aucune accélération pipeline/service distant n'est déduite de ces microbenchmarks.

### Allocations et rétention finales, pas chiffres réemployés de V2

| Mesure | Baseline | Finale |
|---|---:|---:|
| Allocations par réponse imbriquée isolée | 39 | 24 |
| Demandés, petit | 5703 | 3135 |
| Demandés, gros | 29511 | 26943 |
| Vivant du lot petit | 639488 | 495104 |
| Pic du lot petit | 640320 | 495960 |
| Vivant du lot gros | 3686912 | 3542528 |
| Pic du lot gros | 3687744 | 3543384 |

Lot gros : toujours **1 591 040 octets protobuf**, économie de rétention
**144 384 octets = 3 × 376 × 128**, soit **−3,92 %** de la baseline `Bytes`.
Lot petit : **−22,58 %**. Sept reallocs du vecteur de lot inchangées ; pas de
realloc sur la réponse isolée. Aucun champ/raw/inconnu supprimé pour ce gain.

**Compromis de pic temporaire explicitement non monotone :**

- 257 candidats : allocations **1302→523**, reallocs **21→14**, demandés
  **564256→475352**, mais pic **220880→222904 octets** (+2024, environ 0,92 %).
- Sparse1 : pic **2757285→3266789** ; sparse2 **2757674→3267939** (environ +18,5 %).
  Reprendre le `Vec<RefField>` peut garder sa capacité géométrique inutilisée
  pendant la copie des inconnus, alors que l'ancien second vecteur était compact.
  Cette hausse temporaire est distincte de la grosse réservation spéculative de
  modèles désormais supprimée ; elle n'est pas masquée par le gain de rétention.

Les tailles sont logiques au contrat `GlobalAlloc` ; `max_request_bytes` inclut
les alloc/realloc réussis de **toutes** les catégories, pas seulement les modèles.
Les reallocs chargent leur nouvelle taille entière. **Ni RSS ni trafic physique
de copies mesuré**, aucune nouvelle garantie de heap du collecteur.

## Validation finale et limites

**156 tests protocol + engine `--all-targets` passent sur stable 1.99.1 et MSRV
1.88.0 ; 2 doctests passent sur chacun ; Clippy stable `-D warnings` passe.**
Clippy MSRV reste indisponible (composant absent, aucun téléchargement demandé).

Cinq tests ciblés : petites capacités/duplicates/ordre, 257 candidats dans les
conteneurs 3/5/7 et partiels, wrong-wire/inconnus, fusion/présence vide, sparse
avec 20 000 inconnus, 90 000 wrappers invalides avec/sans préfixe valide,
100 000 champs exactement et rejet du suivant. Tous les anciens oracles stricts
UTF-8 écrasé, malformed sans réparation, proto2/oneof/sécurité, raw/copied-root
et budgets restent exécutés, sans assouplissement.

Pas de nouveau profil h2, Linux, audio/Opus/CMake, Android, service distant,
identifiant ou `.env`. Pas de revue indépendante de modèle revendiquée. Le
parent a identifié le cas défensif et relu la structure de correction ; cela ne
remplace pas une revue indépendante indisponible.

## Reproduction et disque

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=t cargo test --locked -p rambler-protocol -p rambler-engine --all-targets
# Même commande avec cargo +1.88.0, puis --doc sur chaque toolchain.
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=t cargo clippy --locked -p rambler-protocol -p rambler-engine --all-targets -- -D warnings
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=t cargo build --locked --release \
  -p rambler-engine --example backend_cpu_profile
# Exécuter les binaires préservés en ordre AB/BA, jamais Cargo dans le timing.
./t/release/examples/backend_cpu_profile.exe --decode-only --iterations 100000 --repeats 1
./t/release/examples/backend_cpu_profile.exe --decode-shapes --iterations 100000 --repeats 1
./t/release/examples/backend_cpu_profile.exe --decode-sparse --iterations 100000 --repeats 1
./t/release/examples/backend_cpu_profile.exe --decode-errors --iterations 32 --repeats 1
```

Cible courte `t`, debug dev/test=0, incremental=0. Maximum observé pour la reprise
corrective : **998 Mio**, puis suppression du cache debug stable avant MSRV ;
**851 Mio** après MSRV. Sources/binaires et preuves légères restent dans le
scratchpad dédié pour revue, séparément des caches supprimables.
Après revue locale/parent du correctif et vérification des preuves, `t` a été
supprimé ; les exécutables, snapshots et métriques préservés occupent environ
**5 Mio** dans le scratchpad dédié.
