# Android TLS: host APK integration and acceptance

`GrpcTransport` uses ring/rustls, verifies the endpoint, and requires HTTP/2
ALPN on Android as on desktop. The difference is **chain policy**: a desktop
connection loads native roots as before, while Android asks the host app's
platform trust manager. There is no Android filesystem, environment-variable,
built-in root, or trust-all fallback.

## Host initialization

- Declare `android.permission.INTERNET` in the ordinary host APK. Load the SDK
  library and call its JNI entry point from or after `Application.onCreate`,
  before starting TLS connections. Do not initialize from `JNI_OnLoad` or use a
  standalone `adb shell` executable as a substitute for an app process.
- From the app's JNI entry point, obtain `&mut jni::Env<'_>` using `jni` 0.22.4
  (`EnvUnowned::with_env`) and call `rambler_transport::init_android_tls(env)`.
  Handle the returned `Result` before connecting. This requires no Java verifier
  class, AAR, keep rules, passed-in root list, or unsafe pointers in the SDK.
  Success is process-lived and repeat calls are no-ops; a failed call can be
  retried. The app's default manager is selected at initialization. If the host
  itself replaces that provider later, it must restart to select a new manager.
- Call `GrpcTransport::connect_ready(TransportConfig::tls(authority)?)` for a
  readiness probe; it negotiates H2 and awaits a PING acknowledgement but opens
  no RPC and does not consult RPC metadata. On success, call `close().await`.

The adapter uses [TrustManagerFactory.init(null)][init] and the public
[hostname-aware X509TrustManagerExtensions][extension]. Android installs its
[app network security provider][provider]. The default factory's [null-store
branch][factory] selects the app configuration, and the [root trust manager][root]
uses the supplied host to select its rules. This is how the app's
[Network Security Configuration][config] is consulted on versions that support
it, rather than reimplementing profile/user/system CA, domain, or pin rules. In
particular, enumerating `getAcceptedIssuers()` does **not** establish whether a
server's domain policy has any roots. If the manager is usable but rejects a
chain, including for lack of a trusted anchor for that host, the app sees a
certificate rejection rather than a guessed empty-store error.

The upstream `rustls-platform-verifier` 0.7.1 Android backend was checked but is
not used: its [Kotlin release implementation][upstream] ignores `Context` and
supplies `AndroidCAStore` explicitly, which is not the app-default policy above.
Rambler instead uses maintained `jni` bindings to public Android/Java APIs and
rustls' own DNS/IP name and TLS signature checks. The original DNS name or IP,
not the address picked by TCP racing, is the verification identity. No retries
use a weaker verifier. Android TLS session resumption is disabled, so a new
connection checks the certificate again.

On Android API 36+, the adapter passes a stapled OCSP response when available
through the corresponding public overload. Rustls does not expose a separate
TLS SCT list to the verifier; Android can use embedded and stapled SCTs and is
allowed to reject when evidence is insufficient. Older API levels use the
public three-argument method. These are API-level choices, not fallback after
rejection. Verification is synchronous within rustls; the existing overall
connect timeout remains, but cannot interrupt a Java trust check mid-call.

## Fixed diagnostics

| Transport outcome | What was observed |
| --- | --- |
| `TrustUnavailable` | Desktop loader delivered no parsable roots, or the Android app did not initialize a usable verifier / the JNI platform verifier could not run. Uninitialized Android TLS fails before DNS or TCP. |
| `CertificateRejected` | Typed TLS certificate failure or absent certificate; on Android this includes a platform `CertificateException` or the independent DNS/IP name mismatch. |
| `Resolve` | The system resolver failed or returned no addresses. It does not claim a specific DNS error or expose the host. |
| `Connect` | Other failures, such as exhausted TCP attempts, TLS negotiation without H2, or H2 setup failure. |
| `Timeout` | The transport's overall connect timer expired. It does not imply that DNS, TCP, TLS or H2 in particular was responsible. |

No OS errors, exception text, chain bytes, or endpoint are emitted by these
errors. The CLI's existing generic transport bucket remains unchanged.

## Validation

Desktop: run `cargo test --workspace --all-features --all-targets --locked
--offline`, `cargo clippy --workspace --all-features --all-targets --locked
--offline -- -D warnings`, `cargo fmt --all -- --check`, and
`python -I tools/check_rust_native_roots.py` for the separate OS public-root
smoke. The local TLS regression uses a checked-in synthetic CA, leaf with only
`DNS:localhost`, and intentionally public fixture key. These files are for
loopback tests only; never install that CA in a real trust store or use the key
outside the tests. The test covers verified H2 ALPN, IP/name mismatch, untrusted
chain, and lack of H2 ALPN.

Android cross-build: use an official NDK and the Rust `aarch64-linux-android`
target. This lot used Rust 1.88.0, NDK `27.1.12297006`, and API 24. On Windows
Git Bash set `ndk_bin` to the NDK's `toolchains/llvm/prebuilt/windows-x86_64/bin`:

```sh
export CC_aarch64_linux_android="$ndk_bin/clang.exe"
export AR_aarch64_linux_android="$ndk_bin/llvm-ar.exe"
export CFLAGS_aarch64_linux_android=--target=aarch64-linux-android24
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ndk_bin/clang.exe"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS='-Clink-arg=--target=aarch64-linux-android24 -Clink-arg=-Wl,-z,max-page-size=16384'
cargo +1.88.0 test -p rambler-transport --all-targets --target aarch64-linux-android --no-run --locked --offline
```

**Platform acceptance is separate from cross-building.** From an ordinary
ARM64 APK, without environment overrides, trust-store changes, credentials,
audio or an RPC:

1. In a fresh process, omit initialization and check `TrustUnavailable`. Then
   initialize from the app's JNI entry point and use the default APK policy to
   connect to a public HTTPS server that supports H2 PING; close cleanly.
2. Use a controlled local H2/TLS peer serving a synthetic certificate whose SAN
   is `DNS:localhost`, with **no IP SAN**. From an APK trusting that test CA under
   a `localhost` domain rule, `localhost:<port>` must succeed. For the very same
   listener and chain, `127.0.0.1:<port>` must return `CertificateRejected`. In
   an APK variant without that CA, the matching `localhost` connection must
   also return `CertificateRejected`. Never install the fixture CA on the device.
3. Exercise an app policy narrower than system trust as a separate guard (for
   example, an app domain config that uses only an unrelated fixture CA for the
   public test host); the otherwise valid public chain must be rejected. This
   detects accidental bypass of the app's domain rules in favor of wider roots.
4. Repeat the applicable cases on the 4 KiB reference device and a 16 KiB
   device/emulator with the APK's native libraries actually loaded. The API 36+
   verifier overload also needs an APK runtime check on that API level. ELF
   alignment and Android cross-linking are not APK or platform validation.

[init]: https://developer.android.com/reference/javax/net/ssl/TrustManagerFactory#init(java.security.KeyStore)
[extension]: https://developer.android.com/reference/android/net/http/X509TrustManagerExtensions
[config]: https://developer.android.com/privacy-and-security/security-config
[provider]: https://android.googlesource.com/platform/frameworks/base/+/android-14.0.0_r1/core/java/android/security/net/config/NetworkSecurityConfigProvider.java
[factory]: https://android.googlesource.com/platform/frameworks/base/+/android-14.0.0_r1/core/java/android/security/net/config/RootTrustManagerFactorySpi.java
[root]: https://android.googlesource.com/platform/frameworks/base/+/android-14.0.0_r1/core/java/android/security/net/config/RootTrustManager.java
[upstream]: https://github.com/rustls/rustls-platform-verifier/blob/v/0.7.1/android/rustls-platform-verifier/src/main/java/org/rustls/platformverifier/CertificateVerifier.kt
