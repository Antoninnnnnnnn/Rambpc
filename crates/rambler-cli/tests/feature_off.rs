#![cfg(not(feature = "audio"))]

#[test]
fn wav_command_is_unavailable_without_audio_feature() {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rambler"));
    command.env_clear();
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let output = command
        .args(["dictate-wav", "unused.wav"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["category"], "arguments");
}
