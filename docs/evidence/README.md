# Observations réseau

Ces fichiers sont des observations expurgées, pas des preuves de fonctionnement
Rambler. Aucun audio ni identifiant utilisateur n'a été envoyé lors de ces tests.
Les mesures ponctuelles ne sont pas des benchmarks et ne permettent pas de
conclure à un gain de performance.

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
