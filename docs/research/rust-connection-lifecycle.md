# Rust shared-connection lifecycle and error integrity

Base: `e427fb1`. The earlier transport lot was imported as a starting point, then corrected after independent raw-HTTP/2 counterexamples. Scope: transport source/tests and this document only. No automatic replay/reconnect, Android, external service, credentials or environment-file access.

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

Source/cleanup precedence is unchanged. A failed source or cleanup can replace the network result, but does not acknowledge that hidden network failure. Actual h2 causes remain attached to the selected wire error until supervisor redaction after cleanup; private peer details are never formatted, placed in a public error chain, or retained in completion metadata.

The driver also polls its connection result before honoring local shutdown cancellation. An already-ready terminal error must not become successful cancellation; a unit test checks that race and that genuinely pending IO still stops. This is not a promise to detect errors that arrive only after local cancellation has discarded the connection.

## Reconnection and cleanup contract

There is no automatic reconnect, retry or replay. Explicit `GrpcTransport::connect(config.clone())` creates an independent transport for **new** sources only. Existing sessions/clones are not moved or rebound. An explicitly closed transport remains closed. `start` is not a health check: an accepted source on a dead channel belongs to its supervisor and receives its one asynchronous close attempt. Synchronous rejection still uses ordinary Rust Drop.

Observe old sessions, then explicitly close the old transport. Drain outstanding errors by repeating close until OK; a driver `Connect` after a session gRPC failure is possible and intentional. Source `close` is cooperative, called once, and must eventually finish. Deadlines bound RPC work, not asynchronous cleanup. Final transport Drop cannot await cleanup: keep the runtime alive and await close.

## Offline coverage and limits

The 13 lifecycle tests cover invalid initial SETTINGS, active/idle TCP loss, typed and flow-blocked GOAWAY cases, RST CANCEL mapping, sibling isolation, graceful GOAWAY with an active stream, distinct/same-reason successive non-OK GOAWAYs, queue-full responses, interrupted close, cleanup precedence, explicit replacement and no replay. PING/PONG, oneshots, semaphores and counters determine event order; sleeps do not choose failure timing. TCP_NODELAY is explicit in the lifecycle peer fixtures.

The inherited 512-session reuse test checks exact payloads, one call per source, single source close/drop, isolated refused streams and one accepted TCP. It is a correctness check, not a latency benchmark. Private unit regressions additionally check the real receive GOAWAY type and shutdown-vs-ready-error selection. Existing transport suites cover framing/status mapping, source/provider failures, cancellations, response/upload backpressure, observed/unobserved outcomes and concurrent/interrupted closes.

Passing these schedules, even repeatedly, is not proof that all interleavings are bug-free. There is no multi-hour WAN, real-service, TLS-rotation, proxy or idle-health guarantee. No idle keepalive is added. h2 native flow buffers are separate from capacity-one application queues. Unobserved failures require ledger memory proportional to those failures; there is no lossy cap. An `Unavailable`/GOAWAY/upload-EOF code is never evidence of server receipt or permission to replay.

## Validation

On Windows, offline and locked, with build artifacts owned by this worktree. `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` avoids unnecessary disk/debug artifacts. Earlier disk-exhaustion/linker failures and a corrected fixture synchronization timeout are not counted as RED assertions.

- RED: imported-lot reviewer probe reproduced both defects; assertive double-GOAWAY regression failed at the missing driver-error assertion.
- RED: typed wire-cause regression failed before the receive-side recovery poll.
- GREEN: `cargo test --offline --locked -p rambler-transport` — 61 tests passed (4 unit, 3 credential, 11 HTTP/2, 13 lifecycle, 29 loopback, 1 source-error).
- GREEN: `cargo clippy --offline --locked -p rambler-transport --all-targets -- -D warnings` — passed.
- GREEN: unchanged reviewer probe after correction — case 1 still returns `Grpc(Internal)`, `Connect`, OK (intentional conservative duplicate); case 2 now returns `ResourceExhausted`, `Cleanup`, **`Connect`**, OK instead of silently returning OK twice.
- GREEN: `cargo fmt -p rambler-transport -- --check` and `git diff --check` — passed; only platform LF/CRLF notices were emitted.
- GREEN: all six built transport test binaries executed 100 times — **100/100 repetitions, 6,100 test executions** passed, including 51,200 inherited short-session correctness checks. Each subprocess had a 30-second bound, in addition to per-case 5-second/3-second bounds. No timing benchmark was run.

No root lockfile, engine, CLI or other agent's loopback fixture was imported/changed. No commit or push was made.
