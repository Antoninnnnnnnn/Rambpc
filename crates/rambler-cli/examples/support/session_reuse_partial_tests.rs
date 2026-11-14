use super::*;

#[test]
fn output_hash_framing_distinguishes_boundaries_order_and_empty_previews() {
    assert_ne!(
        ordered_output_hash(&["ab", "c"]),
        ordered_output_hash(&["a", "bc"])
    );
    assert_ne!(
        ordered_output_hash(&["a", "b"]),
        ordered_output_hash(&["b", "a"])
    );
    assert_ne!(ordered_output_hash(&[]), ordered_output_hash(&[""]));
    assert_ne!(ordered_output_hash(&[""]), ordered_output_hash(&["", ""]));
}

#[tokio::test]
async fn unpolled_observation_collector_still_owns_and_cleans_source() {
    struct Source(Arc<AtomicUsize>);
    impl AudioSource for Source {
        fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
            Box::pin(async { Ok(None) })
        }
        fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
            Box::pin(async {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }
    let server = TestServer::new(common::complete).await;
    let engine = Engine::connect(TransportConfig::loopback(server.address).unwrap())
        .await
        .unwrap();
    let closed = Arc::new(AtomicUsize::new(0));
    let mut observations = PartialObservations::default();
    let collection = collect_observed(
        engine.start(Box::new(Source(closed.clone())), SessionOptions::default()),
        &mut observations,
    );
    drop(collection);
    bounded(engine.close()).await.unwrap();
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    assert_eq!(observations.updates, 0);
    bounded(server.close()).await;
}

fn partial_response(
    parts: &[(Option<&str>, Option<f64>)],
    state: u64,
    final_text: bool,
    secondary: bool,
) -> Bytes {
    use rambler_protocol::{
        RECOGNITION_RESPONSE,
        wire::{bytes_field as b, string_field as s, varint_field as v},
    };
    let mut partials = Vec::new();
    for (text, score) in parts {
        let mut part = text.map(|text| s(1, text).unwrap()).unwrap_or_default();
        if let Some(score) = score {
            // Partial confidence is field2/fixed64, not the final hypothesis float.
            part.push(0x11);
            part.extend(score.to_le_bytes());
        }
        partials.extend(b(1, &part).unwrap());
    }
    let mut recognition = [v(1, state).unwrap(), b(4, &partials).unwrap()].concat();
    if final_text {
        let hyp = s(1, "Hello, controlled source.").unwrap();
        recognition.extend(b(3, &b(3, &hyp).unwrap()).unwrap());
    }
    if secondary {
        let hyp = b(3, &s(1, "PRIVATE SECONDARY").unwrap()).unwrap();
        recognition.extend(b(5, &hyp).unwrap());
        recognition.extend(b(7, &hyp).unwrap());
    }
    // Unknown payloads and service language/identifier-like values must not leak.
    recognition.extend(s(99, "PRIVATE UNKNOWN ID").unwrap());
    let result = [
        b(1, &recognition).unwrap(),
        s(3, "PRIVATE LANGUAGE ID").unwrap(),
    ]
    .concat();
    Bytes::from(b(RECOGNITION_RESPONSE, &result).unwrap())
}

fn ordered_output_hash(outputs: &[&str]) -> String {
    let mut framed = Vec::new();
    for output in outputs {
        framed.extend((output.len() as u64).to_le_bytes());
        framed.extend(output.as_bytes());
    }
    hash(&framed)
}

fn manifest(dir: &std::path::Path, count: usize) -> PathBuf {
    let pcm = vec![0; 321 * 2];
    std::fs::write(dir.join("private-audio.wav"), wave(&pcm)).unwrap();
    let case = json!({"input":"private-audio.wav", "language":"en-US",
        "pcm_sha256":hash(&pcm), "markers":["hello", "controlled source"]});
    let path = dir.join("private-manifest.json");
    std::fs::write(&path, serde_json::to_vec(&vec![case; count]).unwrap()).unwrap();
    path
}

#[tokio::test]
async fn native_field4_previews_are_framed_hashed_and_reset_per_session() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let server = TestServer::new(move |request| {
        let observed = observed.clone();
        async move {
            let ordinal = observed.fetch_add(1, Ordering::SeqCst);
            drain(request.into_inner()).await;
            let mut responses = if ordinal == 0 {
                vec![
                    partial_response(
                        &[
                            (Some("PRIVATE stable "), Some(0.9)),
                            (Some("tentative "), Some(0.899)),
                            (Some("latched"), Some(1.0)),
                        ],
                        0,
                        false,
                        true,
                    ),
                    // Missing text (even -inf score) does not trigger the latch.
                    partial_response(
                        &[(None, Some(f64::NEG_INFINITY)), (Some("kept"), Some(0.9))],
                        0,
                        false,
                        false,
                    ),
                    // Present empty text with absent score DOES trigger the latch.
                    partial_response(
                        &[(Some(""), None), (Some("after empty"), Some(1.0))],
                        0,
                        false,
                        false,
                    ),
                    partial_response(&[(Some("default tentative"), None)], 0, false, false),
                    partial_response(&[], 0, false, false),
                    // Native helper deliberately preserves NaN/+inf stability.
                    partial_response(
                        &[
                            (Some("nonfinite"), Some(f64::NAN)),
                            (Some("+inf"), Some(f64::INFINITY)),
                        ],
                        0,
                        false,
                        false,
                    ),
                    // A simultaneous field4/final remains independently observable.
                    partial_response(&[(Some("last preview"), Some(1.0))], 1, true, false),
                ]
            } else {
                vec![response()]
            };
            responses.push(Bytes::from_static(b"\x08\x01"));
            Ok(replies(futures_util::stream::iter(
                responses.into_iter().map(Ok),
            )))
        }
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = args(manifest(dir.path(), 2), Some(server.address));
    options.observe_partials = true;
    let (report, ok) = bounded(run(options)).await;
    assert!(ok, "{report}");
    let first = &report["sessions"][0];
    let observations = &first["partial_observations"];
    assert_eq!(observations["updates"], 7);
    assert_eq!(observations["updates_before_first_state1"], 6);
    assert_eq!(observations["state1_coincident_updates"], 1);
    assert_eq!(observations["parts"], 11);
    assert_eq!(observations["missing_text_parts"], 1);
    assert_eq!(observations["empty_text_parts"], 1);
    assert_eq!(observations["stable_nonempty_updates"], 4);
    assert_eq!(observations["tentative_nonempty_updates"], 3);
    assert_eq!(observations["nonfinite_score_updates"], 1);
    assert_eq!(
        observations["hash_encoding"],
        "ordered_u64le_length_prefixed_utf8_v1"
    );
    assert_eq!(
        observations["stable_outputs_sha256"],
        ordered_output_hash(&[
            "PRIVATE stable ",
            "kept",
            "",
            "",
            "",
            "nonfinite+inf",
            "last preview"
        ])
    );
    assert_eq!(
        observations["tentative_outputs_sha256"],
        ordered_output_hash(&[
            "tentative latched",
            "",
            "after empty",
            "default tentative",
            "",
            "",
            ""
        ])
    );
    assert_eq!(
        observations["rendered_outputs_sha256"],
        ordered_output_hash(&[
            "PRIVATE stable tentative latched",
            "kept",
            "after empty",
            "default tentative",
            "",
            "nonfinite+inf",
            "last preview"
        ])
    );
    assert_eq!(first["final_updates"], 1);
    assert_eq!(first["text_sha256"], hash(b"Hello, controlled source."));
    let second = &report["sessions"][1]["partial_observations"];
    assert_eq!(second["updates"], 0);
    assert_eq!(second["updates_before_first_state1"], 0);
    assert_eq!(second["state1_coincident_updates"], 0);
    assert_eq!(second["parts"], 0);
    for key in [
        "stable_outputs_sha256",
        "tentative_outputs_sha256",
        "rendered_outputs_sha256",
    ] {
        assert_eq!(second[key], hash(b""));
    }
    assert_eq!(report["retry_count"], 0);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(server.accepted_connections.load(Ordering::SeqCst), 1);
    let serialized = report.to_string();
    for private in [
        "PRIVATE",
        "tentative latched",
        "after empty",
        "default tentative",
        "nonfinite+inf",
        "last preview",
        "Hello, controlled",
        "private-audio",
        "private-manifest",
    ] {
        assert!(!serialized.contains(private), "{private}");
    }
    bounded(server.close()).await;
}

#[tokio::test]
async fn pre_state1_counter_excludes_coincident_and_all_later_previews() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let server = TestServer::new(move |request| {
        let observed = observed.clone();
        async move {
            let ordinal = observed.fetch_add(1, Ordering::SeqCst);
            drain(request.into_inner()).await;
            let preview =
                || partial_response(&[(Some("PRIVATE preview"), Some(1.0))], 0, false, false);
            let mut responses = match ordinal {
                0 => vec![partial_response(
                    &[(Some("PRIVATE final preview"), Some(1.0))],
                    1,
                    true,
                    false,
                )],
                // A state1 without field4 must still latch before the preview.
                1 => vec![response(), preview()],
                // A later state0 must not clear the latch; each session resets it.
                2 => vec![preview(), response(), preview()],
                _ => panic!("unexpected replay"),
            };
            responses.push(Bytes::from_static(b"\x08\x01"));
            Ok(replies(futures_util::stream::iter(
                responses.into_iter().map(Ok),
            )))
        }
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = args(manifest(dir.path(), 3), Some(server.address));
    options.observe_partials = true;
    let (report, ok) = bounded(run(options)).await;
    assert!(ok, "{report}");
    for (index, (total, before, coincident)) in
        [(1, 0, 1), (1, 0, 0), (2, 1, 0)].into_iter().enumerate()
    {
        let session = &report["sessions"][index];
        let observations = &session["partial_observations"];
        assert_eq!(observations["updates"], total);
        assert_eq!(observations["updates_before_first_state1"], before);
        assert_eq!(observations["state1_coincident_updates"], coincident);
        assert_eq!(session["final_updates"], 1);
        assert_eq!(session["text_sha256"], hash(b"Hello, controlled source."));
    }
    assert_eq!(report["retry_count"], 0);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(server.accepted_connections.load(Ordering::SeqCst), 1);
    assert!(!report.to_string().contains("PRIVATE"));
    bounded(server.close()).await;
}

#[tokio::test]
async fn observed_partials_never_promote_or_select_secondary_or_retry_failures() {
    // No final; state1 without field3; late gRPC; missing END; S3 error; final limit.
    for mode in 0..6 {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let server = TestServer::new(move |request| {
            let observed = observed.clone();
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                drain(request.into_inner()).await;
                let mut messages = vec![Ok(partial_response(
                    &[(Some("PRIVATE partial only"), Some(1.0))],
                    u64::from(mode == 1),
                    false,
                    true,
                ))];
                if mode >= 2 {
                    messages.push(Ok(response()));
                }
                if mode == 5 {
                    messages.extend((0..128).map(|_| Ok(response())));
                }
                if mode != 3 {
                    messages.push(Ok(Bytes::from_static(b"\x08\x01")));
                }
                if mode == 2 {
                    messages.push(Err(tonic::Status::unavailable("PRIVATE LATE GRPC")));
                }
                if mode == 4 {
                    messages.push(Ok(Bytes::from_static(b"\x08\x02")));
                }
                Ok(replies(futures_util::stream::iter(messages)))
            }
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let mut options = args(manifest(dir.path(), 2), Some(server.address));
        options.observe_partials = true;
        let (report, ok) = bounded(run(options)).await;
        assert!(!ok, "{report}");
        assert_eq!(report["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(report["sessions"][0]["partial_observations"]["updates"], 1);
        assert_eq!(
            report["sessions"][0]["category"],
            match mode {
                0 | 1 => "ambiguous",
                2 => "grpc",
                3 => "incomplete",
                _ => "engine",
            }
        );
        assert_eq!(report["retry_count"], 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(server.accepted_connections.load(Ordering::SeqCst), 1);
        assert!(!report.to_string().contains("PRIVATE"));
        bounded(server.close()).await;
    }
}

#[tokio::test]
async fn one_case_requires_partial_opt_in_and_observation_bounds_stay_closed() {
    let server = TestServer::new(|request| async move {
        drain(request.into_inner()).await;
        Ok(replies(futures_util::stream::iter([
            Ok(response()),
            Ok(Bytes::from_static(b"\x08\x01")),
        ])))
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = manifest(dir.path(), 1);
    let (report, ok) = bounded(run(args(path.clone(), Some(server.address)))).await;
    assert!(!ok);
    assert_eq!(report["category"], "manifest");
    assert_eq!(server.accepted_connections.load(Ordering::SeqCst), 0);
    let mut options = args(path, Some(server.address));
    options.observe_partials = true;
    let (report, ok) = bounded(run(options)).await;
    assert!(ok, "{report}");
    assert_eq!(report["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(report["sessions"][0]["partial_observations"]["updates"], 0);
    assert_eq!(report["retry_count"], 0);
    bounded(server.close()).await;
    for count in [0, 4] {
        let mut options = args(
            manifest(dir.path(), count),
            Some("127.0.0.1:1".parse().unwrap()),
        );
        options.observe_partials = true;
        let (report, ok) = run(options).await;
        assert!(!ok);
        assert_eq!(report["category"], "manifest");
    }
}
