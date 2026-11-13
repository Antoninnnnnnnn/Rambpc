# Observations réseau

## Portage Rust

- [rust-pcm-functional-public.json](rust-pcm-functional-public.json) : PCM natif
  accéléré/progressif au même texte, réécriture intention3 et sélection avec emoji ;
  quatre appels réels Windows, libopus statique, source/lock/binaire hashés.
- [rust-endurance-120-public.json](rust-endurance-120-public.json) et
  [rust-endurance-300-public.json](rust-endurance-300-public.json) :120s sain, puis
  premier288s sain mais deuxième `incomplete` à282s, troisième non lancé.
  Le rapport du lot long reste **failed_or_inconclusive**, pas corrigé en3/3.
  Chaque session est un processus/canal neuf ; les mesures ne valident pas la
  réutilisation du canal ni une absence générale de fuite. La catégorie
  `incomplete` vient de l'inspection de la sortie CLI privée ; l'ancienne projection
  ne conserve que son exit non nul. Cause exacte non établie par cette capture.
- [rust-windows-ctrl-c.json](rust-windows-ctrl-c.json) : vrai signal Windows dans
  une console isolée, client/RPC loopback annulé, exit130 ; aucun Google. Le premier
  pilote a temporisé puis a été corrigé, sans changer le binaire.

- [rust-first-slice-public.json](rust-first-slice-public.json) : trois sessions
  réelles d'une CLI release **Rust** sur Windows, Ogg/Opus déjà encodé : anglais,
  édition sur contexte et français. Le binaire fonctionne avec un PATH système
  sans Python/FFmpeg ; Python ne sert que de lanceur de développement. TLS vérifié,
  END S3 et gRPC OK, sortie après nettoyage, zéro retry. Hashes exacts du binaire,
  du lock et des fichiers sources avant leur premier commit d'intégration.
  Les textes synthétiques connus ont été inspectés ; pas de nouvelle inspection
  binaire indépendante, ni preuve d'endurance/PCM natif ou de gain de vitesse Rust.
  Les captures stdout/stderr originales restent dans `artifacts/`, hors git.

Ce répertoire distingue les diagnostics de transport des véritables réponses
Rambler. Les deux premiers fichiers ci-dessous n'envoyaient aucun audio ni
identifiant utilisateur. Les fichiers fonctionnels ajoutés ensuite utilisent
des phrases synthétiques contrôlées et des extraits naturels publics licenciés,
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
  l'extrait anglais LibriSpeech du premier corpus. Audio non versionné.
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

## Continuation : corpus naturel, endurance et hints

- [reference-0.3-wheel-smoke.json](reference-0.3-wheel-smoke.json) : roue 0.3.0
  construite et installée dans un venv vierge sans PyAV ; imports publics, modules
  identiques au source, fixture dérivée et émission des hints/locales secondaires
  vérifiés offline. Aucun appel Google dans ce smoke.

- [natural-expansion-inputs.json](natural-expansion-inputs.json) : provenance et
  licences des dix extraits supplémentaires, sept anglais et trois français.
- [natural-expansion-public.json](natural-expansion-public.json) : dix transports
  réussis en accéléré ; [natural-expansion-quality.json](natural-expansion-quality.json)
  résume WER 3/109 EN et 3/79 FR, avec limites de référence et de normalisation.
- [natural-expansion-realtime-public.json](natural-expansion-realtime-public.json) :
  trois sessions à débit simulé, textes identiques aux essais accélérés.
- [endurance-120-public.json](endurance-120-public.json) et
  [endurance-300-public.json](endurance-300-public.json) : une session de 118,682 s
  puis trois de 288,352 s ; source intégrale, marqueurs et finales ordonnées
  contrôlés. RSS courant échantillonné, CPU/wall et hashes de modules sont indiqués ;
  mesures sur base 0.2 avant hints, sans preuve d'absence de fuite.
- [input-hints-public.json](input-hints-public.json) : dix transports réussis avec
  manifeste hashé et résumés ; effets des biais, préférences et langues secondaires
  bornés aux observations décrites dans [raffinement](../refinement.md).

Ces 13 + 4 + 10 transports, plus huit essais de composition décrits dans
[composition](../research/composition.md), font 35 transports de continuation ;
ce n'est ni une liste de 35 capacités validées ni un ajout aux comptes historiques.
Les rapports structurés d'endurance ne contiennent ni raw protobuf, ni valeurs
inconnues, ni identifiants de service ; les captures privées de continuation restent
sous `artifacts/continuation-private/`, hors git. Les textes de corpus contrôlés
publiés restent soumis à leur provenance et à une revue de confidentialité.

L'outil offline `tools/sanitize_corpus_report.py` utilise une allowlist fermée et
refuse toute sortie existante. Les textes sont hachés par défaut ; `--include-text`
sert uniquement après revue d'un corpus contrôlé. L'outil n'est pas une garantie
universelle d'anonymisation : texte, labels locaux, métadonnées machine et même hashes
peuvent nécessiter une revue. Ne pas publier directement les rapports complets de
`validate_corpus.py --include-results`, même pour de l'audio synthétique.
