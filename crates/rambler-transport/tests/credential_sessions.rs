//! Real HTTP/2 loopback only: invented credentials, no environment or remote access.
use bytes::Bytes;
use futures_util::stream;
use rambler_transport::{
    CredentialValue, DEFAULT_METHOD, GrpcTransport, Metadata, MetadataFuture, MetadataProvider,
    StreamSource, TransportConfig,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, mpsc},
    task::JoinSet,
};

struct RotatingProvider {
    current: Mutex<Metadata>,
    calls: AtomicUsize,
}
impl MetadataProvider for RotatingProvider {
    fn metadata(&self) -> MetadataFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let snapshot = self.current.lock().unwrap().clone();
        Box::pin(async move { Ok(snapshot) })
    }
}

fn invented_metadata(generation: &str) -> Metadata {
    Metadata::new([
        (
            "authorization".into(),
            CredentialValue::Text(format!("invented-{generation}-first")),
        ),
        (
            "authorization".into(),
            CredentialValue::Text(format!("invented-{generation}-second")),
        ),
        (
            "x-goog-api-key".into(),
            CredentialValue::Text(format!("invented-{generation}-key")),
        ),
        (
            "proof-bin".into(),
            CredentialValue::Binary(Bytes::from(format!("\0{generation}\u{7f}").into_bytes())),
        ),
    ])
    .unwrap()
}

fn assert_wire_metadata(headers: &http::HeaderMap, generation: Option<&str>) {
    assert_eq!(headers.get_all("content-type").iter().count(), 1);
    assert_eq!(headers["content-type"], "application/grpc");
    assert_eq!(headers.get_all("te").iter().count(), 1);
    assert_eq!(headers["te"], "trailers");
    assert_eq!(headers.get_all("grpc-timeout").iter().count(), 1);
    let Some(generation) = generation else {
        for key in ["authorization", "x-goog-api-key", "proof-bin"] {
            assert!(
                !headers.contains_key(key),
                "stale credential on anonymous stream"
            );
        }
        return;
    };
    let authorization = headers.get_all("authorization").iter().collect::<Vec<_>>();
    assert_eq!(authorization.len(), 2);
    assert_eq!(authorization[0], &format!("invented-{generation}-first"));
    assert_eq!(authorization[1], &format!("invented-{generation}-second"));
    assert_eq!(
        headers["x-goog-api-key"],
        format!("invented-{generation}-key")
    );
    assert_eq!(headers.get_all("x-goog-api-key").iter().count(), 1);
    assert_eq!(headers.get_all("proof-bin").iter().count(), 1);
    // h2 0.4 does not propagate HPACK never-indexed into decoded HeaderValue
    // sensitivity. Pre-send flags are tested separately in credentials.rs;
    // the decoded peer headers cannot qualify that wire representation.
    let metadata = tonic::metadata::MetadataMap::from_headers(headers.clone());
    assert_eq!(
        metadata.get_bin("proof-bin").unwrap().to_bytes().unwrap(),
        Bytes::from(format!("\0{generation}\u{7f}").into_bytes()),
    );
}

#[tokio::test]
async fn rotating_provider_snapshots_and_removal_are_isolated_on_one_connection() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (seen_tx, mut seen_rx) = mpsc::channel(3);
        let release = Arc::new(Semaphore::new(0));
        let release_server = release.clone();
        let server = tokio::spawn(async move {
            // Exactly one accepted TCP connection; all three RPCs must use it.
            let (socket, _) = listener.accept().await.unwrap();
            drop(listener);
            let mut connection = h2::server::handshake(socket).await.unwrap();
            let mut handlers = JoinSet::new();
            let mut requests = 0;
            while let Some(request) = connection.accept().await {
                let (request, mut respond) = request.unwrap();
                requests += 1;
                assert!(requests <= 3, "unexpected retry or extra request");
                assert_eq!(request.method(), http::Method::POST);
                assert_eq!(request.uri().path(), DEFAULT_METHOD);
                seen_tx.send(request.headers().clone()).await.unwrap();
                let release = release_server.clone();
                handlers.spawn(async move {
                    let mut body = request.into_body();
                    while let Some(chunk) = body.data().await {
                        let chunk = chunk.unwrap();
                        assert!(chunk.is_empty(), "test sources must not upload payloads");
                        body.flow_control().release_capacity(chunk.len()).unwrap();
                    }
                    assert!(body.trailers().await.unwrap().is_none());
                    release.acquire().await.unwrap().forget();
                    let response = http::Response::builder()
                        .header("content-type", "application/grpc")
                        .body(())
                        .unwrap();
                    let mut stream = respond.send_response(response, false).unwrap();
                    let mut trailers = http::HeaderMap::new();
                    trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                    stream.send_trailers(trailers).unwrap();
                });
            }
            while let Some(result) = handlers.join_next().await {
                result.unwrap();
            }
            requests
        });
        let provider = Arc::new(RotatingProvider {
            current: Mutex::new(invented_metadata("one")),
            calls: AtomicUsize::new(0),
        });
        let transport = GrpcTransport::connect(TransportConfig::loopback(address).unwrap())
            .await
            .unwrap();
        let mut sessions = Vec::new();
        for generation in [Some("one"), Some("two"), None] {
            *provider.current.lock().unwrap() =
                generation.map(invented_metadata).unwrap_or_default();
            sessions.push(
                transport
                    .start_with_provider(
                        StreamSource::new(stream::empty()),
                        provider.clone(),
                        Duration::from_secs(5),
                    )
                    .unwrap(),
            );
            let headers = seen_rx.recv().await.unwrap();
            assert_wire_metadata(&headers, generation);
            assert_eq!(provider.calls.load(Ordering::SeqCst), sessions.len());
            // No response is allowed to terminate an earlier session before
            // subsequent credentials are selected and observed by the peer.
            assert!(
                sessions
                    .iter()
                    .all(|session| session.completion().terminal_code.is_none())
            );
        }
        assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
        release.add_permits(3);
        for session in &mut sessions {
            assert!(session.next().await.unwrap().is_none());
            let completion = session.finish().await.unwrap();
            assert!(completion.source_exhausted && completion.source_closed);
            assert!(completion.upload_eof_polled);
            assert_eq!(completion.requests_produced, 0);
            assert_eq!(completion.terminal_code, Some(tonic::Code::Ok));
        }
        transport.close().await.unwrap();
        assert_eq!(server.await.unwrap(), 3);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
    })
    .await
    .expect("bounded credential snapshot regression");
}
