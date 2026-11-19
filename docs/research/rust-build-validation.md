# Rust build validation

Current cross-platform evidence: [close-ledger and zero-budget CI](../evidence/rust-close-ledger-zero-budgets-ci-20261115.json).
Earlier sections retain their original local/static checkpoint scope and failures.

## Corrected close-ledger CI — 2026-11-15

At `171ef084cc1b1e7f3b82daefdcd6bb0aadafe442`, all 13 jobs succeeded:
eight native, Rust lint and four Python. Each native job has five Rust test
groups: workspace 402/0/2 on Windows and 403/0/2 elsewhere
(passed/failed/ignored), session reuse 10, exact release finalization 1,
release invalid-budget gate 3, and feature-off 38. These are reruns, not a sum
of distinct tests. The named connection-dying regression passes in all eight
workspaces. Its first bounded close accepts only `Ok` or independent `Connect`;
the second must succeed, with ownership, no-replay and recovery checks retained.
The release budget gate validates three zero-argument rejections on all four
runner families and both toolchains; library zero behavior is unchanged.

The earlier Rust run on `7327f51` failed three macOS jobs due to the old test's
strict close result, not initial connection failure. It remains a historical
failure; this later success does not requalify that run. Between these commits
only the lifecycle test, this document, and its local proof changed; production
is unchanged. Root-helper, PCM, ignored PyAV and offline-only limitations remain
as described below; no real service, phone or new kernel evidence is implied.

## Expanded PCM/admission CI — 2026-11-15

At `4264126852b2c03e25fc231c60f9582569a84ae9`, all 13 jobs completed
successfully: eight native jobs, Rust lint, and four Python jobs. The nine Rust
files changed in `b2a960a` and `Cargo.lock` have identical Git blobs at both
commits; only the workflow and Intel checkpoint document differ between them.

Each native job on Rust 1.88.0 and stable 1.100.0 reported workspace
passed/failed/ignored of 399/0/2 on Windows, 400/0/2 on Linux, macOS ARM64,
and native GitHub Intel (`macos-26-intel`, observed `x86_64-apple-darwin`).
Each additionally reported session-reuse 10, exact release-finalization 1,
and feature-off 35; reruns are not distinct-test totals. Each ran the stdlib
boundary suite (17) and roots-helper suite (23). The root marker represents
unique selection and direct executable exit zero, not an exposed libtest result.
Release PCM reports 1017 input, 3051 decoded samples at 48 kHz, one EOS and
zero service calls. Rust PyAV remains ignored by default. Python pytest results
and exact runner versions, commands, job metadata and hashes are in the proof.
No real service call, handshake, private keys or minimum macOS version is
established. Linux live preflight remains blocked; phone remains 0/3 phrases.

## PCM progress and strict admission — local checkpoint 2026-11-15

Snapshot A is based on `1813602` plus local changes, with 173 selected source
inputs. Windows R1 ran 24 steps: workspace 399 passed, 0 failed, 2 ignored
on each of stable and 1.88; reuse 10, feature-off 35, exact release finalization
1, fmt, docs, boundary and release checks passed. The independent ignored PyAV
oracle was explicitly run on Windows: one test verifies 57 containers. The PCM
release smoke reports 1017 input samples, 3051 after trim, one EOS and zero
service calls. R1 is nevertheless **not qualified**: strict Clippy exited 101
on `never_loop` in a test scaffold, not a runtime regression.

Linux R1 passed 12 scoped phases, including workspace 400/0/2 on 1.88,
reuse 10, feature-off 35, release finalization 1 and PCM smoke. Snapshot B
changes only `crates/rambler-audio/tests/progress.rs`, removing a redundant
one-shot loop around the same select; selected production bytes are unchanged.
Windows R2 passed all six focused steps: source checks, fmt, six progress tests
on each toolchain and strict workspace/all-targets/all-features Clippy. Linux R2
passed five scoped phases including workspace 400/0/2 on 1.88. No full Windows
workspace rerun or release rebuild on B is claimed; release evidence is for A.

The native-root smoke and PyAV oracle are ignored by default. No explicit native
root control was rerun in this lot; the older OS-root qualification concerns
unchanged root code, not a new control run. The full Python suite was not rerun;
Python source was unchanged. The healthy synthetic loopback returns one final
recognition; the default-endpoint admission check is an in-process no-network unit.
No service or device action occurred. Linux live preflight remains blocked by an
observed piped crash-dump collector, with suppression unverified; no credentials
loaded, reservation, RPC or global setting change. This is not a claim that Linux
is unsupported. Phone remains at zero arms, zero voice, zero of three phrases.
At this checkpoint the new code is uncommitted and unpushed, macOS qualification
and remote CI for it remain pending. [Evidence](../evidence/rust-pcm-progress-admission.json).

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

## Review provenance: model attribution correction

On 2026-11-14, the `assistant.message.model` metadata for recent agents requested
with `model: opus` indicated `gpt-6.1-sol`, including the shared-buffer review
and the relevant transport/ledger work. These readings count as independent Sol
contexts, **not Opus validations**. The earlier named attribution must not
artificially strengthen the evidence. Tests and measurements actually performed
remain unchanged. New attempts at an Opus review were stopped before analysis;
no quota error was observed. **As of November 14**, Opus routing remained to be
restored; no substitute for that review was announced. An unauthenticated read of
the local proxy catalog then confirmed that it advertised GPT and Kimi but no
Claude/Opus model. The usual subagent selection/forcing variables were absent
from the controlled environment; no further inference or configuration change
was attempted in that check.

### Routing restored and actual review — November 15, 2026

After the user corrected routing, the integration driver verified the model
actually delivered by the host: `message.model=claude-opus-5-5`. This November 15
verification permits attribution of the **new** architectural review and advice
to Opus. It does not reclassify any request before the fix: those whose metadata
indicated Sol remain Sol reviews. The November 14 history above is preserved.

This review/advice is **static**: the reviewer did not run tests. Contributors
locally retested its findings; this does not qualify the real service or constitute
a real phone/network test. The oneof, OggScratch and mutex CPU micro-optimizations
were rejected as low priority and unmeasured against reliability risks. No speed,
latency or WAN throughput gain is attributed to these rejected proposals.

### November 15 reliability batch

The 310 tests of the committed baseline `90a773a` remain historical. The local
batch (admission, sticky audio errors, first TCP poll, h2 window, finalization)
now has these results on the same selected manifest:

| Local execution | Workspace | Reuse example | CLI without audio | Status |
|---|---:|---:|---:|---|
| Windows R3, MSVC / stable 1.99.1 |376 passed, 0 failed, 1 ignored|10 passed|31 passed|Qualified on Windows only|
| Windows R3, MSVC / 1.88.0 |376 passed, 0 failed, 1 ignored|Not separately rerun|Not separately rerun|MSRV workspace passed|
| Linux R2, WSL2 x86_64 / 1.88.0 |376 passed, 0 failed, 1 ignored|10 passed|31 passed|**FAILED**: release CLI finalization test|
| Linux R3, WSL2 x86_64 / 1.88.0 |376 passed, 0 failed, 1 ignored|10 passed|31 passed|Locally qualified with exclusions below|

Windows R3 completed its 26 steps with exit 0: strict all-target/all-feature
Clippy, formatting, strict rustdoc, 716 Python tests, normal/build audio boundary
with positive control, release CLI builds with/without audio and PCM example. The
PyAV test ignored by default was enabled separately (one test, 57 containers).
The release PCM smoke yielded 1017 input samples, 3051 after trim at 48 kHz, one
EOS and zero service calls. The test
`s3_end_without_grpc_trailers_exits_with_finalize_timeout` also passed with the
actual release audio executable, retained separately before replacement by the
minimal build. These groups and the two toolchains do not add up to distinct tests.

The Linux R2 release audio build command exited successfully, but its
`release-finalization-cli` test exited **101**, with a `peer upload was not complete`
assertion (0 passed, 1 failed). Separate, cross-checked diagnosis shows that Cargo
reused an **old release executable** without `--finalize-timeout`: it rejected the
arguments before any upload (exit 2, `arguments` category). Package cleanup without
`--release` covered only the dev profile; dry runs of both profiles confirm this.
The same test passed with the rebuilt debug binary, without qualifying release.
The driver fix adds cleanup of the same five packages in both profiles, without
changing the engine or native dependency caches.

The release PCM example, PCM smoke and final binary hash phases of R2 were
`not_run`; R2 remains **FAILED**, even after diagnosis. Both targeted Python groups
passed (17 audio boundary tests, 38 dex-methods tests), not the full Python suite.
PyAV, Clippy, fmt and rustdoc were not run in this Linux batch. Linux R3 applies
both cleanups, rebuilds the actual release and then passes all **16 phases**,
including the finalization CLI test (1 passed), PCM example build, its smoke
(1017 samples to 3051 at 48 kHz, one EOS), final binary hashes and post-run source
verification. The report states `completed_with_explicit_ignored_tests`, exit 0:
the ignored PyAV test remains a Linux exclusion, not an implicit success. Both
targeted Python groups pass again; the full Python suite, Clippy, fmt and rustdoc
remain run on Windows only for this batch. No engine code changed to repair this
qualification; R1/R2 remain historical failures.

The local Windows R3 (`qualified=true`), Linux R2 (`status=failed`) and Linux R3
(`completed_with_explicit_ignored_tests`, exit 0) reports retain exit codes and the
same canonical before/after digest of the **167 selected files**, excluding
docs/evidence:
`3059408291b04f4cf79c518b5e6d49cadcb87de5d04e4a5aa78f93679546f87e`.
This selector does not cover all build dependencies. The
[Windows R3 proof](../evidence/rust-reliability-windows-r3.json) publishes current
results; the [Windows R2 proof](../evidence/rust-reliability-windows.json) remains
historical. The [first Windows failure](../evidence/rust-reliability-r1-failed.json)
and [Linux R1/R2 failures](../evidence/rust-reliability-linux-attempts.json) are
preserved without relabelling them as successes. The
[Linux R3 proof](../evidence/rust-reliability-linux-r3.json) completes local
qualification of this snapshot with the explicit exclusions above, without macOS,
remote CI or real-service qualification.

The terminal commit puts fallible waits before irreversible publication of success;
the dedicated regressions pass in Windows R3. The
[reliability contract](rust-reliability-lifecycle.md) separates this finite evidence
from the older 12 scenarios and the transient `tcp-test-final.log` compilation
failure. Android/ART, authentication/provisioning/revocation, COMPOSE, macOS,
remote CI and service/WAN limitations remain unchanged. No performance gain,
proprietary APK detail, private log or secret is published here.

### Offloaded TLS setup — R4

After `99c5ff4`, only `connection.rs` changes in the source selector: native-root
loading/parsing and TLS construction move to a single awaited blocking job after
the TCP winner. The socket stays outside the job, roots remain fresh and TLS
settings unchanged. Cancellation can prevent a queued job from starting, but
cannot interrupt an already running OS loader. The static Opus 5.5 review accepts
the source, whose pool, shutdown and panic hook limits are described in the
[connection contract](rust-connection-lifecycle.md#tls-setup-outside-the-runtime--november-15-local-qualification).

| Local R4 execution | Workspace | Reuse example | CLI without audio | Release CLI finalization |
|---|---:|---:|---:|---:|
| Windows MSVC / stable 1.99.1 |382 passed, 0 failed, 1 ignored|10 passed|31 passed|1 passed|
| Windows MSVC / 1.88.0 |382 passed, 0 failed, 1 ignored|Not separately rerun|Not separately rerun|Not separately rerun|
| Linux WSL2 x86_64 / 1.88.0 |382 passed, 0 failed, 1 ignored|10 passed|31 passed|1 passed|

Windows completed its 26 steps with exit 0, including strict Clippy/fmt/rustdoc,
716 Python tests, the separate PyAV test (57 containers), dependency boundary with
positive control, audio/minimal release builds and PCM smoke. Linux completed its
16 phases with exit 0, including both targeted Python groups (17 and 38 tests),
the rebuilt audio release, its CLI test and PCM smoke. PyAV, the full Python
suite, Clippy, fmt and rustdoc remain excluded from this Linux run. No OS
certificate store or real TLS handshake is exercised by the six new injected
tests; the PCM smoke makes no service call.

Sources before/after and across platforms match the same 167 files:
`b04990f230464b9926a407b58143ec80b4f5ac6a413b0dd309808718743f7a84`.
The [TLS R4 proof](../evidence/rust-tls-setup-offload.json) distinguishes reports,
negative controls and static review without relabelling R3 evidence. This local
result proves neither WAN performance, Android/service parity, actual certificate
rotation, macOS nor remote CI.

## macOS read-only scout — 2026-11-15

A static, read-only scout at `b0591f5` inspected the five crate manifests and
platform cfg, file inputs, CLI subprocess tests, native Opus and TLS setup.
No concrete code-local macOS defect survived review. Native macOS builds and
tests, Keychain/TLS, FIFO and file runtime checks were **NOT RUN**; this is not a
macOS qualification. The 382-test Windows/Linux R4 results above are unchanged.

At `b0591f5`, `assert_released` in
`crates/rambler-audio/tests/file_cleanup.rs` was a no-op outside Windows/Linux.
A macOS test pass at that snapshot would therefore not establish descriptor
release through that helper. This was a coverage limitation, not evidence of a
production descriptor leak. The focused follow-up below replaced the macOS no-op;
at that local checkpoint its native runtime and remote CI were **NOT RUN**.
The later executed CI section below qualifies the named macOS control separately.

## Focused platform-gate follow-up — 2026-11-15

Only the test helper and written CI workflow change; production sources,
manifests and lockfile are unchanged. The descriptor audit compares device/inode
using `/proc/self/fd` and `stat64`/`fstat64` on Linux, `/dev/fd` and `fstat` on
macOS. Only `EBADF` close races are ignored; other audit errors fail. A Unix
positive control checks 0→1→0 open descriptors and an open WAV is asserted before
closure. Windows retains its exclusive-open check. This closes a test blind spot,
not a demonstrated production leak or a measured engine speedup.

| Focused local execution | File cleanup | Other checks | Status |
|---|---:|---|---|
| Windows initial R1, stable / 1.88 |3 / 3 passed|Strict audio Clippy, actual release CLI finalization (1 test), release PCM smoke passed|**FAILED**, 13 steps: fmt alone exited 1; `qualified=false` preserved|
| Linux x86_64 / 1.88, snapshot A |4 passed, including positive control|Actual release CLI finalization (1 test), release PCM build/smoke, source/binary checks passed|`passed_partial_linux_scope`, exit 0; 11 phases, including Clippy **NOT_RUN** (not installed)|
| Windows post-format R3, snapshot B |3 / 3 passed|fmt, strict audio Clippy, source before/after passed|6 steps passed; `qualified=true` for this scope only|

The initial Windows/Linux source selector covers **167 files**, not all build
inputs, with canonical digest
`2980b8ca5f180273152200d985cfffe9e3b10dfad723461df8d54b4401c5993d` (A).
The post-format/root selector is
`913eb97a9f60a39a85187dae70703f5a2493128e90681bde2a1e6e0c0cea7199` (B).
Only two mutually exclusive cfg imports in `file_cleanup.rs` swapped order from
A to B: reversing that swap with matching CRLF reproduces the exact initial
archive member and frozen file hash. All selected root file hashes match B;
post-format statuses, test counts and all six log hashes were independently
checked. Linux tested **A, not post-format bytes B**. There was no post-format
release rebuild or Linux rerun; original release evidence remains attached to A.
Both initial release PCM smokes check 1017→3051 samples at 48 kHz, one EOS and
zero service calls. Counts across toolchains/reruns are not distinct tests.

The full workspace, feature-off suite, Python suite, PyAV and rustdoc were not
rerun in this local follow-up; the 382-test R4 workspace results remained historical
at that checkpoint, not a claimed current 383-test workspace pass. macOS and Linux
32-bit native runtime, remote CI, service/device, WAN/parity and provisioning were
then unqualified; the later CI section updates only its explicitly executed scope.
Two background harness preparations are retained as
`setup_failed_before_commands` (line-ending assertion and temporary-driver syntax),
not engine runtime REDs; the initial Windows fmt failure remains separate.
The [focused proof](../evidence/rust-platform-gates.json) publishes allowlisted
statuses, counts and digests without private logs or paths.

The written OS/MSRV matrix now adds an actual audio release CLI build and the
exact loopback test `s3_end_without_grpc_trailers_exits_with_finalize_timeout`,
selecting that executable through the dev-only `RAMBLER_TEST_CLI_EXECUTABLE`
(platform `.exe` path on Windows), plus a release `pcm_channel` synthetic smoke.
At that local checkpoint these CI gates were **written only; remote CI NOT_RUN**.
This historical observation is superseded only for the executed scope below.

## Native public-root smoke — runner correction, 2026-11-15

The opt-in `connection::tests::tls_setup_native_public_roots_smoke` loads native
public roots through the existing blocking setup and requires TLS configuration
with ALPN exactly `h2`. It is ignored by default and performs no DNS, TCP,
handshake or RPC; no certificate values/names/hashes are printed. The
[original local Windows proof](../evidence/rust-native-roots-windows.json) remains
historical, as does the successful `1561f87` matrix below, which predates this test.
The [runner-history proof](../evidence/rust-native-root-runner-history.json)
records the subsequent failures and bounded local correction without rewriting either.

At `bc5449c`, [Rust CI2](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43726164598)
**FAILED** only in the Ubuntu stable/1.88 native-root step. Both selected the
intended ignored test but failed the `SSL_CERT_FILE`-absence assertion **before
native OS-root loading or TLS configuration**. The precise downstream environment
injector is unverified. Windows/macOS on both toolchains and lint succeeded;
macOS therefore exercised native public-root loading/configuration, not a handshake.
[Python CI2](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43726164608)
passed all four jobs. Local Linux R1 reproduced the same precondition failure
(0 passed, 1 failed, 0 ignored, 52 filtered); its 167 selected sources were unchanged
and containment valid. That failure is preserved, not a failed root-loader diagnosis.

Correction `9069059` changes the launcher only, leaving the Rust assertion,
production TLS, manifests and lockfile unchanged. Cargo builds with `--no-run
--message-format=json`; the stdlib helper selects exactly one test-profile
`rambler_transport` library executable from artifacts, without filename guesses
or package-ID matching. It directly lists and runs that executable from the
transport directory, enforcing a unique exact ignored-test listing and exit 0.
Each direct child receives a fresh environment with both SSL certificate override
names removed case-insensitively; the parent environment is unchanged. Build/list/run
deadlines are 120/20/30 seconds. Raw child output is captured, not emitted; only a
fixed success marker is published. There are no custom roots, TLS relaxations or retries.

```sh
python -I -B -m unittest discover -s tests -p test_rust_native_roots.py -v
python -I -B tools/check_rust_native_roots.py
```

The 23 pure stdlib tests include malformed/ambiguous artifacts and literal
U+0085/U+2028/U+2029 JSON paths; the Unicode regression was RED before the LF-only
record-splitting fix. Local Linux R2 passed all six phases on Rust 1.88, including
23 unit tests, direct native-root success, cleanup and source verification. Native
Linux root loading/configuration **actually succeeded locally after the direct fix**.
Windows R1's six subprocesses all exited 0, including stable/1.88 direct runs,
23 units and fmt, but its original aggregate remains `qualified=false`: its summary
regex rejected CRLF. A separate saved reassessment parsed the existing captured
summary correctly and passed, **without rerunning tests**. Neither direct-helper
run exports libtest totals, so no numeric Rust pass count is inferred.

Both local platforms preserved the same 169 selected source files before/after,
with digest `65029fd139d299a41f34905c55a02ca9fcf453f7624f7f4596e5099714ec76b3`;
this selector is not all build inputs. Static decision-review metadata records
15 assistant responses from Opus 5.5; that advice is not execution evidence.
At the supplied checkpoint, corrected [Rust CI3](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43730415832)
and [Python CI3](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43730415914)
at `906905988c5c51d83b05b259cd642a165325b7d2` were **IN_PROGRESS / pending**, not passed;
no later outcome was polled for that historical consolidation. Both workflows
have since completed successfully, including all six native jobs and the direct
native-root step. The [verified CI3 follow-up](../evidence/rust-native-roots-ci-20261115.json)
records the later outcomes separately without relabelling CI2 or local R1 failures.

These results do not prove TLS handshake/hostname/chain validation, root completeness
or rotation, private-key handling, provisioning, service/device interoperability or
WAN performance. No app/network/service execution accompanies the local correction.

## Executed remote CI matrix — 2026-11-15

The push on `feat/rust-engine` executed both public workflows at source commit
`1561f878a0b8714af669771fc7bd091784b1f3d9`:
[Rust run 43722181759](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43722181759)
and [Python run 43722181718](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43722181718).
**Both completed successfully: six native jobs, one Rust lint job and four Python
jobs.** The [allowlisted proof](../evidence/rust-remote-ci-20261115.json) records
job URLs, source SHA, commands, counts, versions and verified report/log hashes.
It qualifies that source commit, not later documentation-only commits or byte
equality with earlier local CRLF source selectors. No merge or PR is implied.

| Native runner / logged architecture | Toolchain / executed rustc | Workspace passed / failed / ignored | Reuse passed | Feature-off passed | Release CLI exact test passed |
|---|---|---:|---:|---:|---:|
| Windows Server 2025 / x86_64 MSVC | 1.88.0 / 1.88.0 |382 / 0 / 1|10|31|1|
| Windows Server 2025 / x86_64 MSVC | stable / 1.100.0 |382 / 0 / 1|10|31|1|
| Ubuntu 26.04.1 / x86_64 | 1.88.0 / 1.88.0 |383 / 0 / 1|10|31|1|
| Ubuntu 26.04.1 / x86_64 | stable / 1.100.0 |383 / 0 / 1|10|31|1|
| macOS 26.7.2 / aarch64 (arm64) | 1.88.0 / 1.88.0 |383 / 0 / 1|10|31|1|
| macOS 26.7.2 / aarch64 (arm64) | stable / 1.100.0 |383 / 0 / 1|10|31|1|

Counts are per invocation/toolchain, **not additive across rows or overlapping
groups**; workspace includes doc-tests. The one-test Unix difference is the
positive descriptor control: file cleanup has four passes on Unix versus three
on Windows, already included in workspace totals. On both macOS toolchains,
`descriptor_audit_detects_an_independently_opened_file_and_its_release` genuinely
ran and passed, replacing the earlier no-op coverage limitation. Empty harnesses
are not ignored tests. Rustc verbose host output supports the listed architectures;
this is Apple Silicon evidence, not an Intel-and-Apple-Silicon matrix. Executed
version output also records Cargo 1.88.0 / 1.100.0 and CMake 4.4.5. The separate
lint job passed fmt and strict all-target/all-feature Clippy; its report establishes
the stable setup selection, not an independently recorded runtime version.

Each native job fetched with `cargo fetch --locked`, then executed these gates:

```sh
python -m unittest discover -s tests -p test_rust_audio_boundary.py -v
python tools/check_rust_audio_boundary.py
cargo test --workspace --all-features --locked --offline
cargo test -p rambler-cli --features audio --example session_reuse --locked --offline
cargo build -p rambler-cli --bin rambler --features audio --release --locked --offline
cargo test -p rambler-cli --features audio --test finalization_cli --locked --offline -- s3_end_without_grpc_trailers_exits_with_finalize_timeout --exact
cargo run -p rambler-audio --example pcm_channel --release --locked --offline
cargo test -p rambler-cli --no-default-features --locked --offline
```

The finalization test uses a **debug test harness** with the logged dev-only
`RAMBLER_TEST_CLI_EXECUTABLE` override selecting the built release CLI (`.exe` on
Windows), not a release harness. This is logged selection plus exact loopback-test
success, not independent instrumentation of the spawned executable. Each release
PCM smoke reports **1017 input samples → 3051 at 48 kHz, one EOS, zero service
calls**. Each dependency-boundary checker suite passed 17 tests and its production
check passed both feature-off normal/build edges and the audio-on positive control.

| Python runner | Setup-reported Python | Passed | Skipped | Subtests passed, separately |
|---|---|---:|---:|---:|
| Windows |3.11.9|640|71|259|
| Ubuntu |3.11.18|640|71|259|
| Windows |3.14.10|710|1|259|
| Ubuntu |3.14.10|710|1|259|

Python ran `python -m pytest -q -W error -o faulthandler_timeout=30`, CLI help and
wheel build successfully. Versions above come from setup-python success messages;
quiet pytest did not log an independent runtime version banner. The 259 subtests
are **not additional test totals**; ignored-file counts are unavailable, not inferred.
Optional audio installation was skipped on 3.11 and succeeded on 3.14. The earlier
local 716-test result is not requalified as a CI total. Exact Python commands are
from the checked-in workflow with corresponding successful report steps.

Four source reports and 17 referenced evidence files were checked for SHA-256 and
byte length without publishing raw logs/API bodies or private paths. The Python/lint
and Ubuntu reports captured an earlier Rust `in_progress`/null-conclusion snapshot;
later Windows completed/success and macOS success observations establish the final
status. These earlier observations, prior local failures and scoped `NOT_RUN`
records remain historical, not relabelled successes.

**Limits:** the independent Rust PyAV oracle was explicitly ignored once per native
workspace job, not run in CI. macOS descriptor/WAV, PCM and release CLI loopback
runtime are now tested; GUI/microphone, native Keychain/trust-store behavior and
live TLS are not proven. Service/device interoperability, WAN quality/performance,
complete Android/service parity, provisioning/revocation and Linux 32-bit remain
outside this evidence. Hosted images/actions and stable toolchains move; setup/fetch
needs network, and offline tests may open loopback sockets. No bit-for-bit
reproducibility claim or new build/test execution accompanies this consolidation.

## CI 7327f51 follow-up: independent close error

Rust CI `43744193442` failed on three macOS jobs; Python `43744193492` passed.
The panic at `lifecycle.rs:514` came from the first `transport.close()`, not the
initial connection: the first echo and all four accepted-session cleanups had
already succeeded. After an intentional peer disconnect, the driver can report
`Connect` depending on the platform. The test incorrectly required `Ok` every
time, contrary to the contract for preserving that fault.

The test fix accepts only `Ok` or `Connect`, then requires a successful bounded
second close, without changing production or removing ownership, no-replay and
explicit replacement assertions. The lifecycle suite passes locally on Windows
(14 tests per stable/1.88 toolchain), as do fmt and strict transport Clippy. New
macOS qualification remains pending corrected CI; the three initial failures
remain preserved in [the proof and its limitations](../evidence/rust-macos-close-ledger-fix-20261115.json).
