use serde_json::Value;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::process::Command;

async fn zero_budget_is_rejected_before_input(flag: &str) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("invented-missing-budget-input.ogg");
    let context = "invented-private-budget-context";
    #[cfg(feature = "audio")]
    let commands = ["dictate", "dictate-wav"].as_slice();
    #[cfg(not(feature = "audio"))]
    let commands = ["dictate"].as_slice();

    // DEV-ONLY override, shared with the existing release finalization gate.
    let executable = std::env::var_os("RAMBLER_TEST_CLI_EXECUTABLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_rambler")));
    assert!(executable.is_absolute(), "CLI executable must be absolute");
    for subcommand in commands {
        let mut command = Command::new(&executable);
        command.env_clear().kill_on_drop(true).stdin(Stdio::null());
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        command
            .arg(subcommand)
            .arg(&input)
            .args(["--before", context, flag, "0"]);
        let output = tokio::time::timeout(Duration::from_secs(8), command.output())
            .await
            .expect("child deadline")
            .expect("spawn CLI");

        assert_eq!(output.status.code(), Some(2), "{subcommand} {flag}");
        assert!(output.stderr.is_empty(), "{subcommand} {flag}");
        let stdout = std::str::from_utf8(&output.stdout).expect("UTF-8 stdout");
        assert!(!stdout.contains("invented-missing-budget-input.ogg"));
        assert!(!stdout.contains(context));
        let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON value");
        assert!(value.is_object());
        assert_eq!(value["type"], "error");
        assert_eq!(value["category"], "arguments");
        assert!(value.get("pcm").is_none());
    }
}

#[tokio::test]
async fn zero_overall_budget_rejected_before_input_open() {
    zero_budget_is_rejected_before_input("--timeout").await;
}

#[tokio::test]
async fn zero_context_budget_rejected_before_input_open() {
    zero_budget_is_rejected_before_input("--context-timeout").await;
}

#[tokio::test]
async fn zero_finalization_budget_rejected_before_input_open() {
    zero_budget_is_rejected_before_input("--finalize-timeout").await;
}
