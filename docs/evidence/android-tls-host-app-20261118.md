# Android host-app TLS qualification — 2026-11-18

**Transport source:** `72af1b05e90a1b81a3aa46a0767b4e21dd27e114` (parent trust fix `3cebb2b`). **Canary APK SHA-256:** `90ff1949f27fdfcf0890d5c8e8ada44b7a8a6e93cd93150e17bed48ca6ec4ca1`. Device: authorized Xiaomi test phone, Android 14/API 34, ARM64, 4 KiB runtime pages. No microphone, Rambler RPC, account credentials or editor data used.

The signed canary requested only `INTERNET`. The authorized installation used the existing root account and Android Package Manager for the single test user; the Activity's runtime UID was verified non-root. The app initialized the verifier from an ordinary JNI entry point and used TLS with hostname verification and h2 ALPN. There was no change to system certificates or global settings. A synthetic test CA was scoped to the canary's loopback domain.

| Probe | App result | Independent observation |
| --- | --- | --- |
| Public HTTPS, system anchors, HTTP/2 PING only | `verified_h2_pong` | No application RPC or credentials sent |
| Owned TLS peer, app-scoped CA and DNS SAN `localhost` | `verified_h2_pong` | The owned peer sent a PONG |
| Same owned peer and leaf, numeric loopback requested without a matching IP SAN | `certificate_rejected` | The handshake closed before the peer sent an h2 response |

The package, staging file, owned reverse mapping and local peer were removed. The Windows Job exited empty. The baseline APK's USB install had been refused. The first corrected run's log collector failed to recognize outcomes containing the digit in `h2`; the app's records were recovered with a corrected filter, then the same APK passed one bounded full verification with both independent peer observations. No failed artifact was counted as a passing run.

Android ARM64 cross-link and strict Clippy passed with Rust 1.88/NDK r27.1. The APK signature, absence of host path/private keys, and 16 KiB alignment of ELF and ZIP entries were checked; 16 KiB loading was **not** tested. The test validates platform trust and hostname rejection on this API level, not arbitrary custom trust managers, a narrower-than-system app policy, or the API 36 certificate-transparency path. The [Android integration contract](../../crates/rambler-transport/ANDROID_TLS.md) explains initialization, the lossless Android policy host and the synchronous-verification deadline limit. OSV returned no indexed advisory for the 12 new exact public crate versions in the patch's lockfile delta; that is not a general security attestation.
