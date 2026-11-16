# Frontières HTTP/2 : type MIME et métadonnées invalides

## Pourquoi ce lot

Le passage parallèle a couvert dix périmètres du backend en lecture seule, puis
contre-vérifié deux propositions et synthétisé la couverture (13 agents Sol au
total, aucune erreur/skippage signalé). Une proposition était confirmée par
lecture du code, l'autre restait non résolue faute d'observation de la conversion
tonic/h2. Les tests locaux RED/GREEN sont distincts de ces conclusions statiques.
Ce passage n'est pas une preuve de perfection ou de couverture exhaustive.

## Type MIME gRPC insensible à la casse

[RFC 9110 §8.3.1](https://www.rfc-editor.org/rfc/rfc9110.html#section-8.3.1)
spécifie : « The type and subtype tokens are case-insensitive. » Le client
comparait pourtant les octets du Content-Type à des littéraux minuscules : une
réponse HTTP200 `Application/Grpc`, avec DATA gRPC valide et trailers OK, était
refusée avant lecture des messages.

La correction compare uniquement la base `application/grpc` sans distinction de
casse ASCII, sans allocation de chaîne mise en minuscules. Les mêmes frontières
restent exigées : fin exacte, suffixe commençant par `+` ou paramètres par `;`.
`application/grpc-web`, une base prolongée par `X`, les doublons et les contrôles
d'encodage/statut ne sont pas rendus permissifs. Les payloads ne sont ni réécrits
ni redécodés différemment. Ce n'est pas une réponse de blocage safety ignorée.

Aucun incident du véritable service n'est attribué à ce défaut : la casse de ses
en-têtes n'a pas été réobservée dans ce lot. La reproduction est HTTP/2 locale.

## En-têtes de connexion refusés dès la validation

Cinq noms interdits par HTTP/2 sont désormais rejetés par `Metadata::new` avec
`InvalidMetadata` :

- `connection`
- `keep-alive`
- `proxy-connection`
- `transfer-encoding`
- `upgrade`

[RFC 9113 §8.2.2](https://www.rfc-editor.org/rfc/rfc9113.html#section-8.2.2).
Dans les dépendances verrouillées, `tonic`0.14.9 `MetadataMap::into_headers` ne
retire pas ces valeurs et `h2`0.4.20 les rejette dans sa validation locale de
requête. Un test avec métadonnées inventées et Tokio duplex en mémoire établit
ce chemin, avec une métadonnée personnalisée valide comme contrôle positif.
Aucun DNS, socket distant, TLS ou compte n'est nécessaire à cette observation.

Le comportement antérieur acceptait ces noms à la construction du snapshot,
puis échouait plus tard à la soumission h2. La nouvelle validation est un refus
précoce explicite, **pas** une suppression silencieuse. Elle ne généralise pas
l'interdiction à toutes les clés `grpc-*`, ne modifie pas les valeurs du provider
et préserve limites, doublons, binaire, marqueurs sensibles et contrat des trois
variables d'environnement. C'est un resserrement sur des entrées qui ne pouvaient
pas former une requête HTTP/2 valide, pas un nouveau mécanisme d'authentification.

## Preuves et reproduction

```sh
cargo test -p rambler-transport --locked --offline
cargo +1.88.0 test -p rambler-transport --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```

La [preuve structurée](../evidence/rust-http2-boundaries.json) sépare reproductions
initiales, corrections, contre-revues et vérifications de l'arbre final. Les logs
complets restent privés ; hashes de sources/logs et résultats sont conservés.

Aucun appel Google/appareil, aucun accès réel renouvelé, aucune requête rejouée,
aucune modification du protocole S3, du codec ou de la politique de résultat.
Pas de gain de vitesse ou de latence revendiqué ; les comparaisons de type MIME
ont lieu dans la validation des en-têtes, pas dans le chemin d'encodage audio.
