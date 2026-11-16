# Enveloppes gRPC regroupées sans concaténation du payload

## Décision locale — 16 novembre 2026

**Au checkpoint local : qualification positive Windows réussie ; nouvelle CI
distante encore en attente.**
Le candidat a été mesuré non committé sur `70bbf3b`, avec quatre fichiers source
identifiés dans la [preuve locale](../evidence/rust-grpc-envelope-coalescing-windows-20261116.json).
Le manifeste sélectionné de 188 fichiers est identique avant/après la campagne.
Il ne constitue ni une attestation du TCB complet ni celle d'un nouveau binaire
CLI release.

## Qualification CI ultérieure — source `8cb381e`

Les [neuf jobs Rust](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43859489195)
et les [quatre jobs Python](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43859489085)
réussissent sur le commit exact `8cb381e8d26d448db80481d4aab9472aee3b096c`, tentative 1.
La [preuve CI séparée](../evidence/rust-grpc-envelope-coalescing-ci-20261116.json)
conserve les identités et hashes des API/logs. Les 72 groupes Rust sont recomptés
par commande : huit configurations Windows/Linux/macOS ARM/macOS Intel ×
Rust 1.88/stable (stable CI observée : 1.100.0). Le workspace donne **429/0/2**
sous Windows et **430/0/2** ailleurs ; SDK9, reuse10, profil audio8, banc préfixe7,
banc wire3, finalisation1, budgets3 et feature-off38 passent par job. Ne pas
additionner ces réexécutions comme des tests distincts.

Les sept nouveaux noms de coalescence, y compris les deux assertions de panique
attendue, les quatre tests HPACK et les oracles de framing antérieurs sont
confirmés dans chaque workspace. Le job macOS Intel/1.88 réussit cette fois sur
le candidat corrigé. Les jobs Python 3.11 donnent **663 réussis/71 ignorés** et
les jobs 3.14 **733/1**, chacun sur Windows et Linux, avec **335 sous-tests
rapportés séparément**. Les 13 jobs, les hosts/architectures natifs et 1605020
octets de logs sont audités.

Ce nouveau succès ne réécrit ni la preuve locale au checkpoint précédent ni
l'échec de `70bbf3b`. Aucun backlog du job historique n'est reconstitué, aucun
ancien client n'est relancé pour provoquer une erreur. Les gates de banc ne
rejouent pas les campagnes d'allocations ou de temps ; le smoke racines qualifie
une configuration, pas un handshake TLS. Aucune nouvelle qualification de service,
provisionnement, Linux live ou perfection globale n'en découle.

## Échec conservé, pas relance destinée à obtenir du vert

La CI de `70bbf3b` a terminé avec **12 jobs réussis et un échec** : macOS Intel,
Rust 1.88, oracle `every_small_upload_length_and_large_boundaries_preserve_raw_grpc_envelopes`.
Le pair h2 rapporte un GOAWAY `ENHANCE_YOUR_CALM` d'origine bibliothèque. Les quatre
tests HPACK et l'oracle de fenêtre de trois octets passent dans ce même job.
[Preuve terminale distincte](../evidence/rust-credential-hpack-wire-ci-failed-20261116.json) ·
[run Rust échoué](https://github.com/Antoninnnnnnnn/Rambpc/actions/runs/43850604441).

Deux analyses statiques indépendantes identifient l'envoi du préfixe de cinq
octets comme DATA séparée comme un mécanisme plausible d'accumulation de petites
frames chez le pair. **La séquence de frames du job échoué n'a pas été capturée** :
ce n'est pas une cause historique démontrée sur le wire. L'échec reste un échec,
même après les succès locaux. Aucun ancien client n'est rejoué pour provoquer
une erreur, et aucune limite du pair n'est désactivée ou relevée.

## Changement de production

Le writer transmet un `Chain<Bytes, Bytes>` privé : une vue sur le préfixe et une
sur le payload original. Chaque crédit positif produit une soumission DATA
couvrant les parties disponibles de **la même enveloppe**. Quand le crédit suffit,
le préfixe n'est plus envoyé comme une frame séparée.

- Aucun batching entre messages, attente ajoutée ou tampon de concaténation du
  payload ; pas de nouvelle dépendance, d'`unsafe` ou d'API publique.
- Les claims restent bornés à 16 KiB et au crédit effectivement accordé. Les
  branches de reset et d'erreur typée sont conservées ; la réservation est
  libérée une fois par enveloppe, au lieu de deux fois pour ses deux parties.
- Les crédits partiels peuvent encore fragmenter le préfixe ou traverser sa
  frontière avec le payload. Les octets, l'ordre et les messages vides sont
  conservés ; END_STREAM reste vide et le progrès EOF suit sa soumission réussie.
- Le constructeur de préfixe cache, la réception, les credentials, TLS, l'audio,
  les politiques de source/annulation et les limites sont inchangés. Les autres
  modifications de production ne propagent que le type de buffer privé.

Les assertions de pointeur vérifient les sous-vues non vides du payload original,
ainsi que `Chain`, `Take`, `advance` et les vues vectorisées. Elles ne mesurent pas
les allocations du transport complet. Les partages de `Bytes`, découpages,
préfixes de repli et opérations internes de h2 peuvent encore allouer ou copier.
Le [banc historique des constructeurs](rust-grpc-prefix-allocation.md) ne mesure
pas ce nouveau writer et n'est pas requalifié rétrospectivement.

## Validation positive du candidat

Sept nouveaux tests : cinq tests unitaires de découpage/vue et deux intégrations
sur le vrai transport, pair numérique loopback sous limites par défaut. La porte
source suit le PING/PONG du pair ; celui-ci draine normalement et rend le crédit.
Aucun backlog artificiel, sleep, retry ou accommodation de limite.

- Matrice complète : **260 messages, 117140 octets gRPC**, contenu et ordre exacts,
  y compris message vide et tailles supérieures à une frame.
- Frontières ciblées : neuf messages, **33821 octets gRPC**, dix DATA non vides
  attendues sous crédit initial suffisant ; vérification des frontières du
  préfixe cache et de la taille maximale d'une frame.
- Les deux oracles antérieurs, dont le préfixe réellement fragmenté par une
  fenêtre de trois octets, restent **inchangés**, tout comme les quatre tests HPACK.
  Annulation, voisin actif, refus d'authentification fictifs, cause GOAWAY, upload
  bloqué, EOF, nettoyage et absence de connexion supplémentaire restent contrôlés.
- Windows stable 1.99.1 et MSRV 1.88 : transport **126/0/1** chacun et cible de
  coalescence **2/0/0** chacune ; workspace stable **429/0/2**, SDK gardé **9/0/0**,
  CLI sans audio **38/0/0**. Format et Clippy transport tous targets/features avec
  `-D warnings` passent. Ces groupes rejoués ne sont pas des tests distincts à sommer.
- Les 14 commandes et leurs jobs supervisés réussissent ; les 188 hashes de
  sources sont identiques avant/après. CLI historique, Cargo.lock, budget consommé
  et preuve de CI échouée restent inchangés.

```sh
cargo test -p rambler-transport --locked --offline
cargo +1.88.0 test -p rambler-transport --locked --offline
cargo test -p rambler-transport --test grpc_envelope_coalescing --locked --offline
cargo clippy -p rambler-transport --all-targets --all-features --locked --offline -- -D warnings
cargo fmt --all -- --check
```

## Limites maintenues

Cette campagne ne garantit pas toute distribution de messages ou de crédits,
ni toutes les politiques de pair. Elle ne mesure ni accélération globale,
allocations du moteur, latence WAN, qualité ASR ou optimum. Aucun nouveau build
ou appel de la CLI release historique, vrai credential, service, téléphone ou
réservation ; le budget Windows réel reste consommé. G1 Linux reste non satisfait
et Linux live **NO_GO**. Provisionnement/accès durable, parité Android complète et
confidentialité du service réel restent hors qualification.
