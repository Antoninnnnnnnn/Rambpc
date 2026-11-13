# Observations réseau

Ce répertoire distingue les diagnostics de transport des véritables réponses
Rambler. Les deux premiers fichiers ci-dessous n'envoyaient aucun audio ni
identifiant utilisateur. Les fichiers fonctionnels ajoutés ensuite utilisent
des phrases synthétiques contrôlées, puis un unique extrait naturel public licencié,
et des métadonnées applicatives en mémoire, dont les valeurs ne sont jamais
enregistrées ici. Les limites et
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
- `native-realtime.json` : six sessions natives sur le code0814813, profil20/1000ms,
  source PCM intégrale, Ogg redécodé identique entre pages, aucun FFmpeg à l'exécution,
  provenance du code et des dépendances enregistrée.
- `lifecycle.json` : sélection/contexte différé, annulation volontaire, fermeture
  source puis deux sessions concurrentes isolées sur le même canal.
- `installed-wheel-smoke.json` : vraie dictée depuis la roue installée dans un venv
  vierge sans PyAV, configuration locale, aucun APK/JADX lu à l'exécution.
- `ci-0814813.json` : succès des quatre jobs Windows/Linux × Python3.11/3.14 au
  jalon correspondant ; CI offline/loopback, pas preuve réseau Google sur Linux.
- `environment.json` : environnement non sensible et conditions non contrôlées ;
  snapshot après premiers essais, pas provenance de code rétroactive.

Trois réponses finales **originales** inspectées (pas reconstruites) sont publiques
comme [fixtures](../../tests/fixtures/live_synthetic_responses.json), avec SHA exact,
hex et phrases de synthèse. Les tests offline vérifient le décodage et le maintien
des champs inconnus. Ces fixtures n'incluent aucun header, credential, cookie,
identifiant de compte ou texte personnel. Les autres captures et audios restent
ignorés par git. Les rapports textuels publics sont réservés aux corpus contrôlés
ou explicitement publiables avec leur licence, pas à des dictées personnelles.

## Raffinement du 2026-11-13

- `refinement-page-grid.json` : 20 transports réussis, cinq par page 20/40/60/100 ms,
  source native, politique `muxer`, toutes les mises à jour finales ordonnées et PCM
  décodés identiques. Pas de capture protobuf brute. Défaut 20 ms conservé ; données
  et outlier à interpréter selon [performance.md](../performance.md).
- `natural-corpus-inputs.json` : provenance, hashes et attribution CC BY 4.0 de
  l'unique extrait anglais LibriSpeech utilisé. Audio non versionné.
- `refinement-capabilities-public.json` : projection expurgée du premier corpus
  de cinq cas. Cinq transports réussis, mais **4/5 rendus** dans le code d'alors :
  intention 3 non encore interprétée. Ce bilan historique reste inchangé.
- `refinement-long-realtime-public.json` : une dictée de 40,564 s PCM à débit simulé,
  intégralement consommée, texte corrigé et référence finale conservés, collecte
  complète 41,454 s. Même texte que le cas accéléré, pas preuve d'endurance prolongée.
- `refinement-rewrite-public.json` : nouveau test réel après extension du rendu,
  texte réécrit intention 3/champ 3 rendu avec succès ; pas de remplacement d'éditeur.
- `refinement-wheel-smoke.json` : roue 0.2.0 finale construite, installée dans un venv
  vierge sans PyAV ; imports, égalité des modules avec le source et rendu de la fixture
  dérivée vérifiés. Offline uniquement, aucun appel Google supplémentaire.
- [`live_rewrite_response.json`](../../tests/fixtures/live_rewrite_response.json) :
  **fixture dérivée/expurgée**, et non quatrième payload original. Composants sûrs
  conservés, wrapper reconstruit, source et composants hashés, extension 74956225
  retirée. Inspection indépendante du source : intention 3/champ 3 confirmée ;
  portée générale de remplacement inconnue.

Les trois copies `*-public.json` indiquent hash/longueur du rapport privé source,
hash de l'outil de projection et résumés des payloads. Les octets bruts, valeurs
inconnues et identifiants opaques de service/session ne sont pas publiés ; la
bibliothèque, elle, les préserve pour l'appelant. Le hash du code SDK utilisé figure
dans les versions ; le seul HEAD ne suffit pas pour l'essai réécriture sur arbre modifié.

L'outil offline `tools/sanitize_corpus_report.py` utilise une allowlist fermée et
refuse toute sortie existante. Les textes sont hachés par défaut ; `--include-text`
sert uniquement après revue d'un corpus contrôlé. L'outil n'est pas une garantie
universelle d'anonymisation : texte, labels locaux, métadonnées machine et même hashes
peuvent nécessiter une revue. Ne pas publier directement les rapports complets de
`validate_corpus.py --include-results`, même pour de l'audio synthétique.
