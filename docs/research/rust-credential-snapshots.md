# Métadonnées natives : snapshots, rotation et retrait

## Préflight strict de la CLI — qualification locale

Sur l'endpoint normal, la CLI ouvre d'abord l'entrée, puis exige les trois champs
exacts `RAMBLER_API_KEY`, `RAMBLER_ANDROID_PACKAGE` et `RAMBLER_ANDROID_CERT`
avant `Engine::connect`. Toute absence, valeur vide, configuration partielle,
valeur Unicode invalide ou erreur `Metadata` est rejetée localement ; en cas
d'échec, la fermeture de l'entrée ouverte est attendue. Cela ne garantit pas
l'absence d'I/O lors de l'ouverture. Le snapshot `Metadata` immuable est transmis
à `SessionOptions`, sans relecture de l'environnement. Le loopback explicite
n'inspecte pas ces variables.

Pour la bibliothèque, `EnvironmentCredentials` reste un provider permissif :
tout absent produit des métadonnées vides. Le choix strict explicite est
`EnvironmentCredentials::required_snapshot_with(&self, lookup: impl FnMut(&str) -> Result<String, std::env::VarError>) -> Result<Metadata, TransportError>` ;
`required_snapshot(&self)` lit l'environnement. Snapshots et diagnostics restent
expurgés à l'affichage. Ce préflight ne démontre ni authentification réelle ni
provisionnement renouvelable. La qualification locale offline distingue les
snapshots A et B ; voir [résultats et limites](rust-build-validation.md#pcm-progress-and-strict-admission--local-checkpoint-2026-11-15) et [preuve](../evidence/rust-pcm-progress-admission.json).

## Contrat historique exercé

Deux filets offline supplémentaires vérifient le transport existant ; aucune API,
implémentation de production, dépendance ou politique de retry n'est modifiée.
Les valeurs sont inventées, l'environnement réel n'est jamais lu et aucun service
Google ni appareil n'est contacté.

- `crates/rambler-transport/tests/credential_sessions.rs` ouvre **un seul TCP
  loopback**. Le serveur abandonne son listener après cette acceptation. Trois
  RPCs utilisent le même transport et le même `MetadataProvider` mutable :
  génération `one`, génération `two`, puis snapshot vide.
- Le serveur retient toutes les réponses jusqu'à l'observation des trois requêtes.
  Les sessions précédentes sont donc encore actives quand le fournisseur change.
  Chaque snapshot est vérifié côté réception : deux valeurs `authorization`
  ordonnées, clé inventée et octets binaires exacts ; la troisième requête ne
  contient aucune de ces valeurs. Les en-têtes de contrôle restent uniques.
- Les sources sont vides : zéro payload/audio envoyé. Chaque session exige EOF
  upload, fermeture source et statut gRPC OK. Le compteur fournisseur doit rester
  exactement à trois, et le serveur doit avoir reçu exactement trois requêtes.
- Le test unitaire
  `duplicate_text_and_binary_values_keep_sensitive_flags_when_converted_to_headers`
  contrôle les marqueurs sensibles des doublons textuels et du binaire **avant
  envoi**, après la même conversion `tonic::Request`/timeout/en-têtes que le writer.

## Limites de l'oracle

Le décodeur HPACK de `h2` 0.4.20 ne propage pas le marqueur « never indexed » dans
`HeaderValue::is_sensitive` (`hpack/decoder.rs`, branche `LiteralNeverIndexed`,
TODO explicite). Une assertion initiale sur les valeurs reçues échouait pour cette
raison ; elle a été retirée, pas transformée en défaut de production. Les deux
contrôles finaux séparent donc fidélité/isolation côté pair et sensibilité
pré-sérialisation. **Aucune inspection indépendante des bits HPACK n'est revendiquée.**
Une première compilation du nouveau harness utilisait aussi un argument générique
superflu de `h2::server::handshake` ; cette erreur de test a été corrigée.

Ce test n'établit ni renouvellement automatique, ni provisionnement, ni
récupération après révocation. Le fournisseur de test remplace explicitement des
snapshots déjà disponibles. La connexion est volontairement plaintext **loopback** :
ce n'est pas une nouvelle qualification TLS ou d'authentification distante.
Pas de mesure de performance, d'endurance ni de garantie d'isolation universelle.

## Reproduction

```sh
cargo test -p rambler-transport --locked --offline
cargo +1.88.0 test -p rambler-transport --locked --offline
cargo clippy -p rambler-transport --all-targets --locked --offline -- -D warnings
cargo fmt -p rambler-transport -- --check
```

La [preuve ciblée](../evidence/rust-credential-snapshots.json) conserve les sources
hashées, plateformes, résultats, logs hashés et limites. Il s'agit de la suite
transport, pas d'un nouveau run de tout le workspace ou du service Rambler.

## Refus du pair, remplacement explicite et voisin actif — 16 novembre 2026

Deux nouveaux tests `credential_rejection.rs` couvrent séparément
`UNAUTHENTICATED` et `PERMISSION_DENIED`. Un seul TCP loopback porte trois
sessions explicites : voisin actif, session refusée, puis nouvelle session après
remplacement explicite des métadonnées fictives du fournisseur. Le listener reste
observé jusqu'à fermeture : toute seconde connexion acceptée fait échouer le test.

Les stream IDs 1/3/5, les valeurs inventées exactes et les messages distincts
excluent un rejeu observé dans cette trace. Le fournisseur partagé est appelé deux
fois, celui du voisin une fois ; le compteur intermédiaire n'est qu'un contrôle
de cohérence, pas une preuve indépendante de non-rejeu. Le voisin continue après
refus et remplacement ; l'ancienne session conserve son erreur typée. Toutes les
sources sont fermées et détruites exactement une fois, puis la fermeture du
transport est attendue et vérifiée idempotente. Un nouveau start sur transport
fermé ne rappelle pas le fournisseur.

La revue ciblée Opus a relevé que supprimer le listener ne détectait pas une
tentative de reconnexion. Main a gardé une branche d'acceptation prioritaire avant
l'exécution. Aucun défaut de production n'est revendiqué : ce lot ajoute des
tests, sans changement de Cargo, TLS, authentification ou politique de retry.

### Validation Windows locale

- Transport : **107 réussis, 0 échec, 1 ignoré** sur stable 1.99.1 et MSRV 1.88.0.
- Workspace stable, toutes features : **404 réussis, 0 échec, 2 ignorés**.
- Formatage global et Clippy transport tous targets avec `-D warnings` passent.
- Les deux noms de tests sont confirmés dans chacun des trois groupes ; ces
  réexécutions ne sont pas des tests distincts à additionner.
- Les 175 sources sélectionnées restent identiques avant/après ; CLI release
  inchangée. Pas de nouveau build release ou de qualification Linux/macOS déduit.

```sh
cargo test -p rambler-transport --test credential_rejection --locked --offline
cargo +1.88.0 test -p rambler-transport --locked --offline
cargo test --workspace --all-features --locked --offline
```

[Preuve, hashes et commandes par groupe](../evidence/rust-credential-rejection-offline-20261116.json).
Les délais de 5 s par étape et 20 s par scénario bornent les blocages ; ce ne sont
pas des seuils de performance. Pas de sleep ni de vrai credential, audio ou service.

Le refus est envoyé **après upload complet**, en headers HTTP 200 puis trailers
gRPC séparés. Le refus précoce headers-only et un voisin encore en upload ne sont
pas combinés avec le remplacement ici. Une trace finie ne prouve pas tout
comportement futur ; la rotation explicite fictive n'est ni un provisionnement,
ni une autorisation ou récupération après révocation auprès du vrai service.
Le smoke racines et l'oracle PyAV ignorés ne sont pas requalifiés par ces commandes.

### CI du contrat après upload complet — commit `0e5ce23`

Les 13 jobs Rust/Python ont réussi sur
`0e5ce2336ce6d85fc487930198fb3ca93491b7cf`. Les huit jobs natifs couvrent
Windows, Ubuntu, macOS ARM64 et macOS Intel, chacun sur Rust 1.88.0 et stable.
Les 48 groupes Rust sont qualifiés par commande : workspace Windows 404/0/2,
autres OS 405/0/2 ; reuse 10, profiling 8, finalisation 1, budgets 3 et feature-off
38 réussites par job. Les deux noms du contrat après upload sont présents dans
les huit workspaces. Les huit gates publics racines/PCM et le lint passent aussi.

Les quatre jobs Python passent : 663 réussis/71 ignorés sous 3.11, 733/1 sous
3.14, chacun sur deux OS ; 335 sous-tests séparés, sans addition aux tests.
Les API run/job/step, le HEAD et les hashes des 1 492 824 octets de logs sont
vérifiés. [Preuve CI et versions observées](../evidence/rust-credential-rejection-ci-20261116.json).
Cette CI ne qualifie pas l'extension précoce décrite ensuite, ni une authentification
réelle. Le smoke racines établit une configuration, pas un handshake TLS.

## Refus headers-only pendant un autre upload — extension locale

Deux tests supplémentaires rejouent les mêmes codes avec HTTP 200, `grpc-status`
dans les HEADERS initiaux et END_STREAM, sans DATA ni trailers. Le pair attend
le premier message complet et l'entrée réelle dans le second futur `next` avant
le refus ; l'EOF de la source refusée n'est jamais libéré. Après cleanup, ses flags
`source_exhausted` et `upload_eof_polled` restent faux, `source_closed` vrai.

Le voisin est encore en upload pendant refus, remplacement des métadonnées et
succès de la nouvelle session explicite. Sa porte est ouverte seulement ensuite :
le pair vérifie ses deux frames distinctes complètes et sa fin saine. Les comptes
des futurs `next` entrés ne sont pas des comptes de tous les appels bas niveau à
`Future::poll`. Refus sticky, absence observée de rejeu/seconde connexion, isolation,
cleanup unique et fermeture idempotente restent contrôlés par le harness partagé.
Les deux scénarios précédents et leurs noms sont conservés.

Windows stable 1.99.1 et MSRV 1.88.0 passent **109/0/1** au transport ; workspace
stable toutes features **406/0/2**, fmt global et Clippy transport strict passent.
Les quatre noms sont confirmés dans les trois groupes, sources avant/après et
CLI release inchangées. Pas de nouveau build release ; CI de cette extension
non exécutée à ce checkpoint. [Preuve locale distincte](../evidence/rust-credential-rejection-early-offline-20261116.json).

Ces pairs et messages inventés n'établissent ni révocation réelle, provisionnement,
récupération autorisée, qualité ASR, gain de performance ou parité Android. Aucun
nouvel appel service, appareil ou chargement de vrais credentials n'est effectué.

### Qualification distante de l'extension — source `1d66138`

Les 13 jobs de `1d66138fbb79a1dcad1f52e6d2d4c49350da462d` ont réussi :
huit natifs Windows/Ubuntu/macOS ARM64/macOS Intel × Rust 1.88.0/stable, lint
et quatre Python. Les quatre noms de scénarios sont confirmés dans chacun des
huit workspaces. Chaque workspace Windows rapporte **406/0/2**, les autres OS
**407/0/2** ; les cinq autres groupes restent reuse 10, profiling 8, finalisation
1, budgets 3 et feature-off 38 réussites par job. Les quatre groupes Python
restent 663/71 sous 3.11 et 733/1 sous 3.14, chacun sur deux OS, avec 335
sous-tests rapportés séparément.

Les métadonnées API run/job/step et 1 494 689 octets de logs sont vérifiés ; les
48 groupes Rust restent séparés par commande, sans addition de réexécutions.
La source Linux/MSRV et macOS Intel/MSRV est donc effectivement exécutée,
pas déduite de l'inspection statique. Le dernier job Intel 1.88 a terminé avec
succès après son build release. [Preuve CI liée au commit](../evidence/rust-credential-rejection-early-ci-20261116.json).

Le checkpoint local « CI non exécutée » précédent reste historique. Les
qualifications de racines publiques/PCM sont celles des gates offline existants,
pas un handshake, une authentification distante ou une nouvelle dictée Rambler.
Les limites de provisionnement, révocation réelle et parité restent inchangées.
