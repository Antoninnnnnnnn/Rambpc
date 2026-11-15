//! Shared-connection receive pressure, using only a raw h2 numeric-loopback peer.
//! The ignored baseline is an observation of the original 65535-byte connection
//! window, not the desired contract. The active regression requires sibling B to
//! finish before the application consumes any response from A.
use bytes::Bytes;
use futures_util::future::poll_fn;
use rambler_transport::{
    CredentialValue, GrpcTransport, Metadata, RequestSource, Session, TransportConfig,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::oneshot,
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;
use tonic::Code;

const STREAM_WINDOW: usize = 65_535;
const FRAME_BYTES: usize = 16 * 1024;
const A_MESSAGES: usize = 12;
// Each message (including its envelope) occupies exactly one maximum-size DATA
// frame. The capacity-one application queue accepts the first message; the
// reader releases the second DATA frame and then blocks enqueueing message two.
const RELEASED_BEFORE_PAUSE: usize = 2 * FRAME_BYTES;
const A_SATURATED_AT: usize = STREAM_WINDOW + RELEASED_BEFORE_PAUSE;
const B_PAYLOAD: &[u8] = &[0x00, 0xff, 0x42];
const STEP: Duration = Duration::from_secs(3);
const B_BUDGET: Duration = Duration::from_secs(1);
const WHOLE: Duration = Duration::from_secs(20);

fn payload(index: usize) -> Bytes {
    Bytes::from(
        (0..FRAME_BYTES - 5)
            .map(|offset| ((index * 37 + offset * 13) % 251) as u8)
            .collect::<Vec<_>>(),
    )
}

fn frame(payload: &[u8]) -> Bytes {
    let mut bytes = Vec::with_capacity(payload.len() + 5);
    bytes.push(0);
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    Bytes::from(bytes)
}

fn headers() -> http::Response<()> {
    http::Response::builder()
        .header("content-type", "application/grpc")
        .body(())
        .unwrap()
}

fn ok_trailers() -> http::HeaderMap {
    let mut trailers = http::HeaderMap::new();
    trailers.insert("grpc-status", "0".parse().unwrap());
    trailers
}

async fn bounded<F: std::future::Future>(future: F) -> F::Output {
    tokio::time::timeout(STEP, future)
        .await
        .expect("loopback scenario step exceeded deadline")
}

/// Every source is finite and empty; cleanup is independently counted for both
/// sessions, so a healthy response cannot conceal an unjoined source.
struct EmptySource(Arc<AtomicUsize>);
impl RequestSource for EmptySource {
    fn next(
        &mut self,
    ) -> rambler_transport::SourceFuture<'_, Result<Option<Bytes>, rambler_transport::SourceError>>
    {
        Box::pin(async { Ok(None) })
    }

    fn close(
        &mut self,
    ) -> rambler_transport::SourceFuture<'_, Result<(), rambler_transport::SourceError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

async fn empty_upload(mut request: http::Request<h2::RecvStream>) {
    while let Some(data) = request.body_mut().data().await {
        let data = data.unwrap();
        assert!(data.is_empty(), "the synthetic upload has no messages");
        request
            .body_mut()
            .flow_control()
            .release_capacity(data.len())
            .unwrap();
    }
}

async fn send_a(mut response: h2::SendStream<Bytes>, saturated: oneshot::Sender<usize>) {
    let mut saturated = Some(saturated);
    let mut sent = 0;
    for index in 0..A_MESSAGES {
        let mut bytes = frame(&payload(index));
        while !bytes.is_empty() {
            response.reserve_capacity(bytes.len());
            let capacity = poll_fn(|cx| {
                // This is a real poll with this task's registered waker, not a
                // capacity() snapshot or a synthetic Pending from a sleep.
                let polled = response.poll_capacity(cx);
                if polled.is_pending() && sent == A_SATURATED_AT {
                    assert_eq!(response.capacity(), 0);
                    if let Some(gate) = saturated.take() {
                        gate.send(sent).expect("A saturation observer is alive");
                    }
                }
                polled
            })
            .await
            .expect("A stream remains open")
            .unwrap();
            assert!(capacity > 0);
            let count = capacity.min(bytes.len());
            response.send_data(bytes.split_to(count), false).unwrap();
            sent += count;
        }
    }
    assert!(
        saturated.is_none(),
        "A must hit the exact paused-reader gate"
    );
    assert_eq!(sent, A_MESSAGES * FRAME_BYTES);
    response.reserve_capacity(0);
    response.send_trailers(ok_trailers()).unwrap();
}

async fn send_b(
    mut response: h2::SendStream<Bytes>,
    first_poll: oneshot::Sender<()>,
    pending: Arc<AtomicBool>,
) {
    let mut bytes = frame(B_PAYLOAD);
    let mut first_poll = Some(first_poll);
    while !bytes.is_empty() {
        response.reserve_capacity(bytes.len());
        let capacity = poll_fn(|cx| {
            let polled = response.poll_capacity(cx);
            if polled.is_pending() {
                assert_eq!(response.capacity(), 0);
                pending.store(true, Ordering::SeqCst);
            }
            if let Some(gate) = first_poll.take() {
                gate.send(()).expect("B capacity observer is alive");
            }
            polled
        })
        .await
        .expect("B stream remains open")
        .unwrap();
        assert!(capacity > 0);
        let count = capacity.min(bytes.len());
        response.send_data(bytes.split_to(count), false).unwrap();
    }
    response.reserve_capacity(0);
    response.send_trailers(ok_trailers()).unwrap();
}

struct Peer {
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
}
impl Peer {
    async fn close(mut self) {
        self.stop.cancel();
        bounded(self.task.as_mut().unwrap()).await.unwrap();
        self.task.take();
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[derive(Debug)]
struct Observation {
    b_completed_while_a_paused: bool,
}

async fn collect_b(session: &mut Session) {
    assert_eq!(session.next().await.unwrap().unwrap().as_ref(), B_PAYLOAD);
    assert!(session.next().await.unwrap().is_none());
}

async fn scenario() -> Observation {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stop = CancellationToken::new();
    let stopped = stop.clone();
    let (ready_tx, ready_rx) = oneshot::channel();
    let (a_tx, a_rx) = oneshot::channel();
    let (b_tx, b_rx) = oneshot::channel();
    let b_pending = Arc::new(AtomicBool::new(false));
    let observed_pending = b_pending.clone();
    let task = tokio::spawn(async move {
        let (tcp, remote) = listener.accept().await.unwrap();
        assert!(remote.ip().is_loopback());
        tcp.set_nodelay(true).unwrap();
        // Exactly one connection: accepting B on this same h2 connection rules
        // out accidentally testing two independent connection windows.
        let mut connection = h2::server::handshake(tcp).await.unwrap();
        let mut pings = connection.ping_pong().unwrap();
        let settings_barrier = pings.ping(h2::Ping::opaque());
        tokio::pin!(settings_barrier);
        let mut ready_tx = Some(ready_tx);
        let mut a_tx = Some(a_tx);
        let mut b_tx = Some(b_tx);
        let mut calls = JoinSet::new();
        let mut streams = 0;
        loop {
            tokio::select! {
                biased;
                _ = stopped.cancelled() => break,
                result = &mut settings_barrier, if ready_tx.is_some() => {
                    result.unwrap();
                    // Request A is started only after the client has processed
                    // server SETTINGS and answered PING, with the h2 driver live.
                    ready_tx.take().unwrap().send(()).unwrap();
                }
                result = calls.join_next(), if !calls.is_empty() => {
                    result.unwrap().unwrap();
                }
                accepted = connection.accept() => {
                    let Some(accepted) = accepted else { break };
                    let (request, mut reply) = accepted.unwrap();
                    streams += 1;
                    assert_eq!(u32::from(request.body().stream_id()), 2 * streams - 1);
                    let name = request.headers().get("x-session").unwrap().clone();
                    let response = reply.send_response(headers(), false).unwrap();
                    if name == "a" {
                        let gate = a_tx.take().expect("one A stream");
                        calls.spawn(async move {
                            empty_upload(request).await;
                            send_a(response, gate).await;
                        });
                    } else {
                        assert_eq!(name, "b");
                        let gate = b_tx.take().expect("one B stream");
                        let pending = observed_pending.clone();
                        calls.spawn(async move {
                            empty_upload(request).await;
                            // B is fresh: no DATA has spent any of its own
                            // initial 65535-byte stream window. Pending for an
                            // eight-byte frame therefore exposes shared pressure.
                            send_b(response, gate, pending).await;
                        });
                    }
                }
            }
        }
        // Both response handlers must finish normally on the expected RED path
        // too: the scenario drains A before shutting down this peer.
        while let Some(result) = calls.join_next().await {
            result.unwrap();
        }
        assert_eq!(streams, 2);
    });
    let peer = Peer {
        stop,
        task: Some(task),
    };
    let transport = bounded(GrpcTransport::connect(
        TransportConfig::loopback(address).unwrap(),
    ))
    .await
    .unwrap();
    bounded(ready_rx).await.unwrap();
    let a_closes = Arc::new(AtomicUsize::new(0));
    let b_closes = Arc::new(AtomicUsize::new(0));
    let metadata = |name: &str| {
        Metadata::new([("x-session".into(), CredentialValue::Text(name.into()))]).unwrap()
    };
    let mut a = transport
        .start(EmptySource(a_closes.clone()), metadata("a"), WHOLE)
        .unwrap();
    // Never call A.next() before the B observation. This leaves the production
    // capacity-one response queue and one in-flight response entirely untouched.
    let a_bytes_at_pending = bounded(a_rx).await.unwrap();
    assert_eq!(a_bytes_at_pending, A_SATURATED_AT);
    assert_eq!(a.completion().responses_received, 2);
    let mut b = transport
        .start(EmptySource(b_closes.clone()), metadata("b"), WHOLE)
        .unwrap();
    bounded(b_rx).await.unwrap();
    // Preserve the very same collection future after timeout: dropping a
    // partially completed collector could discard B's already observed message.
    let b_completed_while_a_paused;
    let b_registered_pending;
    {
        let collection = collect_b(&mut b);
        tokio::pin!(collection);
        b_completed_while_a_paused = tokio::time::timeout(B_BUDGET, &mut collection)
            .await
            .is_ok();
        b_registered_pending = b_pending.load(Ordering::SeqCst);
        assert_eq!(a.completion().responses_received, 2);
        eprintln!(
            "paused A: sent={a_bytes_at_pending}, outstanding={}, responses=2; fresh B: registered_pending={}, completed_before_A_drain={b_completed_while_a_paused}",
            a_bytes_at_pending - RELEASED_BEFORE_PAUSE,
            b_registered_pending,
        );
        // Cleanup is part of the proof, not skipped when B was blocked. Check
        // every byte of every A payload, not just a byte count or CRC.
        for index in 0..A_MESSAGES {
            assert_eq!(bounded(a.next()).await.unwrap().unwrap(), payload(index));
        }
        assert!(bounded(a.next()).await.unwrap().is_none());
        if !b_completed_while_a_paused {
            bounded(&mut collection).await;
        }
    }
    for session in [&mut a, &mut b] {
        let completion = bounded(session.finish()).await.unwrap();
        assert!(completion.source_exhausted);
        assert!(completion.source_closed);
        assert!(completion.upload_eof_polled);
        assert_eq!(completion.terminal_code, Some(Code::Ok));
        assert_eq!(completion.requests_produced, 0);
        assert_eq!(completion.requests_polled, 0);
    }
    assert_eq!(a.completion().responses_received, A_MESSAGES as u64);
    assert_eq!(b.completion().responses_received, 1);
    assert_eq!(a_closes.load(Ordering::SeqCst), 1);
    assert_eq!(b_closes.load(Ordering::SeqCst), 1);
    bounded(transport.close()).await.unwrap();
    peer.close().await;
    eprintln!(
        "drained A: all {A_MESSAGES} payloads identical; A/B trailers OK; sources closed exactly once; transport and peer joined"
    );
    Observation {
        b_completed_while_a_paused,
    }
}

async fn run() -> Observation {
    tokio::time::timeout(WHOLE, scenario())
        .await
        .expect("whole connection-window scenario exceeded deadline")
}

#[tokio::test]
async fn fresh_sibling_completes_while_other_consumer_is_paused() {
    let observed = run().await;
    // Assert only after healthy drain and all explicit cleanup, including RED.
    assert!(
        observed.b_completed_while_a_paused,
        "fresh sibling B was starved by paused A despite unused stream credit: {observed:?}"
    );
}
