//! Positive, bounded DATA-boundary observations on the actual transport path.
//! Numeric owned loopback only; default h2 peer limits, immediate normal draining.
//! This is not a reproduction of the preserved CI failure or a load test.
use bytes::Bytes;
use futures_util::{StreamExt, stream};
use rambler_transport::{GrpcTransport, Metadata, StreamSource, TransportConfig};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};
use tonic::Code;

#[tokio::test]
async fn default_peer_coalesces_small_envelopes_and_bounds_unread_frame_cost() {
    let lengths: Vec<_> = (0usize..=256).chain([1024, 16_384, 65_536]).collect();
    assert_eq!(lengths.len(), 260);
    assert_eq!(lengths.iter().sum::<usize>() + 5 * lengths.len(), 117_140);
    // These first 35,210 bytes fit both default 65,535-byte upload windows.
    // Their frame sizes cannot depend on when the peer releases capacity.
    let expected: Vec<_> = lengths[..258].iter().map(|length| 5 + length).collect();
    verify_default_peer(lengths, expected, true).await;
}

#[tokio::test]
async fn default_credit_preserves_prefix_and_max_frame_coalescing_boundaries() {
    let lengths = vec![0, 1, 4, 250, 251, 255, 256, 16_379, 16_380];
    // All 33,821 bytes fit the default initial stream AND connection credit.
    // No cross-request batching: 16,379 gets one frame; 16,380 gets two.
    assert_eq!(lengths.iter().sum::<usize>() + 5 * lengths.len(), 33_821);
    let expected = vec![5, 6, 9, 255, 256, 260, 261, 16_384, 16_384, 1];
    verify_default_peer(lengths, expected, false).await;
}

async fn verify_default_peer(
    lengths: Vec<usize>,
    expected_frame_prefix: Vec<usize>,
    full_matrix: bool,
) {
    tokio::time::timeout(Duration::from_secs(30), async move {
        const METHOD: &str = "/synthetic.EnvelopeCoalescing/Upload";
        assert!(lengths.len() <= 260);
        assert!(lengths.iter().all(|&length| length <= 65_536));
        let wire_bytes = lengths.iter().sum::<usize>() + 5 * lengths.len();
        let message_count = lengths.len() as u64;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release_source, source_ready) = oneshot::channel::<()>();
        let source_lifetime = Arc::new(());
        let source_guard = source_lifetime.clone();
        let source_lengths = lengths.clone();
        let source = StreamSource::new(
            stream::once(async move {
                source_ready
                    .await
                    .expect("source released only after peer PONG");
                stream::iter(source_lengths.into_iter().enumerate()).map(move |(index, length)| {
                    let _keep_source_alive = &source_guard;
                    Ok(Bytes::from(
                        (0..length)
                            .map(|offset| ((index * 37 + offset * 13) % 256) as u8)
                            .collect::<Vec<_>>(),
                    ))
                })
            })
            .flatten(),
        );
        assert_eq!(Arc::strong_count(&source_lifetime), 2);

        let client = async move {
            let config = TransportConfig::loopback(address)
                .unwrap()
                .with_method(METHOD)
                .unwrap();
            let transport = GrpcTransport::connect(config).await.unwrap();
            let mut session = transport
                .start(source, Metadata::default(), Duration::from_secs(25))
                .unwrap();
            assert_eq!(session.next().await.unwrap(), None);
            let done = session.finish().await.unwrap();
            assert_eq!(done.terminal_code, Some(Code::Ok));
            assert_eq!(done.requests_produced, message_count);
            assert_eq!(done.requests_polled, message_count);
            assert_eq!(done.responses_received, 0);
            assert!(done.source_exhausted && done.upload_eof_polled && done.source_closed);
            assert_eq!(Arc::strong_count(&source_lifetime), 1, "source dropped");
            assert_eq!(session.next().await.unwrap(), None);
            drop(session);
            transport.close().await.unwrap();
        };

        let peer = async move {
            let (tcp, remote) = listener.accept().await.unwrap();
            assert!(remote.ip().is_loopback());
            let single_connection = async move {
                // Deliberately no Builder overrides: no raised credit, frame
                // budget, reset allowance or other anti-abuse accommodation.
                let mut connection = h2::server::handshake(tcp).await.unwrap();
                let mut pings = connection.ping_pong().unwrap();
                let barrier = async move {
                    pings.ping(h2::Ping::opaque()).await.unwrap();
                    release_source.send(()).expect("owned source gate is alive");
                };
                tokio::pin!(barrier);
                let mut settings_applied = false;
                let mut accepted = None;
                while !settings_applied || accepted.is_none() {
                    tokio::select! {
                        () = &mut barrier, if !settings_applied => {
                            settings_applied = true;
                        }
                        incoming = connection.accept() => {
                            let incoming = incoming.expect("one upload RPC").unwrap();
                            assert!(accepted.is_none(), "unexpected second RPC");
                            accepted = Some(incoming);
                        }
                    }
                }
                let (mut request, mut reply): (
                    http::Request<h2::RecvStream>,
                    h2::server::SendResponse<Bytes>,
                ) = accepted.unwrap();
                assert_eq!(u32::from(request.body().stream_id()), 1);
                assert_eq!(request.method(), http::Method::POST);
                assert_eq!(request.uri().path(), METHOD);
                assert_eq!(request.headers()["content-type"], "application/grpc");
                assert_eq!(request.headers()["te"], "trailers");

                let exchange = async move {
                    let mut raw = Vec::with_capacity(wire_bytes);
                    let mut frames = 0;
                    let mut empty_frames = 0;
                    let mut unread_cost = 0usize;
                    let mut peak_cost = 0usize;
                    while let Some(chunk) = request.body_mut().data().await {
                        let chunk = chunk.unwrap();
                        assert!(
                            chunk.len() <= wire_bytes - raw.len(),
                            "bounded exact upload"
                        );
                        if chunk.is_empty() {
                            empty_frames += 1;
                            assert_eq!(empty_frames, 1, "at most the final empty DATA");
                            assert!(request.body().is_end_stream(), "no nonfinal empty DATA");
                        } else {
                            assert!(empty_frames == 0, "no DATA after END_STREAM");
                            assert!(chunk.len() <= 16_384);
                            if let Some(&expected) = expected_frame_prefix.get(frames) {
                                assert_eq!(chunk.len(), expected, "DATA frame {frames}");
                            }
                            frames += 1;
                            // Numerical upper bound from h2 0.4.20 counts.rs:
                            // never refund on application dequeue, but retain
                            // large-frame repayment. We still drain/release
                            // normally; no unread backlog is manufactured.
                            if chunk.len() < 256 {
                                unread_cost += 256 - chunk.len();
                            } else {
                                unread_cost = unread_cost.saturating_sub(chunk.len() - 256);
                            }
                            peak_cost = peak_cost.max(unread_cost);
                            assert!(peak_cost <= 32_767, "unchanged default frame-cost bound");
                            if full_matrix && frames == 251 {
                                assert_eq!(unread_cost, 31_626);
                                assert_eq!(peak_cost, 31_626);
                            }
                            if full_matrix && frames == 257 {
                                assert_eq!(raw.len() + chunk.len(), 34_181);
                                assert_eq!(unread_cost, 31_611);
                                assert_eq!(peak_cost, 31_626);
                            }
                        }
                        raw.extend_from_slice(&chunk);
                        request
                            .body_mut()
                            .flow_control()
                            .release_capacity(chunk.len())
                            .unwrap();
                    }
                    assert!(request.body().is_end_stream());
                    assert!(request.body_mut().trailers().await.unwrap().is_none());
                    assert!(
                        request.body_mut().data().await.is_none(),
                        "stable upload EOF"
                    );
                    assert_eq!(raw.len(), wire_bytes);
                    if full_matrix {
                        assert!(frames >= expected_frame_prefix.len());
                        assert_eq!(peak_cost, 31_626);
                    } else {
                        assert_eq!(frames, expected_frame_prefix.len(), "exact DATA count");
                    }

                    // Independent byte-order oracle: no production framing
                    // helper or protobuf decoder participates in these checks.
                    let mut cursor = 0;
                    for (index, expected_length) in lengths.into_iter().enumerate() {
                        let header = &raw[cursor..cursor + 5];
                        assert_eq!(header[0], 0);
                        assert_eq!(
                            u32::from_be_bytes([header[1], header[2], header[3], header[4]])
                                as usize,
                            expected_length
                        );
                        cursor += 5;
                        let payload = &raw[cursor..cursor + expected_length];
                        assert!(
                            payload.iter().enumerate().all(|(offset, &byte)| {
                                byte == ((index * 37 + offset * 13) % 256) as u8
                            }),
                            "payload and order for message {index}"
                        );
                        cursor += expected_length;
                    }
                    assert_eq!(cursor, raw.len(), "no omitted or extra bytes");
                    let headers = http::Response::builder()
                        .header("content-type", "application/grpc")
                        .body(())
                        .unwrap();
                    let mut response = reply.send_response(headers, false).unwrap();
                    let mut trailers = http::HeaderMap::new();
                    trailers.insert("grpc-status", "0".parse().unwrap());
                    response.send_trailers(trailers).unwrap();
                };
                let drive_to_close = async {
                    match connection.accept().await {
                        None => {}
                        Some(Ok(_)) => panic!("unexpected second RPC"),
                        Some(Err(error)) => panic!("peer connection failed: {error:?}"),
                    }
                };
                tokio::join!(exchange, drive_to_close);
            };
            tokio::pin!(single_connection);
            // Retain the listener until this connection closes: a second TCP
            // connection cannot be hidden by a successful first exchange.
            tokio::select! {
                biased;
                extra = listener.accept() => {
                    extra.unwrap();
                    panic!("unexpected extra TCP connection");
                }
                () = &mut single_connection => {}
            }
        };
        // Both futures are owned here. Timeout/panic drops sockets and futures;
        // there is no fixture driver task to detach or error to tolerate.
        tokio::join!(client, peer);
    })
    .await
    .expect("bounded positive envelope-coalescing oracle");
}
