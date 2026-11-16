# Contrats de fiabilité Rust : admission, erreurs et finalisation

État documentaire au **15 novembre 2026**, lot construit depuis la base
`90a773a`, intégré dans `58da700`. **Windows R3 et Linux R3 qualifient localement
le même snapshot, avec exclusions explicites. Linux R1/R2 restent FAILED.** Les
310 tests du snapshot précédent
qualifient uniquement la base commitée, pas ce lot. Les résultats intégrés et leurs
limites sont séparés dans [la validation des builds](rust-build-validation.md#lot-de-fiabilité-du-15-novembre).

## Admission locale avant consommation

Le superviseur attend successivement la résolution/validation des métadonnées,
`SendRequest::ready()` et le succès local de `send_request` avant d'autoriser le
premier `RequestSource::next()`. L'engine bénéficie de cette barrière avant de
consommer son `AudioSource`. Un refus connu localement ne doit donc pas lire de
nouveaux chunks audio pour rien.

Cette barrière n'est ni un acquittement du pair, ni une authentification acceptée,
ni une preuve de réception des HEADERS ou de l'audio. Une connexion silencieusement
perdue derrière un NAT peut encore paraître prête localement. `start` reste
synchrone : il peut accepter une source dont le **constructeur a déjà travaillé**
(ouverture/prélecture/initialisation), voire dont le producteur indépendant tourne
déjà. La barrière ne remonte pas le temps et n'est pas une promesse de capture
microphone sans activité préalable.

Une fois la source acceptée par `GrpcTransport::start`, le superviseur conduit son
nettoyage **une fois**, même si l'admission échoue avant le premier `next`.
Un rejet synchrone de ce transport brut avant `Ok(Session)` relève du Drop Rust
ordinaire. **`Engine::start` supervise au contraire le cleanup audio même s'il
rejette une configuration invalide ou un moteur fermé** : attendre
`Engine::close().await` aussi après ce rejet. Les sources ne doivent pas bloquer
le runtime ni paniquer ; leur close doit finir coopérativement. Drop seul ne peut
l'attendre.

Aucun retry, rejeu, reconnect automatique, health check, keepalive ou PING de
production n'est ajouté. Les PING de fixtures servent uniquement à ordonner les
tests. Créer explicitement un autre transport ne déplace aucune ancienne session
et n'autorise aucun rejeu de sa source.

## Connexion initiale : course TCP seulement

- Une résolution DNS, sous le délai global de connexion, conserve **toutes** les
  adresses, même les doublons. L'ordre de chaque famille est préservé ; les familles
  alternent en commençant par celle de la première réponse.
- La course TCP est de style Happy Eyeballs et applique à l'**initiation de l'API
  applicative** les délais inspirés du
  [RFC 8305 §5](https://www.rfc-editor.org/rfc/rfc8305.txt), pas l'intégralité du RFC :
  aucune course DNS AAAA/A ni implémentation complète de RFC 6724. La première
  tentative est épinglée et pollée immédiatement, avec le waker réel avant tout
  yield ; l'espacement normal reste **250 ms**. Après échec observé, l'échéance
  suivante devient
  `min(échéance prévue, max(dernier lancement + 10 ms, maintenant))` : elle ne
  reporte jamais un départ programmé. Le dernier lancement est horodaté **après
  son premier poll**, pas à la construction, ni depuis la tentative échouée.
  Le §5 recommande 100 ms comme minimum (`SHOULD`) et 250 ms par défaut ; 10 ms est
  son plancher absolu obligatoire (`MUST NOT` démarrer plus tôt). L'accélération
  sur échec utilise ici ce plancher applicatif, sans minimum permanent de 100 ms.
  **Aucune mesure ni preuve d'espacement des SYN noyau n'est revendiquée**, ni gain
  de performance. Des yields après initiation empêchent une longue série d'échecs
  immédiatement disponibles de monopoliser l'exécuteur et de masquer le délai
  externe. Une adresse tardive n'est pas éliminée par une limite arbitraire aux
  huit premières réponses.
- Au succès, à l'erreur finale ou à l'annulation, les futures/sockets perdantes
  sont abandonnées et fermées avant de passer à TLS. Cette sélection de destination
  appartient à l'établissement initial, pas à un retry de RPC.
- Le délai externe, **10 s par défaut**, couvre DNS, TCP, TLS et handshake h2.
  Son expiration reste `TransportError::Timeout`, pas un `Connect` générique.
  Environ **41 tentatives bloquées** peuvent coexister sous ce seul défaut 10 s /
  250 ms (également limitées par la liste DNS) : ce n'est pas une borne fixe pour
  un timeout choisi arbitrairement par l'appelant. Les tentatives échouées sortent
  de la course.
- TLS démarre uniquement sur le TCP gagnant, conserve le nom d'hôte original pour
  sa vérification et l'autorité, les racines système et ALPN `h2`. Un échec TLS
  termine cette connexion : il ne lance ni ne reprend une course vers une autre
  destination et ne réduit jamais les vérifications TLS.

## Backpressure et équité finie

La fenêtre de réception h2 de connexion est **1 Mio** ; celle de chaque stream
reste **65 535 octets**. Les files applicatives requête/réponse restent de capacité
**un**, avec les éléments en vol distincts. Un consommateur suspendu ne doit pas
épuiser à lui seul l'ancien crédit de connexion et bloquer le petit échange d'un
frère dans le scénario couvert.

Ce crédit fournit une marge d'équité **finie**, pas une isolation pour un nombre
arbitraire N de streams suspendus. Les buffers h2, les allocations racines de
`Bytes`, les sources et le ledger d'erreurs restent distincts. Ni cette fenêtre,
ni les files de capacité un ne constituent une borne RSS globale. Un consommateur
qui ne draine pas peut toujours retarder les trailers de sa propre session.

## Deux contrats terminaux à ne pas confondre

Pour les sources audio fournies (`PcmInput`, `Pcm16Opus`, WAV), une faute source
reste typée et **sticky** : les appels `AudioSource::next()` suivants restent
`Err` jusqu'au close explicite, au lieu de transformer corruption/troncature/
abandon en EOF sain. Aucun EOS Opus réussi n'est fabriqué pour masquer la faute.
La libération de l'upstream est unique et peut déjà avoir commencé ; cela n'efface
pas la faute exposée par `next`. Le close explicite reste une transition séparée,
avec sa propre issue de cleanup.

En revanche, `rambler_transport::Session::next()` est volontairement **fused** :
après avoir livré son erreur terminale une fois, il rend `Ok(None)` aux appels
suivants. `finish()` conserve et rend l'issue terminale, y compris cette erreur.
Cet EOF après erreur n'est donc pas le contrat des `AudioSource`, ni un nouvel
acquittement de réussite. L'intégrateur doit observer l'issue, pas seulement le
dernier `None`.

### Préserver primaire et cleanup

`TransportError::PrimaryAndCleanup(Box<PrimaryAndCleanup>)` préserve la faute
primaire sélectionnée et une faute distincte du close. Le type est public, mais
sa construction et ses champs sont privés ; `primary()`, `cleanup()` et
`into_parts()` permettent une lecture explicite. Debug/Display et la chaîne
d'erreur automatique n'exposent pas les erreurs privées de l'appelant. Une
annulation explicite accompagnée d'un cleanup défaillant reste `Cleanup`, pas une
paire dont le primaire serait `Cancelled`.

L'engine ne défait cette paire que **localement**, pour son propre adaptateur de
requêtes et son audio mémorisé : il restaure son primaire et conserve le cleanup
distinct dans son ledger `AudioCleanup`, sans deuxième close audio ni deuxième
livraison de la même faute de cleanup. Il ne faut pas généraliser cette opération
au `From<TransportError>` public, ni dédupliquer par texte/code d'erreur.

La priorité du transport pour une faute source effectivement observée n'est pas
une règle universelle remplaçant un délai ou une annulation **déjà sélectionné par
l'engine**. Chaque couche conserve son point de sélection et ses responsabilités.
Une erreur de session observée n'acquitte pas non plus le driver indépendant :
voir [le ledger de connexion](rust-connection-lifecycle.md). Des close successifs
peuvent légitimement révéler des fautes distinctes ; les attendre jusqu'à `Ok`.

## Délais et budget de finalisation

`SessionOptions.timeout` reste le délai dur global, **30 s par défaut** à partir
du démarrage de la session. Il **inclut la parole**, les métadonnées, l'admission,
l'upload, le contexte tardif et la livraison terminale ; ce n'est pas « 30 s après
que l'utilisateur a fini de parler ». Le setup de connexion a son délai distinct
ci-dessus. Un cleanup asynchrone est toujours attendu coopérativement, même après
l'expiration : aucun de ces délais ne constitue un abandon forcé du nettoyage.

`SessionOptions.finalize_timeout: Option<Duration>` vaut **`None` par défaut**.
La CLI expose `--finalize-timeout` en secondes. Lorsqu'il est présent, ce budget
ne commence **qu'après EOF audio et cleanup audio réussi**. Il couvre ensuite le
contexte, la fin d'upload/END, le statut gRPC et la livraison finale sous
backpressure. Son échéance effective est le minimum de son échéance propre et de
l'échéance dure : il ne prolonge jamais la session. `Some(Duration::ZERO)` est
invalide ; un budget énorme ou une addition d'instant débordante est plafonné
sans risque par l'échéance dure. Le provider de contexte garde aussi son propre
budget (`context_timeout`, défaut 2 s), limité par le temps restant.

Un dépassement du budget de finalisation doit rester distinct (`FinalizeTimeout`)
du timeout dur (`Transport(Timeout)`). Aucun `max_audio`, faux EOF automatique ou
arrêt de microphone n'est ajouté. Le producteur doit décider de la fin normale.

Exemples de réglage **illustratifs, non mesurés comme optimaux** : pour une source
pouvant durer deux minutes, `--timeout 150 --finalize-timeout 10` laisse au maximum
10 s après EOF/cleanup, sans dépasser 150 s au total ; `--context-timeout 2` garde
son plafond propre. Une parole de plus de 150 s n'obtient aucun sursis. Sans
`--finalize-timeout`, seul le délai dur et les délais spécifiques existants
s'appliquent.

### Publication irréversible du succès

La correction locale place **toutes les attentes fallibles avant un commit privé
irréversible**, nettoyage et réservation de la file d'événements compris. Après
réservation, annulation et échéances sont encore vérifiées avant publication ;
aucune attente fallible ne suit le commit. Un EOF sain publié ne devient donc pas
ultérieurement une erreur ; `metrics.eof_seconds` date le **commit réussi**, pas
le dernier échantillon PCM ni le simple drainage d'une file.

Les régressions de file pleine, expiration après réservation, priorité des délais
et annulation après EOF passent dans Windows R3. Le passage antérieur de 12 cas
n'était pas cette preuve. Ces scénarios finis ne prouvent pas tous les
ordonnancements ; l'échec Linux R2 distinct reste enregistré ci-dessous.

## Historique ciblé, distinct de la qualification intégrée

Les noms ci-dessous identifient des logs locaux ciblés, pas des liens vers une
preuve publiée ni un total workspace. Ils décrivent les snapshots intermédiaires
exécutés, pas nécessairement le dernier état partagé des sources.

- Admission : `admission-evidence-20261115/red-01.log` contient trois assertions
  en échec ; `green-admission-09.log` indique le passage des cinq tests filtrés
  d'admission. `green-transport-10.log`, `green-engine-07.log` et
  `green-cli-08.log` contiennent les résultats ciblés transport/moteur/CLI. Ces
  groupes ne sont pas additionnés en un compteur de tests distincts.
- Erreurs audio sticky : `sticky-errors-8sqitv/red-final.log` contient six échecs
  de régression ; `green-final.log` contient notamment les six cas sticky réussis
  et les groupes audio voisins. Ce n'est pas une preuve de tous les interleavings.
- Course TCP initiale : `happy-eyeballs-wwjpVW/after-test-2.log` et
  `tcp-test-current.log` indiquent dix tests de connexion réussis chacun sur
  l'ancien pacing sans plancher. Le premier correctif (`tcp-pacing-floor/red.log`,
  sortie 101, puis `green-02.log`, 13 réussites, et `clippy-lib.log`, sortie 0)
  conserve sa preuve historique, **mais son oracle mesurait la construction et
  manquait le premier poll différé après yield**. Il ne qualifie pas ce pacing.
  Dans `tcp-first-poll-20261115/red-02.log`, les oracles corrigés échouent au runtime
  (sortie 101, 13 réussites / 6 échecs) : deux initiations à **20 ms / 20 ms** après
  une suspension de 20 ms, cas pending à 270 ms, coût de premier poll et sélection
  Ready/cooperation. Après correction, `green-01.log` passe **19 tests** (sortie 0 :
  16 avec horloge injectée sans I/O, ordre des familles, driver, socket loopback).
  `clippy-lib.log` et `fmt-check.log` passent aussi (sortie 0). Le parcours des
  **129** échecs conserve **129** drops en **1 280 ms virtuelles**, la neuvième
  adresse gagne à 2 s ; les cas post-poll vérifient 10 ms après ReadyErr et 250 ms
  après Pending. Sources RED/GREEN exactes, hashes et invariants sont conservés ;
  voir [la preuve ciblée de pacing](rust-connection-lifecycle.md#correction-ciblée-du-pacing-tcp--15-novembre-2026).
  Ce sont des oracles d'initiation applicative, **pas une preuve de pacing SYN/RFC**,
  ni une qualification intégrée ou un banc WAN.
- Fenêtre h2 : `connection-window-zEaYDA/desired-red-1.log` échoue sur le frère
  bloqué ; `happy-eyeballs-wwjpVW/window-green-1.log` passe ce cas ciblé. Cela ne
  généralise pas l'équité à N streams quelconques.
- `happy-eyeballs-wwjpVW/tcp-test-final.log` est un **échec transitoire de
  compilation** (`E0061`, signature partagée lib/wire en cours d'édition), pas
  un RED runtime ni une validation finale. Ne pas le transformer en assertion
  réseau échouée ou en relance verte présumée.

Les sorties de tests ont été lues ; les logs historiques n'ont pas tous un code
de sortie enregistré indépendamment. La correction de pacing ci-dessus a, elle,
relancé uniquement les tests de connexion et le Clippy bibliothèque, avec codes
de sortie conservés. Aucun journal privé n'est reproduit. Windows R2 et Linux R1
restent des preuves historiques sur leurs snapshots immuables ; Linux R1 reste
**FAILED**, sans réécriture après correction.

## Qualification intégrée

Windows R3 qualifie le snapshot incluant le premier poll TCP corrigé et le commit
terminal. Linux R2 utilise le même manifeste sélectionné de 167 fichiers, inchangé
avant/après. Ses tests workspace, exemple et feature-off passent, mais le test
`release-finalization-cli` échoue (sortie 101) à `peer upload was not complete`.
Le diagnostic contre-vérifié identifie un binaire release périmé : il rejette
`--finalize-timeout` avant upload, car le nettoyage par paquet ne couvrait que le
profil dev. Le test identique passe avec le debug reconstruit ; ce n'est pas une
réussite release. **Linux R2 reste non qualifié.** Ses phases release PCM/smoke
suivantes n'ont pas été exécutées. Linux R3, après nettoyage des deux profils,
passe ensuite le même workspace, le test du release reconstruit, le smoke PCM et
le postflight source, sans changement du moteur. Son test PyAV reste ignoré ;
les autres exclusions de plateforme sont conservées.

Les compteurs, rapports locaux exacts et preuves publiques déjà disponibles sont
référencés dans [la validation des builds](rust-build-validation.md#lot-de-fiabilité-du-15-novembre).
La qualification locale du snapshot est publiée avec ses exclusions ; les échecs
historiques ne sont pas réétiquetés.
Une réussite locale ne vaut ni CI distante ni qualification téléphone/service/WAN.

La revue architecturale Opus réelle du 15 novembre est **statique**, sans tests
exécutés par le reviewer ; sa provenance est séparée des demandes historiques
mal routées dans [la validation des builds](rust-build-validation.md).
Les micro-optimisations CPU oneof/OggScratch/mutex ont été écartées comme peu
prioritaires et non mesurées face à la fiabilité ; aucun gain vitesse/WAN n'est
revendiqué. Android/ART, authentification/provisionnement/révocation, COMPOSE et
macOS restent des périmètres distincts non qualifiés par ce lot offline. Aucun
APK propriétaire, détail de capture privée ou accès réel n'est nécessaire ici.
