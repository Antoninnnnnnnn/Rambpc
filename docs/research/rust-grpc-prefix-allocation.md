# Préfixes gRPC courts sans allocation par message

**Portée historique du banc : constructeur de préfixe uniquement.** Une
[coalescence ultérieure du writer](rust-grpc-envelope-coalescing.md) regroupe
préfixe et payload dans un buffer chaîné. Les mesures ci-dessous ne qualifient
pas ce nouveau chemin complet et ne sont pas rejouées ou réattribuées.

## Décision locale — 16 novembre 2026

**Au point de contrôle local : candidat retenu ; CI distante encore en attente.** Le payload était
déjà envoyé sans copie de framing. Le seul changement est son préfixe gRPC de
cinq octets : `grpc_prefix` emprunte une table statique immuable pour les longueurs
0–255, puis reprend exactement l'ancien constructeur pour les autres longueurs.
La table contient **1 280 octets de données source** ; ce n'est pas une mesure de
la taille liée, de mémoire physique ou de RSS.

La conversion fallible de la taille vers u32 reste au même endroit. `send_bytes`,
capacité/flow-control, attentes, source, reset, EOF, progrès et réception sont
inchangés. Aucune copie de payload supprimée, aucun regroupement HTTP/2 ajouté,
aucun `unsafe` dans la bibliothèque, dépendance ou réglage audio supplémentaire.

[Preuve typée, compteurs, tailles natives et toutes les durées brutes](../evidence/rust-grpc-prefix-allocation-windows-20261116.json).
Source mesurée : candidat non committé sur `7a03f38`, identifié par sept hashes et
le manifeste sélectionné de 186 fichiers. Ce n'est pas une attestation du TCB ni
une qualification rétrospective d'un futur commit.

## Qualification CI distante ultérieure

La CI hors ligne a depuis réussi sur la source exacte `98d46590ec708c7e1ae6157323a07b8eaa41ddd7` : [run Rust](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43832001772) et [run Python](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43832001623). La [preuve CI](../evidence/rust-grpc-prefix-allocation-ci-20261116.json) relève 13 jobs, dont 8 natifs, et 72 groupes de commandes Rust : ce ne sont pas des tests distincts à sommer. Pour chaque toolchain, le workspace donne **418/0/2** sous Windows et **419/0/2** sous Linux et macOS ; les architectures macOS ARM et Intel sont confirmées. Dans chaque job natif, banc préfixe **7/0/0**, SDK gardé **9/0/0** et banc wire **3/0/0**, groupes non additionnables.

Cette CI vérifie les invariants, la matrice H2 brute, la fragmentation forcée et les tailles PCM/Opus synthétiques. Elle ne reproduit pas la campagne locale d'allocations ou de temps et n'établit ni distribution de voix réelle ni performance ou hits du service. La preuve locale reste inchangée et décrit son propre point de contrôle. G1 reste non satisfait et Linux live NO_GO ; aucune nouvelle qualification des credentials, du téléphone, du core ou du provisioning.

## Allocation effectivement observée

Un exécutable release contient les constructeurs ancien et candidat. Deux runs
séquentiels, 16 longueurs fixes, 4096 opérations comptées par version/cas :

| Domaine mesuré | Avant, par constructeur | Après, par constructeur |
|---|---|---|
| 11 longueurs courtes de 0 à 255 | 1 allocation + 1 libération, 5 octets demandés | **0 allocation, 0 libération, 0 octet demandé** |
| 256, 1024, 65535, 65536, u32::MAX | 1 allocation + 1 libération, 5 octets demandés | Identique |

Les compteurs sont identiques entre les deux runs. Les 256 petites longueurs
sont aussi vérifiées fonctionnellement et par identité de pointeur vers la table,
mais seules les 11 longueurs indiquées sont instrumentées pour leurs allocations.
Le cas u32::MAX ne construit que cinq octets de préfixe, jamais un payload géant.

Les cinq contrôles de repli ne déclenchent pas le seuil prédéfini d'une médiane
instrumentée plus lente de >10 % **dans les deux runs**. Cela ne démontre pas une
absence universelle de ralentissement ni une accélération du transport complet.

## Pertinence pour le pipeline audio par défaut

Pour ne pas déduire la taille d'un message de son seul bitrate, un test utilise
le vrai `Pcm16Opus`, puis le vrai `audio_request` pour chaque chunk non vide,
comme `Engine`. Trois signaux synthétiques non vocaux de 80017 échantillons,
source par chunks de 640 octets, profil natif mono16k/24kbps/VoIP/VBR inchangé.
Chaque signal est encodé avec pages20ms et contrôle1000ms ; chaque chunk est
ensuite enveloppé pour elapsed0/128/i64::MAX. Cela donne 18 relevés, pas 18
encodages indépendants. Tous les échantillons, EOF naturels, fermeture unique,
destruction et cleanup idempotent sont vérifiés.

Comptes **par signal/config**, aux tailles maximales de l'horodatage testé :

| Signal | Pages20ms : éligibles / messages | Message max | Pages1000ms : éligibles / messages | Message max |
|---|---:|---:|---:|---:|
| Silence | 253 / 253 | 79 octets | 3 / 8 | 502 octets |
| Triangles harmoniques | 253 / 253 | 142 octets | 3 / 8 | 2859 octets |
| Bruit déterministe | 253 / 253 | 115 octets | 3 / 8 | 2873 octets |

La majorité requise avant mesure est donc établie pour chacune des neuf lignes
20ms. Les autres largeurs d'elapsed ne sont pas mesurées séparément : l'arithmétique
inchangée de `audio_request` les borne entre les extrêmes testés. **Éligibilité**
seulement : pas une trace de hits sur le service, de voix représentative, de
qualité ASR ou de tailles init/contexte/END. Les pages1000ms restent un contrôle,
ni un nouveau défaut ni une proposition de batching.

## Framing, backpressure et qualification

- Oracle indépendant sur un unique RPC/TCP numérique loopback : **260 messages**,
  tous les payloads0–256 puis1024/16384/65536, 115840 octets de payload et
  117140 octets wire. Flag0, longueur big-endian, payload/ordre exacts, message
  vide, EOF stable, statut OK seulement après drainage et fermeture sont vérifiés.
- Autre petit RPC de quatre octets : fenêtre distante3, source retenue jusqu'au
  Ping/Pong du pair après SETTINGS. **Première DATA de trois octets observée**,
  préfixe fragmenté correctement, neuf octets totaux vérifiés avant OK. Seul ce
  petit fixture utilise la fenêtre3, pas le grand corpus.
- Windows stable1.99.1 et Rust1.88 : transport **115/0/1** chacun, banc **7/0/0**
  chacun. Les six nouveaux noms de tests et quatre refus d'authentification
  existants sont confirmés par commande. Workspace stable **418/0/2**, SDK gardé
  **9/0/0**, CLI feature-off **38/0/0** ; format/Clippy strict/build réussis.
  Ne pas sommer les groupes rejoués comme des tests distincts.
- Les 16 commandes QA, le build debug préalable et six captures supervisées
  (quatre no-op, deux mesures) passent ; jobs vides, sources et binaires inchangés.
  Le binaire CLI release historique reste inchangé et n'est pas invoqué.
- Première QA conservée : transport et banc passaient sur les deux versions, puis
  Clippy refusait `% 2 == 0` dans l'ordre du banc. Remplacé par `is_multiple_of(2)`
  compatible1.88, sans correction de production ; seconde QA séparée réussie.
  Aucun résultat de mesure supplémentaire ou cas retiré.
- Le gate CI explicite teste les invariants, pas la campagne de temps. Aucun
  credential, vrai service, téléphone, réservation ou réglage global ; G1/Linux
  live, accès durable et parité Android ne sont pas résolus par ce changement.

## Interprétation des temps et reproduction

Les huit répétitions de32768 opérations par version/cas suivent1024 warmups ;
A/B–B/A tourne par cas/répétition. Les passes d'allocations et de temps sont
séparées. Les 512 durées entières sont conservées ; les colonnes zéro de temps en
phase0 et de compteurs en phase1 sont des **placeholders**, pas des observations.

La porte atomique de l'allocateur instrumenté reste exécutée même lorsque le
comptage est éteint. Son coût ne s'annule pas quand les nombres d'appels diffèrent.
Dispatch opaque et destruction de `Bytes` sont inclus ; fréquence/affinité/bruit
OS ne sont pas contrôlés. Aucun outlier exclu, significativité, CPU processus,
latence WAN, gain du moteur normal ou optimum global affirmé. Le gain retenu est
**une allocation/libération évitée dans les préfixes courts observés**.

```sh
cargo +1.88.0 test -p rambler-transport --locked --offline
cargo +1.88.0 test -p rambler-transport --example profile_grpc_prefix --locked --offline
cargo +stable test -p rambler-cli --features audio --test grpc_prefix_eligibility --locked --offline -- --nocapture
cargo +stable build --release -p rambler-transport --example profile_grpc_prefix --locked --offline
# Attendre la fin de toute compilation, puis deux exécutions séquentielles.
./target/release/examples/profile_grpc_prefix.exe --benchmark-grpc-prefix
./target/release/examples/profile_grpc_prefix.exe --benchmark-grpc-prefix
```

Omettre `.exe` sur Unix. Le chemin suppose le target par défaut. Sans le flag exact,
avec argument supplémentaire ou en build debug standard, le banc reste silencieux.
Conserver toutes les 288 lignes numériques par run, leurs compteurs et leurs temps.
