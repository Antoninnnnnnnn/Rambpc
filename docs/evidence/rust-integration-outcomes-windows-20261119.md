# Independent integration audit corrections

19 November 2026 · audited revision: `9120decab7df7b2ecb277445ef24ff8fb66aa1aa`.
Tested Rust revision: `fb29d02afa4ddf370e82e765abc30a41ac81c1a2`. All corrective tests below use owned loopback peers, synthetic responses and simulated editors. No provider call, APK, device or microphone.

| Case | Verified contract |
| --- | --- |
| Long dictation: text followed by an empty segment | A final with an unsupported edit shape and no ASR hypothesis does not become a successful dictation. The CLI reports the absence of a result, keeps exactly the two ranges `[0,96000)` and `[96000,116000)` and retains « Première phrase. » as the first segment's result. No usable global text or exit code 0. The SDK strict summary does not skip a segment without text either. |
| No final, ambiguity, safety, deletion | Zero finals does not establish that the audio was silent. Multiple finals remain subject to review. A blocked or unknown safety status remains distinct and yields no text. Deletion requires an explicit, supported replacement of the captured window; an empty ASR string is never a deletion. An interpreted result still authorizes no effect on the document. |
| Rich editing | `minimal_app` checks document identity, the exact range, revision and selection or caret before deriving a proposal. In `Paris Paris`, deleting the first `Paris ` leaves the link on the second intact. If the scope of a selection correction is not proven, the model indicates review without a range to apply. |
| CLI telemetry | Transcription and context markers appear in the expected private output, but are absent under `--diagnostics-only` in the actual binary, including on ambiguity. An allowlist retains categories, durations, counters and ranges; tests also check it against nested segment output. The mode changes neither the exit code nor requests. |
| Python reference | `secure=False` requires a numeric loopback TCP authority, with a direct address and proxy disabled. Configuration changed after construction is checked again before channel creation. The TLS path remains unchanged. No test opens a connection to a remote destination. |

## Local validation

- Python 3.14, offline: **947 passed, 5 skipped, 981 successful subtests**. Transport/engine subset: **117 passed**. The admission check proves rejection before channel creation and the suite also exercises a real local IPv4 peer.
- Rust stable: **569 passed, 2 ignored** in the workspace, **28 passed** for the CLI without audio; **6 passed** for `minimal_app`, formatting and strict Clippy.
- Rust 1.88: **569 passed, 2 ignored** in the workspace, **28 passed** for the CLI without audio.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-features --locked --offline
cargo test -p rambler-cli --no-default-features --locked --offline
cargo test -p rambler-cli --features audio --example minimal_app --locked --offline
python -m pytest -q tests
```

## Production limits

The app retains the draft and already validated segments; a failure does not authorize automatic replay of the current sentence. Recovery of audio before EOF requires independent, explicit and bounded host-side retention. Usable text does not predetermine an editing decision; document verification and reversible application must occur in the same host-side transaction.

The independent audit ran `rambpc` against the real service without application authentication metadata: TLS and h2 PING/PONG succeeded, dictation was rejected with `PERMISSION_DENIED`, and there was no new transcription. This observation establishes neither expiration of previous settings nor authorized access for a new app. Real recognition, latency under usage conditions, the microphone and comparison with Gboard are not inferred from these corrective tests.
