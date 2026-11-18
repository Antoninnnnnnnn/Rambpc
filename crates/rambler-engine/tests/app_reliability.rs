//! Public reliability APIs against owned IPv4 loopback peers, with opaque audio fixtures.
mod common;

use bytes::Bytes;
use common::{TestServer, bounded, drain, replies};
use futures_util::stream;
use rambler_engine::{
    AudioSource, CollectorLimits, Engine, EngineError, RECOMMENDED_FINALIZE_TIMEOUT, RetryAudio,
    RetryAudioLimits, RetryAudioUnavailable, SessionOptions, StreamAudio, collect_dictation,
};
use rambler_protocol::{
    AUDIO_DATA, RECOGNITION_RESPONSE, end_request,
    wire::{Field, Value, bytes_field as b, parse_fields, string_field as s, varint_field as v},
};
use rambler_transport::{TransportConfig, TransportError};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncReadExt, net::TcpListener};

const AUDIO: [Bytes; 2] = [Bytes::from_static(b"first"), Bytes::from_static(b"second")];
const EXACT: RetryAudioLimits = RetryAudioLimits {
    max_bytes: 11,
    max_chunks: 2,
};

fn audio() -> Box<dyn AudioSource> {
    let [first, second] = AUDIO;
    // Empty source chunks are not uploaded and must not use up retention slots.
    Box::new(StreamAudio::new(stream::iter([
        Ok(first),
        Ok(Bytes::new()),
        Ok(second),
    ])))
}

fn uploaded_audio(messages: Vec<Bytes>) -> Vec<Vec<u8>> {
    assert_eq!(messages.len(), AUDIO.len() + 2); // INIT, audio, END.
    assert_eq!(messages.last().unwrap().as_ref(), end_request());
    messages[1..messages.len() - 1]
        .iter()
        .map(|message| {
            let fields = parse_fields(message).unwrap();
            let [
                _,
                Field {
                    number: AUDIO_DATA,
                    value: Value::Bytes(data),
                    ..
                },
            ] = fields.as_slice()
            else {
                panic!("expected audio request");
            };
            let fields = parse_fields(data).unwrap();
            let [
                Field {
                    number: 1,
                    value: Value::Bytes(chunk),
                    ..
                },
            ] = fields.as_slice()
            else {
                panic!("expected audio bytes");
            };
            chunk.to_vec()
        })
        .collect()
}

fn final_response() -> Bytes {
    let hypothesis = s(1, "retried").unwrap();
    let recognition = [v(1, 1).unwrap(), b(3, &b(3, &hypothesis).unwrap()).unwrap()].concat();
    b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap())
        .unwrap()
        .into()
}

#[tokio::test]
async fn warm_then_fail_fast_then_explicitly_retry_identical_audio_on_one_connection() {
    bounded(async {
        let uploads = Arc::new(Mutex::new(Vec::new()));
        let server = TestServer::new({
            let uploads = uploads.clone();
            move |request| {
                let uploads = uploads.clone();
                async move {
                    let chunks = uploaded_audio(drain(request.into_inner()).await);
                    let attempt = {
                        let mut uploads = uploads.lock().unwrap();
                        uploads.push(chunks);
                        uploads.len()
                    };
                    if attempt == 1 {
                        // The peer read the entire upload but never sends a final or status.
                        Ok(replies(stream::pending()))
                    } else {
                        Ok(replies(stream::iter([
                            Ok(final_response()),
                            Ok(Bytes::from_static(b"\x08\x01")),
                        ])))
                    }
                }
            }
        })
        .await;
        let config = TransportConfig::loopback(server.address).unwrap();
        let engine = Engine::connect_ready(config).await.unwrap();
        assert_eq!(server.accepted_connections(), 1);
        assert!(uploads.lock().unwrap().is_empty());

        let (retained, first) = RetryAudio::capture(audio(), EXACT);
        let limits = CollectorLimits::default();
        let result = collect_dictation(
            &engine,
            first,
            SessionOptions {
                finalize_timeout: Some(Duration::from_secs(1)),
                ..Default::default()
            },
            limits,
        )
        .await;
        assert!(matches!(result, Err(EngineError::FinalizeTimeout)));
        assert_eq!(uploads.lock().unwrap().len(), 1);

        let options = SessionOptions::default();
        assert_eq!(options.finalize_timeout, None);
        let options = options.with_recommended_finalize_timeout();
        assert_eq!(options.finalize_timeout, Some(RECOMMENDED_FINALIZE_TIMEOUT));
        let second = retained.into_source().unwrap(); // Explicit user action.
        let result = collect_dictation(&engine, second, options, limits)
            .await
            .unwrap();
        assert_eq!(result.text().unwrap(), "retried");
        let expected = AUDIO.map(|chunk| chunk.to_vec()).to_vec();
        assert_eq!(*uploads.lock().unwrap(), [expected.clone(), expected]);
        assert_eq!(server.accepted_connections(), 1);
        engine.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn either_retention_limit_disables_retry_without_interrupting_upload() {
    bounded(async {
        let server = TestServer::new(|request| async move {
            let chunks = uploaded_audio(drain(request.into_inner()).await);
            assert_eq!(chunks, AUDIO.map(|chunk| chunk.to_vec()));
            Ok(replies(stream::iter([Ok(Bytes::from_static(b"\x08\x01"))])))
        })
        .await;
        let engine = Engine::connect(TransportConfig::loopback(server.address).unwrap())
            .await
            .unwrap();
        for limits in [
            RetryAudioLimits {
                max_bytes: EXACT.max_bytes - 1,
                ..EXACT
            },
            RetryAudioLimits {
                max_chunks: EXACT.max_chunks - 1,
                ..EXACT
            },
        ] {
            let (retained, wrapped) = RetryAudio::capture(audio(), limits);
            collect_dictation(&engine, wrapped, Default::default(), Default::default())
                .await
                .unwrap();
            assert!(matches!(
                retained.into_source(),
                Err(RetryAudioUnavailable::LimitExceeded)
            ));
        }
        engine.close().await.unwrap();
        server.close().await;
    })
    .await;
}

#[tokio::test]
async fn ready_waits_for_peer_and_times_out_on_a_silent_tcp_connection() {
    bounded(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = TransportConfig::loopback(listener.local_addr().unwrap())
            .unwrap()
            .with_connect_timeout(Duration::from_secs(1))
            .unwrap();
        let (result, bytes) = tokio::join!(Engine::connect_ready(config), async {
            let (socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            // The peer writes nothing; the connection must close when warm-up times out.
            socket.take(512).read_to_end(&mut bytes).await.unwrap();
            bytes
        });
        assert!(matches!(
            result,
            Err(EngineError::Transport(TransportError::Timeout))
        ));
        assert!(bytes.starts_with(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"));
    })
    .await;
}
