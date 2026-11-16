//! Validated, redacted, immutable per-session metadata. No credentials are embedded.
use bytes::Bytes;
use std::{fmt, future::Future, pin::Pin};
use tonic::metadata::{Ascii, Binary, MetadataKey, MetadataMap, MetadataValue};

use crate::TransportError;

/// A value before validation. Debug deliberately omits both text and bytes.
#[derive(Clone)]
pub enum CredentialValue {
    Text(String),
    Binary(Bytes),
}
impl fmt::Debug for CredentialValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CredentialValue([redacted])")
    }
}

/// A validated, owned snapshot, capped at 100 entries / 65536 key+value bytes.
#[derive(Clone, Default)]
pub struct Metadata(MetadataMap);
impl fmt::Debug for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Metadata([redacted])")
    }
}
impl Metadata {
    /// Validates and snapshots caller metadata without stripping or rewriting it.
    ///
    /// Returns [`TransportError::InvalidMetadata`] for invalid names, values, or size,
    /// including the HTTP/2-prohibited connection-specific names `connection`,
    /// `keep-alive`, `proxy-connection`, `transfer-encoding`, and `upgrade`.
    /// This fail-fast client validation does not establish remote authentication.
    pub fn new(
        entries: impl IntoIterator<Item = (String, CredentialValue)>,
    ) -> Result<Self, TransportError> {
        let mut map = MetadataMap::new();
        let mut size = 0usize;
        for (index, (key, value)) in entries.into_iter().enumerate() {
            if index >= 100
                || key.is_empty()
                || key.len() > 65536
                || matches!(
                    key.as_str(),
                    "connection"
                        | "keep-alive"
                        | "proxy-connection"
                        | "transfer-encoding"
                        | "upgrade"
                )
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
            {
                return Err(TransportError::InvalidMetadata);
            }
            let len = match &value {
                CredentialValue::Text(v) => v.len(),
                CredentialValue::Binary(v) => v.len(),
            };
            size = size
                .checked_add(key.len())
                .and_then(|v| v.checked_add(len))
                .ok_or(TransportError::InvalidMetadata)?;
            if size > 65536 {
                return Err(TransportError::InvalidMetadata);
            }
            match value {
                CredentialValue::Text(value)
                    if !key.ends_with("-bin") && value.bytes().all(|b| (32..=126).contains(&b)) =>
                {
                    let key = MetadataKey::<Ascii>::from_bytes(key.as_bytes())
                        .map_err(|_| TransportError::InvalidMetadata)?;
                    let mut value = MetadataValue::<Ascii>::try_from(value)
                        .map_err(|_| TransportError::InvalidMetadata)?;
                    value.set_sensitive(true);
                    map.append(key, value);
                }
                CredentialValue::Binary(value) if key.ends_with("-bin") => {
                    let key = MetadataKey::<Binary>::from_bytes(key.as_bytes())
                        .map_err(|_| TransportError::InvalidMetadata)?;
                    let mut value = MetadataValue::<Binary>::from_bytes(&value);
                    value.set_sensitive(true);
                    map.append_bin(key, value);
                }
                _ => return Err(TransportError::InvalidMetadata),
            }
        }
        Ok(Self(map))
    }
    pub(crate) fn into_map(self) -> MetadataMap {
        self.0
    }
}

pub type MetadataFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Metadata, TransportError>> + Send + 'a>>;
/// Called once per session, inside its deadline, before invoking gRPC.
/// Futures must be cancellation-safe and cooperative. Return validated snapshots.
pub trait MetadataProvider: Send + Sync + 'static {
    fn metadata(&self) -> MetadataFuture<'_>;
}
impl MetadataProvider for Metadata {
    fn metadata(&self) -> MetadataFuture<'_> {
        Box::pin(async { Ok(self.clone()) })
    }
}

#[derive(Default, Debug)]
pub struct EnvironmentCredentials;
impl EnvironmentCredentials {
    /// Injectable lookup: tests need never read or mutate the real environment.
    pub fn metadata_with(
        &self,
        mut lookup: impl FnMut(&str) -> Option<String>,
    ) -> Result<Metadata, TransportError> {
        let values = [
            "RAMBLER_API_KEY",
            "RAMBLER_ANDROID_PACKAGE",
            "RAMBLER_ANDROID_CERT",
        ]
        .map(|name| lookup(name).unwrap_or_default());
        Self::validate_snapshot(values, false)
    }

    /// Requires all three credentials and returns an immutable, validated snapshot.
    /// Each exact variable is read once, including when another value is invalid.
    /// No credential values are logged or included in errors.
    pub fn required_snapshot_with(
        &self,
        lookup: impl FnMut(&str) -> Result<String, std::env::VarError>,
    ) -> Result<Metadata, TransportError> {
        Self::environment_snapshot_with(true, lookup)
    }

    /// Strict admission for callers that must reject anonymous sessions locally.
    /// The general [`MetadataProvider`] implementation remains anonymous-capable.
    pub fn required_snapshot(&self) -> Result<Metadata, TransportError> {
        self.required_snapshot_with(|name| std::env::var(name))
    }

    fn environment_snapshot_with(
        required: bool,
        mut lookup: impl FnMut(&str) -> Result<String, std::env::VarError>,
    ) -> Result<Metadata, TransportError> {
        let mut invalid = false;
        let values = [
            "RAMBLER_API_KEY",
            "RAMBLER_ANDROID_PACKAGE",
            "RAMBLER_ANDROID_CERT",
        ]
        .map(|name| match lookup(name) {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => String::new(),
            Err(std::env::VarError::NotUnicode(_)) => {
                invalid = true;
                String::new()
            }
        });
        if invalid {
            return Err(TransportError::InvalidMetadata);
        }
        Self::validate_snapshot(values, required)
    }

    fn validate_snapshot(values: [String; 3], required: bool) -> Result<Metadata, TransportError> {
        let present = values.iter().filter(|v| !v.is_empty()).count();
        if present == 0 && !required {
            return Ok(Metadata::default());
        }
        if present != 3 {
            return Err(TransportError::IncompleteCredentials);
        }
        Metadata::new(
            ["x-goog-api-key", "x-android-package", "x-android-cert"]
                .into_iter()
                .zip(values)
                .map(|(k, v)| (k.to_owned(), CredentialValue::Text(v))),
        )
    }
}
impl MetadataProvider for EnvironmentCredentials {
    fn metadata(&self) -> MetadataFuture<'_> {
        Box::pin(async { Self::environment_snapshot_with(false, |name| std::env::var(name)) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connection_specific_metadata_survives_tonic_conversion_but_h2_rejects_it() {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            for key in [
                "connection",
                "keep-alive",
                "proxy-connection",
                "transfer-encoding",
                "upgrade",
                "x-invented-control",
            ] {
                // Bypass input validation so the underlying transport guard remains tested.
                let mut raw = MetadataMap::new();
                let mut value = MetadataValue::<Ascii>::from_static("invented");
                value.set_sensitive(true);
                raw.insert(
                    MetadataKey::<Ascii>::from_bytes(key.as_bytes()).unwrap(),
                    value,
                );
                let mut request = tonic::Request::new(());
                *request.metadata_mut() = Metadata(raw).into_map();
                let headers = request.into_parts().0.into_headers();
                let value = headers.get(key).expect("tonic must preserve the header");
                assert_eq!(value, "invented");
                assert!(value.is_sensitive());

                let (client_io, _peer_io) = tokio::io::duplex(4096);
                let (mut sender, _connection) = h2::client::handshake(client_io)
                    .await
                    .expect("in-memory client handshake must succeed");
                let mut request = http::Request::builder()
                    .uri("http://127.0.0.1/metadata-premise")
                    .version(http::Version::HTTP_2)
                    .body(())
                    .unwrap();
                *request.headers_mut() = headers;
                // h2 checks these headers synchronously, before its connection is driven.
                let result = sender.send_request(request, true);
                if key == "x-invented-control" {
                    assert!(result.is_ok(), "h2 must accept valid custom metadata");
                } else {
                    assert!(
                        result.is_err(),
                        "h2 must reject connection-specific metadata"
                    );
                }
            }
        })
        .await
        .expect("in-memory metadata checks must finish within five seconds");
    }

    #[test]
    fn duplicate_text_and_binary_values_keep_sensitive_flags_when_converted_to_headers() {
        let metadata = Metadata::new([
            (
                "authorization".into(),
                CredentialValue::Text("invented-first".into()),
            ),
            (
                "authorization".into(),
                CredentialValue::Text("invented-second".into()),
            ),
            (
                "proof-bin".into(),
                CredentialValue::Binary(Bytes::from_static(b"\0\xff")),
            ),
        ])
        .unwrap();
        // Exercise the same tonic Request conversion as the h2 RPC writer.
        let mut request = tonic::Request::new(());
        *request.metadata_mut() = metadata.into_map();
        request.set_timeout(std::time::Duration::from_secs(5));
        let headers = request.into_parts().0.into_headers();
        assert_eq!(headers.get_all("authorization").iter().count(), 2);
        assert_eq!(headers.get_all("proof-bin").iter().count(), 1);
        for key in ["authorization", "proof-bin"] {
            assert!(
                headers
                    .get_all(key)
                    .iter()
                    .all(|value| value.is_sensitive())
            );
        }
    }

    #[test]
    fn fake_lookup_snapshots_exact_headers_rotation_and_sensitive_flags() {
        let provider = EnvironmentCredentials;
        let first = provider
            .metadata_with(|name| {
                Some(match name {
                    "RAMBLER_API_KEY" => "invented-key".into(),
                    "RAMBLER_ANDROID_PACKAGE" => "invented.package".into(),
                    "RAMBLER_ANDROID_CERT" => "invented-cert".into(),
                    _ => panic!("unexpected name"),
                })
            })
            .unwrap()
            .into_map();
        assert_eq!(first.get("x-goog-api-key").unwrap(), "invented-key");
        assert_eq!(first.get("x-android-package").unwrap(), "invented.package");
        assert_eq!(first.get("x-android-cert").unwrap(), "invented-cert");
        assert!(first.get("x-goog-api-key").unwrap().is_sensitive());
        let rotated = provider
            .metadata_with(|_| Some("invented-rotated".into()))
            .unwrap()
            .into_map();
        assert_eq!(rotated.get("x-goog-api-key").unwrap(), "invented-rotated");
        assert_eq!(first.get("x-goog-api-key").unwrap(), "invented-key");
        assert!(
            Metadata::new([("x".repeat(65537), CredentialValue::Text(String::new()))]).is_err()
        );
    }
}
