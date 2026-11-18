//! Test-only bridge from fixed SDK fixtures to the offline Python checker.

#[path = "compatible_edit_acceptance.rs"]
mod compatible;

use super::{
    Check, Failed, Peer, SESSION_DEADLINE, audio_common, check, fixtures, prepare_cases, with_peer,
};
use rambler_audio::Pcm16Opus;
use rambler_engine::{
    CollectorLimits, ContextInput, DictationResult, Engine, EngineError, SessionOptions,
    collect_dictation, replacement_proposal,
};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tokio::{process::Command, time::timeout};

fn provenance() -> Value {
    // Labels scoped to this one isolated invocation of the fixed fixture test binary;
    // not a way to identify or authenticate a separately supplied executable or run.
    json!({
        "corpus_id": "native-edit-fixtures-v1",
        "run_id": "one-owned-loopback-run",
        "candidate_id": "native-example-test-binary",
        "policy": "strict",
    })
}

fn declaration(selection_end: usize) -> Value {
    // Manually declared oracle: 13 UTF-16 units before the replacement selection
    // and 9 units in it. Neither fixture.expected nor the SDK produces this document.
    json!({
        "schema": "rambpc-edit-expectations-v1",
        "artifact_id": "predeclared-edit-oracle",
        "provenance": provenance(),
        "cases": [
            {"case_id": "selection", "input_id": "selection-input-v1", "intent": 2,
             "outcome": {"kind": "edit", "window": "Préface 🧭 | révisé 🪐 | épilogue 📚",
                         "selection_utf16": [13, selection_end]}},
            {"case_id": "rewrite", "input_id": "rewrite-input-v1", "intent": 3,
             "outcome": {"kind": "no_edit"}},
        ],
    })
}

async fn collect_result(engine: &Engine, peer: &Peer, ordinal: usize) -> Check<DictationResult> {
    let step = &peer.invocations[ordinal];
    let fixture = &peer.cases[step.case_index].fixture;
    let pcm = audio_common::Source::new([fixture.pcm.clone()]);
    let source = Pcm16Opus::new(pcm, Default::default());
    let result = collect_dictation(
        engine,
        Box::new(source),
        SessionOptions {
            config: step.config.clone(),
            context: ContextInput::Static(fixture.context.clone()),
            timeout: SESSION_DEADLINE,
            ..Default::default()
        },
        CollectorLimits::default(),
    )
    .await
    .map_err(|_| Failed)?;
    // The shared peer only sends the fixed final after verifying init, audio,
    // context, END and half-close. Also check the collector's retained context
    // and response, independently of the proposal fields under comparison.
    peer.confirm(ordinal)?;
    check(
        result.context.as_ref() == Some(&fixture.context)
            && result.final_updates.len() == 1
            && result.final_updates[0].raw == fixture.replies[0],
    )?;
    Ok(result)
}

async fn collect_row(engine: &Engine, peer: &Peer, ordinal: usize) -> Check<Value> {
    let fixture = &peer.cases[peer.invocations[ordinal].case_index].fixture;
    let result = collect_result(engine, peer, ordinal).await?;
    let hypothesis = result.single_hypothesis().map_err(|_| Failed)?;
    let intent = match hypothesis.voice_edit.as_ref() {
        Some(edit) => Some(edit.intent.ok_or(Failed)?),
        None => None,
    };
    let outcome = match replacement_proposal(&result, None) {
        Ok(proposal) => {
            check(proposal.snapshot == fixture.context)?;
            json!({"kind": "edit", "selection_utf16": proposal.selection_utf16_range(),
                   "window": proposal.replacement_text})
        }
        Err(EngineError::UnsupportedEdit) => json!({"kind": "no_edit"}),
        Err(_) => return Err(Failed),
    };
    Ok(json!({
        "case_id": fixture.id, "input_id": format!("{}-input-v1", fixture.id),
        "status": "observed", "intent": intent, "outcome": outcome,
    }))
}

async fn collect_rows(engine: &Engine, peer: &Peer) -> Check<Vec<Value>> {
    let mut rows = Vec::with_capacity(peer.invocations.len());
    for ordinal in 0..peer.invocations.len() {
        rows.push(
            timeout(SESSION_DEADLINE, collect_row(engine, peer, ordinal))
                .await
                .map_err(|_| Failed)??,
        );
    }
    Ok(rows)
}

fn write_fixture(path: &Path, value: &Value) {
    let bytes = serde_json::to_vec(value).expect("serialize fixed synthetic data");
    assert!(bytes.len() <= 4096, "synthetic artifact exceeded its bound");
    assert!(fs::write(path, bytes).is_ok(), "write synthetic artifact");
}

async fn verify_checker(
    declaration: &Path,
    observations: &Path,
    options: &[&str],
    exit_code: i32,
    wanted: Value,
) {
    let checker =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/check_edit_acceptance.py");
    let output = timeout(
        SESSION_DEADLINE,
        Command::new("python")
            .args(["-I", "-S", "-B"])
            .arg(checker)
            .args(options)
            .arg(declaration)
            .arg(observations)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .expect("run the Python checker with a trusted python on PATH");
    let actual = serde_json::from_slice::<Value>(&output.stdout).ok();
    assert!(
        output.status.code() == Some(exit_code)
            && output.stderr.is_empty()
            && actual.as_ref() == Some(&wanted),
        "unexpected checker exit or aggregate for expected exit {exit_code}"
    );
    // Print only the already-verified aggregate, never Python's captured output
    // or either synthetic input document (including on failure).
    println!("{wanted}");
}

#[tokio::test]
#[ignore = "requires Python on PATH and owned IPv4 loopback; explicit offline E2E"]
async fn strict_edit_acceptance_uses_native_proposals() {
    let temp = tempfile::tempdir()
        .ok()
        .expect("private synthetic directory");
    let expected = temp.path().join("expected.json");
    let negative = temp.path().join("negative.json");
    let observed = temp.path().join("observed.json");
    write_fixture(&expected, &declaration(22));
    // 23 is also on a scalar boundary, but includes the first trailing space.
    // A valid, deliberately wrong declaration must mismatch the SAME observation.
    write_fixture(&negative, &declaration(23));

    let cases: Vec<_> = fixtures::cases()
        .into_iter()
        .filter(|case| matches!(case.id, "rewrite" | "selection"))
        .collect();
    assert!(
        cases
            .iter()
            .map(|case| case.id)
            .eq(["rewrite", "selection"])
    );
    let peer = timeout(SESSION_DEADLINE, prepare_cases(cases, 1))
        .await
        .ok()
        .and_then(Result::ok)
        .expect("prepare the two fixed fixtures");
    let rows = with_peer(peer, collect_rows)
        .await
        .ok()
        .expect("collect linked native results and close the owned peer");
    write_fixture(
        &observed,
        &json!({"schema": "rambpc-edit-observations-v1", "artifact_id": "native-sdk-observation",
                "provenance": provenance(), "cases": rows}),
    );
    for (path, negative) in [(&expected, false), (&negative, true)] {
        let wanted = json!({
            "schema": "rambpc-edit-acceptance-result-v1", "scope": "offline-strict-proposal-only",
            "verdict": if negative { "mismatch" } else { "conforming" },
            "counts": {"expected": 2, "conforming_no_edit": 1, "conforming_edit": usize::from(!negative),
                       "mismatch": usize::from(negative), "missing": 0, "unqualified": 0, "unexpected": 0},
        });
        verify_checker(path, &observed, &[], i32::from(negative), wanted).await;
    }
    assert!(temp.close().is_ok(), "remove synthetic artifacts");
}
