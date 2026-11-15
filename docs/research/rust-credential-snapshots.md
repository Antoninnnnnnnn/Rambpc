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
