# Rust shared-connection lifecycle and error integrity

Base historique : `e427fb1`. Les reproductions et résultats chiffrés ci-dessous
concernent ce lot antérieur ; ils ne qualifient pas les modifications locales après
`90a773a`. Le lot initial transport a été corrigé après des contre-exemples HTTP/2
bruts indépendants, sans replay/reconnect automatique, Android, service externe,
credentials ni lecture de fichiers d'environnement. Le contrat actualisé est
résumé ci-dessous et dans [fiabilité et finalisation](rust-reliability-lifecycle.md),
qui sépare les contrats, les résultats intégrés et leurs limites.

## Reproductions and corrected claims

### 1. A reset reason is not a typed connection cause

The peer sends `SETTINGS_INITIAL_WINDOW_SIZE=0`, waits for request HEADERS, then sends `GOAWAY(last_stream_id=0, PROTOCOL_ERROR)` without independently closing TCP. In h2 0.4.20, upload `poll_capacity` returns `None` and `poll_reset` can return `Ok(PROTOCOL_ERROR)`. Converting that bare reason into `h2::Error` does **not** restore `is_go_away()`. The imported lot returned `Grpc(Internal)` from `finish`, then `Connect` from transport `close`.

The original probe reproduced that result here. A second, private wire regression checks the actual cause, not just the public gRPC code: before receive-side recovery it failed with `typed receive GOAWAY was replaced by a bare reason or other outcome`. It now verifies a genuine remote, typed GOAWAY reaches the selected wire result before redaction.

Wire results distinguish `H2(h2::Error)`, `ResetReason(h2::Reason)` and local transport errors. On an upload failure with lost origin, the reader is polled once before being dropped. An immediately available typed receive error replaces the lossy upload cause; remote OK or an unrelated application/framing result does not erase the known upload failure. An already precise IO/GOAWAY/RST writer error keeps its precedence.

**The recovery poll never awaits a blocked reader.** If the bounded response queue is full, the bare reason remains honestly unclassified and the independent driver error remains reportable. There is no extra DATA submission, unbounded response buffering, retry or replay. A regression holds the second response send blocked until source cleanup proves the RPC already selected its outcome, before allowing any response drain.

### 2. Non-OK GOAWAY does not mean every active stream has ended

The raw peer keeps stream 1 active, sends `GOAWAY(last=1, ENHANCE_YOUR_CALM)`, then PING. PONG proves the client processed that GOAWAY. A newly accepted source is refused and its observed session result is `ResourceExhausted`. Only then the peer sends `GOAWAY(last=0, PROTOCOL_ERROR)`, killing stream 1; that source's asynchronous cleanup fails.

The imported global connection-observed boolean acknowledged the later driver failure when the earlier refusal was observed. The probe returned `Cleanup` for stream 1, then **both** transport closes returned OK. The assertive regression failed at `first observed GOAWAY must not hide a later driver failure (same_reason=false)`. Excluding only `NO_ERROR` is insufficient: any GOAWAY can leave streams at or below its cutoff alive.

The regression also repeats the scenario with the **same** `ENHANCE_YOUR_CALM` reason for both GOAWAYs. Matching reason or gRPC code is not matching error identity. Both variants gate cleanup, interrupt an in-progress transport close, observe only the cleanup failure, then require `Connect` from the next close and OK from the following close.

## Conservative driver ledger: no false cross-layer acknowledgment

The public h2 0.4.20 API exposes `reason`, IO/GOAWAY/RST kind and initiator, but no error identity, GOAWAY generation/cutoff accessor, or equality linking a stream error to the terminal driver error. Stream and driver errors are separately constructed values. A locally assigned generation cannot correlate them reliably without changes inside h2. Two GOAWAYs can even have identical reasons.

Therefore **a session result never acknowledges the independent driver result**. There is no sticky connection-failure flag, no reason/code comparison and no inference from `SendRequest::ready`. The driver failure gets its own existing ledger entry/observation marker. It is persisted before subsequent cleanup awaits and acknowledged only when a close actually returns that entry, after its last await. Concurrent closes use the existing CAS observation step; interrupted closes retain ownership and outstanding errors. Session final failures retain their own markers, including after reaping.

This intentionally permits a conservative duplicate: session `Grpc(Internal)` followed by driver `Connect`, even when they may share a cause. **The original goal of safely deduplicating every session/driver connection error is not achieved through the public h2 API.** Keeping the driver error is preferable to losing an independent later failure. Both the method documentation and tests explicitly adopt this contract, rather than presenting the duplicate as eliminated.

La précédence source/cleanup décrite par le lot historique ci-dessous a évolué :
le contrat local du 15 novembre préserve désormais une faute primaire et le cleanup
distinct dans `TransportError::PrimaryAndCleanup`, au lieu d'effacer la primaire.
Une annulation explicite avec cleanup défaillant reste `Cleanup`. Le type public
possède des champs et un constructeur privés, avec accès explicite `primary()`,
`cleanup()` et `into_parts()` ; son formatage et la chaîne d'erreur automatique
restent expurgés. L'engine ne défait la paire que pour son adaptateur local et
conserve le secondaire `AudioCleanup` une fois, sans second close audio. La priorité
transport d'une faute source observée ne remplace pas universellement un délai ou
une annulation déjà sélectionné par l'engine. Aucune de ces issues n'acquitte la
faute indépendante du driver. Les causes h2 restent attachées à l'erreur wire
sélectionnée jusqu'à l'expurgation après cleanup, sans exposer les détails du pair.

The driver also polls its connection result before honoring local shutdown cancellation. An already-ready terminal error must not become successful cancellation; a unit test checks that race and that genuinely pending IO still stops. This is not a promise to detect errors that arrive only after local cancellation has discarded the connection.

## Reconnection and cleanup contract

There is no automatic reconnect, retry or replay. Explicit `GrpcTransport::connect(config.clone())` creates an independent transport for **new** sources only. Existing sessions/clones are not moved or rebound. An explicitly closed transport remains closed. `GrpcTransport::start` is not a health check: an accepted source on a dead channel belongs to its supervisor and receives its one asynchronous close attempt. Its synchronous rejection before `Ok(Session)` uses ordinary Rust Drop. In contrast, `Engine::start` supervises audio cleanup even on invalid options or a closed engine; await `Engine::close()` after rejection too.

Observe old sessions, then explicitly close the old transport. Drain outstanding errors by repeating close until OK; a driver `Connect` after a session gRPC failure is possible and intentional. Source `close` is cooperative, called once, and must eventually finish. Deadlines bound RPC work, not asynchronous cleanup. Final transport Drop cannot await cleanup: keep the runtime alive and await close.

## Admission et setup locaux — 15 novembre 2026

`start` n'est pas un health check. Après acceptation de la source par son
superviseur, aucun `source.next()` n'est appelé avant métadonnées valides,
`SendRequest::ready()` puis succès local de `send_request`. Ce n'est ni
acceptation du pair/authentification, ni détection d'un NAT silencieux. Un
constructeur de source peut déjà avoir ouvert/lu/initialisé avant cet appel
synchrone. Le superviseur nettoie une fois la source acceptée même si la barrière
échoue. Un rejet synchrone du transport brut avant `Ok(Session)` reste du Drop
Rust ordinaire ; `Engine::start` supervise le cleanup audio même après rejet de
configuration ou moteur fermé, à attendre via `Engine::close().await`.

Le setup effectue une résolution DNS et utilise toutes les adresses, familles
alternées dans leur ordre interne, sans supprimer les doublons. La course **TCP
seule**, de style Happy Eyeballs, conserve l'espacement normal de **250 ms**.
Après échec observé, l'échéance suivante devient
`min(échéance prévue, max(dernier lancement + 10 ms, maintenant))` : accélération
bornée à 10 ms depuis le **dernier lancement**, jamais report d'un départ programmé.
La course applique ces délais au **démarrage de l'API applicative**, inspirés du
[RFC 8305 §5](https://www.rfc-editor.org/rfc/rfc8305.txt) : 250 ms par défaut,
minimum recommandé de 100 ms (`SHOULD`), plancher absolu de 10 ms (`MUST NOT`).
Chaque future est épinglée et pollée une fois avec le waker réel, avant tout yield ;
le dernier lancement est horodaté **après ce premier poll**, et non à la construction.
L'accélération sur échec n'applique pas un minimum permanent de 100 ms. Cela ne
mesure **pas l'émission des SYN par le noyau**, ni ne prouve leur espacement RFC.
Ce n'est **pas une conformité RFC 8305 complète** : ni course DNS AAAA/A ni
implémentation complète de RFC 6724. Aucun gain de performance n'est revendiqué.
La course yield pour que le délai externe reste observable et ferme les perdants
avant TLS. Le défaut 10 s couvre DNS/TCP/TLS/h2 et son expiration reste `Timeout`.
Environ 41 tentatives bloquées sous ce défaut ne constituent pas une borne pour un
timeout utilisateur arbitraire. TLS garde le nom d'origine, les racines système
et ALPN `h2` ; son échec ne relance jamais une course vers un autre serveur.

La réception h2 dispose d'une fenêtre de connexion de 1 Mio, de 65 535 octets par
stream et de files applicatives de capacité un. Cela fournit une marge finie pour
un frère d'un consommateur suspendu, ni isolation pour N quelconque ni borne RSS
globale. Les buffers natifs et le ledger d'erreurs restent séparés.

`Session::next()` livre l'erreur terminale une fois puis est fused (`Ok(None)`),
alors que `finish()` conserve son issue. Ne pas copier cette règle sur
`AudioSource::next()` : les sources audio fournies gardent leurs erreurs sticky
jusqu'au close explicite. Le commit EOF irréversible et l'état de qualification
sont détaillés dans le [contrat de fiabilité](rust-reliability-lifecycle.md).
Aucun keepalive, PING de production, retry ou reconnect n'est ajouté.

## Correction ciblée du pacing TCP — 15 novembre 2026

L'ancienne accélération sans délai après échec et le premier correctif à 10 ms
sont deux étapes distinctes. `tcp-pacing-floor/red.log` (sortie 101) reproduit
l'absence de plancher ; `green-02.log` (13 tests) et `clippy-lib.log` (sortie 0)
qualifiaient le source `af8595a997f963fbea8c9b34c787ade10f2e8dc7f65fb78a8f8433215c3f1cae`.
**Ces 13 tests n'établissent pas le bon espacement des premiers polls** : leur
oracle enregistrait la construction, avant que `FuturesUnordered` ne pollât la
future, après un yield. Une suspension de 20 ms pouvait consommer tout le plancher.
Les anciennes sources/logs restent historiques, sans réécriture.

La correction du premier poll dispose de preuves séparées, dans le nouveau dossier
temporaire `tcp-first-poll-20261115` :

- **RED runtime**, `red-02.log`, sortie **101** : **13 réussites, 6 échecs**, sans
  erreur de compilation. Les oracles enregistrent désormais le **premier poll**.
  Le parent est pollé jusqu'à sa première suspension, puis l'horloge injectée
  avance de 20 ms ; sans autre avance, les deux départs de l'ancien code sont
  enregistrés à **20 ms / 20 ms** (écart nul). Le cas pending suspendu jusqu'à
  270 ms échoue aussi ; deux cas font consommer 20 ms par le premier poll lui-même
  et exigent ensuite 10 ms après ReadyErr, ou 250 ms après Pending. Les deux autres
  échecs concernent le succès Ready immédiat et le yield après initiation.
  `red-final-connection.rs` conserve exactement le source exécuté. `red-01.log`
  conserve l'étape antérieure (12 réussites / 5 échecs), dont l'ancien fixture de
  coopération attendait encore un lancement compté à la construction.
- **GREEN ciblé**, `green-01.log`, sortie **0** :
  `cargo test --locked --offline -p rambler-transport --lib connection::tests::`,
  **19 tests réussis** : 16 avec horloge injectée sans I/O, un d'ordre des familles,
  un de sélection du driver et un de socket loopback réel. Ils couvrent premier
  poll avant yield, horodatage post-poll, ReadyOk immédiat, ReadyErr à 10 ms,
  Pending à 250 ms, accélération à 200 ms, échec ancien et échéance non reportée,
  neuvième adresse à **2 s**, **129** échecs avec **129** drops en **1 280 ms
  virtuelles**, coopération/timeout et Drop explicite. Le test loopback vérifie
  l'EOF réel du socket perdant avant retour, **pas le pacing des paquets**.
- `clippy-lib.log` et `fmt-check.log`, sorties **0** :
  `cargo clippy --locked --offline -p rambler-transport --lib -- -D warnings` et
  `cargo fmt -p rambler-transport -- --check`. Clippy vise la bibliothèque seulement.
- Rust/Cargo **1.99.1**, Windows ; debug dev/test et incremental désactivés,
  target racine existant, CMake local existant. `invariants.log` confirme que, hors
  des trois documents ciblés, tous les autres fichiers, notamment production/
  manifests/lock, restent inchangés et que le source est identique à
  `green-connection.rs` après tests/Clippy/format :
  SHA-256 `5cc8ad1534668904bf05523afb92e629a9086d0f0d130e435e0065c1e6614565`.

Il s'agit d'oracles bornés d'**initiation applicative**, ni d'une mesure des SYN,
ni d'un banc de performance. Windows R3 et Linux R3 intègrent ensuite ce correctif
dans le même snapshot qualifié localement, avec exclusions explicites. Linux R2
reste en échec sur son test CLI release distinct : le diagnostic identifie un
exécutable périmé, sans cause déduite du pacing. Windows R2 et Linux R1/R2 restent
historiques et intacts. Voir [les résultats intégrés et leurs limites](rust-build-validation.md#lot-de-fiabilité-du-15-novembre).

## Setup TLS hors du runtime — qualification locale du 15 novembre

Après le gagnant TCP et `TCP_NODELAY`, un unique `spawn_blocking` attendu charge
les racines natives, les parse et construit `ClientConfig`. Pas de cache : chaque
connexion recharge ses racines. Le socket reste dans la future appelante ; le délai
externe DNS/TCP/TLS/h2 peut donc progresser sur un runtime mono-thread et ferme le
socket à l'expiration. Nom vérifié, authority, ALPN `h2`, ordre et erreurs ordinaires
restent inchangés (zéro racine acceptée : `Connect`, succès partiel conservé).
Un échec de jointure/panic devient `Supervisor` sous le profil unwind testé ; seul
le texte de l'erreur retournée est expurgé. Le panic hook de l'application peut
encore écrire sur stderr.

Le guard Drop appelle `abort` : un travail encore en file peut être annulé ; un
travail **déjà démarré ne peut pas être interrompu**. Son résultat est alors jeté,
sans démarrer TLS ni accéder à une source. L'appelant doit borner sa concurrence de
connexions : aucun sémaphore global n'est ajouté, et le pool borne ses threads,
pas sa file. Des loaders OS bloqués peuvent aussi retarder le DNS qui partage ce
pool. L'arrêt du runtime peut attendre les travaux bloquants déjà démarrés.
Aucun fallback TLS, racine personnalisée, 0-RTT, PING ou reconnect n'est ajouté.

Qualification Windows ciblée, offline/locked : **25 tests de connexion** (les 19
existants + 6 nouveaux), format et Clippy bibliothèque. Les loaders injectés et
un DER synthétique ne lisent aucun magasin OS. Heartbeat mono-thread, timeout,
annulation en file (pool d'un thread), résultat démarré jeté, panic et erreurs du
provider sont couverts ; chaque attente bloquante a un watchdog OS indépendant.
Deux contrôles négatifs ciblés échouent comme attendu (sortie 101) : sans `abort`,
le loader en file est appelé ; inline, le watchdog libère après 5 s, sans hang.
L'EOF loopback à l'expiration teste une portée de possession analogue à `connect`,
pas un handshake TLS réel ni le magasin système, la rotation ou un service externe.
La revue statique Opus 5.5 accepte ce source. Les qualifications intégrées R4 passent
ensuite sur Windows stable/1.88 et Linux 1.88 : **382 réussis, 0 échoué, 1 ignoré**
par workspace, avec le même manifeste sélectionné de 167 fichiers. Le test PyAV
ignoré est activé séparément sous Windows seulement ; les autres exclusions Linux
restent explicites. Les résultats R3 restent ceux du snapshot précédent. Voir
[la preuve TLS R4](../evidence/rust-tls-setup-offload.json) et
[le détail des builds](rust-build-validation.md#setup-tls-déporté--r4).

## Offline coverage and limits (lot historique)

The 13 lifecycle tests cover invalid initial SETTINGS, active/idle TCP loss, typed and flow-blocked GOAWAY cases, RST CANCEL mapping, sibling isolation, graceful GOAWAY with an active stream, distinct/same-reason successive non-OK GOAWAYs, queue-full responses, interrupted close, cleanup precedence, explicit replacement and no replay. PING/PONG, oneshots, semaphores and counters determine event order; sleeps do not choose failure timing. TCP_NODELAY is explicit in the lifecycle peer fixtures.

The inherited 512-session reuse test checks exact payloads, one call per source, single source close/drop, isolated refused streams and one accepted TCP. It is a correctness check, not a latency benchmark. Private unit regressions additionally check the real receive GOAWAY type and shutdown-vs-ready-error selection. Existing transport suites cover framing/status mapping, source/provider failures, cancellations, response/upload backpressure, observed/unobserved outcomes and concurrent/interrupted closes.

Passing these schedules, even repeatedly, is not proof that all interleavings are bug-free. There is no multi-hour WAN, real-service, TLS-rotation, proxy or idle-health guarantee. No idle keepalive is added. h2 native flow buffers are separate from capacity-one application queues. Unobserved failures require ledger memory proportional to those failures; there is no lossy cap. An `Unavailable`/GOAWAY/upload-EOF code is never evidence of server receipt or permission to replay.

## Validation historique, pas acceptation du lot local du 15 novembre

On Windows, offline and locked, with build artifacts owned by this worktree. `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` avoids unnecessary disk/debug artifacts. Earlier disk-exhaustion/linker failures and a corrected fixture synchronization timeout are not counted as RED assertions.

- RED: imported-lot reviewer probe reproduced both defects; assertive double-GOAWAY regression failed at the missing driver-error assertion.
- RED: typed wire-cause regression failed before the receive-side recovery poll.
- GREEN: `cargo test --offline --locked -p rambler-transport` — 61 tests passed (4 unit, 3 credential, 11 HTTP/2, 13 lifecycle, 29 loopback, 1 source-error).
- GREEN: `cargo clippy --offline --locked -p rambler-transport --all-targets -- -D warnings` — passed.
- GREEN: unchanged reviewer probe after correction — case 1 still returns `Grpc(Internal)`, `Connect`, OK (intentional conservative duplicate); case 2 now returns `ResourceExhausted`, `Cleanup`, **`Connect`**, OK instead of silently returning OK twice.
- GREEN: `cargo fmt -p rambler-transport -- --check` and `git diff --check` — passed; only platform LF/CRLF notices were emitted.
- GREEN: all six built transport test binaries executed 100 times — **100/100 repetitions, 6,100 test executions** passed, including 51,200 inherited short-session correctness checks. Each subprocess had a 30-second bound, in addition to per-case 5-second/3-second bounds. No timing benchmark was run.

No root lockfile, engine, CLI or other agent's loopback fixture was imported/changed. No commit or push was made.
