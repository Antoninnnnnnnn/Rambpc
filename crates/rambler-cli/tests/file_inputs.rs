use std::{
    net::TcpListener,
    path::Path,
    process::{Output, Stdio},
    time::Duration,
};
use tokio::process::Command;

fn subcommands() -> &'static [&'static str] {
    if cfg!(feature = "audio") {
        &["dictate", "dictate-wav"]
    } else {
        &["dictate"]
    }
}
async fn reject_before_connect(path: &Path) {
    for subcommand in subcommands() {
        reject_one(path, subcommand).await;
    }
}
async fn reject_one(path: &Path, subcommand: &str) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let mut command = Command::new(env!("CARGO_BIN_EXE_rambler"));
    command.env_clear().kill_on_drop(true).stdin(Stdio::null());
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    command
        .arg(subcommand)
        .arg(path)
        .args(["--loopback", &address, "--timeout", "1"]);
    // Killing the child on timeout prevents a regression from leaving a
    // blocking OS open in this test's own runtime.
    let output: Output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .unwrap_or_else(|_| panic!("{subcommand}: non-regular input blocked before RPC timeout"))
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    assert!(output.stdout.len() <= 4096);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["category"], "audio");
    assert!(value.get("text").is_none());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&*path.to_string_lossy()));
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[tokio::test]
async fn directories_are_rejected_without_rpc_or_path_disclosure() {
    let directory = tempfile::tempdir().unwrap();
    reject_before_connect(directory.path()).await;
}

#[cfg(unix)]
#[tokio::test]
async fn fifo_symlink_and_devices_are_rejected_without_waiting_for_a_writer() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let fifo = directory.path().join("unwritten.fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let link = directory.path().join("fifo-link");
    symlink(&fifo, &link).unwrap();
    for path in [
        fifo.as_path(),
        link.as_path(),
        Path::new("/dev/null"),
        Path::new("/dev/zero"),
    ] {
        reject_before_connect(path).await;
    }
}

#[cfg(windows)]
#[tokio::test]
async fn windows_named_pipe_is_rejected_without_waiting_for_server_data() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::net::windows::named_pipe::ServerOptions;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for subcommand in subcommands() {
        let name = format!(
            r"\\.\pipe\rambpc-file-preflight-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let _pipe = ServerOptions::new()
            .first_pipe_instance(true)
            .access_inbound(false)
            .access_outbound(true)
            .create(&name)
            .unwrap();
        // Each command gets a fresh server that deliberately never writes.
        reject_one(Path::new(&name), subcommand).await;
    }
}
