# Entrée PCM native bornée et arrêt normal explicite

## Rôle

`rambler_audio::pcm_channel()` fournit un couple `(PcmSender, PcmInput)` original,
sans microphone, Android, fichier, credential ou connexion. L'intégrateur produit
son PCM16LE mono16kHz et compose `Pcm16Opus::new(input, config)` avant de confier
cette source **encodée Ogg/Opus** au moteur existant. Ce n'est pas un nouveau
backend ASR, ni une activation d'intention Rambler supplémentaire.

L'arrêt normal était déjà possible en terminant une `AudioSource` personnalisée.
Ce helper rend ce contrat disponible sans réimplémenter une file et son cycle de
vie dans chaque intégration. Aucun changement de profil codec, bitrate, qualité,
page par défaut, transport, contexte, résultat ou politique safety.

## Producteur et capacité

Le producteur est unique et non clonable ; chaque canal possède son état et sa
file indépendants. Une seule entrée peut être en file. Chaque chunk est borné à
`MAX_INPUT_BYTES` (8 Mio de **longueur logique**), avec fragments vides ou impairs
acceptés. L'encodeur contrôle l'intégrité du dernier échantillon à l'EOF.

```rust,ignore
let owned_pcm: bytes::Bytes = /* PCM obtenu par l'intégrateur */;
// `owned_pcm` reste au producteur si cette attente est annulée.
let permit = writer.reserve().await?;
// Pas d'await entre la remise du chunk et sa soumission.
permit.send(owned_pcm)?;
```

La remise du chunk est synchrone après réservation. Abandonner un permis libère
sa place. Un rejet de chunk trop grand ou de source déjà fermée conserve les
bytes dans `PcmSendError::into_chunk()` ; `kind()` donne une raison fixe.
`Debug`, `Display` et la chaîne d'erreur n'exposent pas le payload.
Aucun retry, rejeu, rééchantillonnage ni pacing automatique.

La file ne borne pas la mémoire détenue par le producteur, le chunk déjà pris par
l'encodeur ou une grande allocation racine retenue par une tranche `Bytes`.
Ce n'est pas une borne RSS globale de 8 Mio. Producteur et consommateur doivent
être conduits **en concurrence** : remplir plusieurs chunks avant de démarrer le
consommateur attendrait légitimement sur la capacité.

## Observation PCM optionnelle — qualification locale

`Pcm16Opus::new_with_progress(source, config)` retourne l'encodeur et un
`PcmProgressHandle` ; `handle.snapshot()` donne un `PcmProgress` cohérent et
monotone, dont les compteurs restent disponibles après fermeture :

- `input_bytes: u64` compte les octets des chunks acceptés en amont, y compris
  ceux encore tamponnés sans encodage lors d'une fermeture précoce ; les chunks
  surdimensionnés rejetés ne comptent pas.
- `encoded_input_samples: u64` compte les vrais échantillons PCM des trames
  encodées et comptabilisées, sans padding nul ni flush de seul lookahead.
- `encoded_frames: u64` compte les trames codec, padding et flush compris.
- `input_eof: bool` indique seulement un `None` naturel de la source amont.
- `output_eof: bool` indique l'EOF encodé naturel après cleanup amont et drainage
  du mux, jamais un simple close explicite, une erreur ou Drop.

```rust,ignore
let (encoder, progress) = Pcm16Opus::new_with_progress(input, config);
// Conduire encoder puis attendre sa terminaison ou son nettoyage.
let snapshot = progress.snapshot();
```

Le constructeur `Pcm16Opus::new` reste sans allocation ni verrou d'observateur ;
aucun coût CPU nul ou performance optimale n'est revendiqué. Ces mesures locales
ne sont ni des hashes PCM, ni des acquittements serveur, ni des flags terminaux
de requête, ni une preuve contre toute perte ou rejeu. Les métriques encodées du
moteur restent distinctes. La [qualification locale A/B](rust-build-validation.md#pcm-progress-and-strict-admission--local-checkpoint-2026-11-15)
et sa [preuve](../evidence/rust-pcm-progress-admission.json) distinguent les
suites complètes et la reprise ciblée après échec Clippy.

## Fin normale, abandon et annulation

- `writer.finish()` consomme le producteur et annonce un EOF volontaire. La source
  draine les chunks déjà acceptés avant `None`. L'encodeur ferme ensuite son
  upstream et flush le tail/lookahead jusqu'à son véritable EOS.
- Détruire le producteur **sans `finish()`** draine les chunks acceptés puis
  renvoie `AudioError::InputAbandoned`, même pour un canal vide. L'erreur reste
  latched jusqu'au close explicite ; l'encodeur ne fabrique aucun EOS réussi.
- `PcmInput::close()` et Drop ferment immédiatement la source, jettent la file
  intentionnellement et réveillent les réservations. La transition a lieu même
  si le futur retourné par close n'est jamais pollé. Un permis tenu avant close
  rejette une soumission qui observe ensuite la fermeture.
- Une soumission concurrente avec close peut être acceptée puis jetée par cette
  annulation. **Acceptation et `finish()` ne sont pas des acquittements de
  consommation, d'upload ou de dictée réussie.** Il faut toujours attendre les
  conditions finales existantes : audio/cleanup, END S3, upload complet et gRPC OK.

`AudioError::InputAbandoned` est une variante publique additive du prototype Rust
0.1 ; les intégrateurs ayant un match exhaustif doivent l'ajouter. Aucun nouveau
champ wire ni nouvelle dépendance.

## Exemple offline réellement exécutable

```sh
cargo run -p rambler-audio --example pcm_channel --locked --offline
```

L'exemple produit 1017 échantillons synthétiques en fragments de 17 octets, conduit
producteur/encodeur avec `tokio::join!`, puis attend le nettoyage. Un lecteur Ogg
indépendant valide framing/CRC, le décodeur libopus couvre le granule final et le
pre-skip : **3051 échantillons à 48kHz après trim**, un EOS. Aucun contenu audio
n'est affiché, aucun appareil ou service n'est contacté. L'Opus reste lossy : ce
contrôle de durée ne prétend pas reproduire le PCM d'entrée à l'identique.

Ce smoke est un usage du package public, pas une dictée distante. La fidélité
avant/après fragmentation est aussi testée par égalité des paquets Opus avec la
source mémoire existante, pages20/1000ms et durée décodée indépendante.

## Validation et limites

Les 19 nouveaux tests sont répartis entre contrat de canal et composition avec
l'encodeur : capacité, réveils vérifiés sans sleeps, réservations annulées, close
non pollé, limites/restitution, isolation, tail, abandon, troncature et EOF vide.
La revue indépendante Sol de la machine d'états est statique, pas une preuve
formelle, une revue Opus ou une exploration exhaustive des ordonnancements.

[Preuve des builds/tests](../evidence/rust-pcm-channel.json). L'échec initial du
check sans `cmake` sur PATH reste conservé ; la validation utilise le CMake local
existant par variable de processus explicite, sans installation/réglage global.
Pas de nouveau gain de latence, d'endurance ou de qualité distante revendiqué.
La parité Android et les limites d'authentification restent distinctes.

## Qualification jusqu'au moteur et au pair HTTP/2

Les tests CLI optionnels audio composent le vrai canal PCM, `Pcm16Opus`, `Engine`
et `GrpcTransport` face à un serveur tonic loopback. Les helpers existants sont
réutilisés depuis les tests engine/audio ; aucune dépendance ni implémentation
moteur de substitution n'est ajoutée. Les réponses S3 du pair sont synthétiques,
pas des reconnaissances du véritable service.

Les cinq tests se complètent sans réattribuer leurs assertions à chaque cas :

- `pcm_channel_engine.rs` (2 tests) vérifie les octets Ogg effectivement reçus,
  CRC/framing par lecteur Ogg indépendant, durée décodée par libopus avec trim,
  égalité des paquets Opus avec la source mémoire et ordre init/audio/contexte/END.
  Le cas fragmenté utilise1607 échantillons et pages20/1000ms ; le cas contexte
  tardif utilise967 échantillons. L'EOF audio/nettoyage précède l'appel au provider,
  mais pas l'EOF du RequestSource complet. END S3 ne suffit pas pendant l'attente
  du statut gRPC ; une erreur terminale PermissionDenied reste une erreur.
- `pcm_engine_failure.rs` (3 tests) vérifie abandon typé du producteur, réservation
  réellement Pending sur file pleine puis réveillée par annulation moteur sans
  transfert des bytes de l'appelant, et END/OK précoce avec producteur toujours
  ouvert classé `IncompleteSession`. Cleanup et suppression du contexte d'échec
  sont contrôlés. **Le cas d'abandon n'établit que l'arrivée d'un payload audio
  commençant par OggS : une page OpusHead suffit, sans preuve d'arrivée d'un paquet
  d'échantillons, de CRC ou de durée décodée dans ce cas.**

### Matrice de la base commitée, publiée le15 novembre2026

Cette qualification appartient à la base intégrée `90a773a`, **pas au lot de
fiabilité local non commité**. Ses compteurs et hashes sont historiques ; aucune
relance finale de ce nouveau lot n'est déduite des résultats ci-dessous.

Les exécutions restent datées du **14 novembre2026**, pas du jour de publication.
Sur chacun de Windows stable, Windows/MSVC1.88 et Linux/WSL1.88 : **310 tests
workspace passent,0 échoué,1 ignoré**. Ce sont305 tests de base plus les cinq tests
ci-dessus, pas930 tests distincts ni cinq dictées distantes. L'exemple
`session_reuse` passe10 tests et la CLI sans audio30, sur les deux OS. Windows
valide aussi Clippy strict, formatage et release CLI avec/sans audio ; Linux
valide release CLI audio. Build de l'exemple PCM et smoke release passent sur les
deux OS :1017 échantillons d'entrée,3051 après trim à48kHz,1 EOS,0 appel service.
Aucun nouveau comportement production ni dépendance.

Lors de cette publication, les96 entrées des manifestes Windows/Linux étaient
strictement identiques et leurs hashes correspondaient au snapshot qualifié.
La preuve publie le digest canonique du manifeste complet et les hashes
sélectionnés des **deux fichiers CLI réels**, des helpers réutilisés et des entrées
bibliothèque pertinentes. Hashes des rapports, logs et sections Linux avaient été
revérifiés sans relance des builds/tests/binaires. Les fichiers modifiés depuis
nécessitent de nouveaux hashes après la validation finale, pas un report implicite
de cette ancienne correspondance.

Les deux contre-revues statiques finales n'identifient aucun défaut concret
bloquant : réservations bloquées/OK précoce absents du fichier normal sont couverts
par le fichier d'échec ; codec/ordre sains absents du fichier d'échec le sont par
le fichier normal. La limite en-tête seul de l'abandon demeure. Une première revue
était bloquée par la cible inexistante `pcm_engine_finish.rs`, puis corrigée par
reprise du workflow : ni échec production, ni succès initial de revue. L'auteur
avait nommé le fichier d'échec `pcm_input_failures.rs` avant son renommage en
`pcm_engine_failure.rs` ; les logs ciblés et les trois logs workspace exécutent
les **noms finaux**, sans inférer leur exécution des anciennes commandes auteur.
Ces revues sont statiques, sans attribution certifiée au modèle Opus ni preuve
formelle d'exhaustivité.

[Preuve d'intégration séparée](../evidence/rust-pcm-engine-loopback.json).
Elle ne réattribue pas les anciens tests de canal/encodeur à une dictée distante,
ne transforme pas un pair simulé en qualification Google et n'établit aucune
nouvelle parité Android, intention, disponibilité d'accès ou performance WAN.
macOS reste non exécuté ; ni les comptes de tests ni les revues ne prouvent un
moteur parfait ou la parité Gboard complète.

## Contrat de fiabilité local — qualification distincte de la base commitée

Les travaux du15 novembre renforcent la frontière source/transport sans changer
la signification de `writer.finish()` : il annonce l'EOF du producteur, pas une
dictée acquittée. Pour les sources audio fournies, un abandon, une troncature ou
une erreur de lecture reste `Err` aux appels `AudioSource::next()` suivants jusqu'au
close explicite ; l'encodeur ne fabrique pas d'EOS sain après une telle erreur.
La libération unique de l'upstream et la persistance de la faute sont deux états
distincts. Le close conserve sa propre issue de cleanup.

Ne pas confondre cette persistance avec `rambler_transport::Session::next()` :
celui-ci est volontairement fused après sa première erreur terminale, et son
`finish()` conserve l'issue. Une paire `TransportError::PrimaryAndCleanup` peut
préserver les deux causes ; l'engine restaure localement la primaire et conserve
le cleanup distinct une fois dans son ledger `AudioCleanup`. Annulation explicite
avec cleanup défaillant reste `Cleanup` au transport.

Le premier `source.next()` attend désormais métadonnées, h2 ready et succès local
de `send_request`. Cette admission ne prouve ni réception/authentification du pair,
ni détection d'une perte silencieuse derrière NAT. Un constructeur de source ou un
producteur indépendant peut déjà avoir travaillé avant le `start` synchrone.
Une source acceptée est nettoyée une fois même sans premier chunk consommé.
Un rejet synchrone du transport brut avant `Ok(Session)` ne fait qu'un Drop
ordinaire ; `Engine::start` supervise le cleanup audio même après rejet pour
configuration invalide ou moteur fermé. Attendre alors `Engine::close().await`.

`SessionOptions.finalize_timeout: Option<Duration>` (défaut `None`, CLI
`--finalize-timeout`) fournit un budget qui commence seulement après EOF **et cleanup
audio réussi**. Il borne contexte, fin d'upload et statut/livraison sous le plafond
du délai dur de 30 s, qui **inclut la parole**. `Some(Duration::ZERO)` est refusé
et un budget énorme est plafonné au dur ; aucun faux EOF, `max_audio` ou arrêt
automatique de la source
n'est ajouté. Les nettoyages restent attendus coopérativement au-delà des délais.

Toutes les attentes fallibles précèdent désormais le commit terminal privé
irréversible : un EOF sain publié ne devient plus erreur et `eof_seconds` date
ce commit réussi, pas le dernier échantillon. Les régressions dédiées et l'état
Windows/Linux sont séparés des anciens 12 scénarios dans le
[contrat de fiabilité](rust-reliability-lifecycle.md) ; les 310 tests historiques
ci-dessus ne qualifient pas ce lot. Aucun rejeu/reconnect automatique, keepalive ou
gain de performance n'est revendiqué, ni nouvelle qualification Android/ART,
authentification, COMPOSE, macOS ou service réel.
