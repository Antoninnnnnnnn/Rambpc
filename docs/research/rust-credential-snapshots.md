# Native metadata: snapshots, rotation and withdrawal

## Strict CLI preflight — local qualification

On the normal endpoint, the CLI first opens the input, then requires the three exact fields `RAMBLER_API_KEY`, `RAMBLER_ANDROID_PACKAGE` and `RAMBLER_ANDROID_CERT` before `Engine::connect`. Any absence, empty value, partial configuration, invalid Unicode value or `Metadata` error is rejected locally; on failure, closure of the opened input is awaited. This does not guarantee absence of I/O during opening. The immutable `Metadata` snapshot is passed to `SessionOptions`, without rereading the environment. Explicit loopback does not inspect these variables.

For the library, `EnvironmentCredentials` remains a permissive provider: any missing value produces empty metadata. The explicit strict choice is `EnvironmentCredentials::required_snapshot_with(&self, lookup: impl FnMut(&str) -> Result<String, std::env::VarError>) -> Result<Metadata, TransportError>`; `required_snapshot(&self)` reads the environment. Snapshots and diagnostics remain redacted on display. This preflight demonstrates neither real authentication nor renewable provisioning. Local offline qualification distinguishes snapshots A and B; see [results and limits](rust-build-validation.md#pcm-progress-and-strict-admission--local-checkpoint-2026-11-15) and [evidence](../evidence/rust-pcm-progress-admission.json).

## Historical contract exercised

Two additional offline safeguards verify the existing transport; no API, production implementation, dependency or retry policy is changed. Values are invented, the real environment is never read and no Google service or device is contacted.

- `crates/rambler-transport/tests/credential_sessions.rs` opens **one loopback TCP connection**. The server drops its listener after that acceptance. Three RPCs use the same transport and mutable `MetadataProvider`: generation `one`, generation `two`, then an empty snapshot.
- The server holds all responses until it observes the three requests. Previous sessions are therefore still active when the provider changes. Each snapshot is verified at reception: two ordered `authorization` values, invented key and exact binary bytes; the third request contains none of these values. Control headers remain unique.
- Sources are empty: zero payload/audio sent. Each session requires upload EOF, source close and gRPC OK status. The provider counter must remain exactly three, and the server must have received exactly three requests.
- The unit test `duplicate_text_and_binary_values_keep_sensitive_flags_when_converted_to_headers` checks sensitive markers on text duplicates and binary **before sending**, after the same `tonic::Request`/timeout/header conversion as the writer.

## Oracle limits

The `h2` 0.4.20 HPACK decoder does not propagate the “never indexed” marker to `HeaderValue::is_sensitive` (`hpack/decoder.rs`, `LiteralNeverIndexed` branch, explicit TODO). An initial assertion on received values failed for this reason; it was removed, not turned into a production defect. The two final checks therefore separate peer-side fidelity/isolation and pre-serialization sensitivity. **No independent inspection of HPACK bits is claimed.** An initial compilation of the new harness also used an unnecessary generic argument to `h2::server::handshake`; this test error was corrected.

This test establishes neither automatic renewal, provisioning nor recovery after revocation. The test provider explicitly replaces already available snapshots. The connection is deliberately plaintext **loopback**: this is not a new TLS or remote authentication qualification. No performance or endurance measurement or universal isolation guarantee.

## Reproduction

```sh
cargo test -p rambler-transport --locked --offline
cargo +1.88.0 test -p rambler-transport --locked --offline
cargo clippy -p rambler-transport --all-targets --locked --offline -- -D warnings
cargo fmt -p rambler-transport -- --check
```

The [targeted evidence](../evidence/rust-credential-snapshots.json) retains hashed sources, platforms, results, hashed logs and limits. This is the transport suite, not a new run of the whole workspace or Rambler service.

## Raw HPACK inspection — local Windows S1 qualification

The “no independent inspection” finding above describes the historical checkpoint, not this new check. Qualified source: the new file `crates/rambler-transport/tests/credential_hpack_wire.rs`, measured uncommitted on baseline `cb7c707`; remote CI still pending at this local checkpoint. No production, Cargo or CI change is included.

The independent oracle traverses frames and decodes HPACK and Huffman from RFC 7541 tables, without delegating the privacy decision to the peer decoder. On a native loopback capture it exactly associates fields and streams of **three concurrently open RPCs**, with rotation of fictional credentials then an empty snapshot, on one TCP connection per fixture. It requires never-indexed for sensitive fields, explicitly accepts static name 23, checks the absence of sensitive entries in the dynamic table and exact value order for each name. Each of the ten sensitive fields in the actual test capture undergoes two targeted mutations, **20 mutations** total, and relevant incomplete prefixes are rejected. No audio, service or real credentials.

The **four targeted tests** pass on Windows stable 1.99.1 and MSRV 1.88; transport **119/0/1** on each, stable workspace all features **422/0/2**, feature-off **38/0/0**, fmt and strict transport Clippy all features pass. These are replayed groups, not counts to add together. QA1 failed only on a Clippy lint in the hexadecimal fixture helper; the fix limited to that helper precedes successful QA2 and the initial failure is preserved. The 187 selected sources are unchanged before/after QA2. [Local evidence](../evidence/rust-credential-hpack-wire-windows-20261116.json).

```sh
cargo test -p rambler-transport --test credential_hpack_wire --locked --offline
cargo +1.88.0 test -p rambler-transport --test credential_hpack_wire --locked --offline
cargo clippy -p rambler-transport --all-targets --all-features --locked --offline -- -D warnings
```

This closure of the inspection gap applies only to the observed S1 trace. Reset remains a finite terminal observation, without proof of absence of unread TCP bytes; the tee's actual overflow path is not exercised, only the flag's negative control is. S2 and CONTINUATION, eviction, table updates, TLS, general HTTP/2 compliance, real service privacy, universal isolation, performance and durable authentication remain outside qualification. No new release CLI build or real call; G1 Linux remains unmet and live NO_GO, Windows MAX1 budget already consumed and phone unchanged.

### Later CI qualification of the four HPACK tests

The four HPACK names are confirmed in the eight successful native workspaces of source `8cb381e8d26d448db80481d4aab9472aee3b096c`: Windows, Linux, macOS ARM and Intel × Rust 1.88/stable. The 13 jobs and 72 Rust groups of this new commit are [qualified separately](../evidence/rust-grpc-envelope-coalescing-ci-20261116.json). It includes [writer coalescing](rust-grpc-envelope-coalescing.md); this is not retrospective qualification of `70bbf3b`, whose CI remains [failed on the framing oracle](../evidence/rust-credential-hpack-wire-ci-failed-20261116.json). All four HPACK tests already passed in the failed job. The S1, reset, TLS, real service, durable access and Linux live limits described above remain unchanged.

## Peer rejection, explicit replacement and active sibling — November 16, 2026

Two new `credential_rejection.rs` tests separately cover `UNAUTHENTICATED` and `PERMISSION_DENIED`. One loopback TCP connection carries three explicit sessions: active sibling, rejected session, then new session after explicit replacement of the provider's fictional metadata. The listener remains observed until close: any second accepted connection fails the test.

Stream IDs 1/3/5, exact invented values and distinct messages rule out observed replay in this trace. The shared provider is called twice, the sibling's once; the intermediate counter is only a consistency check, not independent proof of no replay. The sibling continues after rejection and replacement; the old session retains its typed error. All sources are closed and dropped exactly once, then transport close is awaited and checked idempotent. A new start on the closed transport does not call the provider again.

The targeted Opus review noted that removing the listener did not detect a reconnect attempt. Main retained a priority accept branch before execution. No production defect is claimed: this batch adds tests, without changing Cargo, TLS, authentication or retry policy.

### Local Windows validation

- Transport: **107 passed, 0 failed, 1 ignored** on stable 1.99.1 and MSRV 1.88.0.
- Stable workspace, all features: **404 passed, 0 failed, 2 ignored**.
- Global formatting and transport Clippy all targets with `-D warnings` pass.
- Both test names are confirmed in each of the three groups; these reruns are not distinct tests to add together.
- The 175 selected sources remain identical before/after; release CLI unchanged. No new release build or Linux/macOS qualification inferred.

```sh
cargo test -p rambler-transport --test credential_rejection --locked --offline
cargo +1.88.0 test -p rambler-transport --locked --offline
cargo test --workspace --all-features --locked --offline
```

[Evidence, hashes and commands per group](../evidence/rust-credential-rejection-offline-20261116.json). The 5 s per-step and 20 s per-scenario timeouts bound hangs; they are not performance thresholds. No sleep or real credential, audio or service.

Rejection is sent **after complete upload**, in HTTP 200 headers followed by separate gRPC trailers. Early headers-only rejection and a sibling still uploading are not combined with replacement here. A finite trace does not prove all future behavior; fictional explicit rotation is neither provisioning nor authorization or recovery after revocation from the real service. Ignored roots smoke and PyAV oracle are not requalified by these commands.

### CI of the contract after complete upload — commit `0e5ce23`

All 13 Rust/Python jobs succeeded on `0e5ce2336ce6d85fc487930198fb3ca93491b7cf`. The eight native jobs cover Windows, Ubuntu, macOS ARM64 and macOS Intel, each on Rust 1.88.0 and stable. The 48 Rust groups are qualified by command: Windows workspace 404/0/2, other OSes 405/0/2; reuse 10, profiling 8, finalization 1, budgets 3 and feature-off 38 passes per job. Both names of the post-upload contract are present in all eight workspaces. The eight public roots/PCM gates and lint also pass.

All four Python jobs pass: 663 passed/71 ignored under 3.11, 733/1 under 3.14, each on two OSes; 335 separate subtests, not added to tests. API run/job/step, HEAD and hashes of the 1,492,824 bytes of logs are verified. [CI evidence and observed versions](../evidence/rust-credential-rejection-ci-20261116.json). This CI does not qualify the early extension described next or real authentication. The roots smoke establishes a configuration, not a TLS handshake.

## Headers-only rejection during another upload — local extension

Two additional tests replay the same codes with HTTP 200, `grpc-status` in initial HEADERS and END_STREAM, without DATA or trailers. The peer waits for the first complete message and actual entry into the second `next` future before rejection; EOF of the rejected source is never released. After cleanup, its `source_exhausted` and `upload_eof_polled` flags remain false, `source_closed` true.

The sibling is still uploading during rejection, metadata replacement and success of the new explicit session. Its gate is opened only afterward: the peer verifies its two distinct complete frames and healthy completion. Counts of entered `next` futures are not counts of all low-level `Future::poll` calls. Sticky rejection, observed absence of replay/second connection, isolation, single cleanup and idempotent close remain checked by the shared harness. The two previous scenarios and their names are retained.

Windows stable 1.99.1 and MSRV 1.88.0 pass **109/0/1** for transport; stable workspace all features **406/0/2**, global fmt and strict transport Clippy pass. All four names are confirmed in the three groups, sources before/after and release CLI unchanged. No new release build; CI for this extension not run at this checkpoint. [Separate local evidence](../evidence/rust-credential-rejection-early-offline-20261116.json).

These invented peers and messages establish neither real revocation, provisioning, authorized recovery, ASR quality, performance gain nor Android parity. No new service call, device or loading of real credentials occurs.

### Remote qualification of the extension — source `1d66138`

All 13 jobs of `1d66138fbb79a1dcad1f52e6d2d4c49350da462d` succeeded: eight native Windows/Ubuntu/macOS ARM64/macOS Intel × Rust 1.88.0/stable, lint and four Python. All four scenario names are confirmed in each of the eight workspaces. Each Windows workspace reports **406/0/2**, other OSes **407/0/2**; the other five groups remain reuse 10, profiling 8, finalization 1, budgets 3 and feature-off 38 passes per job. The four Python groups remain 663/71 under 3.11 and 733/1 under 3.14, each on two OSes, with 335 separately reported subtests.

API run/job/step metadata and 1,494,689 bytes of logs are verified; the 48 Rust groups remain separated by command, without adding reruns. Linux/MSRV and macOS Intel/MSRV source is therefore actually executed, not inferred from static inspection. The last Intel 1.88 job completed successfully after its release build. [CI evidence linked to the commit](../evidence/rust-credential-rejection-early-ci-20261116.json).

The previous local “CI not run” checkpoint remains historical. Public roots/PCM qualifications are those of existing offline gates, not a handshake, remote authentication or new Rambler dictation. Limits on provisioning, real revocation and parity remain unchanged.
