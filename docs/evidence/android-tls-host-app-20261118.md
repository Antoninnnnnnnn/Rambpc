# Android host-app TLS qualification — 2026-11-18

**Transport source:** `72af1b05e90a1b81a3aa46a0767b4e21dd27e114` (parent trust fix `3cebb2b`). **Canary APK SHA-256:** `90ff1949f27fdfcf0890d5c8e8ada44b7a8a6e93cd93150e17bed48ca6ec4ca1`. Device: authorized Xiaomi test phone, Android 14/API 34, ARM64, 4 KiB runtime pages. No microphone, Rambler RPC, account credentials or editor data used.

The signed canary requested only `INTERNET`. The authorized installation used the existing root account and Android Package Manager for the single test user; the Activity's runtime UID was verified non-root. The app initialized the verifier from an ordinary JNI entry point and used TLS with hostname verification and h2 ALPN. There was no change to system certificates or global settings. A synthetic test CA was scoped to the canary's loopback domain.

| Probe | App result | Independent observation |
| --- | --- | --- |
| Public HTTPS, system anchors, HTTP/2 PING only | `verified_h2_pong` | No application RPC or credentials sent |
| Owned TLS peer, app-scoped CA and DNS SAN `localhost` | `verified_h2_pong` | The owned peer sent a PONG |
| Same owned peer and leaf, numeric loopback requested without a matching IP SAN | `certificate_rejected` | The handshake closed before the peer sent an h2 response |

The package, staging file, owned reverse mapping and local peer were removed. The Windows Job exited empty. The baseline APK's USB install had been refused. The first corrected run's log collector failed to recognize outcomes containing the digit in `h2`; the app's records were recovered with a corrected filter, then the same APK passed one bounded full verification with both independent peer observations. No failed artifact was counted as a passing run.

Android ARM64 cross-link and strict Clippy passed with Rust 1.88/NDK r27.1. The APK signature, absence of host path/private keys, and 16 KiB alignment of ELF `LOAD` segments, RELRO and ZIP entries were checked. The ARM64 APK ran only at 4 KiB. The [Android integration contract](../../crates/rambler-transport/ANDROID_TLS.md) explains initialization, the lossless Android policy host and the synchronous-verification deadline limit. OSV returned no indexed advisory for the 12 new exact public crate versions in the patch's lockfile delta; that is not a general security attestation.

## API 36 with 16 KiB runtime pages

For the same transport revision, the x86_64 canary APK (SHA-256 `9056d9178120a00738278e9ac6dbc90afd7674a0f93f1e61aa6a8fd650aa27fc`) installed without root and loaded on the official `android-36/google_apis_ps16k/x86_64` emulator image. The guest reported API **36** and `getconf PAGESIZE` returned **16384**. This verifies actual 16 KiB x86_64 loading, not just packaging alignment.

- The owned `localhost` endpoint with an app-scoped CA passed (`verified_h2_pong`); the owned peer independently recorded a PONG. This exercised the API 36 trust-manager overload.
- The same peer with a mismatched IP name returned `certificate_rejected`; the owned peer observed the handshake close without PONG. No public endpoint, app RPC or physical device was used for these two tests.
- The APK and reverse mapping were removed, the peer and emulator stopped, and the owned Windows Job exited empty. A prior boot attempt on the standard API 36 image exceeded the boot deadline before any APK test; it is not counted as a TLS result.

Still outside this evidence: a narrower-than-system app policy, custom trust managers, servers requiring TLS-supplied SCTs for certificate transparency, and the combination of ARM64 with 16 KiB runtime pages.
