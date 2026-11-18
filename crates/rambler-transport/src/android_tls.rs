//! Android app-default chain policy, with rustls endpoint/signature checks.

use crate::TransportError;
use jni::{
    Env, JavaVM, bind_java_type,
    errors::Error as JniError,
    jni_sig, jni_str,
    objects::{Global, JByteArray, JList, JObject, JObjectArray, JValue},
};
use std::sync::{Arc, OnceLock};
use tokio_rustls::rustls::{
    self, CertificateError, DigitallySignedStruct, OtherError, SignatureScheme,
    client::{
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        verify_server_name,
    },
    crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, UnixTime},
    server::ParsedCertificate,
};

// Resolve virtual methods against the public API declarations, not the names
// of the concrete certificate/public-key provider implementations.
bind_java_type! {
    JPublicKey => java.security.PublicKey,
    methods { fn get_algorithm() -> JString, },
}
bind_java_type! {
    JX509Certificate => java.security.cert.X509Certificate,
    type_map = { JPublicKey => java.security.PublicKey, },
    methods { fn get_public_key() -> JPublicKey, },
}

static TRUST: OnceLock<AppTrust> = OnceLock::new();

#[derive(Debug)]
struct AppTrust {
    vm: JavaVM,
    manager: Global<JObject<'static>>,
    sdk: i32,
}

/// Initialize TLS using the Android host application's default trust manager.
/// Call once from an application-owned JNI entry point after Android has bound
/// the application (for example in/after `Application.onCreate`, not `JNI_OnLoad`)
/// and before making any TLS connection. The VM and manager are process-lived;
/// a successful subsequent call is a no-op. An unsuccessful call can be retried.
/// This needs no app-specific Java verifier class or explicit trust store.
///
/// Uses the app's default Android security provider, including per-domain trust
/// rules on Android versions that support Network Security Configuration.
/// If the app replaces the default provider after initialization, restart the
/// process to select that provider instead.
pub fn init_android_tls(env: &mut Env<'_>) -> Result<(), TransportError> {
    if TRUST.get().is_some() {
        return Ok(());
    }
    // Do not clear a pre-existing exception owned by the caller.
    if env.exception_check() {
        return Err(TransportError::TrustUnavailable);
    }
    let trust = env.with_local_frame(24, load_app_trust);
    if env.exception_check() {
        env.exception_clear();
        return Err(TransportError::TrustUnavailable);
    }
    let _ = TRUST.set(trust.map_err(|_| TransportError::TrustUnavailable)?);
    Ok(())
}

fn load_app_trust(env: &mut Env<'_>) -> Result<AppTrust, VerificationFailure> {
    let factory_class = env.find_class(jni_str!("javax/net/ssl/TrustManagerFactory"))?;
    let algorithm = env
        .call_static_method(
            &factory_class,
            jni_str!("getDefaultAlgorithm"),
            jni_sig!(() -> JString),
            &[],
        )?
        .l()?;
    let factory = env
        .call_static_method(
            &factory_class,
            jni_str!("getInstance"),
            jni_sig!((JString) -> javax.net.ssl.TrustManagerFactory),
            &[JValue::from(&algorithm)],
        )?
        .l()?;
    // Android selects the bound app's policy for null. Providing AndroidCAStore
    // explicitly here would replace that policy, including its domain rules.
    env.call_method(
        &factory,
        jni_str!("init"),
        jni_sig!((java.security.KeyStore) -> void),
        &[JValue::from(&JObject::null())],
    )?;
    let managers = env
        .call_method(
            &factory,
            jni_str!("getTrustManagers"),
            jni_sig!(() -> javax.net.ssl.TrustManager[]),
            &[],
        )?
        .l()?;
    let managers = env.cast_local::<JObjectArray<JObject>>(managers)?;
    let x509 = env.find_class(jni_str!("javax/net/ssl/X509TrustManager"))?;
    for index in 0..managers.len(env)? {
        let manager = managers.get_element(env, index)?;
        if !manager.is_null() && env.is_instance_of(&manager, &x509)? {
            // Choose the first X509 manager like JSSE; unsupported wrappers fail
            // closed rather than trying a second manager with a different policy.
            let manager = env.new_object(
                jni_str!("android/net/http/X509TrustManagerExtensions"),
                jni_sig!((javax.net.ssl.X509TrustManager) -> void),
                &[JValue::from(&manager)],
            )?;
            let sdk = env
                .get_static_field(
                    jni_str!("android/os/Build$VERSION"),
                    jni_str!("SDK_INT"),
                    jni_sig!(jint),
                )?
                .i()?;
            return Ok(AppTrust {
                vm: env.get_java_vm()?,
                manager: env.new_global_ref(manager)?,
                sdk,
            });
        }
    }
    Err(VerificationFailure::Unavailable)
}

#[derive(Debug)]
enum VerificationFailure {
    Rejected,
    Unavailable,
}

impl From<JniError> for VerificationFailure {
    fn from(_: JniError) -> Self {
        Self::Unavailable
    }
}

// Catch only an actual Java CertificateException as a rejection. Do not log
// messages or stack traces that might contain certificate/hostname details.
fn cert_failure(env: &mut Env<'_>, error: JniError) -> VerificationFailure {
    if matches!(error, JniError::JavaException) {
        let thrown = env.exception_occurred();
        env.exception_clear();
        if thrown.is_some_and(|thrown| {
            matches!(
                env.is_instance_of(&thrown, jni_str!("java/security/cert/CertificateException")),
                Ok(true)
            )
        }) {
            return VerificationFailure::Rejected;
        }
    }
    VerificationFailure::Unavailable
}

#[derive(Debug, thiserror::Error)]
#[error("Android application TLS trust unavailable")]
pub(crate) struct PlatformTrustUnavailable;

impl AppTrust {
    fn check_chain(
        &self,
        env: &mut Env<'_>,
        leaf: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        policy_host: &str,
        ocsp: &[u8],
    ) -> Result<(), VerificationFailure> {
        let x509 = env.new_string("X.509")?;
        let factory = env
            .call_static_method(
                jni_str!("java/security/cert/CertificateFactory"),
                jni_str!("getInstance"),
                jni_sig!((JString) -> java.security.cert.CertificateFactory),
                &[JValue::from(&x509)],
            )?
            .l()?;
        let chain = JObjectArray::<JX509Certificate>::new(
            env,
            intermediates.len() + 1,
            JX509Certificate::null(),
        )?;
        for (i, der) in std::iter::once(leaf).chain(intermediates).enumerate() {
            // Bound JNI locals even for a chain supplied by an untrusted peer.
            env.with_local_frame(8, |env| {
                let bytes = env.byte_array_from_slice(der.as_ref())?;
                let stream = env.new_object(
                    jni_str!("java/io/ByteArrayInputStream"),
                    jni_sig!((byte[]) -> void),
                    &[JValue::from(&bytes)],
                )?;
                let cert = env
                    .call_method(
                        &factory,
                        jni_str!("generateCertificate"),
                        jni_sig!((java.io.InputStream) -> java.security.cert.Certificate),
                        &[JValue::from(&stream)],
                    )
                    .map_err(|e| cert_failure(env, e))?
                    .l()?;
                let cert = JX509Certificate::cast_local(env, cert)?;
                chain.set_element(env, i, &cert)?;
                Ok::<_, VerificationFailure>(())
            })?;
        }
        // The Android platform manager requires a non-empty authentication type
        // (Conscrypt does not use it for chain selection). Use the actual leaf
        // key type; rustls independently verifies the negotiated signatures.
        let auth_type = chain
            .get_element(env, 0)?
            .get_public_key(env)?
            .get_algorithm(env)?;
        // Keep the validated authority text for policy lookup. A parsed
        // ServerName canonicalizes IPv6 and can select a different domain rule.
        let host = env.new_string(policy_host)?;
        let chain = JObject::from(chain);
        let trusted = if self.sdk >= 36 {
            // API 36 accepts the optional OCSP staple for policy/CT. Rustls
            // does not expose the TLS SCT extension to its ServerCertVerifier;
            // Android can still check embedded SCTs and those in the staple.
            let staple = if ocsp.is_empty() {
                JByteArray::null()
            } else {
                env.byte_array_from_slice(ocsp)?
            };
            env.call_method(
                &self.manager,
                jni_str!("checkServerTrusted"),
                jni_sig!((java.security.cert.X509Certificate[], byte[], byte[], JString, JString) -> java.util.List),
                &[
                    JValue::from(&chain),
                    JValue::from(&staple),
                    JValue::from(&JObject::null()),
                    JValue::from(&auth_type),
                    JValue::from(&host),
                ],
            )
        } else {
            env.call_method(
                &self.manager,
                jni_str!("checkServerTrusted"),
                jni_sig!((java.security.cert.X509Certificate[], JString, JString) -> java.util.List),
                &[JValue::from(&chain), JValue::from(&auth_type), JValue::from(&host)],
            )
        }
        .map_err(|e| cert_failure(env, e))?
        .l()?;
        let trusted = JList::cast_local(env, trusted)?;
        if trusted.is_null() || trusted.is_empty(env)? {
            return Err(VerificationFailure::Unavailable);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct Verifier {
    app: &'static AppTrust,
    provider: Arc<CryptoProvider>,
    policy_host: String,
}

impl Verifier {
    pub(crate) fn new(
        provider: Arc<CryptoProvider>,
        policy_host: String,
    ) -> Result<Self, TransportError> {
        let app = TRUST.get().ok_or(TransportError::TrustUnavailable)?;
        Ok(Self {
            app,
            provider,
            policy_host,
        })
    }
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        leaf: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        name: &ServerName<'_>,
        ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // X509TrustManagerExtensions uses the host to select chain policy; it
        // does not authenticate the leaf's DNS/IP identity for a rustls socket.
        verify_server_name(&ParsedCertificate::try_from(leaf)?, name)?;
        match self.app.vm.attach_current_thread_for_scope(|env| {
            self.app
                .check_chain(env, leaf, intermediates, &self.policy_host, ocsp)
        }) {
            Ok(()) => Ok(ServerCertVerified::assertion()),
            Err(VerificationFailure::Rejected) => {
                Err(CertificateError::ApplicationVerificationFailure.into())
            }
            Err(VerificationFailure::Unavailable) => Err(rustls::Error::Other(OtherError(
                Arc::new(PlatformTrustUnavailable),
            ))),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
