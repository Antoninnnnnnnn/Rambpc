use rambler_transport::{EnvironmentCredentials, TransportError};

const NAMES: [&str; 3] = [
    "RAMBLER_API_KEY",
    "RAMBLER_ANDROID_PACKAGE",
    "RAMBLER_ANDROID_CERT",
];

#[test]
fn required_snapshot_rejects_missing_empty_and_partial_credentials() {
    let provider = EnvironmentCredentials;
    assert!(provider.metadata_with(|_| None).is_ok());
    for values in [
        [None, None, None],
        [Some(""), Some(""), Some("")],
        [Some("invented-key"), None, None],
        [Some("invented-key"), Some(""), Some("invented-cert")],
        [None, Some("invented-package"), Some("invented-cert")],
    ] {
        let result = provider.required_snapshot_with(|name| {
            values[NAMES
                .iter()
                .position(|candidate| *candidate == name)
                .unwrap()]
            .map(str::to_owned)
            .ok_or(std::env::VarError::NotPresent)
        });
        assert!(matches!(result, Err(TransportError::IncompleteCredentials)));
    }
}

#[test]
fn required_snapshot_reads_each_name_once_and_redacts_the_result() {
    let provider = EnvironmentCredentials;
    let mut calls = Vec::new();
    let metadata = provider
        .required_snapshot_with(|name| {
            calls.push(name.to_owned());
            Ok(match name {
                "RAMBLER_API_KEY" => "invented-secret-key",
                "RAMBLER_ANDROID_PACKAGE" => "invented-secret-package",
                "RAMBLER_ANDROID_CERT" => "invented-secret-certificate",
                _ => panic!("unexpected credential lookup"),
            }
            .to_owned())
        })
        .unwrap();
    calls.sort();
    let mut expected = NAMES.map(str::to_owned);
    expected.sort();
    assert_eq!(calls, expected);
    let debug = format!("{metadata:?} {provider:?}");
    assert!(!debug.contains("invented-secret"));
}

#[test]
fn required_snapshot_rejects_invalid_values_and_redacts_errors() {
    for bad in [
        "invented-secret\n".to_owned(),
        "invented-secret\0".to_owned(),
        "invented-secreté".to_owned(),
        format!("invented-secret{}", "x".repeat(65536)),
    ] {
        for name_to_change in NAMES {
            let result = EnvironmentCredentials.required_snapshot_with(|name| {
                Ok(if name == name_to_change {
                    bad.clone()
                } else {
                    "invented-valid".into()
                })
            });
            let error = result.unwrap_err();
            assert!(matches!(error, TransportError::InvalidMetadata));
            assert!(!format!("{error} {error:?}").contains("invented-secret"));
        }
    }
}
