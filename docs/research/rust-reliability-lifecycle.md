# Rust reliability contracts: admission, errors, and finalization

Documentation status as of **November 15, 2026**, batch built from base `90a773a`, integrated into `58da700`. **Windows R3 and Linux R3 locally qualify the same snapshot, with explicit exclusions. Linux R1/R2 remain FAILED.** The 310 tests from the previous snapshot qualify only the committed base, not this batch. Integrated results and their limitations are separated in [build validation](rust-build-validation.md#november-15-reliability-batch).

## Local admission before consumption

The supervisor successively waits for metadata resolution/validation, `SendRequest::ready()`, and local success of `send_request` before permitting the first `RequestSource::next()`. The engine benefits from this barrier before consuming its `AudioSource`. A locally known rejection should therefore not needlessly read new audio chunks.

This barrier is neither a peer acknowledgment, accepted authentication, nor proof of receipt of HEADERS or audio. A connection silently lost behind a NAT can still appear locally ready. `start` remains synchronous: it can accept a source whose **constructor has already done work** (opening/prereading/initialization), or whose independent producer is already running. The barrier cannot turn back time and is not a promise of microphone capture without prior activity.

Once the source is accepted by `GrpcTransport::start`, the supervisor conducts its cleanup **once**, even if admission fails before the first `next`. A synchronous rejection by this raw transport before `Ok(Session)` falls under ordinary Rust Drop. **`Engine::start`, by contrast, supervises audio cleanup even if it rejects invalid configuration or a closed engine**: await `Engine::close().await` after that rejection as well. Sources must not block the runtime or panic; their close must finish cooperatively. Drop alone cannot await it.

No production retry, replay, automatic reconnect, health check, keepalive, or PING is added. Fixture PINGs only order tests. Explicitly creating another transport moves no old session and authorizes no replay of its source.

## Initial connection: TCP race only

- A DNS resolution, under the global connection timeout, retains **all** addresses, even duplicates. Order within each family is preserved; families alternate starting with the family of the first response.
- The TCP race is Happy Eyeballs-style and applies delays inspired by [RFC 8305 §5](https://www.rfc-editor.org/rfc/rfc8305.txt) to **application API initiation**, not the entire RFC: no DNS AAAA/A race or complete RFC 6724 implementation. The first attempt is pinned and polled immediately, with the real waker before any yield; normal spacing remains **250 ms**. After an observed failure, the next deadline becomes `min(échéance prévue, max(dernier lancement + 10 ms, maintenant))`: it never postpones a scheduled start. The last launch is timestamped **after its first poll**, not at construction or from the failed attempt. §5 recommends 100 ms as a minimum (`SHOULD`) and 250 ms by default; 10 ms is its mandatory absolute floor (`MUST NOT` start sooner). Acceleration on failure uses this application floor, without a permanent 100 ms minimum. **No measurement or evidence of kernel SYN spacing is claimed**, nor a performance gain. Yields after initiation prevent a long series of immediately available failures from monopolizing the executor and masking the external timeout. A late address is not eliminated by an arbitrary limit to the first eight responses.
- On success, final error, or cancellation, losing futures/sockets are dropped and closed before proceeding to TLS. This destination selection belongs to initial establishment, not an RPC retry.
- The external timeout, **10 s by default**, covers DNS, TCP, TLS, and h2 handshake. Its expiration remains `TransportError::Timeout`, not a generic `Connect`. About **41 blocked attempts** can coexist under this 10 s / 250 ms default alone (also limited by the DNS list): this is not a fixed bound for an arbitrarily chosen caller timeout. Failed attempts leave the race.
- TLS starts only on the winning TCP, retains the original hostname for verification and authority, system roots, and ALPN `h2`. TLS failure ends this connection: it neither starts nor resumes a race to another destination and never reduces TLS checks.

## Backpressure and finite fairness

The h2 connection receive window is **1 MiB**; each stream's remains **65,535 bytes**. Application request/response queues remain capacity **one**, with in-flight elements distinct. A stalled consumer must not alone exhaust the old connection credit and block a sibling's small exchange in the covered scenario.

This credit provides a **finite** fairness margin, not isolation for an arbitrary number N of stalled streams. h2 buffers, `Bytes` root allocations, sources, and the error ledger remain distinct. Neither this window nor capacity-one queues constitute a global RSS bound. A consumer that does not drain can still delay its own session's trailers.

## Two terminal contracts that must not be confused

For supplied audio sources (`PcmInput`, `Pcm16Opus`, WAV), a source fault remains typed and **sticky**: subsequent `AudioSource::next()` calls remain `Err` until explicit close, instead of turning corruption/truncation/abandonment into healthy EOF. No successful Opus EOS is fabricated to mask the fault. Upstream release is unique and may already have begun; that does not erase the fault exposed by `next`. Explicit close remains a separate transition, with its own cleanup outcome.

Conversely, `rambler_transport::Session::next()` is intentionally **fused**: after delivering its terminal error once, it returns `Ok(None)` on subsequent calls. `finish()` retains and returns the terminal outcome, including that error. This EOF after error is therefore neither the `AudioSource` contract nor a new success acknowledgment. The integrator must observe the outcome, not just the last `None`.

### Preserve primary and cleanup

`TransportError::PrimaryAndCleanup(Box<PrimaryAndCleanup>)` preserves the selected primary fault and a distinct close fault. The type is public, but its construction and fields are private; `primary()`, `cleanup()`, and `into_parts()` permit explicit inspection. Debug/Display and the automatic error chain do not expose the caller's private errors. Explicit cancellation accompanied by failed cleanup remains `Cleanup`, not a pair whose primary is `Cancelled`.

The engine unwraps this pair only **locally**, for its own request adapter and retained audio: it restores its primary and retains cleanup separately in its `AudioCleanup` ledger, without a second audio close or second delivery of the same cleanup fault. This operation must not be generalized to public `From<TransportError>`, nor deduplicated by error text/code.

Transport priority for an actually observed source fault is not a universal rule replacing a timeout or cancellation **already selected by the engine**. Each layer retains its selection point and responsibilities. An observed session error also does not acknowledge the independent driver: see [the connection ledger](rust-connection-lifecycle.md). Successive closes can legitimately reveal distinct faults; await them until `Ok`.

## Timeouts and finalization budget

`SessionOptions.timeout` sets a **30 s default deadline** starting when the session starts. It covers speech, metadata, admission, upload, late context, and terminal delivery. Connection setup uses the separate timeout above. Asynchronous cleanup is cooperative and is awaited after either timeout expires.

`SessionOptions.finalize_timeout: Option<Duration>` is **`None` by default**. The CLI exposes `--finalize-timeout` in seconds. When present, this budget starts **only after audio EOF and successful audio cleanup**. It then covers context, upload completion/END, gRPC status, and final delivery under backpressure. Its effective deadline is the minimum of its own deadline and the hard deadline: it never extends the session. `Some(Duration::ZERO)` is invalid; an enormous budget or overflowing instant addition is safely capped by the hard deadline. The context provider also retains its own budget (`context_timeout`, default 2 s), limited by remaining time.

Exceeding the finalization budget must remain distinct (`FinalizeTimeout`) from hard timeout (`Transport(Timeout)`). No `max_audio`, automatic false EOF, or microphone stop is added. The producer must decide normal termination.

**Illustrative settings, not measured as optimal**: for a source that may last two minutes, `--timeout 150 --finalize-timeout 10` leaves at most 10 s after EOF/cleanup, without exceeding 150 s total; `--context-timeout 2` retains its own cap. Speech longer than 150 s gets no extension. Without `--finalize-timeout`, only the hard timeout and existing specific timeouts apply.

### Irreversible publication of success

The local fix places **all fallible waits before a private irreversible commit**, including cleanup and event queue reservation. After reservation, cancellation and deadlines are still checked before publication; no fallible wait follows the commit. Published healthy EOF therefore does not later become an error; `metrics.eof_seconds` timestamps the **successful commit**, not the last PCM sample or mere queue drainage.

Full-queue, expiration-after-reservation, timeout priority, and cancellation-after-EOF regressions pass in Windows R3. The earlier pass of 12 cases was not this evidence. These finite scenarios do not prove all schedules; the distinct Linux R2 failure remains recorded below.

## Targeted history, separate from integrated qualification

The names below identify targeted local logs, not links to published evidence or a workspace total. They describe intermediate snapshots run, not necessarily the latest shared source state.

- Admission: `admission-evidence-20261115/red-01.log` contains three failing assertions; `green-admission-09.log` reports all five filtered admission tests passing. `green-transport-10.log`, `green-engine-07.log`, and `green-cli-08.log` contain targeted transport/engine/CLI results. These groups are not added into a distinct test count.
- Sticky audio errors: `sticky-errors-8sqitv/red-final.log` contains six regression failures; `green-final.log` contains the six passing sticky cases and neighboring audio groups. This is not evidence of all interleavings.
- Initial TCP race: `happy-eyeballs-wwjpVW/after-test-2.log` and `tcp-test-current.log` each report ten successful connection tests on old pacing without a floor. The first fix (`tcp-pacing-floor/red.log`, exit 101, then `green-02.log`, 13 passes, and `clippy-lib.log`, exit 0) retains its historical evidence, **but its oracle measured construction and missed the first poll deferred after yield**. It does not qualify that pacing. In `tcp-first-poll-20261115/red-02.log`, corrected oracles fail at runtime (exit 101, 13 passes / 6 failures): two initiations at **20 ms / 20 ms** after a 20 ms suspension, pending case at 270 ms, first-poll cost and Ready/cooperation selection. After correction, `green-01.log` passes **19 tests** (exit 0: 16 with injected clock without I/O, family order, driver, loopback socket). `clippy-lib.log` and `fmt-check.log` also pass (exit 0). The **129** failures retain **129** drops in **1 280 virtual ms**, and the ninth address wins at 2 s; post-poll cases check 10 ms after ReadyErr and 250 ms after Pending. Exact RED/GREEN sources, hashes, and invariants are retained; see [targeted pacing evidence](rust-connection-lifecycle.md#targeted-tcp-pacing-correction--november-15-2026). These are application-initiation oracles, **not SYN/RFC pacing evidence**, integrated qualification, or a WAN benchmark.
- h2 window: `connection-window-zEaYDA/desired-red-1.log` fails on the blocked sibling; `happy-eyeballs-wwjpVW/window-green-1.log` passes this targeted case. This does not generalize fairness to arbitrary N streams.
- `happy-eyeballs-wwjpVW/tcp-test-final.log` is a **transient compilation failure** (`E0061`, shared lib/wire signature being edited), not a runtime RED or final validation. Do not turn it into a failed network assertion or presumed green rerun.

Test output was read; historical logs do not all have an independently recorded exit code. The pacing fix above, however, reran only connection tests and library Clippy, with exit codes retained. No private log is reproduced. Windows R2 and Linux R1 remain historical evidence on their immutable snapshots; Linux R1 remains **FAILED**, without rewriting after correction.

## Integrated qualification

Windows R3 qualifies the snapshot including corrected first TCP poll and terminal commit. Linux R2 uses the same selected 167-file manifest, unchanged before/after. Its workspace, example, and feature-off tests pass, but `release-finalization-cli` fails (exit 101) at `peer upload was not complete`. Cross-checked diagnosis identifies a stale release binary: it rejects `--finalize-timeout` before upload, because per-package cleanup covered only the dev profile. The identical test passes with rebuilt debug; this is not a release success. **Linux R2 remains unqualified.** Its subsequent release PCM/smoke phases were not run. Linux R3, after cleaning both profiles, then passes the same workspace, rebuilt release test, PCM smoke, and source postflight, without engine changes. Its PyAV test remains ignored; other platform exclusions are retained.

Counts, exact local reports, and already available public evidence are referenced in [build validation](rust-build-validation.md#november-15-reliability-batch). Local snapshot qualification is published with its exclusions; historical failures are not relabeled. Local success is neither remote CI nor phone/service/WAN qualification.

The November 15 real Opus architectural review is **static**, without tests run by the reviewer; its provenance is separate from misrouted historical requests in [build validation](rust-build-validation.md). CPU oneof/OggScratch/mutex micro-optimizations were discarded as low priority and unmeasured relative to reliability; no speed/WAN gain is claimed. Android/ART, authentication/provisioning/revocation, COMPOSE, and macOS remain separate scopes not qualified by this offline batch. No proprietary APK, private capture detail, or live access is necessary here.
