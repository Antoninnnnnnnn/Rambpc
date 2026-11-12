# Observations réseau

Ce répertoire distingue les diagnostics de transport des véritables réponses
Rambler. Les deux premiers fichiers ci-dessous n'envoyaient aucun audio ni
identifiant utilisateur. Les fichiers fonctionnels ajoutés ensuite utilisent
uniquement des phrases synthétiques contrôlées et des métadonnées applicatives
en mémoire, dont les valeurs ne sont jamais enregistrées ici. Les limites et
conditions de comparaison sont dans [performance.md](../performance.md).

## `connectivity.json`

Une connexion TCP puis TLS vérifiée vers `speechs3proto2-pa.googleapis.com:443`
a négocié TLS 1.3 / ALPN `h2`. `dns_plus_tcp_ms` englobe résolution DNS et TCP ;
`tls_handshake_ms` couvre seulement le handshake TLS après TCP. Échantillon unique.
Aucun échange HTTP/gRPC lors de ce contrôle.

Reproduction de principe en Python standard : `socket.create_connection` puis
`ssl.create_default_context().wrap_socket(..., server_hostname=host)`, avec
`set_alpn_protocols(["h2"])` et `time.perf_counter` de part et d'autre. Ne pas
désactiver la vérification du certificat.

## `anonymous-empty-rpc.json`

Un message protobuf vide, puis half-close, sans métadonnées applicatives ni
identifiants : statut gRPC `PERMISSION_DENIED`, « Permission denied on the service. ».
Aucune réponse protobuf reçue. Ceci **ne prouve pas** qu'une requête conforme au
client réel serait refusée, ni qu'Android est requis.

Reproduction (réseau réel, une seule tentative, aucune donnée audio) :

```sh
python tools/probe_empty_request.py --timeout 10 --output local/anonymous-probe.json
```

Le script refuse d'écraser un fichier de résultat existant. Installer `grpcio`
dans l'environnement virtuel au préalable. Version Python/gRPC, endpoint,
paramètres et durée figurent dans l'observation. L'absence de champs d'une requête
réelle et des métadonnées d'identification du client limite fortement ce test.

## Réponses fonctionnelles réelles

- `reconstructed-default-session.json` : premier flux complet Ogg/Opus depuis
  Windows, service/routage Agentic, texte attendu, VoiceEditingResult2, fin S3 puis
  gRPC OK, sans compte/Zwieback. Durée audio6,665s envoyée en accéléré.
- `functional-cases.json` : correction orale, contexte avant curseur et français,
  via l'API publique `RamblerEngine` sur un canal réutilisé. Vérification SHA-256
  de l'entrée réellement consommée. Les33 payloads réponses ont été redécodés
  indépendamment, sans importer notre moteur/codec.
- `native-pcm-cli.json` : CLI PCM avec PyAV/libopus, sans binaire FFmpeg à
  l'exécution, dans un nouveau processus ; exit0 et texte correct. Le rapport
  enregistre les hashes des sources propres utilisées.
- `performance-baseline.json` :21 sessions séquentielles dont warmup, comparaisons
  blocs224/8192 et canaux API neufs/réutilisés. Toutes réussies ; mêmes textes et
  hashes audio, aucune preuve de handshake physique neuf pour chaque canal API.
- `realtime-paging.json` :6 sessions à source FFmpeg `-re`, pages1000/20ms, mêmes
  PCM décodés et textes, métriques producteur et fins séparées.
- `environment.json` : environnement non sensible et conditions non contrôlées ;
  snapshot après premiers essais, pas provenance de code rétroactive.

Trois réponses finales **originales** inspectées (pas reconstruites) sont publiques
comme [fixtures](../../tests/fixtures/live_synthetic_responses.json), avec SHA exact,
hex et phrases de synthèse. Les tests offline vérifient le décodage et le maintien
des champs inconnus. Ces fixtures n'incluent aucun header, credential, cookie,
identifiant de compte ou texte personnel. Les autres captures et audios restent
ignorés par git. Les rapports avec résultat textuel sont réservés à ce corpus
synthétique explicitement publiable, pas à des données dictées par l'utilisateur.
