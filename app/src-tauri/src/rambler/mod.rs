//! Adapter to the Rambler/Jetson SDK (`rambler-engine`, `rambler-audio`,
//! `rambler-transport`). The only speech engine. Owned by the "core" work package.
//!
//! Responsibilities: keep one warm `Engine` (connect_ready after consent and
//! access), run one take as `capture_channel` -> (`Pcm48kTo16k`) ->
//! `stream_long_pcm_dictation` with `InteractivePcmProfile::default()`,
//! `ContextInput::None`, interpret each segment with `RenderPolicy::AsrOnly`,
//! and report admitted segments / typed outcome to the controller.

mod capture;
mod recognition;

pub(crate) use capture::{CaptureHealth, FRAME_WAIT_MS, OpenedMic, open};
pub(crate) use recognition::{MAX_DRAFT_BYTES, Segment, collect, connection_failure};

use rambler_transport::{EnvironmentCredentials, Metadata, TransportConfig};
use zeroize::Zeroizing;

/// Rambler access configured by the person: service address and the three
/// access values. Never logged; `Debug` is redacted. Never stored in the binary.
#[derive(Clone)]
pub struct RamblerAccess {
    authority: String,
    metadata: Metadata,
}

impl std::fmt::Debug for RamblerAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RamblerAccess(..)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AccessError {
    #[error("L’adresse du service est invalide.")]
    InvalidAuthority,
    #[error("Les données d’accès sont incomplètes ou invalides.")]
    InvalidCredentials,
}

pub const ENV_AUTHORITY: &str = "RAMBLER_AUTHORITY";

impl RamblerAccess {
    pub fn new(
        authority: &str,
        api_key: Zeroizing<String>,
        android_package: Zeroizing<String>,
        android_cert: Zeroizing<String>,
    ) -> Result<Self, AccessError> {
        let authority = authority.trim().to_owned();
        TransportConfig::tls(authority.clone()).map_err(|_| AccessError::InvalidAuthority)?;
        let metadata = EnvironmentCredentials
            .required_snapshot_with(|name| {
                Ok(match name {
                    "RAMBLER_API_KEY" => api_key.trim().to_owned(),
                    "RAMBLER_ANDROID_PACKAGE" => android_package.trim().to_owned(),
                    "RAMBLER_ANDROID_CERT" => android_cert.trim().to_owned(),
                    _ => String::new(),
                })
            })
            .map_err(|_| AccessError::InvalidCredentials)?;
        Ok(Self {
            authority,
            metadata,
        })
    }

    /// `RAMBLER_AUTHORITY` plus `RAMBLER_API_KEY`, `RAMBLER_ANDROID_PACKAGE`,
    /// `RAMBLER_ANDROID_CERT` from the process environment. `Ok(None)` when
    /// none of them is set.
    pub fn from_environment() -> Result<Option<Self>, AccessError> {
        let get = |name: &str| Zeroizing::new(std::env::var(name).unwrap_or_default());
        let authority = get(ENV_AUTHORITY);
        let (key, package, cert) = (
            get("RAMBLER_API_KEY"),
            get("RAMBLER_ANDROID_PACKAGE"),
            get("RAMBLER_ANDROID_CERT"),
        );
        if authority.is_empty() && key.is_empty() && package.is_empty() && cert.is_empty() {
            return Ok(None);
        }
        Self::new(&authority, key, package, cert).map(Some)
    }

    pub fn authority(&self) -> &str {
        &self.authority
    }

    pub fn transport_config(&self) -> TransportConfig {
        TransportConfig::tls(self.authority.clone()).expect("validated at construction")
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
}
