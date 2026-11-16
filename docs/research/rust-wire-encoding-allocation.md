# Réduction des allocations de sérialisation protobuf

## Décision locale — 16 novembre 2026

**Optimisation de production retenue localement, CI distante encore en attente.**
Les encodeurs de `crates/rambler-protocol/src/wire.rs` écrivent désormais clé,
longueur et valeur dans un seul vecteur dimensionné avant allocation, sans vecteur
temporaire de varint. Les octets, signatures, limites et précédences d'erreur sont
conservés ; le décodeur, les assembleurs de requêtes, `audio_request`, le codec et
les dépendances sont inchangés. Aucun `unsafe` n'est ajouté à la bibliothèque.

Les 149 cas synthétiques ont des sorties exactement identiques avant/après dans
les deux runs. Le nombre d'appels `alloc + alloc_zeroed + realloc` et la somme des
tailles demandées n'augmentent dans aucun cas ; les 108 cas de champs varint/bytes
utilisent strictement moins d'appels. Les 30 varints seuls restent à un appel.

[Preuve typée et toutes les mesures brutes](../evidence/rust-wire-encoding-allocation-windows-20261116.json).
La source mesurée est le candidat non committé sur `d9af4f5`, identifié par ses
six hashes de fichiers et le manifeste sélectionné de 182 fichiers. Ce n'est pas
une qualification rétroactive de tout futur commit ni une attestation de TCB.

## Vérification des requêtes complètes

Une revue ciblée a signalé un risque réel à mesurer : supprimer la capacité libre
des petits vecteurs peut ajouter des réallocations lors des `append` successifs.
Avant toute mesure, le banc a donc été étendu avec 11 cas d'assemblage complet,
en plus des 138 petits encodeurs. Les corps init/contexte sont copiés exactement
du baseline et utilisent ses anciens encodeurs ; types et validation de config
sont partagés, inchangés. Les fixtures/configs sont préparées hors mesure, mais
validation, assemblage et destruction du résultat sont inclus.

Comptes **par opération**, observés identiquement dans les deux runs ; les octets
sont des sommes de tailles demandées aux hooks, **pas** de la mémoire vive ou un pic.

| Cas synthétique | Appels avant → après | Octets demandés avant → après |
|---|---:|---:|
| Init par défaut | 60 → 34 | 1102 → 741 |
| Init avec hints 128 octets | 102 → 59 | 4232 → 3674 |
| Init avec hints 1024 octets | 102 → 59 | 15616 → 15058 |
| Contexte simple, 0 octet | 4 → 2 | 40 → 7 |
| Contexte simple, 128 octets | 6 → 2 | 306 → 266 |
| Contexte simple, 1024 octets | 6 → 2 | 2098 → 2058 |
| Contexte simple, 65536 octets | 6 → 2 | 131125 → 131085 |
| Contexte curseur, 0 octet avant | 13 → 7 | 160 → 76 |
| Contexte curseur, 128 octets avant | 14 → 6 | 787 → 698 |
| Contexte curseur, 1024 octets avant | 14 → 6 | 5267 → 5178 |
| Contexte curseur, 65536 octets avant | 14 → 6 | 327835 → 327746 |

Les cas init avec hints ajoutent deux biais, deux préférences, deux langues
secondaires et les trois options de reconnaisseur. Le contexte curseur comporte
aussi une sélection Unicode et un après contenant NUL ; aucun texte réel n'est lu.
Un champ bytes de 1024 octets au numéro 1 passe séparément de 3 à 1 appels.

## Qualification et historique

- Windows stable local 1.99.1 et Rust 1.88 : **75/0/0** au protocole chacun,
  **3/0/0** pour les invariants du banc chacun. Les noms des quatre nouveaux tests
  différentiels, du test fixed32 et du golden init existant sont confirmés.
- Workspace stable toutes features : **411/0/2** ; CLI sans audio **38/0/0**.
  Fmt global, Clippy protocole tous targets strict et build release du banc passent.
  Ces groupes se recouvrent : ne pas les sommer comme tests distincts.
- Les 14 commandes de qualification et leurs logs sont vérifiés ; sources
  identiques avant/après. Les cinq lancements du banc (trois no-op, deux mesures)
  terminent à zéro avec jobs vides, sorties et hashes contrôlés.
- Le premier lot QA a échoué **à la compilation**, avant les tests : ma fixture
  de contexte complète omettait les champs de provenance `raw/unknown_fields`.
  Ajout de leurs valeurs par défaut, sans correction du moteur ; échec conservé,
  second lot séparé réussi. Aucune mesure supplémentaire ou exclusion cachée.
- CLI release historique inchangée et non invoquée ; aucun credential, service,
  téléphone, réservation, réglage global ou changement du profil audio. Son ancien
  résultat distant ne qualifie pas ce nouvel encodeur. G1/Linux live inchangés.
- Un gate CI explicite exécute les trois tests du banc, pas la campagne de temps.

## Mesure et limites

Un seul exécutable release contient baseline et candidat. Par cas et par version,
128 opérations sont comptées, puis une passe **séparée** mesure six répétitions,
avec 8 warmups et ordre A/B–B/A alterné. Deux runs séquentiels, toutes compilations
terminées ; les 3576 durées entières et tous les compteurs sont conservés.

Les médianes instrumentées des 11 requêtes complètes sont plus basses dans les
deux runs. **Cela n'est pas un gain chiffré du moteur normal** : les hooks de
l'allocateur du banc exécutent encore un chargement atomique quand le comptage
est désactivé. Les nombres d'appels diffèrent, donc ce coût ne s'annule pas entre
versions. Dispatch opaque, destruction des vecteurs, caches et bruit OS restent
inclus ; fréquence, alimentation et affinité ne sont pas contrôlées. Aucun outlier
retiré, aucune significativité, CPU processus, RSS, latence WAN, qualité ASR,
parité Android ou optimum global affirmé. Le gain retenu est la réduction des
appels/tailles d'allocation observée sur ces chemins, avec fidélité inchangée.

## Reproduction

Depuis la racine, dépendances verrouillées déjà disponibles ; aucune clé ni
entrée externe requise :

```sh
cargo +1.88.0 test -p rambler-protocol --locked --offline
cargo +1.88.0 test -p rambler-protocol --example profile_wire_encoding --locked --offline
cargo +stable build --release -p rambler-protocol --example profile_wire_encoding --locked --offline
# Attendre la fin de toute compilation, puis exécuter deux fois séquentiellement.
./target/release/examples/profile_wire_encoding.exe --benchmark-wire-encoding
./target/release/examples/profile_wire_encoding.exe --benchmark-wire-encoding
```

Omettre `.exe` sur Unix. Le chemin suppose le target Cargo par défaut. Sans le
flag exact, avec argument supplémentaire ou en build debug standard, le banc
reste silencieux et ne mesure rien. Comparer les 149 sorties déclarées égales,
tous les compteurs et toutes les durées ; ne pas sélectionner les seuls bons cas.
