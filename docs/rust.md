# Moteur et CLI Rust

Le portage est séparé de la référence Python 0.3 : aucun interpréteur Python,
Android, téléphone relais ni processus FFmpeg dans le chemin Rust. Il utilise le
même service S3Bidi Rambler et le même routage, pas une API Gemini publique.
Voir [le contrat de portage](architecture/portage-rust.md).

## Construire et vérifier hors service réel

Depuis la racine, avec Rust/Cargo et les outils de compilation natifs de la
plateforme (MSVC sur Windows ; compilateur C pour `ring`) :

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked -p rambler-cli
```

Le manifeste déclare Rust 1.88 minimum ; cela ne signifie pas qu'une matrice de
compilateurs/OS a déjà été exécutée. Les tests n'utilisent que des fixtures et des
serveurs loopback, jamais Google. Le téléchargement initial des dépendances Cargo
nécessite Internet ; `--locked` fixe leurs versions, pas leur disponibilité locale.

Le binaire est `target/release/rambler.exe` sur Windows et
`target/release/rambler` sur Linux/macOS. Aucun environnement virtuel Python n'est
nécessaire. La présence de sources Python dans le dépôt ne constitue pas une
dépendance du binaire.

## CLI fichier Ogg/Opus

Fournir les trois variables d'[authentification](research/authentication.md) dans
l'environnement : `RAMBLER_API_KEY`, `RAMBLER_ANDROID_PACKAGE`,
`RAMBLER_ANDROID_CERT`, toutes présentes ou toutes absentes. Aucun secret n'est
fourni dans un argument de commande ; la CLI ne charge pas `.env` implicitement.
Le chargement explicite Git Bash est décrit dans le README.

```sh
# Windows / Git Bash ; retirer .exe sur Linux/macOS
target/release/rambler.exe dictate input.ogg --language fr-FR
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --before 'The meeting is tomorrow at ten.'
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --before 'The meeting is tomorrow at ' --selected ten --after '.' --cursor-aware
```

L'entrée doit être un **flux Ogg/Opus continu, mono, source 16 kHz**, lu par
fragments de 8 192 octets. Seule la signature `OggS` est contrôlée par cette CLI :
ce n'est pas un validateur complet du conteneur ou du codec. Elle ne convertit pas
un WAV et ne simule pas le temps réel. Ne pas étiqueter des octets PCM comme Opus.

Options : `--timeout` (30 s), `--context-timeout` (2 s), ainsi que les listes
répétables `--bias-term`, `--preference`, `--secondary-language`. Ces dernières
sont émises à l'init ; le contexte texte est envoyé après l'audio. Leur encodage ne
prouve pas leur effet sur le serveur. Ne pas placer de texte personnel dans les
arguments si l'historique shell/la liste des processus peut être consulté.

La sortie est une ligne JSON UTF-8 après collecte **et** nettoyage. Elle contient
les finales connues ordonnées, le contexte et les métriques, mais pas les octets
bruts, champs opaques, identifiants de résultat ou credentials. Une suppression
valide reste `"text":""`. Une ambiguïté conserve les finales et produit un code
de sortie non nul ; aucune dernière hypothèse n'est choisie arbitrairement. Le
contenu textuel reste sensible : la sortie n'est pas un journal automatiquement
anonymisé.

`--loopback 127.0.0.1:PORT` est réservé aux tests locaux en clair. Il refuse les
adresses non-loopback et n'utilise **pas** les credentials de l'environnement.
Le chemin normal utilise TLS vérifié avec les racines système et ALPN HTTP/2.

## Bibliothèques

- `rambler-protocol` : encodage des requêtes et décodage protobuf borné ; présence,
  inconnus, ordre et fusion proto2 préservés, octets racine exacts.
- `rambler-transport` : connexion HTTP/2 partagée, framing gRPC, métadonnées,
  délais et superviseurs de nettoyage. Client `h2` + `tokio-rustls`/`ring` ;
  `tonic` reste utilisé pour les types gRPC et les serveurs de test. Le client
  tonic/hyper initial n'offrait pas le contrôle de reset nécessaire au test
  d'annulation sous backpressure ; ce changement ne modifie pas le backend.
- `rambler-engine` : assemblage des sessions, source audio asynchrone, contexte
  différé, événements, collecte bornée et politique de résultat/proposition.
- `rambler-cli` : adaptateur fichier minimal, sans microphone, GUI ou insertion.

`Engine::connect(TransportConfig)` possède son transport ;
`Engine::new(transport)` utilise un transport injecté qui reste à fermer par
l'appelant. `Engine::start(Box<dyn AudioSource>, SessionOptions)` prend la source
en charge immédiatement. `StreamAudio::new(stream)` adapte un flux de fragments
`Bytes`. Les implémentations de source doivent coopérer avec l'annulation et leur
`close` asynchrone doit terminer.

`SessionOptions` regroupe la configuration, les délais, un
`Option<Arc<dyn MetadataProvider>>` et `ContextInput::{None, Static, Provider}`.
Un provider de contexte est appelé une seule fois, après EOF/nettoyage audio,
sous le minimum du budget de contexte et du délai restant. Les credentials sont
injectables par session ; `EnvironmentCredentials` est une implémentation fournie.

`collect_dictation(&engine, source, options, limits)` rend une future dont
l'ownership est établi dès l'appel, même si elle n'est jamais pollée. Elle conserve
par défaut au plus 128 finales et 8 Mio de réponses brutes associées. Pour traiter
les événements, consommer `EngineSession::next()` jusqu'à `None` et appeler
explicitement `close()` en cas d'arrêt anticipé.

## Finalité, annulation et résultats

- Une fin réussie nécessite END S3, upload entièrement épuisé/half-clos, source
  fermée et statut gRPC OK. Une finale reçue avant cela n'est pas une réussite.
- Une erreur S3 n'interrompt pas le drainage requis pour détecter une erreur gRPC
  tardive. Aucun échec ne déclenche un rejeu automatique d'audio.
- Arrêt normal : terminer la source et drainer les réponses. Annulation :
  `request_cancel()` puis `close().await` ; ce n'est pas une finalisation de dictée.
- `Drop` signale l'annulation mais ne peut pas attendre un nettoyage asynchrone.
  Garder Tokio vivant et attendre explicitement la fermeture des sessions/moteurs.
  Un nettoyage coopératif peut dépasser le délai réseau.
- `Engine::close()` et `GrpcTransport::close()` attendent les nettoyages, puis
  livrent au plus une erreur non encore observée par appel. S'il y en a plusieurs,
  rappeler `close()` jusqu'à `Ok` pour les relever toutes ; elles ne sont pas
  silencieusement acquittées ensemble. Une future de fermeture interrompue avant
  son résultat ne doit pas acquitter une erreur qu'elle n'a pas livrée.
- Les files sont bornées ; un consommateur suspendu peut retarder la lecture des
  trailers. Les compteurs d'upload décrivent le travail client, pas un accusé de
  réception du serveur.
- Le texte simplifié exige une unique finale/hypothèse interprétable : sans
  VoiceEdit, texte d'hypothèse ; intention 2, texte/contexte édité vérifié ;
  intention 3, champ `edited_text` seulement. Les autres cas restent ambigus.
- Une proposition de remplacement reste limitée à l'intention 2 et ne modifie
  rien. Vérifier snapshot texte/révision avant application ; les positions sont
  en unités UTF-16, pas en octets Rust. Les révisions texte et entier sont des types
  distincts ; les entiers canoniques ne sont pas bornés à 64 bits.

## Périmètre encore distinct

Cette tranche accepte de l'Ogg/Opus déjà encodé. Le chemin WAV/PCM avec encodeur
natif, les mesures Rust CPU/RSS/endurance et la validation réelle multiplateforme
ne sont pas implicitement couverts par les preuves Python. La composition
intention 4 n'a pas été démontrée sur le profil observé. Le provisionnement et la
révocation des métadonnées applicatives restent des limites externes.
