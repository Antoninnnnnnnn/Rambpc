# Moteur et CLI Rust

Le portage est séparé de la référence Python 0.3 : aucun interpréteur Python,
Android, téléphone relais ni processus FFmpeg dans le chemin Rust. Il utilise le
même service S3Bidi Rambler et le même routage, pas une API Gemini publique.
Voir [le contrat de portage](architecture/portage-rust.md).

## Construire et vérifier hors service réel

Depuis la racine, avec Rust/Cargo et les outils de compilation natifs de la
plateforme (MSVC sur Windows ; compilateur C pour `ring` et libopus), ainsi que
CMake pour l'audio et les tests :

```sh
cargo fmt --all -- --check
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
# Binaire Ogg minimal, sans encodeur ni CMake dans ses dépendances de build
cargo build --release --locked -p rambler-cli
# Binaire avec entrée WAV/PCM native
cargo build --release --locked -p rambler-cli --features audio
```

Le workspace et les dev-dependencies de test incluent libopus même si la feature
CLI est désactivée. Seul le build normal du binaire Ogg peut s'en passer. Si CMake
n'est pas sur le PATH, définir `CMAKE` avec le chemin absolu de son exécutable.
Voir [le guide audio](../crates/rambler-audio/BUILDING.md) pour la toolchain validée,
l'outil de build isolé sans installation globale et les licences de redistribution.
Libopus 1.6.1 est liée statiquement ; les DLL système/CRT MSVC restent nécessaires,
pas une DLL Opus, Python ou FFmpeg.

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

## CLI PCM native optionnelle

Après construction avec `--features audio` :

```sh
target/release/rambler.exe dictate-wav input-16k-mono.wav --language fr-FR
target/release/rambler.exe dictate-wav input-16k-mono.wav --language fr-FR --realtime
# Comparaison explicite du batching, sans changer les trames codec 20 ms
target/release/rambler.exe dictate-wav input-16k-mono.wav --language fr-FR --page-duration-ms 60
```

Le WAV doit être PCM16LE mono16k, pas du PCM arbitraire automatiquement rééchantillonné.
La source est lue progressivement (chunks CLI 20 ms), encodée VoIP/VBR24kbps et
muxée en Ogg continu ; le flush/lookahead et la granule EOS conservent les derniers
échantillons, sans allonger la durée décodée par le padding. `--realtime` simule une
source fichier, pas un microphone ; le premier chunk n'attend pas artificiellement.
`--page-duration-ms` accepte les multiples de 20 entre 20 et 1000, défaut 20.
Les autres options de contexte, langues et métadonnées sont les mêmes que `dictate`.
Le build sans feature audio ne propose pas cette commande.

Les [quatre essais réels complémentaires](evidence/rust-pcm-functional-public.json)
valident PCM accéléré/progressif au même texte final, réécriture intention 3 et
sélection avec emoji. Les 156 tests Rust et un oracle indépendant ignoré par défaut
(passé séparément, 57 conteneurs) sont distincts de cette preuve réseau. Les notices
[libopus/wrapper](../crates/rambler-audio/THIRD_PARTY_LICENSES) accompagnent toute
redistribution du binaire audio.

Ctrl-C pendant collecte ou fermeture supprime le résultat, annule puis attend le
nettoyage, avec erreur JSON et exit 130. Une erreur de fermeture est également
indiquée. La supervision est testée par simulation déterministe et par
[signal console Windows réel en loopback](evidence/rust-windows-ctrl-c.json), dans
une nouvelle console dont les seuls processus attachés sont le pilote et son
client enfant. Le premier pilote a temporisé ; l'isolation/héritage console a été
corrigé sans modifier le binaire. Reproduction offline avec le venv de test et le
fixture contrôlé préparé :

```sh
.venv/Scripts/python -I tools/validate_rust_ctrl_c.py . local/ctrl-c-new.json
```

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
- `rambler-audio` : source WAV stricte et adaptateur PCM → Ogg/Opus natif statique,
  fragmentation, tail/flush, pacing et cleanup bornés ; aucun microphone.
- `rambler-cli` : adaptateur fichier minimal, sans microphone, GUI ou insertion.

Un [exemple d'intégration Rust compilé par les doctests](../crates/rambler-engine/src/lib.rs)
illustre la collecte depuis une source fournie par l'appelant, le contexte et les
credentials injectés. Pour les signatures détaillées :

```sh
cargo doc -p rambler-engine -p rambler-transport -p rambler-protocol --no-deps --open
```

`SourceError` masque les détails sensibles dans `Display`, `Debug` et la chaîne
d'erreur automatique. `original()` est un accès explicite à l'erreur typée,
compatible avec `downcast_ref` ; ne pas journaliser ce contenu aveuglément.

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
  réception du serveur. `metrics.eof_seconds` marque la fin du consommateur après
  drainage/nettoyage, **pas** l'instant où les derniers échantillons PCM sont lus.
- Le texte simplifié exige une unique finale/hypothèse interprétable : sans
  VoiceEdit, texte d'hypothèse ; intention 2, texte/contexte édité vérifié ;
  intention 3, champ `edited_text` seulement. Les autres cas restent ambigus.
- Une proposition de remplacement reste limitée à l'intention 2 et ne modifie
  rien. Vérifier snapshot texte/révision avant application ; les positions sont
  en unités UTF-16, pas en octets Rust. Les révisions texte et entier sont des types
  distincts ; les entiers canoniques ne sont pas bornés à 64 bits.

## Endurance explicite et rapports privés

`tools/validate_rust_endurance.py` est un **outil de développement Python**, pas
un composant du binaire. Il lance le vrai exécutable, mesure son RSS/CPU enfant,
borne sorties/durée/sessions, exige les marqueurs dans le texte rendu et compare
les finales connues ordonnées. Il s'arrête au premier échec/non-concluant, sans
rejeu. Chaque session est un processus et un canal neufs.

Après préparation du corpus synthétique contrôlé et chargement explicite de
l'environnement autorisé, exemple Windows / Git Bash :

```sh
.venv/Scripts/python tools/validate_rust_endurance.py --allow-live --stage 120 \
  --executable target/release/rambler.exe \
  --audio artifacts/endurance-synthetic/endurance-two-minute-en-16k.wav \
  --timeout 150 --sessions 1 --compiler-version "$(rustc --version)" \
  --build-version 'COMMIT;release;features=audio' \
  --marker-beginning 'amber orchard' --marker-middle 'cobalt lantern' \
  --marker-end 'violet harbor' --output local/rust120-new.json \
  --public-output local/rust120-public-new.json
```

Le palier `--stage 300` requiert un `--previous-report`120 sain compatible avec le
même binaire/configuration ; utiliser le fichier cinq minutes et un délai330s,
au plus trois sessions. Le gate est une précaution locale, pas une attestation.
La preuve enregistrée contient un **échec au deuxième essai long** ; ne pas
présenter la commande comme une garantie de succès. Les empreintes pré/post du
fichier ne sont pas une preuve indépendante de chaque octet consommé.

Le rapport complet conserve stdout/stderr privés bornés ; la projection
`--public-output` est distincte, par allowlist, et doit encore être inspectée.
Les versions compiler/build sont des déclarations de l'appelant, pas déduites du
binaire. Les métriques sont échantillonnées, pas des pics exacts. Sous Windows,
le PATH enfant est limité aux répertoires système ; cette politique sous Unix
n'établit pas l'absence d'interpréteurs dans ces répertoires.

## Périmètre encore distinct

Le moteur prend de l'Ogg/Opus déjà encodé ou du PCM via l'adaptateur natif
optionnel. Les mesures d'endurance Rust sont rapportées séparément des preuves
Python, avec leur modèle de processus/canal ; aucune exécution réelle
multiplateforme n'en est déduite. La composition intention 4 n'a pas été démontrée
sur le profil observé. Le provisionnement et la révocation des métadonnées
applicatives restent des limites externes.
