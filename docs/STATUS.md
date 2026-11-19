# Engine status — 2026-11-18

The [getting started guide](../app/README.md) describes current platform coverage. This log retains dated engine validations. Current actions and results are detailed in the [roadmap](ROADMAP.md) and the [evidence index](evidence/README.md).

- **Native SDK and CLI**: Ogg/Opus or mono PCM input at 16/48 kHz and a UTF-16 edit proposal without automatic application. The [integration guide](research/sdk-integration-guide.md) describes revision, anchor and grapheme-boundary checks. [Local Windows qualification](evidence/rust-native-qualification-kit-windows-20261117.json) covers synthetic loopback paths, not the real service.
- **Real service, 18 November**: the native client obtained exact dictation and replacement, and a polite rewrite response (intent 3, field 7) that strict mode refused to render. PixelBoard 1.3.5 did not rewrite the selection on the tested non-Pixel device. [Observations and limits](evidence/pixelboard-vs-native-real-20261118.json).
- **Still open**: provider-documented durable access, a Linux caller with real access, a matched Android reference and server contracts for COMPOSE/RAMBLE/RAMBLE_HISTORY. Available observations establish neither Android parity nor representative quality or latency.

## Log

### Engine status — 2026-11-17

#### Overall objective: backend unfinished

The milestones and CI runs below qualify specific scopes, **not a complete or “perfect” backend**. The Opus milestone and its 14 green jobs do not close the following functional gaps.

| Area | Verifiable status | What is needed to close the point |
|---|---|---|
| Durable access | Snapshots, replacement by generation and server refusals handled client-side | Legitimate provisioning contract, validity/rotation and provider-side revocation; do not invent OAuth renewal |
| COMPOSE / RAMBLE / RAMBLE_HISTORY | Values decoded and retained; no server semantics implemented | Authorized exchanges demonstrating activation and contract; an ASR fallback does not replace them |
| Android parity | Static traces and several native behaviors observed | Matched service → client → editor reference, without a rejected hooking/tampering method |
| Real Linux with credentials | Loopback SDK, public core control and bounded public path of the native admission example qualified separately | Linux caller, secrets channel and real-session mode not qualified; real Linux access remains NO_GO |
| Real performance and reliability | Bounded synthetic local gains and offline invariants documented | No evidence of representative natural voice/WAN or universally optimal speed |

Sources: [functional matrix](research/rust-feature-parity.md),
[authentication](research/authentication.md),
[Linux qualification and limits](research/rust-linux-guarded-loopback.md),
[Opus Release and 14 CI jobs](research/rust-opus-release-build.md).

The two targeted 16 November audits (credential lifecycle and structured results) find no demonstrated fixable client gap within their scopes; this is not proof of absence of defects. They provide no server contract. The user then renewed authorization on 16 November to make useful calls without repeated per-trial requests, and offered native Linux hosts for qualification. Counts and results of earlier campaigns remain historical and intact; new trials must be tracked separately, without automatic replay of a started RPC or bypassing auth/TLS/attestation/security. The Opus study retains exactly its four runs. No gain or durable access should be inferred from a new series of mocks. Microphone, GUI and OS insertion remain outside the initial engine scope, not missing cloud features.

#### Native qualification kit — four integrated paths

`native_qualification` combines the PCM/Opus codec, engine, transport and result policies against an owned localhost peer and a reused connection. Four Release paths pass: literal, intent 3 rewrite, intent 2 Unicode selection and field 4 previews before audio EOF. The executable emits a JSON report only after stream validation and clean closure. The comparator rejects data or result differences before any timing comparison. **5 Rust tests, 5 Python tests, Clippy and formatting** pass locally; the Release report is accepted in self-comparison as a compatibility check, not a gain measurement. No external endpoint or private input; this bench does not qualify real-service behavior. [Guide](research/native-qualification-kit.md) · [Local evidence](evidence/rust-native-qualification-kit-windows-20261117.json).

#### Streaming sessions — grouped local qualification

**Two new scenarios** pass on Windows, with strict Clippy and formatting for both packages, in one validation without retry. They compose the real engine and transport to a loopback peer, without changing production code.

- **Concurrent PCM**: the peer observes two active RPCs (same TCP endpoint) and distinct Opus packets, then a cancellation. The neighboring stream retains its own packets, trimmed duration, context and final result through clean success. Two RPCs observed, not a measurement of unused connections.
- **Partial previews**: two distinct field 4 responses pass through the decoder and stable/revisable projection. The bench replaces the preview rather than concatenating it, then verifies a distinct final, S3 END and successful EOF.

These synthetic scenarios qualify neither partial emission by the real service, advanced-mode parity nor its latency. No new performance measurement. [Local evidence](evidence/rust-streaming-integration-windows-20261117.json).

#### Strict rendering — one overall check, 17 November

`render(Strict)` reuses the already checked candidate instead of calling `text()` again and traversing hypotheses a second time. The `text()` API retains its own check; safety and ambiguity refusals still precede projection. The test distinguishes empty ASR text from absent text. **23 targeted tests**, formatting and strict Clippy pass on Windows. This removes a traversal, not a timed latency gain. [Local evidence](evidence/rust-strict-render-single-pass-windows-20261117.json).

#### Local native pipeline — baseline `a89abd2`, 17 November

One Release run of `engine_loopback_profile --chunks 16384 --repeats 1` passes all four scenarios, including two with injected slowdowns. Per scenario: **16,384 blocks of 640 bytes** verified by the local peer, ordered context and end, **128 finals** and one source close. Observation scenarios compare all raw responses and check the single EOF; collection scenarios compare retained finals. This is one measurement per scenario, including synthetic peer work and cleanup, not a speed comparison. Blocks are opaque: the test measures neither codec, dictation nor Rambler latency. No old bench replayed. [Four measurements and limits](evidence/rust-engine-loopback-current-windows-20261117.json).

#### PCM audio — partition invariance, 17 November

A test compares Opus packets produced by the adapter with those from the same codec fed samples directly: six lengths, three partitions each (whole block, byte by byte and frame boundaries), with empty blocks and nonzero endings. Independent Ogg reading checks CRC, EOS and duration after trimming; input is exhausted and its close called only once. **19 targeted tests pass on Windows**, along with strict Clippy and formatting. CMake/MSBuild preparation failures and the first lint are retained. No production change, new benchmark or service call. [Local evidence](evidence/rust-pcm-partition-invariance-windows-20261117.json).

#### Oneof — reduced temporary allocations, 17 November

The new candidate keeps singleton groups on the stack and builds a chunk vector only for an actual merge. Unknown fields, order, raw, presence, resets, validation of overwritten occurrences and budgets remain covered by independent oracles and unchanged existing suites. The seven new integration tests pass on baseline and candidate; the candidate passes **336 per target/command, 1 ignored, 47 suites**, strict Clippy and formatting. Suites overlap: their counts are not added as distinct tests.

Two oracle-only checks precede **five alternating pairs, ten measurement processes and 500 raw rows**, without retry. Simple recognition response: **7 → 5 allocations**; nested edit result: **14 → 10**; sixteen typed-member changes: **37 → 20**. The four allocation/reallocation/requested-byte/retention counters and logical peaks do not increase in any of the 25 measured workloads. Model retention is unchanged; this is not RSS. The conservative timing filter fixed before measurement flags no case, without excluding a smaller, noisy or out-of-bench regression. Empty controls and actual merges do not justify a general timing gain.

Independent source review then numerical review found no blocker/discrepancy in their scopes; MAIN accepts the bounded local allocation gain. Fresh separate caches, pinned sources/binaries, Job membership and emptiness verified. CI for `b49bc5c` finishes at **14/14** after one targeted rerun: the macOS ARM/Rust 1.88 job had been cancelled before any step for lack of a runner. The nine Rust successes are reused and the four Python jobs remain unchanged. The eight new Rust tests pass in each of the eight native jobs. No new service, credential, device, microphone or replay of old safety/Opus campaigns. No complete backend, WAN latency or universal speed is inferred. [Method and limits](research/rust-oneof-run-allocation.md) · [Evidence and 500 rows](evidence/rust-oneof-run-allocation-windows-20261117.json) · [CI provenance](evidence/rust-oneof-run-allocation-ci-20261117.json).

#### Advanced modes and native access — documentary audits, 17 November

The static audit of baseline `ee336cf` does not demonstrate a confirmed missing client field for COMPOSE/RAMBLE/HISTORY: preferences, late context, legacy/cursor projections and structured results are already represented. It does not qualify server activation or their effects. An authorized matched reference across request, raw response, S3 helper, PixelBoard filter and editor state is needed to distinguish a client gap from local processing or a server gate; no invented flag, account state or SEND/CLEAR effect. [Audit and precise discriminants](research/rust-advanced-mode-contract-audit.md).

The bounded public search (six queries, two official pages read) identified no native provisioning contract for this exact S3. This result establishes neither universal absence of an access route nor authorization or prohibition of a particular configuration. Possession of a key or an old success does not replace verifiable provenance and usage scope. No new service/device connection or credential reading in these two audits. [Sources, bounds and prerequisites](research/native-access-public-research.md).

#### Safety decoding — measured local gain, 17 November

Source `c89f140` avoids materializing the exact `hypothesis.field30=0/1` markers as unknown, without changing parsing, budgets, final status, raw bytes or genuinely unknown fields. One marker drops from **13 to 11 allocations**; sixteen alternating markers from **28 to 11**. Five alternating process pairs, **350 raw rows**, without retry, confirm these counts on seven synthetic fixtures. The two new tests first reproduce unnecessary retention on the unchanged decoder; the candidate passes **328 tests per target/command, 1 ignored**, then local strict Clippy and formatting. No WAN, ASR, end-to-end or RSS gain or universal optimum inferred.

The first publication mistakenly omitted raw rows while retaining their count and aggregates; the corrected artifact restores them without new measurement, with public test, Clippy and Job receipts. MAIN observations and provenance limits are distinguished from machine receipts. [Measurement and limits](research/rust-safety-field-allocation.md) · [Corrected evidence](evidence/rust-safety-field-allocation-windows-20261117.json).

The first CI for `c89f140` finishes with **12 successes and two macOS ARM cancellations before any project step**, due to lack of hosted capacity. The targeted ARM/1.88 rerun succeeds; the simultaneous ARM/stable request, refused by GitHub, is retained. After that attempt ended, one ARM/stable rerun succeeds. The final view is **14/14 successes**: two accepted job reruns, no rerun of successes. The nine Rust successes from the previous attempt retain starts, ends and steps despite reissued IDs; the four Python records remain identical. This CI covers `c89f140`, not the unpublished oneof prototype; no service, credential or REAL session requalified. [CI incident and receipts](evidence/rust-safety-field-allocation-ci-20261117.json).

#### REAL caller — simulated regressions, 17 November

R1 remains frozen **MAIN_NO_GO, without invoking the launcher**, despite the two conditional source GO decisions: its false cleanup indicator is retained as evidence. R2 then passes **24 simulated tests**, then R3 **27 simulated tests**, each in a separate Windows trial, without failure/error/skip; Job membership and emptiness and unchanged pins verified. These are overlapping suites, not 51 distinct tests. R1/R2 methods serve only for diagnostics under fakes. R3 retains cleanup debt until result recording, including if it fails; the single start marker and forced exit also cover display and closed-result construction failures.

**GO limited to simulated regressions, never real.** No REAL preparation, credential reading, native engine invocation or Rambler service request occurred in these trials. Real remote-system behavior, READY, the secrets channel and session remain unqualified. Provisioning and native authorization of the configuration remain separate prerequisites, without inference from embedded headers, a local file or a past success. MAIN orchestrators remain specific to this workspace, not portable public launchers; no perfection or complete parity inferred. [R1 NO_GO retained](evidence/linux-session-real-caller-source-review-20261117.json) · [Bounded R2/R3 evidence](evidence/linux-session-real-caller-fake-qa-20261117.json) · [Provisioning](research/authentication.md#documentation-supplement--november-17-2026).

#### Frame producer — simulated QA, 17 November

The developer library `credential_frame_source.py` passes **60 tests** on Windows/Python 3.14, without failure, error or skip, after independent review and targeted follow-up on opening flags. Import and construction are inert; the probe reads metadata only. The single call builds a bounded frame, with fail-closed errors and best-effort mutable erasure. All inputs are fictional: no real credential file or service was used. Pins unchanged during the trial, external Job membership and emptiness verified. Files have explicit LF rules. **READY/channel integration, real metadata, confidentiality, the operational producer and REAL remain unqualified**; the single local trial proves neither Linux execution nor durable access. [Contract](research/credential-frame-source.md) · [Bounded local evidence](evidence/credential-frame-source-fake-qa-20261117.json).

A separate additional case then accepts **real Windows file operations on a new file containing only invented values**: expected frame, second call refused, file/pins unchanged and Job cleaned up. This is only a compatibility point, not qualification of a sensitive file or the READY channel. [Separate observation](evidence/credential-frame-source-windows-public-file-20261117.json).

CI for `45ff404` first finishes with **13 successes and one cancellation**: GitHub did not assign the macOS ARM runner, with no project step recorded. The capacity annotation is retained; one rerun of the cancelled job passes, bringing the final view to **14/14 successes**. The nine Rust successes are reused with identical starts, ends and steps despite new GitHub IDs; the four Python records remain unchanged. No manual public native campaign or Rambler service session is replayed by this offline CI. [CI provenance](evidence/credential-frame-source-ci-20261117.json).

#### Guarded session controller — 17 November

The new controller passes **49 simulated tests on Windows and 49 on Linux**, zero failures/errors/skips. Two test defects were corrected before execution, without changing production code; the following targeted review confirms source GO with simulated QA only. Fixed-fingerprint sources are kept LF on checkouts, without normalizing or weakening runtime checks. Then **one native PUBLIC_GUARD** succeeds with a new case and consumed reservation: invented 85-byte public frame, `--single-session-v1` and `REJECT_CREDENTIALS` refusal (151-byte terminal, EOF, exit 1), then READY and upstream success (38 + 82 = 120 bytes, EOF, exit 0). Two `environ` opens refused EACCES, without reading; internal/external Windows Jobs, cleanup, pins and postinspection verified. RPC ZERO is an invariant of the pinned source, not network telemetry. This case is separate from the 49 simulated tests per OS and the old PUBLIC_FRAME round trip. A separate read-only measurement gives 65536 bytes for an empty pipe, without qualifying the actual REAL pipe or secrets path. **REAL, credential producer/confidentiality, real session, durable provisioning and complete backend remain unqualified; live_go=false.** [Contract](research/linux-session-pipe-receiver.md) · [Historical simulated evidence](evidence/linux-session-pipe-fake-qa-20261117.json) · [Public observation](evidence/linux-session-public-guard-20261117.json).
CI for `da591e9` passes **14/14 jobs on the first attempt**, without rerun or activation of controller native modes.

#### Public input channel — 17 November

The new Windows sender rejects redirected venv interpreters and passes 31 simulated/fixture tests (one inapplicable test ignored), then an explicitly selected public native fixture. The 85-byte frame, EOF, single producer call, ownership and Job cleanup are verified in this case. Two distinct public native cases then pass blocked writing with timeout and explicit cancellation, with Job cleanup. No call to `CancelSynchronousIo` was observed: its effectiveness remains unqualified. [Windows nominal evidence](evidence/windows-owned-pipe-public-qa-20261117.json) · [Timeout/cancellation evidence](evidence/windows-owned-pipe-native-cancellation-20261117.json).
Offline CI for `04203b5` then passes **14/14 jobs on the first attempt**, without rerun or activation of the two opt-in native tests.

The public Linux receiver separately passes 31 tests with simulated ports on Windows. A distinct later observation succeeds in **one public case**: Windows → SSH with existing host key → Linux receiver → Rust PUBLIC_FRAME mode, with 85 bytes, two EACCES refusals without reading environ, EOF, 177-byte closed output and exit 0. Internal and external Jobs, complete postinspection, pins and staging are verified; no retry or automatic reconnection. The qualified Debug binary is an existing copy, not a new build. This case does not replay the historical 11-case native campaign. Zero sessions, healthy=false and zero service are plan/source invariants, not network telemetry. Private MAIN orchestrators are not portable public launchers: see the published helpers, protocol and receiver's neutral prerequisites. [Historical pure evidence](evidence/linux-public-pipe-receiver-qa-20261117.json) · [Bounded public SSH evidence](evidence/public-owned-pipe-ssh-20261117.json) · [Contract and limits](research/linux-public-pipe-receiver.md).
**Secrets channel, real SDK session, provisioning and complete backend remain unqualified; live GO=false.**

Combined CI for both sides at `e1ec3cd` passes **14/14 jobs on the first attempt**, without rerun. This qualifies compilation and offline suites, not an SSH trial, blocked native cancellation or a channel with credentials. [Separate CI evidence](evidence/public-channel-ci-20261117.json).

#### Public native Linux admission — 17 November

On a non-root x86_64 host, the optional **Debug** example and a public supervisor pass 11/11 finite cases: deliberately unhardened silent noargs, two safety refusals before READY with no byte sent, one valid public frame and seven malformed ones. For the eight framed cases, READY then external EACCES refusal on `environ` are observed before public input, without reading its contents. Cleanup of all 11 cases, source and cache verified. The build involves three direct launches and the Fake Debug binary passes 17 Rust tests; the 10 pure Python tests pass separately on Windows and Linux. Do not add these counts as sessions. The 122 measured source files are the Windows working-tree bytes associated with `f27a01b`; 22 differ from Git blobs only by CRLF normalization. These are neither a bit-exact Git snapshot nor the Release CLI.

Zero real sessions, credentials or service; zero service is a plan/source invariant, not an independent network isolation measurement. No G1 rerun or Opus study; R2/R3 and preparation failures are retained in the evidence. Linux caller, Windows secrets sender, provisioning and complete backend remain unqualified; **live NO_GO**. No conclusion on latency, ASR semantics or universal security. [Contract, reproduction and limits](research/linux-real-admission.md) · [Redacted evidence](evidence/linux-native-public-admission-20261117.json).

#### Native qualification and supervised voice reference — November 16

Snapshot `82e6394` passes on an unprivileged native Linux host: workspace **430/0/2**, build of the real Release audio CLI, four checks of that binary, guarded SDK 9, reuse 10, and direct public-roots smoke 1. Groups overlap; do not add them as distinct tests. Unchanged source and cleanup verified; no credentials or real ASR call. [Evidence and limitations](research/rust-native-linux-workspace.md).

Three cases were then observed in a separate public Android editor, with manually spoken sentences: recognized dictation but initial context not preserved, a rewrite instruction dictated literally, and two SEND callbacks in total after several attempts at the last case. Their attribution by utterance is not isolated and the fixture sends no message. Neither S3Bidi routing nor wire parity is inferred from this interface. The initial keyboard and microphone restrictions were restored. [Observations and caveats](evidence/android-supervised-public-voice-20261116.json).

The new experimental Linux admission example passes **17 Fake tests on Windows** and targeted Clippy; at this checkpoint, its native layer, credential input channel, and live mode remained unqualified. It is not included in the Linux snapshot above. [Contract and limitations](research/linux-real-admission.md), [Windows evidence](evidence/linux-real-admission-windows-20261116.json). **Linux live access remains NO_GO; backend remains unfinished.**

The first CI for public batch `81f9a83` finishes at **13 successful jobs / 1 failure**: all eight Rust matrices, VS17, and Python pass, but Linux Clippy rejects the FFI signature of `read`. Targeted native correction to `*mut c_void`, without removing the lint; **fresh CI for `f27a01b` passes 14/14 jobs**, including Linux Clippy, without a workflow rerun. This does not qualify native admission or the credential channel. [Retained failure and source delta](evidence/linux-real-admission-ci-20261117.json).

#### Native Linux — public core control succeeded

On a new Linux x86_64 host, the public control produces a bounded core and the `WCOREDUMP` bit; the protected case produces neither file nor bit and denies external opening of `environ`, without reading it. Exactly two targets are launched and then reaped. All 25 separate pure checks, cleanup, and a subsequent read-only verification pass. No credential, service call, or global setting. The tested protection combines dumpable=0 and core limit=0; neither an isolated dumpability effect nor admission of the real engine with secrets is inferred. The historical WSL failure remains unchanged; **Linux live with credentials NO_GO**. [Protocol and limitations](research/linux-public-core-control.md) · [Redacted evidence](evidence/linux-public-core-control-20261116.json).

#### gRPC envelopes — CI for eight native configurations qualified

All 13 jobs for exact source `8cb381e8d26d448db80481d4aab9472aee3b096c` succeed: [Rust](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43859489195) and [Python](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43859489085). Windows, Linux, macOS ARM, and Intel pass on Rust 1.88 and stable, including the previously failing Intel/1.88 job. The 72 Rust groups are verified separately: workspace **429/0/2** on Windows, **430/0/2** elsewhere. The seven new coalescing names, including two `should_panic`, the four HPACK tests, and earlier oracles are confirmed in all eight workspaces; API/provenance and 1605020 bytes of logs audited. [Separate CI evidence](evidence/rust-grpc-envelope-coalescing-ci-20261116.json). These results do not reconstruct the cause of the old job failure or measure any optimum or overall performance gain. Linux G1 and durable access remain unchanged.

**Separate local checkpoint and historical failure:**

CI for `70bbf3b` remains **failed: 12/13 jobs successful**, with the framing oracle failing on macOS Intel/Rust 1.88 with GOAWAY `ENHANCE_YOUR_CALM`. All four HPACK tests pass in that same job. [Retained failure](evidence/rust-credential-hpack-wire-ci-failed-20261116.json). The exact cause of this trace was not captured; static analyses support a mechanism related to separate small DATA frames, not historical wire proof.

The candidate combines prefix and payload in a private chained buffer, without a payload concatenation buffer, batching across messages, or changing limits. The earlier 260-message and three-byte-window oracles remain unchanged. Windows stable/1.88: transport **126/0/1** each, new integrations **2/0/0** each, workspace **429/0/2**, SDK9 and feature-off38; formatting/strict Clippy pass. All 14 commands and jobs succeed, 188 sources identical before/after. Qualified source: uncommitted candidate on `70bbf3b`; fresh remote CI still pending at this checkpoint. No overall time/allocation gain, durable access, complete parity, or Linux live inferred. [Contract and limitations](research/rust-grpc-envelope-coalescing.md) · [Positive local evidence](evidence/rust-grpc-envelope-coalescing-windows-20261116.json).

#### Metadata HPACK — Windows S1 local qualification

An independent oracle decodes frames and HPACK/Huffman according to RFC 7541 on a native loopback trace: three concurrent RPCs, rotating then removed dummy credentials, one TCP per fixture. It verifies exact stream binding, never-indexed encoding, static name 23, a dynamic table without sensitive entries, and value order per name. Twenty mutations of the actual test capture and negative prefixes check the oracle; no audio, service, or real credential.

All **four targeted tests** pass on Windows stable 1.99.1 and 1.88; transport **119/0/1** on each toolchain, stable workspace **422/0/2**, feature-off **38/0/0**, fmt and strict all-features transport Clippy pass. These groups must not be added together. QA1 failed on a Clippy lint in the hexadecimal helper; its test-only correction precedes successful QA2, with the initial failure retained. The 187 selected sources remain unchanged before/after QA2. Qualified source: measured uncommitted candidate on `cb7c707`; remote CI still pending at this local checkpoint, no production change. [Contract and limitations](research/rust-credential-snapshots.md#raw-hpack-inspection--local-windows-s1-qualification) · [Local evidence](evidence/rust-credential-hpack-wire-windows-20261116.json).

S1 is a finite observation: a reset does not prove absence of unread bytes. S2/CONTINUATION, eviction, table updates, TLS, universal isolation, performance, and durable access remain outside qualification. Linux G1 remains unsatisfied, live NO_GO; Windows real budget MAX1 already consumed and phone unchanged.

#### gRPC prefixes — allocation removed on measured short path

An immutable table of **1280 bytes of source data** replaces allocation of the five-byte prefix for messages 0–255; the payload was already zero-copy, and fallback and flow control remain unchanged. Two runs on the same binary confirm **1 → 0 allocation/free** for the eleven measured short sizes; five fallback controls have identical counts, without exceeding the repeated time-regression threshold. No overall dictation gain attributed.

On the real synthetic PCM/Opus + S3 envelope pipeline, each signal with 20ms pages gives **253/253 eligible messages**, even at the maximum tested elapsed width. 1000ms control: 3/8. This is neither representative speech nor a server hit. Raw framing of 260 messages and a prefix actually split at 3 bytes by the remote window pass on stable/1.88. Transport **115/0/1**, benchmark **7/0/0** each, stable workspace **418/0/2**, SDK9, feature-off38, formatting/Clippy/build pass. First-batch lint failure retained; historical CLI and budgets unchanged. These local measurements remain a historical checkpoint when CI was still pending.

Offline remote CI has now passed on exact source `98d46590ec708c7e1ae6157323a07b8eaa41ddd7`: [Rust](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43832001772) and [Python](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43832001623), 13 jobs including 8 native and 72 Rust command groups, not distinct tests. Per toolchain, Windows workspace **418/0/2**, Linux and macOS **419/0/2**; macOS ARM and Intel confirmed. Per native job: prefix benchmark **7/0/0**, guarded SDK **9/0/0**, wire benchmark **3/0/0**; do not sum replayed groups. CI checks invariants, the raw H2 matrix, forced fragmentation, and synthetic PCM/Opus sizes, not local times or allocations, real speech, or service performance. G1 remains unsatisfied, Linux live NO_GO; no new qualification of credentials, phone, core, or provisioning.

[Contract and limitations](research/rust-grpc-prefix-allocation.md) · [Historical local evidence](evidence/rust-grpc-prefix-allocation-windows-20261116.json) · [Remote CI evidence](evidence/rust-grpc-prefix-allocation-ci-20261116.json).

#### Serialization optimization — CI for eight native configurations qualified

Commit `43e918d` passes all **13 CI jobs**: Windows, Ubuntu, macOS ARM64 and Intel × 1.88/stable, lint, and four Python jobs. Workspace **411/0/2** on Windows, **412/0/2** elsewhere; five new fidelity/fixed32 names and the three-test benchmark gate are confirmed in all eight native jobs. The 64 Rust groups, 1,545,494 bytes of logs, metadata, steps, hashes, and hosts are verified. [CI evidence for exact commit](evidence/rust-wire-encoding-allocation-ci-20261116.json).

Counter gains remain those of the two local Windows measurements: no new CI timing or WAN/full-engine gain attributed. The local “CI pending” qualification below is historical; durable access, Android parity, and G1/Linux live are not resolved by this optimization. [Measurement and limitations](research/rust-wire-encoding-allocation.md).

#### Protobuf serialization — measured allocation gain (historical local checkpoint)

Generic encoders write without temporary vectors, preserving bytes, errors, and bounds. Two synthetic runs on the same binary confirm all 149 identities and identical counters; no regression in allocation calls/sizes, including all 11 complete init/context assemblies. Default init: **60 → 34 allocator calls**; with hints: **102 → 59**. These are neither peak memory figures nor a full-engine/network speed measurement.

Windows passes protocol **75/0/0** and benchmark **3/0/0** on stable/1.88, stable workspace **411/0/2**, feature-off **38/0/0**, fmt/strict Clippy/build. The first benchmark fixture compilation failure is retained, corrected before both measurements. Production changed only in field encoders; decoder, requests, audio/codec, and Cargo unchanged. Historical CLI not rebuilt/invoked, budgets unchanged. Remote CI not yet run for this candidate; CI evidence below concerns its exact commits. [Contract and limitations](research/rust-wire-encoding-allocation.md) · [Local evidence and all raw values](evidence/rust-wire-encoding-allocation-windows-20261116.json).

#### Guarded SDK — native offline Linux and CI qualified

Source `aaf56df` passes all **13 CI jobs**, including eight native Windows, Ubuntu, macOS ARM64 and Intel × Rust 1.88/stable. Each example gate passes **9/0/0**; Windows workspace **406/0/2**, others **407/0/2**. The 56 command groups and 1,516,223 bytes of logs are verified with provenance, without summing reruns. [CI evidence](evidence/rust-linux-guarded-loopback-ci-20261116.json).

A separate Linux x86_64/Rust 1.88 run passes the nine tests and four cases of the real executable: no-op, public canary with SDK/codec/loopback transport, invalid input, and retained EOF with stopping/reaping of the owned child. Three external denials of opening environ without reading, three proc-root properties, and nine cleanup observations are confirmed; Windows bridge job verified separately. The 178 selected sources and binaries remain unchanged. [Contract, history, and native evidence](research/rust-linux-guarded-loopback.md#completed-linux-and-ci-qualification--16-november-2026).

No credentials, real service, phone, reservation, induced crash, or global setting. This is a development example with fixed public input, not a secrets receiver or TLS/ASR qualification: **G1 remains unsatisfied and Linux live NO_GO**. The Windows qualification below is an earlier checkpoint.

#### Application context — wire boundary clarified, no invented new feature

The static Android path reaches S3 through InputBox1203/field5, already encoded by `ProtocolConfig::preferences`. No additional missing protobuf field is established. OS collection, its gates, and formatting remain absent; server effect and runtime activation not observed. No remote trial or production change. [Trace and limitations](research/rust-application-context-boundary.md).

#### Full SDK behind simulated admission — Windows qualified (historical checkpoint)

The new example actually composes PCM/Opus, Engine, and transport to a numeric loopback peer, with init/audio/context/END order, CRC/decoding, and cleanup verified: 1017 samples, 2034 PCM bytes, 3051 samples after trim at 48 kHz. Its **9 tests** pass on Windows stable and Rust 1.88; stable workspace **406/0/2**, feature-off 38, fmt, strict CLI Clippy, and example build pass. The Windows binary checks no-op and native unsupported refusal before any read or connection. No hardening of the test process or real service call.

Only the example/its registration and CI gate are added; production and lock unchanged. This new example's native Linux mode remains to be run separately; the previous probe does not qualify it. [Contract and Windows evidence](research/rust-linux-guarded-loopback.md). G1 remains unsatisfied; no additional budget or live access inferred.

#### Linux pre-protection — offline observation, not live admission

A [standalone Rust prototype with fixed public input](../tools/reference/LINUX_PRECREDENTIAL_CANARY.md) passes its 12 simulated tests on Windows stable/1.88 and Linux 1.88. On Linux, no-op, literal acceptance, refusal, and stopping the blocked child pass; three external opens of its `environ` are denied to the unprivileged observer, with proc root ownership. Guest cleanup and Windows bridge job verified. Zero credential, service, reservation, phone, induced core crash, or modified global setting. This reference code is outside the Cargo workspace, not production integration or new core CI qualification.

These access effects prove neither actual suppression of cores nor absence of collector launch: **G1 remains unsatisfied and Linux live NO_GO**. No budget is reset. [Evidence and limitations](evidence/linux-precredential-canary-offline-20261116.json).

#### Early-rejection CI — source `1d66138` qualified

All 13 Rust/Python jobs succeed, including eight native jobs on Windows, Ubuntu, macOS ARM64 and Intel × Rust 1.88.0/stable. The four scenarios are confirmed by name in each workspace: Windows **406/0/2**, other OSes **407/0/2**. The 48 Rust groups and 1,494,689 bytes of logs are verified by command and provenance, without summing reruns. Lint, release/roots/PCM gates, and four Python jobs pass. [CI evidence for exact commit](evidence/rust-credential-rejection-early-ci-20261116.json). The local “CI not run” checkpoints below are historical; no new dictation, remote authentication, or post-revocation recovery inferred.

#### Early rejections — local qualification, without new service

Two additional tests cover a headers-only rejection before EOF while a neighboring upload is still active; explicit replacement followed by complete draining of the neighbor passes. All four scenarios pass on Windows: transport 109/0/1 on stable 1.99.1 and Rust 1.88.0, stable all-features workspace 406/0/2, fmt and strict transport Clippy. No production or release binary change. [Contract, evidence, and exclusions](research/rust-credential-snapshots.md#headers-only-rejection-during-another-upload--local-extension). The CI below covers the previous commit, not this local extension.

#### Post-upload rejection CI — source `0e5ce23`

All 13 Rust/Python jobs succeeded. Eight native jobs on Windows, Ubuntu, macOS ARM64 and Intel × 1.88.0/stable: workspace 404/0/2 on Windows, 405/0/2 elsewhere; both post-upload rejection tests pass in all eight. The 48 Rust groups and 1,492,824 bytes of logs are verified by command and provenance; not a sum of distinct tests. The four Python jobs and lint also pass. [CI evidence linked to exact commit](evidence/rust-credential-rejection-ci-20261116.json). No new TLS/real service or provisioning/post-revocation recovery inferred.

#### Authentication rejection — offline isolation

Two new loopback tests cover `UNAUTHENTICATED` and `PERMISSION_DENIED` rejections, an active neighbor, explicitly replaced dummy metadata, and the next session on the same TCP. Monitored listener, stream IDs, and exact messages check the observed absence of reconnect/replay; sources cleaned and transport closed. No production change. Windows passes 107/0/1 transport on stable 1.99.1 and MSRV 1.88.0, then 404/0/2 stable all-features workspace; fmt and strict transport Clippy pass. Counts per command, not a sum of distinct tests. Remote CI/Linux/macOS not run for this batch at this checkpoint. Rejection after full upload only: no durable provisioning or actual post-revocation recovery inferred. [Contract and limitations](research/rust-credential-snapshots.md#peer-rejection-explicit-replacement-and-active-sibling--november-16-2026) · [Local evidence](evidence/rust-credential-rejection-offline-20261116.json).

#### Native release regression — observed result, immutable reservation

A single native opportunity and capture were successfully consumed, without replay or retry: frozen controller `78ce29130fef8ac828d0dac58e7bc2183ddc8861a985ab01c2f9eb753a5bc1f4`, `REGRESSION_HEALTHY_STRICT`, process exit 0, and candidate qualification after that exit. Guards and provenance pass. The 20 offline groups, preflight, strengthened sentinel, and supervised local SAPI generation preceded the trial. The historical NO_GO remains correct for its time: the privacy barrier has since been remediated, without administration, settings change, or old ACL.

The captured release CLI corresponds to HEAD `703f0eee82f6228044115f72be5744aba7c08d6f` and production association `7327f51b89aeec537e8bf5ebaf6db3b0c25e5cef`; it is neither the benchmark binary nor a CI attestation. On a 4,34 s synthetic input, the unpaced client session takes 2,2043591 s: neither a WAN gain nor representative quality. The 223 requests are protobuf messages, not RPCs; their server-side number remains unknown. Terminal flags are inferred from the verified CLI success contract, without independent wire capture. Intent 2/active field 7 is a strict literal request, not COMPOSE/generation; token checks are mechanical and generated-message semantics remain unknown.

The [initial reservation](evidence/rust-native-release-regression-20261115-budget.json) retains `not_observed_at_reservation`; the [separate result](evidence/rust-native-release-regression-20261116.json) does not rewrite it. [Method and limitations](research/rust-pcm-channel.md#native-release-regression--result-distinct-from-reservation--november-16-2026). Linux live, phone, other intents, revocation, long incident, and access limits remain open; no perfection, durable provisioning, or Android parity.

#### Codec attribution CI — source qualification

All 13 Rust/Python jobs for commit `72fd4affa561619745e9d642500467d41364458f` succeeded. Each of the eight native jobs passes the example's new eight-test gate; no timing campaign is run in CI. This qualification covers tests, builds, and checks, without production functionality changes or proof of gain, real ASR, parity, or live authorization. The local Windows measurement remains separate and the linked native ISA is not attested. [CI evidence](evidence/rust-codec-attribution-ci-20261115.json) · [protocol and limitations](research/rust-audio-profiling.md#remote-ci-qualification-of-the-benchmark--2026-11-15).

#### Windows Opus Release — bounded local gain and VS17 check

A fresh independent comparison with the rejected CRC study measures `native_frames` 3,0–3,6× faster and the audio pipeline 67,4–71,6 % shorter on three synthetic Windows signals, with identical inter-build fidelity. Four runs only; no WAN/ASR gain or global optimum claimed. Local Debug/MSRV and all **14 CI jobs** for fix `2334b76` pass, including the explicit VS2022 check. Two ARM jobs were rerun once after GitHub capacity shortage, before any initial execution; incidents retained. [Protocol, incidents, and limitations](research/rust-opus-release-build.md) · [588 raw measurements](evidence/rust-opus-release-build-windows-20261116.json).

#### Private validation directory — local remediation

A new directory is created and accepted by the original strict check, without changing existing permissions. All 30 synthetic tests pass; the real check confirms that locking prevents an open with delete rights while held. No rename, deletion, identifier loaded, or Rambler call. Launcher integration remains separate from this admission. [Evidence and limitations](evidence/rust-private-capture-root-20261115.json).

#### Budgets and closure CI — new result

On `171ef084cc1b1e7f3b82daefdcd6bb0aadafe442`, all 13 Rust/Python jobs succeeded, including eight native jobs across four families and two toolchains. Each workspace reports 402/0/2 on Windows, 403/0/2 elsewhere (passed/failed/ignored); the four other groups report 10, 1, 3, and 38 passes respectively. The group of three verifies rejection of zero CLI budgets on the release binary. These replayed groups are not distinct tests to sum. The closure regression passes in all eight workspaces: the first bounded close accepts only success or an independent `Connect` error, then the second must succeed; ownership and no-replay assertions remain.

The Rust run for `7327f51` failed on three macOS jobs because of the old strict close expectation, and remains a historical failure, not requalification. No production change between these two commits. Real-service, phone, and Linux kernel-binding limits remain unchanged. [Detailed evidence and provenance](evidence/rust-close-ledger-zero-budgets-ci-20261115.json).

#### PCM and admission CI — expanded matrix (historical)

On `4264126852b2c03e25fc231c60f9582569a84ae9`, all 13 Rust/Python jobs
succeeded: eight native, Rust lint and four Python. Windows reports 399 passed,
0 failed, 2 ignored in the workspace per toolchain; Linux and macOS ARM64 and
Intel report 400/0/2. Both `macos-26-intel` jobs are native
`x86_64-apple-darwin`, Rust 1.88.0 and stable 1.100.0. Each native job also
passes reuse 10, exact release finalization 1 and feature-off 35; these reruns
are not added as distinct tests. Rust PyAV remains ignored by default. Python
stdlib and pytest checks are separate.

The public roots launcher reports its marker after unique selection and zero
exit from the direct test, without exposing its libtest stdout; no handshake
is claimed. Synthetic release PCM reports 1017 input samples, 3051 decoded, one
EOS and zero service calls. No new live call, private key, private audio or proof
of a minimum macOS version. The Linux live block remains open without another
attempt; phone unchanged at 0/3 phrases.
[Evidence, counts per job and provenance](evidence/rust-pcm-admission-expanded-ci-20261115.json).
The local checkpoint below retains its historical failures and limits.

#### PCM and strict admission — local qualification of November 15

On snapshot A (base `1813602` and local changes), the workspace passes 399 tests,
0 failures, 2 ignored on Windows stable and 1.88, and 400/0/2 on Linux 1.88.
However, Windows R1 remains **unqualified**: strict Clippy fails on `never_loop`
in a test, not on a runtime regression. On B, only this redundant test loop is
removed: Windows reruns the 6 progress tests on each toolchain, fmt and strict
Clippy; Linux reruns 400/0/2. No full Windows rerun or release rebuild on B;
release evidence remains that of A, despite identical production bytes.

The CLI requires a strict snapshot before connection and awaits input closure if
it rejects it; the permissive library and explicit loopback remain unchanged.
Optional PCM counters are neither hashes nor acknowledgments nor a guarantee
against loss; natural EOF remains distinct from close or error.
[Qualification and limits](research/rust-build-validation.md#pcm-progress-and-strict-admission--local-checkpoint-2026-11-15) ·
[Evidence](evidence/rust-pcm-progress-admission.json).

No new service or device trial: Linux live preflight is blocked by an observed
crash dump collector, without verified removal, loaded credentials, reservation,
RPC or system modification. This block does not mean the Linux engine is
unsupported. The phone remains at 0 arming, 0 voice, 0 phrases out of 3. The new
code is not yet qualified on macOS, committed or pushed at this point; no remote
CI for this code is claimed.

#### Native CI and public roots qualification — code `9069059`

Both CI3 runs of push `906905988c5c51d83b05b259cd642a165325b7d2` are
**completed/success**, verified by API and logs: six native Windows/Linux/macOS ×
Rust 1.88.0/stable jobs, Rust lint and four Python jobs. Stable is actually
**1.100.0**; macOS is **ARM64 only** (`aarch64-apple-darwin`), not Intel
qualification. The counts below are **per job and per command**, without summing
repetitions:

| Platform, each toolchain | Workspace | Reuse example | CLI without audio | Release CLI finalization |
|---|---:|---:|---:|---:|
| Windows |382 passed, 0 failed, 2 ignored|10|31|1|
| Ubuntu |383 passed, 0 failed, 2 ignored|10|31|1|
| macOS ARM64 |383 passed, 0 failed, 2 ignored|10|31|1|

The workspace ignores **the Rust PyAV oracle and roots smoke**. The latter is
then explicitly run in each of the six jobs by the direct helper: exactly one
ignored test entry is required, then the exact test process exits zero. The exact
marker is `Rust native roots OK: exact ignored smoke passed via direct test executable`.
The helper captures libtest stdout: no raw Rust “1 passed” summary is claimed.
The Rust PyAV oracle remains **not run** in this matrix.

This actually qualifies loading **OS public roots** and constructing production
TLS configuration with ALPN `h2`; Rust assertions requiring absence of
`SSL_CERT_FILE` and `SSL_CERT_DIR` remain unchanged. The helper directly launches
the executable with a cleaned environment copy, without having Cargo launch the
test. This is **not** a handshake with a peer, chain/hostname validation,
rotation, proof of root completeness, or an authentication, private-key or service
test.

The launcher's **23 stdlib unit tests** pass in each native job (with the audio
checker's 17 unit tests). They are also included in pytest, not to be added:
Python 3.11 passes **663 tests, 71 ignored** per OS; Python 3.14 **733 tests,
1 ignored** per OS. The **335 subtests** are reported separately. Release audio
CLI build and PCM smoke pass in all six jobs (1017→3051 samples at 48 kHz, one
EOS, zero service calls).

The two Ubuntu CI2 failures and local Linux R1 failure precede the loader:
environment precondition, exact injector not established, not a demonstrated
production roots defect. Fix `9069059` changes only the Python launcher, workflow
and its tests; Rust TLS/assertions/Cargo remain unchanged. FAILED evidence remains
preserved; local results and their limits are separate from CI3. No new native,
service or device call; no WAN gain, Android parity or provisioning/revocation
inferred. The Unicode observation `🙂`→`😎` below remains semantically unconfirmed.
[CI3 evidence, counts and hashes](evidence/rust-native-roots-ci-20261115.json) ·
[Runner history](evidence/rust-native-root-runner-history.json) ·
[Detailed validation](research/rust-build-validation.md).

#### Native CI qualification — code `1561f87` (historical)

The remote Windows/Linux/macOS × Rust 1.88/stable matrix actually completed
successfully, as did Rust lint and the four Python jobs. Rust counts below are
**per job**, not a sum of repetitions:

| Platform | Workspace | Reuse example | CLI without audio | Release CLI finalization |
|---|---:|---:|---:|---:|
| Windows, 1.88 and stable |382 passed, 0 failed, 1 ignored|10|31|1|
| Ubuntu, 1.88 and stable |383 passed, 0 failed, 1 ignored|10|31|1|
| macOS, 1.88 and stable |383 passed, 0 failed, 1 ignored|10|31|1|

The additional positive descriptor check explains the extra Unix test. The release
PCM smoke also passes in all six jobs (1017→3051 samples at 48 kHz, one EOS,
zero service calls). PyAV remains ignored in this CI. Python 3.11 passes 640
tests with 71 ignored per job; Python 3.14 passes 710 with one ignored. The 259
separately reported subtests are not added to these totals.
[Results, versions and exclusions](research/rust-build-validation.md) ·
[CI evidence](evidence/rust-remote-ci-20261115.json).

This run removes the macOS native build/test and CI block for this commit, not
the limits on real TLS/Keychain, Android parity, unestablished intents,
provisioning/revocation or WAN performance. Earlier “not run” notices remain
historical snapshots, not current status.

One additional synthetic Rambler call completes cleanly: selection `🙂` is
replaced by `😎`, with before/after preserved and the entire source cleaned up.
The result does not reproduce the word “eleven” obtained with this audio in
another context: this is a structural Unicode observation, not validation of
semantic quality or determinism. Published UTF-16 ranges are derived offline;
the CLI does not exercise the proposal API or insertion into an editor.
[Observation and limits](evidence/rust-native-non-bmp-selection-public.json).

#### R4 reliability checkpoint — code `b0591f5`

The R4 production snapshot, retained as a historical reference, passes **382
workspace tests, 0 failed, 1 ignored** on Windows stable 1.99.1, Windows/MSVC 1.88
and Linux/WSL 1.88. The ten example tests, 31 CLI tests without audio and the real
release CLI finalization test pass on Windows stable and Linux. Windows passes
its 26 steps, Linux its 16 phases; Linux exclusions remain explicit. Strict
Clippy/fmt/rustdoc, 716 Python tests and the separate PyAV oracle (57 containers)
are qualified on Windows, not inferred for Linux.

This batch hardens admission before audio reading, persistent and cleanup errors,
TCP pacing, finalization and the success commit. TLS roots setup is offloaded from
the async runtime; an already started loader remains non-interruptible. Contracts,
limits and evidence are in [R4 validation](research/rust-build-validation.md#offloaded-tls-setup--r4)
and [the lifecycle](research/rust-reliability-lifecycle.md).

No additional service/device call or WAN gain is inferred from this batch. macOS
and remote CI remain not run; static macOS inspection is not native qualification.
Android parity, unestablished intents and provisioning/recovery after revocation
remain open. The following sections retain their **historical snapshots**: their
counts and hashes do not identify current code.

#### Focused tests / CI follow-up — November 15

The cleanup helper replaces the macOS no-op with a device/inode audit; Linux adds
the positive 0→1→0 check, Windows retains exclusive opening. Initial Windows passes
3 tests on stable and 3 on MSRV, strict audio Clippy, 1 real release CLI test and
PCM smoke, but remains **FAILED**: fmt alone fails among 13 steps. Linux passes its
partial scope (11 phases, 4 cleanup tests, 1 release CLI, PCM smoke; Clippy
**NOT_RUN**, not installed). After only swapping the order of two cfg imports,
Windows passes 6 focused steps, including fmt, 3/3 tests and Clippy; no release
rebuild or post-format Linux rerun is inferred. Initial failures and two failed
harness preparations before commands are retained, not requalified. The 382 R4
workspace tests above remain historical: no current complete 383-test suite is
claimed.

The written OS/MSRV matrix adds release audio CLI build, loopback finalization with
the real executable and release PCM smoke. **Remote CI, macOS and native 32-bit
Linux NOT_RUN**. Production/manifests/lock unchanged; no production leak
proved/fixed, no engine/WAN gain or parity/provisioning inferred.
[Focused validation and exclusions](research/rust-build-validation.md#focused-platform-gate-follow-up--2026-11-15) ·
[Redacted evidence](evidence/rust-platform-gates.json).

#### PCM channel through the engine: five qualified loopback tests

Published November 15, **runs of November 14, 2026**: the two CLI files
`pcm_channel_engine.rs` (2 tests) and `pcm_engine_failure.rs` (3 tests) compose
the real PCM channel, encoder, engine and transport against tonic. The first checks
the Ogg bytes received by the peer, CRC/decoded duration, Opus packet equality
and init/audio/context/END order; the second covers typed abandonment, waking a
truly blocked reservation and early END/OK rejected as success. **In the
abandonment case, only arrival of an Ogg page/header is proven, not arrival of
audio samples.** S3 responses remain synthetic.

**310 workspace tests pass, 0 failed, 1 ignored**, on each of Windows stable,
Windows/MSVC 1.88 and Linux/WSL 1.88. The 10 example tests and 30 feature-off
CLI tests pass on Windows and Linux. Strict Clippy, formatting and release CLI
builds with/without audio pass on Windows; Linux covers the release audio CLI
build. The release PCM example is built and its smoke passes on both OSes
(1017→3051 samples after trim, 1 EOS). The 96 entries in the Windows/Linux source
manifests exactly match current files; the final names of the two tests are
verified in all three workspace logs. No new production behavior or dependency,
zero service/device calls.

Two final static counter-reviews find no concrete blocking defect; additional
limits for each file remain explicit. The first review, blocked by a wrong target
name, is not counted as successful. [Evidence and provenance](evidence/rust-pcm-engine-loopback.json) ·
[Contracts and limits](research/rust-pcm-channel.md). No additional remote
dictation, WAN gain, perfection or Gboard parity is inferred from these tests.
Following sections retain their historical counts.

#### Two HTTP/2 boundaries fixed and reproduced locally

A workflow covers 10 scopes in parallel, with 2 cross-checks and a synthesis
(13 Sol agents). Two separate suites then establish RED/GREEN and counter-review:
valid mixed-case gRPC MIME type now accepted, five metadata names forbidden in
HTTP/2 explicitly rejected at validation. Subtype boundaries, duplicates, limits,
encodings, safety and valid credentials remain preserved; no header is silently
removed. **305 workspace tests pass** on Windows stable 1.99.1, Windows/MSVC 1.88
and Linux 1.88, with one ignored oracle and identical hashed sources. 10 example
tests, 30 feature-off CLI tests, strict Clippy/Windows formatting, release builds
and PCM smokes pass. Zero new Rambler/device calls, no WAN gain claimed.
[Reproductions, contracts and limits](research/rust-http2-boundaries.md).

#### Producer PCM input and explicit normal stop

`pcm_channel()` adds a single producer with a one-chunk queue and cancellable
reservations without premature handover of bytes. `finish()` announces normal EOF
after draining; Drop without finish is an erroneous abandonment. Close wakes
blocked producers, even if its future is not polled. Microphone capture remains
out of scope; codec profile and service are unchanged. Acceptance of a chunk is
never presented as acknowledgment of dictation. **300 workspace tests pass** on
Windows stable 1.99.1, Windows/MSVC 1.88 and Linux 1.88 (one ignored oracle),
including 19 new PCM cases. The 10 observer tests and 30 feature-off CLI tests
also pass; strict Clippy/Windows formatting and native release builds pass. The
offline smoke checks 1017→3051 samples after trim, one EOS, zero service/device.
The initial formatting failure is retained.
[Contract, offline example and limits](research/rust-pcm-channel.md).

#### Native metadata: per-session snapshots

Two new tests cover the shared provider with rotation then removal during three
simultaneously open RPCs on one loopback TCP: exact duplicates and binary, no old
value on the empty session, one provider call per session through closure.
Sensitivity is checked after header conversion **before sending**, not inferred
from the peer's HPACK decoder. The transport suite passes **63 tests** on Windows
stable 1.99.1, Windows/MSVC 1.88 and Linux 1.88 with identical hashed sources;
strict Clippy and Windows formatting pass. This is not a new full workspace run.
No production behavior change, no service/device call and no renewal or access
after revocation demonstrated.
[Contract and focused validation](research/rust-credential-snapshots.md).

#### Android development reference: preparation, not established parity

Official PixelBoard is installed alongside Gboard on the explicitly authorized
test phone. The zero-permission [original inert editor](../tools/reference/android_editor/README.md)
passes 7 case/selection checks and 4 tracking/reset checks on device. No real
submission, personal data or Android engine dependency added. Initial preflight
remains inconclusive. A second preflight with active IME and a minimal
constructor/read check reproduce an **ART SIGSEGV before any audio arming**, with
successful restoration. SDK reading alone, a laboratory method hook and a read
hook alone each hold for 20 s; the exact cause of the crash remains unestablished.
The 3 phrases remain uninjected. The alternative that would neutralize
`startRecording` is rejected statically, without falsifying state. Experimental
audio/UI sources are preserved with 29 passing Node tests and 5 Python assembler
tests, not presented as a functional injector.
[Status, reproducible sources and limits](research/android-reference-validation.md).

#### Native observation of partials

The validator exposes `--observe-partials`: per-session counters and framed
stable/provisional/rendered hashes, distinction before/first state1, without text
or promotion to final. 10 focused tests pass on Windows stable, Windows/MSVC 1.88
and Linux 1.88; strict Windows stable Clippy passes. The first Windows MSRV attempt
fails in CMake configuration in a deeply nested target, then passes with the normal
Cargo target, without source change; failure preserved. No production API/dependency
changed. [Method](research/rust-native-partials.md). One synthetic native trial
at real-time rate completes cleanly with verified complete audio, but 20 responses
and **0 field4**: real partial rendering/pre-state1 progress remain
**inconclusive**, without a second call or retry. The first response is not treated
as a partial. [Evidence](evidence/rust-native-partials-public.json).

#### Decoding: defensive allocations

**279 workspace tests pass** on Windows stable 1.99.1 and 1.88, and Linux 1.88
(R9), with one audio oracle ignored. The 4 example tests and 30 CLI tests without
audio pass on Windows stable and Linux; strict Clippy and Windows formatting pass,
as do Windows release builds with/without audio and Linux audio.
[Validation details](research/rust-build-validation.md).

The [decoder refinement](research/rust-backend-decode-defensive-capacity.md)
reduces measured allocations from 39 to 24 per response, without an API change.
The first child is validated before allocating the list; the initial hint inspects
at most four fields, before filtering. The V2 variant preallocating from an
unvalidated count was rejected, with its measurements retained separately. Local
CPU medians are favorable, but some sparse temporary peaks increase by about
18.5%: neither universal memory optimum nor WAN gain is claimed.

#### Consolidation suite: memory, file inputs and S3 options

**274 workspace tests pass** on Windows stable 1.99.1 and Rust 1.88, and Linux
1.88 (snapshot R6). The reuse example passes 4 tests, the CLI without audio 30;
strict Clippy, formatting and release builds with/without audio pass. macOS and
the remote CI matrix remain not run. Earlier network evidence is not reassigned
to the new binary. After offline tests, three bounded synthetic calls accepted
fields 7/40 omitted, false then true, on one engine: complete PCM hashes, zero
retry and three identical text hashes. [Evidence](evidence/rust-recognizer-options-public.json):
request acceptance, not filtering/punctuation effectiveness. Secondary routing
was not tried.

- [Shared raw](research/rust-backend-profiling.md): identical payloads and
  **−70.82% logical retention** (12,635,008→3,686,912 bytes), text/unknowns
  preserved. Independent review without a concrete finding, performed by Sol
  according to metadata (former Opus attribution corrected); this is not a
  guaranteed RSS reduction or WAN gain. Rust 0.1 API: `raw`/`active_raw` become
  `Bytes`, migration and child retaining its root documented.
- [Regular files](research/rust-file-inputs.md): Unix FIFO and Windows pipe could
  block before the RPC timeout started. Nonblocking Unix open and descriptor type
  check; `GetFileType` check on Windows. RED retained, GREEN on both OSes, zero
  TCP/RPC on rejected inputs.
- [Three RecognizerInfo options](research/rust-recognizer-options.md): offensive
  words preference field7=0/2, auto-punctuation field40, secondary routing field64.
  SDK and CLI, `None` defaults leaving goldens identical; server effect untested,
  no COMPOSE switch or safety-check change assumed.
- Legacy selection alone was already encodable through the flat text field; no
  proposal confuses this projection with the original window.

Other boundaries remain those of the matrix: unestablished intents, local Android
services/models, access after revocation and macOS not run. No other engine or
public Gemini API substituted. Code is not declared mathematically perfect and
the historical long incident is not declared resolved.

#### Native backend consolidation — milestone fdc9fd9

**262 Rust tests pass** on Windows stable 1.99.1 and 1.88.0, as well as Linux x86_64/WSL Ubuntu 24.04 with Rust 1.88. The `session_reuse` example passes 4 tests and the CLI without audio 27 tests; strict Clippy and formatting pass, including release builds with and without audio. The independent PyAV oracle separately passes 57 containers, and the Python reference/tools pass 656 tests in 13.82s. The remote CI matrix was not run; macOS remains unexecuted. [Builds](research/rust-build-validation.md).

- Results: `language`/`spoken_language` fields fixed; containers 3/5/7 and full fallback decoded with merging/budgets/unknowns preserved. Safety covers all these candidates, without selecting them arbitrarily.
- API: explicit rendering policies with provenance, stable/provisional partials, bounded candidate collection with ordinal, SEND/CLEAR classification without side effects. Proposals still controlled by context/revision/UTF-16; no invented COMPOSE.
- Reliability: independent GOAWAY/reset/driver errors preserved. Two lost engine errors reproduced RED then fixed: driver overwritten by session error, and secondary cleanup lost after primary audio error. Thirteen new safeguards cover cancellation/concurrency/reaping; errors must still be retrieved by successive `close()` calls until `Ok`, without audio replay.
- Ogg CLI: progressive, bounded CRC/headers/continuation/sequence/granules/EOS, without decoding or changing bytes. Valid empty EOS also checked with PyAV; OpusTags must start on the second page. Explicit application limits.
- Performance: one exact allocation to serialize audio, identical bytes; [96 CPU measurements and 40 loopback sessions](research/rust-backend-profiling.md). No robust overall pipeline/WAN gain inferred from micro-measurements.
- Real service: [three synthetic cases on one engine](evidence/rust-native-session-reuse-public.json), then [three natural EN/FR clips, 18.885s total](evidence/rust-native-natural-public.json), PCM consumption/EOF/close/hash verified. The three hashed natural texts match historical Python texts; retrospective comparison, not a new quality guarantee or speed benchmark between implementations.

[Detailed validation and provenance](research/rust-backend-validation-2026-11-14.md). The first reuse batch remains inconclusive (`eleven`/`11`); the historical 282s incident is not declared resolved. The verified S3 backend is operational, **not the entire Gboard application or an engine declared perfect**: COMPOSE/unestablished intentions, local Android services/models, recovery after revocation and macOS execution require separate evidence/access. Microphone/GUI/insertion remain outside the initial scope. No other ASR/Gemini API substituted.

#### Parity, robustness, profiling and MSRV — milestone e427fb1 (November 13)

**186 Rust tests pass** on the integrated tree with stable 1.99.1 and Rust 1.88.0, Windows x86_64/MSVC; an independent audio oracle is ignored by default, passed separately on 57 containers. Formatting and Clippy all targets/features pass. The Windows/Linux/macOS × 1.88/stable CI matrix is written but **not run remotely**. [Build validation](research/rust-build-validation.md).

The [parity matrix](research/rust-feature-parity.md) links APK symbols, Rust APIs and network evidence without confusing enums with validated capabilities. The parity review found an ignored safety marker `hypothesis.field30`: decoding 0/1, unknown preservation and rendering/proposal rejection are now tested. The CLI displays no candidate or context for `safety_blocked` or `unrecognized_safety`. Final targeted review found no issues; no real blocked case was provoked, no new Google request or device interaction in this batch. Rejections outside VoiceEdit and on unknown values are explicit hardening.

New generative tests exercise 128 proto2 sequences and 343 safety traces, exact limits, truncated occurrences, gRPC partitions and interrupted concurrent closes. `Presentation` distinguishes result types without simplifying their text or proving overall completion. `EncodedAudioProgress` separates encoded chunks/bytes, audio EOF and cleanup from RequestSource flags; these are neither PCM counters nor server acknowledgments.

The [offline release ABBA profiling](research/rust-audio-profiling.md) measures encoder, mux and pipeline on 5.001s synthetic signals. Grouped median total 45–134ms, mux 14–81µs; **no robust gain from the CRC candidate**, rejected. Codec, quality, bitrate and production audio sources remain unchanged. This benchmark establishes neither a global optimum nor network acceleration.

The Python/tools suite passes 656 tests in 11.20s, in a bounded rerun with stack diagnosis. As in an earlier batch, a silent run had stalled around 605 and was stopped after 120s; the verbose rerun did not reproduce the stall. This observation is neither hidden nor attributed to an unproven cause. Python 0.3 reference remains frozen: the new safety policy is explicitly ported to Rust, not retrospectively claimed for Python.

#### Rust consolidation: diagnosis without false success — milestone 4fdf29e

The Rust core comprises protocol/transport, sessions/results, native WAV/PCM and CLI. **163 integrated Rust tests and 654 strict Python reference/tools tests pass**, including formatting/Clippy all targets/features and audio release build. The independent 57-container oracle passed separately; the OS/MSRV matrix is not inferred from these Windows runs.

After the stopped long batch, a new binary adds only typed structural observations of `IncompleteSession`, without changing success conditions, codec or introducing replay. The [120s qualification stage](evidence/rust-diagnostic-endurance-120-public.json) succeeds, then [one 300s test](evidence/rust-diagnostic-endurance-300-public.json) finishes in 290.904s with 14,422 requests and the same ordered known finals as the first healthy long test. **The 282s incident is not reproduced, not declared resolved.**

Independent review, with nine targeted loopback checks, demonstrates no client defect consistent with this incident and recommends against multiplying service calls. The original failure's flags/timeline are missing and cannot be recovered afterward; this prevents attributing its cause to the client or service. [Observations, flag semantics and limits](evidence/rust-incomplete-diagnostic-followup.json). The next authorized-use incident will have diagnostics, without accepting truncated results or replaying them automatically.

#### Native Rust audio and measured endurance — milestone eb87cc5

PCM input is ported to `rambler-audio`: strict PCM16LE mono 16k WAV, static libopus 1.6.1, VoIP/VBR 24kbps, 20ms frames/pages by default, flush and EOS without losing final samples. The `dictate-wav` CLI is optional (`--features audio`). **156 Rust tests pass**, plus an oracle ignored by default run separately on **57 Ogg containers**: exact decoded duration at 48k and 16k. The minimal Ogg build has no normal Opus/CMake dependency. The extended Python reference/tools suite passes **653 strict tests** in the bounded rerun. A first run had stalled and was stopped; the rerun with stack diagnosis enabled did not reproduce the stall.

[Four additional successful calls](evidence/rust-pcm-functional-public.json): accelerated then progressive English PCM with the same text, intention 3 rewrite and `ten`→`eleven` selection with emoji preserved. This evidence adds to the first three Ogg calls, without becoming a general quality measure.

The [Rust 118.682s stage](evidence/rust-endurance-120-public.json) finishes in 119.929s, with all markers rendered, healthy RSS/CPU coverage and sampled maximum RSS 11,898,880 bytes (~11.35 MiB). At the [288.352s stage](evidence/rust-endurance-300-public.json), the first test finishes in 290.905s (max RSS 11,862,016 bytes; child CPU at last sample 12.984s), but **the second fails `incomplete` after 282.202s**; the third is not launched. The precise cause of this early end is not established by the old error output. No partial result is reported as successful and no automatic replay masks the failure. Diagnostic analysis is separate from this assessment.

Each Rust benchmark session creates a fresh process and channel, unlike the three Python sessions on a reused channel. RSS/CPU are sampled, not exact peaks/executions; no speed comparison or proof of absence of leaks is inferred from these figures.

[Real Windows console Ctrl-C](evidence/rust-windows-ctrl-c.json) is verified on a **loopback** RPC, in an isolated console: exit 130, RPC cancelled, no result. The first test driver timed out; its console isolation/inheritance was fixed without changing the binary. The reproducible driver is versioned at `tools/validate_rust_ctrl_c.py`. [Rust guide](rust.md).

#### First validated Rust slice

The native workspace contains `rambler-protocol`, `rambler-transport`, `rambler-engine` and `rambler-cli`. **121 integrated Rust tests pass**, as do formatting, Clippy all targets with warnings denied and release compilation. The **643 Python reference tests** also pass (623 from the 0.3 freeze and 20 from the result policy oracle).

**Three successful real Rust calls on Windows**: English dictation, instruction on context `ten`→`eleven`, French dictation. The Rust process was launched with PATH limited to Windows system directories, without Python/FFmpeg; Python development orchestration is not a binary dependency. Verified TLS, unchanged S3Bidi service/Rambler routing, declared platform `Rust`, S3 completion and gRPC OK, no automatic replay. [Inspected evidence](evidence/rust-first-slice-public.json) with hashes of binary, lockfile and measured sources; no opaque payload or service identifier published.

Loopback tests cover cancellation of an upload blocked by flow control with reset of only that stream, neighboring sessions preserved on one connection, late errors, deadline, framing, HTTP/RST mapping compared with tonic, exactly-once cleanup and preservation of multiple background errors. Explicit close remains necessary to await asynchronous cleanup.

This evidence uses **already encoded Ogg/Opus**: native PCM port, Rust CPU/RSS/endurance measurements and real execution on other OSes are separate batches, not successes inferred from Python tests. No overall Rust speed gain is claimed. See [the Rust guide](rust.md) to build, run and integrate the engine; GUI/microphone/insertion remain out of scope.

#### SDK 0.3 continuation and Rust reference

SDK 0.3 integrates initial context options and **623 strict local tests** pass. Default protocol goldens remain identical. Python is the compliance reference for the requested port: independent Rust library and CLI first; Tauri, microphone and system integrations remain later and outside this batch. See the [Rust porting contract](architecture/portage-rust.md), [desktop recommendation](architecture/application-desktop-multiplateforme.md) and [refinement](refinement.md). The [0.3 reference wheel](evidence/reference-0.3-wheel-smoke.json) is built and verified in a clean venv without PyAV: imports, module equality, rewrite fixture and hints/secondary language wire pass offline.

**35 additional successful transports**: 13 natural, eight composition tests, four endurance and ten hints tests. This count does not mean 35 validated features. The ten additional natural clips (7 EN, 3 FR; 72.825 s) yield 3/109 and 3/79 normalized WER errors, with formatting and verb-tense differences; three progressive tests recover exactly their accelerated text. General quality remains open. [Provenance](evidence/natural-expansion-inputs.json), [quality](evidence/natural-expansion-quality.json), [progressive](evidence/natural-expansion-realtime-public.json).

Synthetic endurance succeeded for one 118.682 s session in a first process, then three 288.352 s sessions on one channel in a second process, at simulated rate, with complete source, markers and identical ordered finals across these three long sessions. Reports show sampled RSS, CPU and duration, without proving absence of leaks; these measurements are on the **0.2 baseline before hints**, with module hashes retained. [120](evidence/endurance-120-public.json) · [300](evidence/endurance-300-public.json).

The ten [hints](evidence/input-hints-public.json) tests establish no general gain: one name changes with a punctuation/conjunction change; British preference and secondary language have no demonstrated isolated benefit. Tested voice invocations remain literal; COMPOSE 4 is not observed.

#### Refinement 0.2.0 — historical milestone of 2026-11-13

Local branch: `feat/engine-refinement`, built on `1f52c94` without modifying the published branch. **438 local tests pass** with asyncio debug and warnings treated as errors. The [final 0.2.0 wheel](evidence/refinement-wheel-smoke.json) is built and installed in a clean venv without PyAV: public imports, modules identical to source and derived fixture rendering verified offline. Initial measurements carry checkpoint `4b96696`; the rewrite follow-up also records hashes of the new rendering code.

- Two defects fixed: merging repeated singular protobuf messages (including the same oneof member) and asynchronous producer cleanup interrupted by RPC completion. Neither was observed in the 42 historical tests; offline/loopback reproductions and canonical verification added.
- Bounded final result API, explicit ambiguity, side-effect-free replacement proposal and snapshot/revision check: [guide](refinement.md).
- **20/20 grid sessions** at 20/40/60/100 ms succeeded, identical decoded PCM and all ordered final updates. Default 20 ms retained; 60 ms saves about 26% of Ogg with about 17 ms additional final median on this benchmark. No new speed optimum established; [distributions and limits](performance.md).
- Five functional cases completed transport; four unique renderings on the first pass. Real rewrite, intention 3/field 3, revealed an API limit then succeeded in a new test after the rendering fix. Initial 4/5 report is retained, not rewritten.
- Natural English 3.505 s clip: WER 0/8 words, without generalization. Complete 40.56 s dictation accelerated then at simulated rate (collection 41.454 s), same corrected text and final reference retained. Selection with emoji succeeded. The composition request remained dictation of the instruction, not validated composition.

The **27 new complete transports** are the 20 measurements, five functional cases, one progressive long dictation and rewrite follow-up; they are not 27 first-try rendering successes. Public reports are redacted, captures with opaque identifiers remain outside git. No new remote CI or Google/Linux execution is claimed for this local refinement.

#### Real milestone of 2026-11-12

**Responses from the real Rambler path were obtained by a native Windows client without Android.** This is no longer just a static lead or a gRPC connection. No phone, Google account, OAuth or Zwieback used for these tests.

Path exercised: `speechs3proto2-pa.googleapis.com` / `S3Bidi`, service `beyond-a2a-recognizer`, routing `gboard_gemini_v3_streaming_voice_edit_mul`, Ogg/Opus 16k mono, late context, S3 completion then gRPC OK status.

#### Capabilities and level of validation

| Capability | Current evidence |
|---|---|
| English and French dictation | Real service, controlled synthetic corpus |
| Spoken correction “ten… no, eleven” | Real service, corrected text |
| Editing existing text by instruction | Real service, context before cursor and edited history returned |
| Structured intentions and edit results | Real `VoiceEditingResult`, intentions 2 and 3; historical cases and rewrite independently decoded |
| Polite rewrite in context | Real service, intention 3/field 3 rendered by SDK; no replacement proposal for this intention |
| Natural EN/FR speech | First EN clip then ten additional clips (7 EN, 3 FR), WER and limits documented; three cases also progressive |
| Long dictations | 40.56 s then 118.68 s stage and three 288.35 s sessions; complete source, markers and identical long finals, limited endurance |
| Hints and secondary languages | Initial fields exposed and ten successful transports; no overall gain or universal preference effect demonstrated |
| Selection with emoji | Real service, non-BMP context retained and `ten`→`eleven` selection |
| Three-part context / selection | Real service: `ten`→`eleven`, delayed context called after audio EOF, before/selection/after returned |
| Successive sessions / reused channel | 21/21 accelerated benchmark sessions succeeded; identical audio and text |
| Progressive feed at simulated rate | 12/12 sessions succeeded: six FFmpeg, six native encoder; identical decoded PCM between pages in each experiment |
| Cancellation, concurrent isolation | Real service: source closed after cancellation, then concurrent English/French sessions on same channel without mixing |
| Late errors and deadlines | Loopback gRPC tests through transport and engine; no error injection into the real service |
| Unknown protobuf / new fields | Preserved, including a field 5 observed in the service but absent from the APK's optimized schema |
| PCM16 input with optional native encoder | Validated on the real service through CLI: PyAV/libopus, no FFmpeg executable |

At the previous milestone, **356 local tests passed** in asyncio debug mode, warnings treated as errors; Python wheel built and installed in a clean environment without PyAV. [Milestone 0814813 CI](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43304009641) succeeded on Windows/Linux and Python 3.11/3.14 (local network tests only).

The first probe, three functional cases, 21 accelerated sessions, 12 progressive, one PCM CLI, three selection/concurrency cases and an installed wheel smoke test represent **42 successful complete sessions**, plus **one successful voluntary cancellation**. The initial anonymous diagnostic was rejected; it is not counted as a successful session. Do not extrapolate this corpus to all intentions, languages, voice qualities or network conditions. `transcript` field responses may already be corrected or contain the complete history: this is not a guarantee of verbatim audio.

#### Evidence and performance

- [Observations and conditions](evidence/README.md)
- [First complete call](evidence/reconstructed-default-session.json)
- [Correction, context, French](evidence/functional-cases.json)
- [Accelerated benchmark](evidence/performance-baseline.json)
- [FFmpeg source buffering](evidence/realtime-paging.json)
- [Progressive native encoder, hashed source and commit](evidence/native-realtime.json)
- [Selection, delayed context, cancellation and concurrency](evidence/lifecycle.json)
- [Installed wheel, local configuration, no APK at runtime](evidence/installed-wheel-smoke.json)
- [Three inspected public real payloads](../tests/fixtures/live_synthetic_responses.json)
- [Refinement native 20/40/60/100 ms grid](evidence/refinement-page-grid.json)
- [Initial redacted functional corpus, historical 4/5 assessment](evidence/refinement-capabilities-public.json)
- [Progressive 40.56 s dictation, redacted copy](evidence/refinement-long-realtime-public.json)
- [Rewrite after rendering extension, redacted copy](evidence/refinement-rewrite-public.json)
- [Final 0.2.0 wheel, installation and offline smoke without PyAV](evidence/refinement-wheel-smoke.json)
- [Derived and redacted rewrite fixture, not original](../tests/fixtures/live_rewrite_response.json)

The **33 responses from the first three functional cases** were redecoded independently of our Python codec; corresponding public fixtures contain only controlled synthetic text and inspected numeric fields. Later evidence (selection, lifecycle and native benchmark) comes from the engine codec and is not presented as a new independent binary inspection. Remaining captures stay local, outside git. No application metadata in public observations.

Preliminary findings: batch 8192 reduces messages 67→5 and protobuf payloads by about 6.5%, but no clear final latency gain in the small benchmark. Ogg 20ms pages instead of 1000ms: median first-text delay after source exhaustion 935→325ms, with identical decoded PCM and more Ogg bytes. The native benchmark on commit 0814813 reproduces the gain: **990→328ms** (S3 completion: 991→330ms), three repetitions per setting, no FFmpeg process. These durations are observed client-side, not server compute time; both experiments use a simulated source, not a real microphone. Do not mix the two producers' EOF clocks (FFmpeg encoded stdout versus native PCM reader).

#### Dependencies and limits

- Python>=3.11, grpcio; the optional native input codec requires Python>=3.12 and PyAV. Already encoded streams do not depend on it.
- Three application metadata values must be configured for authorized use; values never embedded in the repository. No automatic credential provisioning. No manually copied temporary session token was needed during these tests.
- Static metadata has no observed refresh. Revocation or a future restriction must be handled explicitly, not bypassed or retried.
- Zwieback: branch conditional on Jetson and `jetson_send_zb`, default false. Initial decompilation gave an incorrect additional condition on the separate application; bytecode allowed it to be corrected. See the auth note.
- Non-contractual internal service: schema/routing/availability may change.
- No raw PCM labeled Opus, alternative Gemini API, graphical application or microphone/environment integration in this scope.
- Asyncio cleanup assumes cooperative producers. Partial results do not constitute session completion. S3 errors and final gRPC status are separate.
- Beyond the limited natural corpora and 288 s synthetic sessions, representative quality, longer endurance, actually lost transport and credential renewal after revocation remain unvalidated.

#### Useful history

Repository initially empty; first documentation/wire batch pushed in `4745be3`, branch `feat/rambler-engine`. Static PixelBoard 1.3.4 APK analysis (SHA in research notes), targeted bytecode verification, then real server evidence. The proposed Xiaomi was not used. Technical documentation and measurements remain versioned with code to enable contributions.
