mod common;
#[path = "support/ogg_fixture.rs"]
mod ogg_fixture;
use bytes::Bytes;
use common::{TestServer, bounded, replies};
use ogg::reading::PacketReader;
use rambler_engine::{
    AudioSource, CollectorLimits, Engine, EngineError, OggAudio, SessionOptions, SourceError,
    SourceFuture, collect_dictation,
};
use rambler_protocol::{
    AUDIO_DATA, end_request,
    wire::{Value, parse_fields},
};
use rambler_transport::TransportConfig;
use std::{
    io::{self, Cursor},
    process::{Command, Output},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;

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
    let mut reader = PacketReader::new(Cursor::new(&valid));
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

#[tokio::test]
async fn valid_libopus_padding_at_and_over_the_shared_source_limit() {
    use ogg::writing::{PacketWriteEndInfo as End, PacketWriter};

    let original = ogg_fixture::fixture(1);
    let mut reader = PacketReader::new(Cursor::new(&original));
    let head = reader.read_packet_expected().unwrap().data;
    let tags = reader.read_packet_expected().unwrap().data;
    let packet = reader.read_packet_expected().unwrap().data;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("padded.ogg");
    for size in [65_536, 65_537] {
        let mut padded = vec![0; size];
        padded[..packet.len()].copy_from_slice(&packet);
        opus::packet::pad(&mut padded, packet.len()).unwrap();
        let mut writer = PacketWriter::new(Vec::new());
        let serial = 0x12345678;
        writer
            .write_packet(head.clone(), serial, End::EndPage, 0)
            .unwrap();
        writer
            .write_packet(tags.clone(), serial, End::EndPage, 0)
            .unwrap();
        writer
            .write_packet(padded, serial, End::EndStream, 960)
            .unwrap();
        let bytes = writer.into_inner();

        // The independent Ogg reader and Opus decoder accept both streams;
        // the extra byte is a source policy error, not an invalid Opus packet.
        let mut reader = PacketReader::new(Cursor::new(&bytes));
        assert_eq!(reader.read_packet_expected().unwrap().data, head);
        assert_eq!(reader.read_packet_expected().unwrap().data, tags);
        let padded = reader.read_packet_expected().unwrap().data;
        let mut decoder = opus::Decoder::new(16_000, opus::Channels::Mono).unwrap();
        assert_eq!(decoder.decode(&padded, &mut [0; 320], false).unwrap(), 320);
        assert_eq!(padded.len(), size);
        assert!(reader.read_packet().unwrap().is_none());

        std::fs::write(&path, &bytes).unwrap();
        let mut source: Box<dyn AudioSource> = Box::new(OggAudio::open(&path).await.unwrap());
        let mut output = Vec::new();
        loop {
            match source.next().await {
                Ok(Some(page)) => output.extend_from_slice(&page),
                Ok(None) => {
                    assert_eq!(size, 65_536);
                    assert_eq!(output, bytes);
                    break;
                }
                Err(error) => {
                    assert_eq!(size, 65_537);
                    assert_eq!(
                        error.original().to_string(),
                        "oversized Opus packet or tags"
                    );
                    break;
                }
            }
        }
        source.close().await.unwrap();
        std::fs::remove_file(&path).unwrap();
    }
}

async fn recording_peer(
    observe: impl Fn(&[Bytes]) + Send + Sync + 'static,
) -> (TestServer, Arc<Mutex<Vec<Bytes>>>) {
    let received = Arc::new(Mutex::new(Vec::new()));
    let captured = received.clone();
    let observe = Arc::new(observe);
    let server = TestServer::new(move |request| {
        let captured = captured.clone();
        let observe = observe.clone();
        async move {
            let mut requests = request.into_inner();
            while let Ok(Some(request)) = requests.message().await {
                let mut captured = captured.lock().unwrap();
                captured.push(request);
                observe(&captured);
            }
            Ok(replies(futures_util::stream::iter([Ok(
                Bytes::from_static(b"\x08\x01"),
            )])))
        }
    })
    .await;
    (server, received)
}

fn corrupt_final_page(mut bytes: Vec<u8>) -> Vec<u8> {
    *bytes.last_mut().unwrap() ^= 1; // invalidate only the last page's CRC
    bytes
}

fn assert_no_end_request(requests: &[Bytes]) {
    assert!(!requests.iter().any(|r| r.as_ref() == end_request()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_reports_final_page_crc_failure_as_audio_error_and_sends_no_end() {
    let (server, received) = recording_peer(|_| {}).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad-final-crc.ogg");
    std::fs::write(&path, corrupt_final_page(ogg_fixture::fixture(2))).unwrap();
    let output = command(path.clone(), server.address.to_string()).await;
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "error");
    assert_eq!(value["category"], "audio");
    assert!(value.get("text").is_none());
    std::fs::remove_file(path).unwrap();
    bounded(server.close()).await;
    // The child may cancel before the RPC starts; assert only what this peer saw.
    assert_no_end_request(&received.lock().unwrap());
}

struct GatedOgg {
    source: OggAudio,
    pages_before_gate: usize,
    release: Option<oneshot::Receiver<()>>,
}

impl AudioSource for GatedOgg {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async {
            if self.pages_before_gate == 0
                && let Some(release) = self.release.as_mut()
            {
                // Borrow the receiver so dropping a pending next does not lose it.
                release.await.map_err(SourceError::new)?;
                self.release = None;
            }
            let page = self.source.next().await?;
            if page.is_some() && self.pages_before_gate > 0 {
                self.pages_before_gate -= 1;
            }
            Ok(page)
        })
    }

    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.source.close()
    }
}

fn audio_pages(requests: &[Bytes]) -> Vec<Vec<u8>> {
    requests
        .iter()
        .flat_map(|request| parse_fields(request).unwrap())
        .filter_map(|field| match (field.number, field.value) {
            (AUDIO_DATA, Value::Bytes(audio)) => Some(audio),
            _ => None,
        })
        .flat_map(|audio| parse_fields(&audio).unwrap())
        .filter_map(|field| match (field.number, field.value) {
            (1, Value::Bytes(page)) => Some(page),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_reports_late_ogg_crc_failure_after_peer_observes_audio_without_end() {
    // Two media pages: let the peer see OpusHead, OpusTags, and the first
    // complete Opus packet before asking the real source for the corrupt EOS.
    const VALID_PAGES: usize = 3;
    let original = ogg_fixture::fixture(2);
    let mut reader = PacketReader::new(Cursor::new(&original));
    let [head, tags, audio] = std::array::from_fn(|_| reader.read_packet_expected().unwrap().data);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gated-final-crc.ogg");
    std::fs::write(&path, corrupt_final_page(original.clone())).unwrap();
    let (release, wait) = oneshot::channel();
    let release = Mutex::new(Some(release));
    let (server, received) = recording_peer(move |requests| {
        let mut release = release.lock().unwrap();
        if release.is_none() {
            return;
        }
        let pages = audio_pages(requests);
        if pages.len() < VALID_PAGES {
            return;
        }
        assert_eq!(pages.len(), VALID_PAGES);
        let prefix = pages.concat();
        assert!(original.starts_with(&prefix));
        // Validate the peer's actual audio, not just receipt of the RPC or its headers.
        let mut reader = PacketReader::new(Cursor::new(prefix));
        for expected in [&head, &tags, &audio] {
            assert_eq!(&reader.read_packet_expected().unwrap().data, expected);
        }
        release.take().unwrap().send(()).unwrap();
    })
    .await;
    let source = GatedOgg {
        source: OggAudio::open(&path).await.unwrap(),
        pages_before_gate: VALID_PAGES,
        release: Some(wait),
    };
    let engine = bounded(Engine::connect(
        TransportConfig::loopback(server.address).unwrap(),
    ))
    .await
    .unwrap();
    let result = bounded(collect_dictation(
        &engine,
        Box::new(source),
        SessionOptions::default(),
        CollectorLimits::default(),
    ))
    .await;
    bounded(engine.close()).await.unwrap();
    bounded(server.close()).await;
    let Err(EngineError::Audio(error)) = result else {
        panic!("expected an audio source error, got {result:?}");
    };
    let error = error.original().downcast_ref::<io::Error>().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "invalid Ogg CRC");
    let requests = received.lock().unwrap();
    assert_eq!(audio_pages(&requests).len(), VALID_PAGES);
    assert_no_end_request(&requests);
    std::fs::remove_file(path).unwrap();
}
