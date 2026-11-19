# Coalesced gRPC envelopes without payload concatenation

## Local decision — November 16, 2026

**At the local checkpoint: positive Windows qualification passed; new remote CI still pending.** The candidate was measured uncommitted on `70bbf3b`, with four source files identified in the [local evidence](../evidence/rust-grpc-envelope-coalescing-windows-20261116.json). The selected 188-file manifest is identical before and after the campaign. It is neither an attestation of the complete TCB nor of a new release CLI binary.

## Later CI qualification — source `8cb381e`

The [nine Rust jobs](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43859489195) and [four Python jobs](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43859489085) pass on exact commit `8cb381e8d26d448db80481d4aab9472aee3b096c`, attempt 1. The [separate CI evidence](../evidence/rust-grpc-envelope-coalescing-ci-20261116.json) retains API/log identities and hashes. The 72 Rust groups were recounted by command: eight Windows/Linux/macOS ARM/macOS Intel configurations × Rust 1.88/stable (observed CI stable: 1.100.0). The workspace reports **429/0/2** on Windows and **430/0/2** elsewhere; SDK 9, reuse 10, audio profile 8, prefix benchmark 7, wire benchmark 3, finalization 1, budgets 3 and feature-off 38 pass per job. Do not add these reruns as distinct tests.

The seven new coalescing names, including both expected panic assertions, the four HPACK tests and previous framing oracles are confirmed in each workspace. The macOS Intel/1.88 job passes this time on the corrected candidate. Python 3.11 jobs report **663 passed/71 ignored** and 3.14 jobs **733/1**, each on Windows and Linux, with **335 separately reported subtests**. The 13 jobs, native hosts/architectures and 1605020 bytes of logs are audited.

This new success rewrites neither the local evidence at the previous checkpoint nor the failure of `70bbf3b`. No backlog from the historical job is reconstructed, and no old client is rerun to provoke an error. Benchmark gates do not replay allocation or timing campaigns; the roots smoke test qualifies a configuration, not a TLS handshake. No new service qualification, provisioning, Linux live qualification, or overall perfection follows.

## Failure preserved, not a rerun intended to get green

CI for `70bbf3b` ended with **12 successful jobs and one failure**: macOS Intel, Rust 1.88, oracle `every_small_upload_length_and_large_boundaries_preserve_raw_grpc_envelopes`. The h2 peer reports a library-origin GOAWAY `ENHANCE_YOUR_CALM`. The four HPACK tests and the three-byte window oracle pass in that same job. [Separate terminal evidence](../evidence/rust-credential-hpack-wire-ci-failed-20261116.json) · [failed Rust run](https://github.com/Antoninnnnnnnn/Sambler/actions/runs/43850604441).

Two independent static analyses identify sending the five-byte prefix as separate DATA as a plausible mechanism for accumulation of small frames at the peer. **The frame sequence of the failed job was not captured**: this is not a historically demonstrated wire cause. The failure remains a failure even after local successes. No old client is rerun to provoke an error, and no peer limit is disabled or raised.

## Production change

The writer sends a private `Chain<Bytes, Bytes>`: a view of the prefix and one of the original payload. Each positive credit produces a DATA submission covering available parts of **the same envelope**. When credit suffices, the prefix is no longer sent as a separate frame.

- No batching across messages, added wait, or payload concatenation buffer; no new dependency, `unsafe`, or public API.
- Claims remain bounded by 16 KiB and the credit actually granted. Reset and typed-error branches are retained; the reservation is released once per envelope instead of twice for its two parts.
- Partial credits can still fragment the prefix or cross its boundary with the payload. Bytes, order and empty messages are preserved; END_STREAM remains empty and EOF progress follows its successful submission.
- The cached prefix constructor, receive path, credentials, TLS, audio, source/cancellation policies and limits are unchanged. Other production changes only propagate the private buffer type.

Pointer assertions verify nonempty subviews of the original payload, as well as `Chain`, `Take`, `advance` and vectored views. They do not measure allocations of the full transport. `Bytes` sharing, slicing, fallback prefixes and h2 internals can still allocate or copy. The [historical constructor benchmark](rust-grpc-prefix-allocation.md) does not measure this new writer and is not retrospectively requalified.

## Positive candidate validation

Seven new tests: five slicing/view unit tests and two integrations on the real transport, numeric loopback peer under default limits. The source gate follows the peer's PING/PONG; the peer drains normally and returns credit. No artificial backlog, sleep, retry, or limit accommodation.

- Full matrix: **260 messages, 117140 gRPC bytes**, exact contents and order, including an empty message and sizes greater than one frame.
- Targeted boundaries: nine messages, **33821 gRPC bytes**, ten nonempty DATA frames expected with sufficient initial credit; checks of cached prefix boundaries and maximum frame size.
- The two previous oracles, including the prefix actually fragmented by a three-byte window, remain **unchanged**, as do the four HPACK tests. Cancellation, active sibling, fictional authentication rejections, GOAWAY cause, blocked upload, EOF, cleanup and absence of an additional connection remain checked.
- Windows stable 1.99.1 and MSRV 1.88: transport **126/0/1** each and coalescing target **2/0/0** each; stable workspace **429/0/2**, guarded SDK **9/0/0**, CLI without audio **38/0/0**. Format and transport Clippy all targets/features with `-D warnings` pass. These replayed groups are not distinct tests to sum.
- The 14 commands and their supervised jobs pass; the 188 source hashes are identical before/after. Historical CLI, Cargo.lock, consumed budget and failed CI evidence remain unchanged.

```sh
cargo test -p rambler-transport --locked --offline
cargo +1.88.0 test -p rambler-transport --locked --offline
cargo test -p rambler-transport --test grpc_envelope_coalescing --locked --offline
cargo clippy -p rambler-transport --all-targets --all-features --locked --offline -- -D warnings
cargo fmt --all -- --check
```

## Retained limits

This campaign does not guarantee every distribution of messages or credits, or every peer policy. It measures neither overall acceleration, engine allocations, WAN latency, ASR quality nor an optimum. No new build or call of the historical release CLI, real credential, service, phone or reservation; the real Windows budget remains consumed. G1 Linux remains unmet and Linux live **NO_GO**. Provisioning/durable access, complete Android parity and real service privacy remain outside qualification.
