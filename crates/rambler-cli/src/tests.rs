use super::{admit_credentials, failure_output, supervise};
use rambler_engine::{DiagnosticMetrics, EngineError, IncompleteSessionDiagnostic, SourceError};
use rambler_transport::Completion;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

struct PendingCollection(Arc<AtomicBool>);

struct AdmissionAudio {
    closed: Arc<AtomicBool>,
    fail_close: bool,
}
impl rambler_engine::AudioSource for AdmissionAudio {
    fn next(
        &mut self,
    ) -> rambler_engine::SourceFuture<'_, Result<Option<bytes::Bytes>, SourceError>> {
        panic!("admission must not poll audio")
    }

    fn close(&mut self) -> rambler_engine::SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async {
            tokio::task::yield_now().await;
            self.closed.store(true, Ordering::SeqCst);
            if self.fail_close {
                Err(SourceError::new(std::io::Error::other(
                    "invented-close-detail",
                )))
            } else {
                Ok(())
            }
        })
    }
}

#[test]
fn strict_snapshot_reads_exact_names_once_and_preserves_permissive_lookup() {
    use rambler_transport::{EnvironmentCredentials, TransportError};
    let expected = [
        "RAMBLER_API_KEY",
        "RAMBLER_ANDROID_PACKAGE",
        "RAMBLER_ANDROID_CERT",
    ];
    for values in [
        [None, None, None],
        [Some(""), Some(""), Some("")],
        [Some("invented"), None, Some("invented")],
    ] {
        let mut names = Vec::new();
        let result = EnvironmentCredentials.required_snapshot_with(|name| {
            let index = names.len();
            names.push(name.to_owned());
            values[index]
                .map(str::to_owned)
                .ok_or(std::env::VarError::NotPresent)
        });
        assert!(matches!(result, Err(TransportError::IncompleteCredentials)));
        assert_eq!(names, expected);
    }
    for invalid in ["invented\nvalue", "invented-é"] {
        let mut calls = 0;
        let result = EnvironmentCredentials.required_snapshot_with(|_| {
            calls += 1;
            Ok(invalid.into())
        });
        assert!(matches!(result, Err(TransportError::InvalidMetadata)));
        assert_eq!(calls, 3);
    }
    let mut calls = 0;
    let result = EnvironmentCredentials.required_snapshot_with(|_| {
        calls += 1;
        if calls == 1 {
            Err(std::env::VarError::NotUnicode("invented".into()))
        } else {
            Err(std::env::VarError::NotPresent)
        }
    });
    assert!(matches!(result, Err(TransportError::InvalidMetadata)));
    assert_eq!(calls, 3);
    assert!(EnvironmentCredentials.metadata_with(|_| None).is_ok());
    assert!(
        EnvironmentCredentials
            .metadata_with(|_| Some(String::new()))
            .is_ok()
    );
}

#[tokio::test]
async fn admission_rejects_locally_and_awaits_cleanup_without_polling() {
    use rambler_transport::EnvironmentCredentials;
    for fail_close in [false, true] {
        let closed = Arc::new(AtomicBool::new(false));
        let mut source = AdmissionAudio {
            closed: closed.clone(),
            fail_close,
        };
        let result = admit_credentials(&mut source, false, || {
            EnvironmentCredentials.required_snapshot_with(|_| Err(std::env::VarError::NotPresent))
        })
        .await;
        let value = match result {
            Err(value) => value,
            Ok(_) => panic!("anonymous normal endpoint must be rejected"),
        };
        assert!(closed.load(Ordering::SeqCst));
        assert_eq!(value["category"], "credentials");
        assert!(value["grpc_code"].is_null());
        assert_eq!(value.get("close_error_category").is_some(), fail_close);
        if fail_close {
            assert_eq!(value["close_error_category"], "cleanup");
        }
        assert!(!value.to_string().contains("invented-close-detail"));
    }
}

#[tokio::test]
async fn loopback_admission_skips_lookup_and_normal_admission_reuses_snapshot() {
    use rambler_engine::AudioSource;
    use rambler_transport::EnvironmentCredentials;
    let closed = Arc::new(AtomicBool::new(false));
    let mut source = AdmissionAudio {
        closed: closed.clone(),
        fail_close: false,
    };
    let credentials = admit_credentials(&mut source, true, || {
        panic!("explicit loopback must not inspect credentials")
    })
    .await
    .unwrap();
    assert!(credentials.is_none());
    assert!(!closed.load(Ordering::SeqCst));
    let mut reads = 0;
    let credentials = admit_credentials(&mut source, false, || {
        EnvironmentCredentials.required_snapshot_with(|_| {
            reads += 1;
            Ok("invented-credential".into())
        })
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(reads, 3);
    credentials.metadata().await.unwrap();
    credentials.metadata().await.unwrap();
    assert_eq!(reads, 3);
    assert!(!closed.load(Ordering::SeqCst));
    source.close().await.unwrap();
}

#[test]
fn absent_pcm_observation_preserves_terminal_json_exactly() {
    let observation = super::PcmObservation::default();
    let value = failure_output(&EngineError::FinalizeTimeout, None, None);
    assert_eq!(observation.append_to(value.clone()), value);
}

#[cfg(feature = "audio")]
#[tokio::test]
async fn pcm_terminal_observation_is_exact_and_does_not_claim_eof_after_close() {
    use rambler_audio::{EncoderConfig, Pcm16Opus};
    use rambler_engine::AudioSource;
    let closed = Arc::new(AtomicBool::new(false));
    let (mut audio, handle) = Pcm16Opus::new_with_progress(
        AdmissionAudio {
            closed: closed.clone(),
            fail_close: false,
        },
        EncoderConfig::default(),
    );
    audio.close().await.unwrap();
    assert!(closed.load(Ordering::SeqCst));
    let observation = super::PcmObservation {
        handle: Some(handle),
    };
    let value = observation.append_to(failure_output(&EngineError::FinalizeTimeout, None, None));
    assert_eq!(
        value["pcm"],
        serde_json::json!({
            "input_bytes": 0,
            "encoded_input_samples": 0,
            "encoded_frames": 0,
            "input_eof": false,
            "output_eof": false,
        })
    );
    assert_eq!(value["category"], "finalize_timeout");
}

#[cfg(feature = "audio")]
#[tokio::test]
async fn missing_credentials_report_zero_pcm_after_awaited_cleanup_without_network() {
    use rambler_audio::{EncoderConfig, Pcm16Opus};
    use rambler_transport::TransportError;
    let closed = Arc::new(AtomicBool::new(false));
    let (mut audio, handle) = Pcm16Opus::new_with_progress(
        AdmissionAudio {
            closed: closed.clone(),
            fail_close: false,
        },
        EncoderConfig::default(),
    );
    // Exercise admission directly: no endpoint or connection future exists,
    // including if credential validation regresses. next() panics if polled.
    let rejection = admit_credentials(&mut audio, false, || {
        Err(TransportError::IncompleteCredentials)
    })
    .await
    .err()
    .expect("missing credentials must be rejected");
    assert!(closed.load(Ordering::SeqCst));
    let value = super::PcmObservation {
        handle: Some(handle),
    }
    .append_to(rejection);
    assert_eq!(value["category"], "credentials");
    assert_eq!(
        value["pcm"],
        serde_json::json!({
            "input_bytes": 0, "encoded_input_samples": 0, "encoded_frames": 0,
            "input_eof": false, "output_eof": false,
        })
    );
}

#[test]
fn incomplete_output_contains_only_explicit_completion_diagnostics() {
    let error = EngineError::IncompleteSession(Arc::new(IncompleteSessionDiagnostic {
        saw_s3_end: true,
        completion: Completion {
            requests_produced: 3,
            requests_polled: 3,
            responses_received: 2,
            source_exhausted: false,
            upload_eof_polled: false,
            source_closed: true,
            terminal_code: Some(tonic::Code::Ok),
        },
        metrics: DiagnosticMetrics {
            audio: rambler_engine::EncodedAudioProgress {
                chunks: 1,
                bytes: 19,
                eof: false,
                cleanup_finished: true,
                cleanup_succeeded: true,
            },
            requests_produced: 3,
            request_protobuf_bytes: 100,
            responses_received: 2,
            response_protobuf_bytes: 20,
            elapsed_seconds: 2.0,
            first_response_seconds: Some(0.1),
            eof_seconds: None,
            timeline: Default::default(),
        },
    }));
    let value = failure_output(&error, None, None);
    assert_eq!(value["category"], "incomplete");
    assert_eq!(value["incomplete"]["saw_s3_end"], true);
    assert_eq!(value["incomplete"]["completion"]["source_exhausted"], false);
    assert_eq!(value["incomplete"]["completion"]["terminal_code"], "Ok");
    assert_eq!(value["incomplete"]["metrics"]["requests_produced"], 3);
    assert_eq!(value["incomplete"]["metrics"]["audio"]["bytes"], 19);
    assert_eq!(value["incomplete"]["metrics"]["audio"]["eof"], false);
    let serialized = value.to_string();
    for forbidden in ["session_id", "raw", "text", "metadata"] {
        assert!(!serialized.contains(&format!("\"{forbidden}\":")));
    }
    let sensitive = EngineError::Audio(SourceError::new(std::io::Error::other(
        "PRIVATE-AUDIO-DETAIL",
    )));
    let value = failure_output(&sensitive, Some(&sensitive), None);
    assert_eq!(value["category"], "audio");
    assert!(!value.to_string().contains("PRIVATE-AUDIO-DETAIL"));
    assert!(value.get("incomplete").is_none());
}

impl Future for PendingCollection {
    type Output = Result<u32, &'static str>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}
impl Drop for PendingCollection {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn interruption_drops_collector_before_waiting_for_cleanup() {
    let dropped = Arc::new(AtomicBool::new(false));
    let check = dropped.clone();
    let (result, cleanup, interrupted) = supervise(
        PendingCollection(dropped),
        move || async move {
            assert!(check.load(Ordering::SeqCst));
            tokio::task::yield_now().await;
            Err("cleanup")
        },
        async { true },
    )
    .await;
    assert!(result.is_none());
    assert_eq!(cleanup, Err("cleanup"));
    assert_eq!(interrupted, Some(true));
}

#[tokio::test]
async fn interrupt_during_cleanup_discards_completed_result_and_awaits_cleanup() {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let flag = finished.clone();
    let (result, cleanup, interrupted) = supervise(
        async { Ok::<_, ()>(42) },
        move || async move {
            sender.send(()).unwrap();
            tokio::task::yield_now().await;
            flag.store(true, Ordering::SeqCst);
            Ok(())
        },
        async {
            receiver.await.unwrap();
            true
        },
    )
    .await;
    assert_eq!(result, Some(Ok(42)));
    assert_eq!(cleanup, Ok(()));
    assert_eq!(interrupted, Some(true));
    assert!(finished.load(Ordering::SeqCst));
}

#[test]
fn finalize_timeout_cli_defaults_validation_and_category() {
    use super::{Cli, Command};
    use clap::Parser;
    let mut commands = vec!["dictate"];
    if cfg!(feature = "audio") {
        commands.push("dictate-wav");
    }
    for command in commands {
        let parse = |extra: &[&str]| {
            let mut args = vec!["rambler", command, "synthetic-input"];
            args.extend_from_slice(extra);
            Cli::try_parse_from(args)
        };
        let common = |cli: Cli| match cli.command {
            Command::Dictate { common } => common,
            #[cfg(feature = "audio")]
            Command::DictateWav { common, .. } => common,
        };
        let defaults = common(parse(&[]).unwrap());
        assert_eq!(defaults.finalize_timeout, None);
        assert_eq!(defaults.timeout, 30);
        assert_eq!(defaults.context_timeout, 2);
        for (value, expected) in [("1", 1), ("7", 7), ("18446744073709551615", u64::MAX)] {
            let budgets = common(
                parse(&[
                    "--timeout",
                    value,
                    "--context-timeout",
                    value,
                    "--finalize-timeout",
                    value,
                ])
                .unwrap(),
            );
            assert_eq!(budgets.timeout, expected);
            assert_eq!(budgets.context_timeout, expected);
            assert_eq!(budgets.finalize_timeout, Some(expected));
        }
        // CLI-only validation: parser tests never open input or connect,
        // even if a range check regresses. Library validation is unchanged.
        for flag in ["--timeout", "--context-timeout", "--finalize-timeout"] {
            for invalid in ["0", "-1", "1.5", "word", "18446744073709551616"] {
                assert!(parse(&[flag, invalid]).is_err());
            }
        }
    }
    let value = failure_output(&EngineError::FinalizeTimeout, None, None);
    assert_eq!(value["category"], "finalize_timeout");
    assert!(value["grpc_code"].is_null());
    assert!(!value.to_string().contains("deadline"));
}
