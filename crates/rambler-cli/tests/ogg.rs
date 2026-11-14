mod common;
#[path = "support/ogg_fixture.rs"]
mod ogg_fixture;
use bytes::Bytes;
use common::{TestServer, replies};
use std::{
    process::{Command, Output},
    sync::{Arc, Mutex},
    time::Duration,
};

async fn command(path: std::path::PathBuf, address: String) -> Output {
    tokio::time::timeout(
        Duration::from_secs(15),
        tokio::task::spawn_blocking(move || {
            let mut command = Command::new(env!("CARGO_BIN_EXE_rambler"));
            command.env_clear();
            if let Some(root) = std::env::var_os("SystemRoot") {
                command.env("SystemRoot", root);
            }
            command
                .arg("dictate")
                .arg(path)
                .args(["--loopback", &address, "--timeout", "5"])
                .output()
                .unwrap()
        }),
    )
    .await
    .unwrap()
    .unwrap()
}

#[tokio::test]
async fn invalid_first_crc_and_non_opus_header_make_zero_tcp_connections_or_rpcs() {
    use ogg::writing::{PacketWriteEndInfo as End, PacketWriter};
    let valid = ogg_fixture::fixture(1);
    let mut bad_crc = valid.clone();
    bad_crc[22] ^= 1;
    let mut reader = ogg::reading::PacketReader::new(std::io::Cursor::new(&valid));
    let mut writer = PacketWriter::new(Vec::new());
    let mut head = reader.read_packet().unwrap().unwrap().data;
    head[9] = 2; // correctly checksummed, but unsupported stereo OpusHead
    writer
        .write_packet(head, 0x12345678, End::EndPage, 0)
        .unwrap();
    writer
        .write_packet(
            reader.read_packet().unwrap().unwrap().data,
            0x12345678,
            End::EndPage,
            0,
        )
        .unwrap();
    writer
        .write_packet(
            reader.read_packet().unwrap().unwrap().data,
            0x12345678,
            End::EndStream,
            960,
        )
        .unwrap();
    for bytes in [bad_crc, writer.into_inner()] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("préflight 音声.ogg");
        std::fs::write(&path, bytes).unwrap();
        let output = command(path.clone(), listener.local_addr().unwrap().to_string()).await;
        assert!(!output.status.success());
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["category"], "audio");
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_crc_failure_after_upload_stays_audio_error_and_never_sends_end_request() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let captured = received.clone();
    let server = TestServer::new(move |request| {
        let captured = captured.clone();
        async move {
            let mut requests = request.into_inner();
            while let Ok(Some(request)) = requests.message().await {
                captured.lock().unwrap().push(request);
            }
            Ok(replies(futures_util::stream::iter([Ok(
                Bytes::from_static(b"\x08\x01"),
            )])))
        }
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("late.ogg");
    let mut bytes = ogg_fixture::fixture(128);
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&path, bytes).unwrap();
    let output = command(path.clone(), server.address.to_string()).await;
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "error");
    assert_eq!(value["category"], "audio");
    assert!(value.get("text").is_none());
    std::fs::remove_file(path).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server.close())
        .await
        .unwrap();
    let requests = received.lock().unwrap();
    assert!(!requests.is_empty(), "failure must occur after RPC starts");
    assert!(
        requests.iter().any(|request| {
            rambler_protocol::wire::parse_fields(request)
                .unwrap()
                .iter()
                .any(|field| field.number == rambler_protocol::AUDIO_DATA)
        }),
        "some real audio must arrive before the late failure"
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.as_ref() == b"\x10\x01\x18\x01")
    );
}
