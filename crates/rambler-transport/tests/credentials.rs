use bytes::Bytes;
use rambler_transport::*;

#[test]
fn fake_environment_all_three_or_none_and_rotation() {
    let provider = EnvironmentCredentials;
    assert!(provider.metadata_with(|_| None).is_ok());
    assert!(matches!(
        provider.metadata_with(|name| (name == "RAMBLER_API_KEY").then(|| "fake-secret".into())),
        Err(TransportError::IncompleteCredentials)
    ));
    for fake_key in ["fake-secret", "fake-rotated"] {
        let metadata = provider
            .metadata_with(|name| {
                Some(match name {
                    "RAMBLER_API_KEY" => fake_key.into(),
                    "RAMBLER_ANDROID_PACKAGE" => "invented.package".into(),
                    "RAMBLER_ANDROID_CERT" => "invented-certificate".into(),
                    _ => panic!("unexpected lookup"),
                })
            })
            .unwrap();
        assert!(!format!("{metadata:?} {provider:?}").contains(fake_key));
    }
}

#[test]
fn strict_validation_and_all_formatting_are_redacted() {
    for (key, value) in [
        ("Upper", CredentialValue::Text("fake-secret".into())),
        ("bad key", CredentialValue::Text("fake-secret".into())),
        ("", CredentialValue::Text("fake-secret".into())),
        ("key", CredentialValue::Text("fake-secret\r\n".into())),
        ("key", CredentialValue::Text("fake-secret\0".into())),
        ("key", CredentialValue::Text("fake-secreté".into())),
        (
            "key",
            CredentialValue::Binary(Bytes::from_static(b"fake-secret")),
        ),
        ("key-bin", CredentialValue::Text("fake-secret".into())),
    ] {
        assert!(!format!("{value:?}").contains("fake-secret"));
        let error = Metadata::new([(key.into(), value)]).unwrap_err();
        assert!(!format!("{error} {error:?}").contains("fake-secret"));
    }
    let error = SourceError::new(std::io::Error::other("fake-secret"));
    for wrapper in [
        TransportError::Source(error.clone()),
        TransportError::Cleanup(error),
    ] {
        assert!(!format!("{wrapper} {wrapper:?}").contains("fake-secret"));
        assert!(std::error::Error::source(&wrapper).is_none());
    }
    for bad in ["fake-secret\n", "fake-secreté", "fake-secret\t"] {
        let result = EnvironmentCredentials.metadata_with(|name| {
            Some(if name == "RAMBLER_API_KEY" {
                bad.into()
            } else {
                "invented".into()
            })
        });
        assert!(matches!(result, Err(TransportError::InvalidMetadata)));
    }
}

#[test]
fn metadata_rejects_the_five_http2_connection_specific_names() {
    let rejected = [
        "connection",
        "keep-alive",
        "proxy-connection",
        "transfer-encoding",
        "upgrade",
    ]
    .into_iter()
    .filter(|key| {
        matches!(
            Metadata::new([((*key).into(), CredentialValue::Text("invented".into()))]),
            Err(TransportError::InvalidMetadata)
        )
    })
    .count();
    assert_eq!(rejected, 5, "all five names must fail metadata validation");
}

#[test]
fn metadata_keeps_custom_grpc_and_control_names_outside_the_five_name_guard() {
    assert!(
        Metadata::new(
            [
                "x-invented",
                "grpc-invented",
                "content-type",
                "te",
                "connection-id",
                "upgrade-token"
            ]
            .into_iter()
            .map(|key| (key.into(), CredentialValue::Text("invented".into())))
        )
        .is_ok()
    );
}

#[test]
fn validation_snapshots_binary_duplicates_printable_ascii_and_limits() {
    let mut value = "original".to_owned();
    let metadata = Metadata::new([
        ("authorization".into(), CredentialValue::Text(value.clone())),
        (
            "authorization".into(),
            CredentialValue::Text("duplicate".into()),
        ),
        (
            "blob-bin".into(),
            CredentialValue::Binary(Bytes::from_static(b"\x00\xff")),
        ),
        ("empty".into(), CredentialValue::Text(String::new())),
        ("punctuation".into(), CredentialValue::Text(" ~".into())),
    ])
    .unwrap();
    value.clear();
    assert!(!format!("{metadata:?}").contains("original"));
    assert!(
        Metadata::new((0..101).map(|_| ("key".into(), CredentialValue::Text("x".into())))).is_err()
    );
    assert!(Metadata::new([("key".into(), CredentialValue::Text("x".repeat(65536)))]).is_err());
    assert!(Metadata::new([("key".into(), CredentialValue::Text("x".repeat(65533)))]).is_ok());
}
