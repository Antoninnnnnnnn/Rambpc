# Rust build validation

## Scope and prerequisites

The workspace declares Rust 1.88. The checked-in `Cargo.lock` is the dependency
snapshot for these builds; use `--locked` and review lockfile changes separately.
This gives repeatable dependency selection, not bit-for-bit reproducible binaries:
compiler, runner image, C toolchain, OS SDK, CMake and build environment can
change. The stable toolchain and `*-latest` CI images deliberately move. A
cross-platform pass is required when updating the lockfile or MSRV; do not assume
that a dependency's Rust version on one target proves compatibility on others.

Rust edition 2024 requires an edition-aware toolchain. Native builds need a C
compiler and linker, platform SDK, and CMake. The bundled Opus source in
`opusic-sys` is compiled locally; `ring` also uses native C. On Windows install
Visual Studio C++ build tools and Windows SDK; on Linux use GCC or Clang and
standard development tools; on macOS use Xcode command line tools. CMake must
be on `PATH` or provided in `CMAKE`. Even the CLI feature-off **tests** have
native audio dev-dependencies, so they still need these tools. A short target
path can avoid Windows MSBuild path-length failures.

For a clean local dependency cache, first run `cargo +1.88.0 fetch --locked`
(network required). Then run from the workspace root, without credentials:

```sh
cargo +1.88.0 test --workspace --all-features --locked --offline
cargo +1.88.0 test -p rambler-cli --no-default-features --locked --offline
cargo +stable fmt --all -- --check
cargo +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```

`--offline` applies after fetching; it does not prohibit test code from opening
loopback sockets. Tests use synthetic/local data and loopback, not a live Google
service or `.env`. The independent PyAV audio test is explicitly ignored unless
its separate Python prerequisite is arranged. These commands do not prove live
service interoperability.

## Executed locally

On 2026-11-13, Windows 11 x86_64 MSVC, rustup initially had stable 1.99.1 and
1.98.0, with only `x86_64-pc-windows-msvc` installed. Installed 1.88.0 using
`rustup toolchain install 1.88.0 --profile minimal` without changing the stable
default. CMake 3.31.10 came from an isolated unpacked wheel via `CMAKE`; Visual
Studio 2022 Community MSVC 14.44 and Windows SDK 10.0.26100 were present.
The lockfile was unchanged.

- `cargo +1.88.0 test --workspace --all-features --locked --offline`: passed,
  including audio and loopback; one independent PyAV test ignored. Cold build
  finished in 3m18s before test execution.
- `cargo +1.88.0 test -p rambler-cli --no-default-features --locked --offline`:
  passed, including the feature-off regression.
- `cargo +stable fmt --all -- --check`: passed.
- `cargo +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`:
  passed with rustc 1.99.1.

An initial all-features attempt with `CARGO_TARGET_DIR` under the long session
scratchpad failed in CMake compiler detection. The MSBuild log showed FTK1011
creating long tracking-log paths, not an incompatible C compiler or Rust
dependency. Retrying with the shorter workspace `target` succeeded. No dependency
or cryptographic downgrade and no MSRV increase was necessary for this Windows
locked graph. That result does not validate unselected target dependencies.

## Integrated refinement recheck

The parent then repeated both Rust1.88 commands on the integrated refinement
(safety decoding/policy, encoded-audio observations, event presentation and
adversarial tests): **186 tests passed** for the all-feature workspace, one
independent audio oracle ignored by default; the feature-off CLI passed too.
Stable1.99.1 also passed the workspace, all-target/all-feature Clippy, formatting
and the release CLI build with audio. The PyAV independent oracle was run
separately and passed57 complete containers. None of these local Windows runs
establishes Linux/macOS compatibility or live-service equivalence for new fields.

## Consolidated backend recheck — 2026-11-14

The current snapshot (Linux transfer R5) includes response containers/fallbacks,
progressive Ogg validation and the engine's primary/secondary shutdown-error fixes.

| Execution | Workspace | CLI example | CLI audio feature off | Build / lint |
|---|---:|---:|---:|---|
| Windows11 MSVC, stable1.99.1 |262 passed,1 ignored|4 passed|27 passed|Strict Clippy, formatting, release with/without audio passed|
| Windows11 MSVC,1.88.0 |262 passed,1 ignored|Not separately rerun on1.88|27 passed|Workspace compiled; release validator built with stable|
| Linux x86_64, WSL2 Ubuntu24.04,1.88.0 |262 passed,1 ignored|4 passed|27 passed|Native ELF release audio build passed|
| macOS |Not executed|Not executed|Not executed|No linked/runtime claim|

The independent ignored audio oracle was explicitly run on Windows1.88 and passed
57 complete containers with the installed development PyAV decoder. Python/tools
also passed656 tests (13.82s) with asyncio debug and faulthandler enabled. The CLI's
normal feature-off release build passed with a deliberately nonexistent `CMAKE`;
feature-off tests still have native audio development dependencies.

Linux used the already-running RelayTests WSL distro, GCC13.3 and isolated Rust1.88 /
CMake3.31.10 tools with no profile/global installation changes. Source and fixtures
were transferred without credentials into an isolated Linux directory; the distro's
`/mnt/c` was not a Windows mount. No phone/Android/VM relay or live Google calls
were used in these tests. Earlier8MiB fixture timeout and obsolete driver-error
expectation failed before targeted fixture/contract corrections; R5 is the successful
current-code run, not a relabelling of those failures.

Reproduce the example tests explicitly (the ordinary workspace command does not
run them):

```sh
cargo test -p rambler-cli --features audio --example session_reuse --locked --offline
```

## Shared raw / file preflight / recognizer options recheck

The next integrated snapshot R6 passed **274 workspace tests** (one independent
audio oracle ignored by default), four reuse-example tests and30 feature-off CLI
tests. This ran on Windows stable1.99.1 and Linux1.88; Windows1.88 separately passed
the274 workspace tests and30 feature-off tests. Strict all-target Clippy, formatting,
feature-off release with deliberately invalid CMake, release audio and the native
reuse-validator build passed. Linux release audio passed too. No remote CI or
macOS run is implied.

The Linux FIFO CLI probe changed from externally killed exit124 after2s to immediate
audio-error exit1 with53 bytes of JSON on both commands. Windows named-pipe testing
first exposed a remaining metadata-only check failure; the final descriptor-type
check passed with a fresh no-data pipe instance per command. See
[rust-file-inputs.md](rust-file-inputs.md). No credentials or live RPC were used.

## Defensive decoder allocation recheck

The final bounded decoder snapshot R9 passed **279 workspace tests** on Linux
x86_64 / Ubuntu24.04 / Rust1.88, with one independent audio oracle ignored;
the reuse example passed4 tests and the audio-feature-off CLI passed30.
Windows11 MSVC passed the same279 workspace tests on stable1.99.1 and1.88;
stable also passed the4 example tests,30 feature-off tests, strict all-target /
all-feature Clippy and formatting. Windows release builds passed with audio and
without it (the latter with a deliberately nonexistent `CMAKE`).
The Linux native release audio build passed. R8 already exited successfully, but its
combined stdout/stderr file capture was incomplete; R9 used pipe capture and
preserved the full log instead of inferring missing totals. No service calls,
credentials, dependency changes or new system installation were involved.
The [decoder report](rust-backend-decode-defensive-capacity.md) separates the
rejected V2 and records the final sparse-memory tradeoffs.

A separate bounded Sol differential check compared6,791 unique inputs (at most
64KiB each) against commit `29fc0d143f255bf581faca595be290713993543d`:
47,537 complete Debug model/error comparisons with production limits, then47,537
with both scratch-only source copies instrumented to a100-field budget.
**Zero differences** were found, including raw/unknown bytes, ordering, earlier
invalid UTF-8, merges, oneofs and budget boundaries. The production source and
limits were not changed by instrumentation. This is finite regression evidence,
not a proof of equivalence or an Opus review. Seed, corpus digest, output digests,
source and harness hashes are in the [public summary](../evidence/rust-backend-decode-differential.json).
The scripts/corpus/source snapshots remain local development artifacts; the
checked-in `decode_capacity` tests reproduce the focused regression cases.

## Provenance des revues : correction du modèle

Le2026-11-14, les métadonnées `assistant.message.model` des agents récents demandés
avec `model: opus` indiquent `gpt-6.1-sol`, y compris la revue des buffers partagés
et les travaux transport/ledger concernés. Ces lectures comptent comme contextes
indépendants Sol, **pas comme validations Opus**. L'ancienne attribution nominative
ne doit pas renforcer artificiellement le niveau de preuve. Les tests et mesures
réellement exécutés restent inchangés. Les nouvelles tentatives de revue Opus ont
été arrêtées avant analyse ; aucune erreur de quota n'a été constatée. **À cette
date du14 novembre**, le routage Opus restait à rétablir ; aucune substitution de
cette revue n'était annoncée. La lecture sans authentification du catalogue du
proxy local confirmait alors qu'il annonçait GPT et Kimi, mais aucun modèle
Claude/Opus. Les variables usuelles de sélection/forçage de sous-agent étaient
absentes de l'environnement contrôlé ; aucune inférence supplémentaire ni
modification de configuration n'avait été tentée dans ce contrôle.

### Routage rétabli et revue réelle — 15 novembre 2026

Après la correction du routage par l'utilisateur, le pilote d'intégration a
vérifié le modèle effectivement livré par l'hôte :
`message.model=claude-opus-5-5`. Cette vérification du15 novembre permet d'attribuer
la **nouvelle** revue architecturale et ses conseils à Opus. Elle ne requalifie
aucune demande antérieure au correctif : celles dont les métadonnées indiquaient
Sol restent des revues Sol. L'historique du14 novembre ci-dessus est conservé.

Cette revue/advice est **statique** : le reviewer n'a pas exécuté les tests. Ses
constats ont fait l'objet de contre-tests locaux par les contributeurs ; cela ne
constitue ni qualification du service réel ni essai téléphone/réseau réel. Les
micro-optimisations CPU oneof, OggScratch et mutex ont été écartées comme peu
prioritaires et non mesurées face aux risques de fiabilité. Aucun gain de vitesse,
de latence ou de débit WAN n'est attribué à ces propositions non retenues.

### Lot de fiabilité du 15 novembre

Les 310 tests de la base commitée `90a773a` restent historiques. Le lot local
(admission, erreurs audio sticky, premier poll TCP, fenêtre h2, finalisation)
dispose désormais de ces résultats sur un même manifeste sélectionné :

| Exécution locale | Workspace | Exemple reuse | CLI sans audio | Statut |
|---|---:|---:|---:|---|
| Windows R3, MSVC / stable 1.99.1 |376 réussis, 0 échoué, 1 ignoré|10 réussis|31 réussis|Qualifié Windows seulement|
| Windows R3, MSVC / 1.88.0 |376 réussis, 0 échoué, 1 ignoré|Non relancé séparément|Non relancé séparément|Workspace MSRV réussi|
| Linux R2, WSL2 x86_64 / 1.88.0 |376 réussis, 0 échoué, 1 ignoré|10 réussis|31 réussis|**FAILED** : test CLI release de finalisation|

Windows R3 termine ses 26 étapes avec sortie 0 : Clippy strict all-target/all-feature,
formatage, rustdoc strict, 716 tests Python, frontière audio normal/build avec
contrôle positif, builds release CLI avec/sans audio et exemple PCM. Le test PyAV
ignoré par défaut est activé séparément (un test, 57 conteneurs). Le smoke PCM
release donne 1017 échantillons d'entrée, 3051 après trim à 48 kHz, un EOS et zéro
appel service. Le test `s3_end_without_grpc_trailers_exits_with_finalize_timeout`
passe aussi avec le véritable exécutable release audio, conservé séparément avant
son remplacement par le build minimal. Ces groupes et les deux toolchains ne
s'additionnent pas en tests distincts.

La commande de build release audio Linux R2 sort avec succès, mais son test
`release-finalization-cli` termine en sortie **101**, avec une assertion
`peer upload was not complete` (0 réussi, 1 échoué). Le diagnostic séparé,
contre-vérifié, montre que Cargo a réutilisé un **ancien exécutable release** sans
`--finalize-timeout` : celui-ci rejette les arguments avant tout upload (sortie 2,
catégorie `arguments`). Le nettoyage par paquet sans `--release` ne couvrait que
le profil dev ; les dry-runs des deux profils le confirment. Le même test avec
le binaire debug reconstruit passe, sans qualifier le release. La réparation du
pilote ajoute le nettoyage des cinq mêmes paquets dans les deux profils, sans
modifier le moteur ni les caches de dépendances natives.

Les phases release exemple PCM, smoke PCM et hashes binaires finaux de R2 sont
`not_run` ; R2 reste **FAILED**, même après diagnostic. Les deux groupes Python
ciblés passent (17 tests frontière audio, 38 tests dex-methods), pas la suite Python
complète. PyAV, Clippy, fmt et rustdoc n'ont pas été exécutés dans ce lot Linux.
Le résultat Linux suivant est en attente ; aucune réussite release après réparation
n'est encore revendiquée.

Les rapports locaux `reliability-windows-20261115-r3.json` (`qualified=true`) et
`reliability-linux-20261115-r2-private/report.json` (`status=failed`) conservent
les codes de sortie et le même digest canonique avant/après des **167 fichiers
sélectionnés**, hors docs/evidence :
`3059408291b04f4cf79c518b5e6d49cadcb87de5d04e4a5aa78f93679546f87e`.
Ce sélecteur ne couvre pas toutes les dépendances de build. La
[preuve Windows R3](../evidence/rust-reliability-windows-r3.json) publie les résultats
courants ; la [preuve Windows R2](../evidence/rust-reliability-windows.json) reste
historique. Le [premier échec Windows](../evidence/rust-reliability-r1-failed.json)
et les [échecs Linux R1/R2](../evidence/rust-reliability-linux-attempts.json) sont
conservés sans les réétiqueter en succès. L'acceptation du lot Windows/Linux reste
en attente du prochain résultat Linux et de sa publication.

Le commit terminal place les attentes fallibles avant la publication irréversible
du succès ; les régressions dédiées passent dans Windows R3. Le
[contrat de fiabilité](rust-reliability-lifecycle.md) sépare ces preuves finies des
anciens 12 scénarios et de l'échec transitoire de compilation `tcp-test-final.log`.
Les limites Android/ART, authentification/provisionnement/révocation, COMPOSE,
macOS, CI distante et service/WAN restent inchangées. Aucun gain de performance,
détail propriétaire d'APK, journal privé ou secret n'est publié ici.

## Written CI matrix, not yet executed

`.github/workflows/rust.yml` defines Windows, Linux and macOS jobs on 1.88.0 and
stable, fetching the locked graph first and then testing offline with and
without the CLI audio feature. A separate stable Linux job checks formatting and
all-target, all-feature Clippy. Hosted runners must provide CMake and a native C
toolchain. The matrix has **not** run remotely; local Linux execution is reported
separately above, and macOS remains unexecuted. Runner images and actions are not
immutable, and fetch/setup needs network. No remote CI, push or PR was triggered
by this validation; code and documentation are committed separately.
