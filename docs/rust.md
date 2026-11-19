# Moteur et CLI Rust

Le portage est séparé de la référence Python 0.3 : aucun interpréteur Python,
Android, téléphone relais ni processus FFmpeg dans le chemin Rust. Il utilise le
même service S3Bidi Rambler et le même routage, pas une API Gemini publique.
Voir [le contrat de portage](architecture/portage-rust.md).

## Validation CI récente

La CI hors service réel de `171ef084cc1b1e7f3b82daefdcd6bb0aadafe442`
a réussi sur quatre familles natives avec Rust 1.88.0 et stable. Chacun des
huit jobs vérifie trois rejets de budgets CLI zéro sur le binaire release et
la régression de fermeture ; ces restrictions CLI ne changent pas la sémantique
zéro de la bibliothèque. La première fermeture bornée de ce test accepte `Ok`
ou l'erreur indépendante `Connect`, et la seconde doit réussir. L'échec des
trois jobs macOS du précédent run `7327f51` reste historique.
[Résultats et limites](evidence/rust-close-ledger-zero-budgets-ci-20261115.json).

## Construire et vérifier hors service réel

Depuis la racine, avec Rust/Cargo et les outils de compilation natifs de la
plateforme (MSVC sur Windows ; compilateur C pour `ring` et libopus), ainsi que
CMake pour l'audio et les tests :

```sh
cargo fmt --all -- --check
cargo test --workspace --all-features --locked
# Les tests de cet exemple ne font pas partie du cargo test workspace implicite
cargo test -p rambler-cli --features audio --example session_reuse --locked
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

Rust **1.88.0 a été exécuté sur Windows x86_64/MSVC et Linux x86_64/GCC**.
Les snapshots exacts et les limites de chaque revalidation sont dans la
[validation du backend](research/rust-backend-validation-2026-11-14.md).
La CI du commit `4264126852b2c03e25fc231c60f9582569a84ae9` a exécuté avec
succès Windows, Linux, macOS ARM64 et macOS Intel natif sous 1.88.0 et stable.
Les résultats PCM/admission, comptes et limites figurent dans la
[validation des builds](research/rust-build-validation.md) et sa
[preuve CI](evidence/rust-pcm-admission-expanded-ci-20261115.json). Les tests de ce lot utilisent
seulement des fixtures et des serveurs loopback, sans appel réel au service. Le téléchargement initial
nécessite Internet ; `--locked` fixe les versions, pas leur disponibilité locale.

Le binaire est `target/release/rambler.exe` sur Windows et
`target/release/rambler` sur Linux/macOS. Aucun environnement virtuel Python n'est
nécessaire. La présence de sources Python dans le dépôt ne constitue pas une
dépendance du binaire.

## CLI fichier Ogg/Opus

Fournir les trois variables d'[authentification](research/authentication.md) dans
l'environnement : `RAMBLER_API_KEY`, `RAMBLER_ANDROID_PACKAGE` et
`RAMBLER_ANDROID_CERT`, toutes présentes et non vides. Sur l'endpoint normal,
la CLI ouvre d'abord l'entrée puis valide les trois champs avant `Engine::connect` ;
une absence, une configuration partielle, une valeur Unicode invalide ou une
métadonnée invalide échoue localement et la fermeture de l'entrée est attendue.
Le snapshot immuable est transmis à `SessionOptions` sans relecture ; snapshots et
diagnostics sont expurgés à l'affichage. Aucun secret n'est fourni dans un argument
de commande ; la CLI ne charge pas `.env` implicitement.
Le chargement explicite Git Bash est décrit dans le README.

```sh
# Windows / Git Bash ; retirer .exe sur Linux/macOS
target/release/rambler.exe dictate input.ogg --language fr-FR
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --before 'The meeting is tomorrow at ten.'
target/release/rambler.exe dictate instruction.ogg --language en-US \
  --before 'The meeting is tomorrow at ' --selected ten --after '.' --cursor-aware
```

L'entrée doit être un **flux Ogg/Opus continu, mono, source 16 kHz**, version1
et mapping0. La CLI émet une page validée par chunk, sans modifier les octets :
CRC, séquence, numéro de série unique, continuation, en-têtes, granules et EOS.
La première page `OpusHead` est contrôlée avant le RPC ; une corruption ultérieure
est une erreur source, jamais un EOF réussi. La page EOS n'est rendue qu'après
vérification de l'absence d'octets supplémentaires. Pages bornées à65 307 octets,
paquets et `OpusTags` à64Kio : une extension/padding légal plus grand reste
**hors profil pris en charge**, pas universellement invalide. Ce n'est pas un
décodeur Opus : durée codec, payload audio et qualité audible ne sont pas certifiés.
Elle ne convertit pas un WAV et ne simule pas le temps réel. Ne pas étiqueter du
PCM comme Opus. Le SDK `AudioSource` reste un contrat d'octets encodés fournis par
l'appelant ; cette validation fichier appartient à la CLI.

Les commandes fichier exigent des **fichiers réguliers** (liens vers un fichier
régulier permis), pas des FIFO, pipes, consoles ou périphériques. Sous Unix,
l'ouverture non bloquante permet de vérifier le descripteur sans attendre un
producteur ; sous Windows, son type est interrogé avant toute lecture. Les flux
vivants restent possibles via une implémentation asynchrone `AudioSource`, pas en
présentant un pipe comme un fichier. Le délai RPC ne borne pas tous les blocages
possibles du système de fichiers ou d'un disque défaillant. Voir
[préflight et régressions](research/rust-file-inputs.md).

Options : `--timeout` (30 s, **parole comprise**), `--context-timeout` (2 s),
`--finalize-timeout` (optionnel, absent par défaut), ainsi que les listes répétables
`--bias-term`, `--preference`, `--secondary-language`. Le budget optionnel de
finalisation commence après EOF et cleanup audio réussi ; il borne contexte,
fin d'upload et statut/livraison, sans dépasser le délai dur `--timeout`. Zéro est
refusé ; un budget énorme est plafonné au dur. Aucun `max_audio` ni faux EOF n'est
ajouté ; le cleanup reste attendu coopérativement même après expiration.
Le commit terminal irréversible, ses régressions et l'état de qualification
Windows/Linux sont distingués dans
[fiabilité et finalisation](research/rust-reliability-lifecycle.md).
Par exemple `--timeout 150 --finalize-timeout 10` illustre au plus 10 s après
EOF/cleanup sous 150 s au total, pas un réglage optimal mesuré ni 150 s de parole
plus 10 s garanties. Les listes sont émises à l'init ; le contexte texte est envoyé
après l'audio. Leur encodage ne prouve pas leur effet sur le serveur. Ne pas placer
de texte personnel dans les arguments si l'historique shell/la liste des
processus peut être consulté.

Pour `--timeout`, `--context-timeout` et `--finalize-timeout`, zéro est rejeté
à l'analyse des arguments, avant ouverture de l'entrée, lecture des credentials
d'environnement ou connexion réseau : JSON expurgé `type=error`,
`category=arguments`, code de sortie 2. Cela ne garantit pas le rejet de toute
configuration invalide avant connexion. Côté bibliothèque, les durées nulles de
`SessionOptions` restent des erreurs `EngineError::InvalidConfig` ; l'appelant
doit attendre `Engine::close().await` pour le nettoyage coopératif, même après
un rejet. [Validation locale et limites](evidence/rust-zero-cli-budgets-20261115.json) ;
la nouvelle étape CI vérifie aussi le binaire release sur les plateformes natives.

`ProtocolConfig` expose aussi `block_offensive_words: Option<bool>`,
`auto_punctuation: Option<bool>` et `secondary_routing: Option<String>`.
Les trois valeurs par défaut `None` préservent l'init historique octet pour octet.
La CLI accepte `--block-offensive-words true|false`,
`--auto-punctuation true|false` et `--secondary-routing VALEUR`. Ce sont des champs
vérifiés du client S3, pas une preuve de leur effet serveur ni un déclencheur
COMPOSE. La préférence de mots offensants ne modifie jamais les contrôles safety
sur les réponses. [Schéma, conditions et limites](research/rust-recognizer-options.md).

La sortie est une ligne JSON UTF-8 après collecte **et** nettoyage. Elle contient
les finales connues ordonnées, le contexte et les métriques, mais pas les octets
bruts, champs opaques, identifiants de résultat ou credentials. Une suppression
valide reste `"text":""`. Une ambiguïté conserve les finales et produit un code
de sortie non nul ; aucune dernière hypothèse n'est choisie arbitrairement. Le
contenu textuel reste sensible : la sortie n'est pas un journal automatiquement
anonymisé. Des finales non retenues comme texte utilisable peuvent encore contenir
la dictée ou le contexte privé dans cette sortie détaillée.

Pour brancher la CLI à la télémétrie, utiliser **`--diagnostics-only`** avec `dictate`
ou `dictate-wav`. La projection retire le texte, les hypothèses et le contexte,
y compris dans les segments terminés en mode interactif. Elle conserve seulement
les catégories connues, durées, compteurs et plages d’échantillons et porte le marqueur
`"diagnostics_only":true` après une analyse valide des arguments. Le code de sortie
reste celui de la dictée. Cette option ne transforme pas la sortie en résultat de
reconnaissance, ne change pas les données envoyées au service et ne masque pas les
arguments que le système ou le shell peuvent conserver.

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
Sur la sortie finale, `dictate-wav` ajoute un objet `pcm` lorsque l'observateur
existe : `input_bytes`, `encoded_input_samples`, `encoded_frames`, `input_eof` et
`output_eof`. La sortie Ogg et celle sans feature audio restent inchangées. Ces
compteurs locaux sont distincts des métriques encodées et ne prouvent ni réception
serveur ni absence de perte ou de rejeu. Leurs définitions et l'API optionnelle
sont dans le [contrat PCM](research/rust-pcm-channel.md). La
[qualification locale A/B](research/rust-build-validation.md#pcm-progress-and-strict-admission--local-checkpoint-2026-11-15)
précise les périmètres ; voir aussi la [preuve](evidence/rust-pcm-progress-admission.json).

Les [quatre essais réels complémentaires](evidence/rust-pcm-functional-public.json)
valident PCM accéléré/progressif au même texte final, réécriture intention 3 et
sélection avec emoji. Une [preuve distincte de réutilisation native](evidence/rust-native-session-reuse-public.json)
valide ensuite trois cas sur un même moteur, avec hash des PCM réellement consommés.
L'oracle indépendant, ignoré par défaut puis passé séparément sur57 conteneurs,
est distinct de ces preuves réseau. Les notices
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
- `rambler-audio` : source WAV stricte, entrée PCM producteur bornée et adaptateur
  PCM → Ogg/Opus natif statique, fragmentation, tail/flush, pacing fichier et
  cleanup bornés ; aucun microphone.
- `rambler-cli` : adaptateur fichier minimal, sans microphone, GUI ou insertion.

Un [exemple d'intégration Rust compilé par les doctests](../crates/rambler-engine/src/lib.rs)
illustre la collecte depuis une source fournie par l'appelant, le contexte et les
credentials injectés. `Presentation::new(&response)` fournit une vue empruntée du membre actif
(reconnaissance, événement vocal, variante opaque). `RecognitionCompletion::Complete`
signifie state1, **pas** une fin globale. Les présences des conteneurs3/4/5/7
sont indépendantes. `PartialTranscript::from_result` sépare préfixe stable et
suffixe provisoire selon le client observé ; ne jamais ajouter les aperçus de
réponses successives. La vue n'autorise aucune application dans l'éditeur.
Pour les signatures détaillées :

```sh
cargo doc -p rambler-engine -p rambler-transport -p rambler-protocol --no-deps --open
```

`SourceError` masque les détails sensibles dans `Display`, `Debug` et la chaîne
d'erreur automatique. `original()` est un accès explicite à l'erreur typée,
compatible avec `downcast_ref` ; ne pas journaliser ce contenu aveuglément.

`Engine::connect(TransportConfig)` possède son transport ;
`Engine::new(transport)` utilise un transport injecté qui reste à fermer par
l'appelant. `Engine::start(Box<dyn AudioSource>, SessionOptions)` prend la source
en charge immédiatement. Avant le premier `source.next()`, le transport attend
métadonnées valides, h2 ready et succès local de `send_request` ; ce n'est ni une
acceptation du pair/authentification, ni un health check/détecteur de NAT silencieux.
Le constructeur de la source peut déjà avoir travaillé avant le `start` synchrone.
`Engine::start` supervise le nettoyage audio même en cas de configuration invalide
ou de moteur fermé ; attendre `Engine::close().await` pour terminer ce nettoyage
également après un rejet. Pour le transport brut, c'est différent : un rejet
synchrone de `GrpcTransport::start` avant `Ok(Session)` ne transfère pas la source
au superviseur et relève du Drop Rust ordinaire. Après acceptation, la source est
nettoyée une fois même sans admission RPC réussie. `StreamAudio::new(stream)`
adapte un flux de fragments `Bytes`. Les implémentations de source doivent coopérer
avec l'annulation et leur `close` asynchrone doit terminer.

L'établissement initial résout une fois le DNS et conserve toutes les adresses,
en alternant les familles. La course TCP de style Happy Eyeballs conserve le
stagger normal de **250 ms** ; après échec observé, elle accélère avec l'échéance
`min(échéance prévue, max(dernier lancement + 10 ms, maintenant))`, sans reporter
un départ programmé. Chaque future épinglée reçoit son premier poll avec le waker
réel avant tout yield ; le dernier lancement est horodaté **après ce poll**, pas à
la construction. Ces délais d'**initiation de l'API applicative** sont inspirés du
[RFC 8305 §5](https://www.rfc-editor.org/rfc/rfc8305.txt) : défaut recommandé 250 ms,
minimum recommandé 100 ms (`SHOULD`), plancher absolu **10 ms** (`MUST NOT`).
Ils ne constituent **ni une mesure ni une preuve d'espacement des SYN noyau**,
ni une conformité RFC 8305 complète : aucune course DNS AAAA/A ni implémentation
complète de RFC 6724. Aucun gain de performance n'est revendiqué. La course yield
après initiation pour ne pas masquer le timeout et ferme les perdants avant TLS.
Le plafond de connexion 10 s par défaut couvre
DNS/TCP/TLS/h2 et conserve `Timeout` ; environ 41 tentatives bloquées sous ce défaut
n'est pas une borne pour un timeout utilisateur arbitraire. Nom TLS original,
racines système et ALPN restent inchangés ; une faute TLS ne déclenche pas une
autre destination. Ce n'est pas un retry/replay/reconnect de session et aucun
keepalive/PING n'est ajouté. La [correction de premier poll et ses 19 tests ciblés](research/rust-connection-lifecycle.md#correction-ciblée-du-pacing-tcp--15-novembre-2026)
est distincte des [résultats intégrés Windows/Linux](research/rust-build-validation.md#lot-de-fiabilité-du-15-novembre).
Les 13 anciens tests dont l'oracle enregistrait la construction des futures ne
qualifient pas le premier poll.
La fenêtre h2 de réception connexion 1 Mio / stream 65 535 octets et les files
applicatives de capacité un donnent une marge finie, pas une isolation pour N
streams quelconques ni une borne RSS globale.

Pour du PCM produit progressivement, `rambler_audio::pcm_channel()` fournit un
producteur unique et une source à composer avec `Pcm16Opus`. Réserver avant de
remettre les bytes évite leur perte lors d'une attente de capacité annulée.
`PcmSender::finish()` annonce la fin normale ; Drop sans finish est un abandon
en erreur, pas un EOF réussi. Conduire producteur et collecte en concurrence puis
attendre la vraie terminaison moteur. [Contrat et exemple offline exécutable](research/rust-pcm-channel.md).
Ce helper n'ajoute aucune capture microphone et n'est pas un acquittement d'upload.

`SessionOptions` regroupe la configuration, les délais, un
`Option<Arc<dyn MetadataProvider>>` et `ContextInput::{None, Static, Provider}`.
`timeout` vaut 30 s par défaut dès le démarrage, métadonnées/parole/upload/contexte
compris ; `finalize_timeout: Option<Duration>` vaut `None` par défaut et ne commence
qu'après EOF audio et cleanup réussi. Son échéance ne dépasse jamais le délai dur,
même pour une durée énorme ; `Some(Duration::ZERO)` est invalide. Un provider de
contexte est appelé une seule fois après ce cleanup, sous le minimum de son budget
de contexte et des délais restants. Les credentials sont injectables par session ;
`EnvironmentCredentials` est une implémentation fournie et reste permissive si
les trois variables sont absentes. Pour imposer leur présence, utiliser
`required_snapshot()` ou `required_snapshot_with(...)` ; la CLI normale utilise
un snapshot strict, contrairement au loopback explicite. Ce contrôle ne prouve
pas une authentification distante ni un provisionnement renouvelable ; sa
qualification locale et ses limites sont décrites dans le [contrat des snapshots](research/rust-credential-snapshots.md).
Le commit terminal de ce
budget et ses preuves sont décrits dans le [contrat de fiabilité](research/rust-reliability-lifecycle.md).

Migration du prototype Rust 0.1 : un littéral exhaustif `SessionOptions` doit
ajouter `finalize_timeout: None` ou utiliser `..SessionOptions::default()`.
Les matches exhaustifs doivent aussi couvrir `EngineError::FinalizeTimeout` et
`TransportError::PrimaryAndCleanup`. Le budget absent préserve le défaut de
30 s ; les nouvelles causes restent structurées et expurgées à l'affichage.

`collect_dictation(&engine, source, options, limits)` rend une future dont
l'ownership est établi dès l'appel, même si elle n'est jamais pollée. Elle conserve
par défaut au plus 128 finales et 8 Mio de réponses brutes associées. Pour traiter
les événements, consommer `EngineSession::next()` jusqu'à `None` et appeler
explicitement `close()` en cas d'arrêt anticipé.

`collect_segments` / `collect_segment_session` retiennent aussi les réponses avec
un conteneur d'hypothèses3/5/7, indépendamment du state, ainsi que les marqueurs
state1. Les previews4 seuls ne sont pas retenus. `SegmentResult.updates` préserve
chaque réponse et son ordinal client (incluant les réponses non retenues), sans
concaténation, déduplication ou sélection implicite. Les limites sont partagées
avec le collecteur strict et mesurent les octets wire retenus, pas le RSS.

Les conteneurs et leurs fallbacks sont structurellement décodés ;
`all_hypotheses()` les parcourt dans l'ordre canonique3/5/7, pas un classement ni
un ordre wire global. Les helpers `hypotheses()` et le rendu restent limités au
conteneur primaire. `RecognitionResponse.language` et `spoken_language` sont
les champs3/4, pas des identifiants de résultat. Voir
[schéma et provenance](research/rust-response-containers.md).

### Octets bruts partagés : migration du prototype Rust0.1

Les champs `raw` des modèles décodés et `RecognitionResponse.active_raw` utilisent
maintenant `bytes::Bytes`, pas `Vec<u8>`. Les signatures des fonctions et les types
wire `Field`/`Value` ne changent pas. Utiliser `raw.as_ref()` pour emprunter les
octets, `raw.to_vec()` lorsqu'un `Vec` indépendant est nécessaire, et
`raw: vec![...].into()` dans les constructions manuelles de modèles.

Le décodeur copie une seule fois le payload racine borné, puis partage les vues
enfants ; il ne conserve pas un gros buffer de transport extérieur au message.
Une véritable fusion de plusieurs occurrences garde sa concaténation logique.
Un petit enfant peut toutefois garder toute sa racine (au plus8Mio) : pour une
conservation isolée prolongée, détacher explicitement son `raw` avec
`bytes::Bytes::copy_from_slice(child.raw.as_ref())`. Texte, présence et champs
inconnus ne sont pas supprimés. Le [profil mémoire](research/rust-backend-profiling.md)
mesure −70,82% de rétention logique sur son banc, pas une baisse RSS garantie.

## Finalité, annulation et résultats

- Une fin réussie nécessite END S3, upload entièrement épuisé/half-clos, source
  fermée et statut gRPC OK. Une finale reçue avant cela n'est pas une réussite.
- Une erreur S3 n'interrompt pas le drainage requis pour détecter une erreur gRPC
  tardive. Aucun échec ne déclenche un rejeu automatique d'audio.
- `EngineError::IncompleteSession(Arc<IncompleteSessionDiagnostic>)` expose END S3,
  les compteurs/flags du transport et des métriques finales sans texte/raw/ID,
  relevés après son nettoyage. La CLI les projette dans `error.incomplete` ; ils
  distinguent un END absent d'un upload interrompu, sans prouver la cause serveur.
  Le payload typé remplace la variante unitaire du prototype0.1 initial.
  `source_exhausted` vise le RequestSource entier (init/audio/contexte/END), pas
  directement l'EOF PCM ; `source_closed` signifie cleanup terminé, même précoce.
  `upload_eof_polled` reste une soumission locale END_STREAM, pas un acquittement
  serveur, et `saw_s3_end` seul ne garantit pas le tail audio.
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
  Une erreur audio primaire observée n'acquitte pas un cleanup secondaire distinct :
  `Engine::close()` peut donc encore retourner `AudioCleanup`, puis `Ok` une fois
  les autres erreurs relevées. Le moteur propriétaire préserve aussi le driver.
- Les sources audio fournies gardent une faute typée `Err` dans
  `AudioSource::next()` jusqu'au close explicite ; pas d'EOF/EOS sain fabriqué.
  À l'inverse, `rambler_transport::Session::next()` est intentionnellement fused
  après livraison de son erreur terminale et `finish()` conserve cette issue.
- `TransportError::PrimaryAndCleanup` préserve une primaire et un cleanup distinct,
  avec construction/champs privés et accès explicite aux deux causes, sans fuite
  par formatage/chaîne automatique. Annulation explicite avec cleanup défaillant
  reste `Cleanup`. L'engine défait cette paire seulement pour son adaptateur local
  et conserve `AudioCleanup` une fois, sans second close ; cela ne change pas la
  conversion publique générale. La priorité transport d'une faute source observée
  n'efface pas universellement un délai/une annulation déjà choisi par l'engine.
- Les files sont bornées ; un consommateur suspendu peut retarder la lecture des
  trailers. Les compteurs d'upload décrivent le travail client, pas un accusé de
  réception du serveur. Toutes les attentes fallibles, nettoyage et réservation
  de file compris, précèdent le commit privé irréversible. Annulation et délais
  sont revérifiés juste avant publication : un EOF sain publié ne devient plus
  erreur et `metrics.eof_seconds` date ce commit réussi, pas le dernier échantillon
  PCM. Voir [les régressions et la qualification du lot](research/rust-reliability-lifecycle.md).
- `EngineSession::audio_progress()` et `metrics.audio` comptent les chunks non
  vides et octets encodés réellement rendus à l'engine, l'EOF audio et les flags
  `cleanup_finished`/`cleanup_succeeded`. Ils restent accessibles après une erreur
  sur la session ; attendre la terminaison/fermeture pour un snapshot final.
  Ce n'est ni un nombre d'échantillons PCM ni une preuve d'audio reçu par le serveur.
  L'EOF audio peut être vrai pendant que le contexte tardif bloque encore le
  RequestSource. Les champs sont ajoutés aux structs du prototype0.1.
- `DictationResult::check_safety()` et `SegmentResult::check_safety()` inspectent
  toutes les hypothèses des réponses retenues, conteneurs secondaires/fallbacks
  compris, sans pour autant les sélectionner pour le rendu.
  `text()`, `single_hypothesis()` et `replacement_proposal()` refusent les candidats
  safety1 (`SafetyBlocked`) et le champ30 non interprété (`UnrecognizedSafety`),
  avant l'ambiguïté. La CLI retourne une erreur sans texte ni finales. Les données
  structurées restent explicitement consultables par l'intégrateur. Absence/0
  inchangés ; refus hors VoiceEdit et sur inconnu volontairement conservateurs,
  [portée et preuve statique](research/rust-feature-parity.md).
- Le texte simplifié exige une unique finale/hypothèse interprétable : sans
  VoiceEdit, texte d'hypothèse ; intention 2, texte/contexte édité vérifié ;
  intention 3, champ `edited_text` seulement. Les autres cas restent ambigus.
- Une proposition de remplacement reste limitée à l'intention 2 et ne modifie
  rien. Vérifier snapshot texte/révision avant application ; les positions sont
  en unités UTF-16, pas en octets Rust. Les révisions texte et entier sont des types
  distincts ; les entiers canoniques ne sont pas bornés à 64 bits.

### Rendu et commandes explicitement opt-in

`DictationResult::render(RenderPolicy)` retourne texte, provenance, intention et
raison de fallback éventuel. `Strict` conserve le contrat de `text()` décrit
ci-dessus. `AsrOnly` projette le texte d'hypothèse, jamais le transcript VoiceEdit.
`S3ClientCompatible` rend les intentions2/3, membres3/7 ; sinon il utilise le getter
ASR (absence → chaîne vide). Un edit accepté vide reste vide. Toutes les politiques
exigent unicité et sécurité ; ce helper ne reproduit pas la sélection Android.

`replacement_proposal_with_policy` reste sans effets : en mode compatible,
intention2/champs3 ou7, et intention3/**champ7 seulement**. Intention3/champ3 peut
être rendue mais ne prouve pas la portée d'un remplacement ; `AsrOnly` ne produit
aucune proposition. La portée reste la fenêtre de contexte capturée.

`classify_command(CommandProfile::S3DefaultPatternsV1, ime_action)` reconnaît les
regex par défaut SEND/CLEAR/CLEAR ALL sur un candidat final édité unique et sûr.
CLEAR est reconnu mais rejeté par la branche client ; SEND exige IME action4.
Le résultat expose préconditions/disposition et `applied=false`, sans envoi,
suppression ni insertion système. Ce profil versionné n'interprète pas des regex
configurées à distance. Ce n'est pas une implémentation COMPOSE.

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

## Validation de plusieurs sessions natives

```sh
cargo run --release -p rambler-cli --features audio --example session_reuse -- \
  manifest.json --allow-live --timeout 30
```

Cet exemple de développement exige explicitement `--allow-live` (ou un endpoint
`--loopback`), accepte2 à3 WAV contrôlés, borne manifeste et fichiers, utilise un
seul moteur/connexion explicite et s'arrête au premier échec sans retry. Chaque
cas déclare `input`, `language`, `pcm_sha256`, `markers` ; options
`marker_alternatives`, `expected_text_sha256` et contexte/hints documentées dans
[son source](../crates/rambler-cli/examples/session_reuse.rs). La sortie ne contient
que hashes, compteurs et catégories, pas texte/chemin/credentials. Le manifeste
accepte aussi `block_offensive_words`, `auto_punctuation`, `secondary_routing` ;
les valeurs ne sont pas échoées dans le rapport. Les tests
loopback comptent physiquement une connexion TCP ; ce nombre n'est pas inventé
pour la preuve distante. Le premier lot avec marqueur `eleven` au lieu de `11`
reste [inconclusif](evidence/rust-reuse-initial-marker-failure.json) ; le lot
suivant a déclaré les variantes avant exécution, sans réécrire cet échec.

## Performance mesurée

La construction des requêtes audio utilise une allocation exacte et une seule
copie du payload, après vérification de toutes les bornes. Le
[profil A/B](research/rust-backend-profiling.md) publie96 mesures CPU et40 sessions
loopback, source/binaire hashés et snapshots distingués du code ultérieur.
Gain micro-CPU établi ; **pas de gain global WAN ou pipeline robuste établi**.
Le candidat CRC antérieur a été rejeté, pas conservé sur la seule intuition.

## Périmètre encore distinct

Le moteur prend de l'Ogg/Opus déjà encodé ou du PCM via l'adaptateur natif
optionnel. Les mesures d'endurance Rust sont rapportées séparément des preuves
Python, avec leur modèle de processus/canal ; aucune exécution réelle
multiplateforme n'en est déduite. La composition intention 4 n'a pas été démontrée
sur le profil observé. Le provisionnement et la révocation des métadonnées
applicatives restent des limites externes.
