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
