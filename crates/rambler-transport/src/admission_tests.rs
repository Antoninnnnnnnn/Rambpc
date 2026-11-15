//! Admission means local metadata success + h2 readiness + send_request success.
//! It does not mean server acceptance, delivery, or silent-NAT detection.
//! Source polling is gated on admission; accepted ownership always closes once.
use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const SCENARIO_LIMIT: Duration = Duration::from_secs(8);
const PEER_LIMIT: Duration = Duration::from_secs(5);
const RPC_LIMIT: Duration = Duration::from_secs(3);

#[derive(Default)]
struct Counts {
    next: AtomicU64,
    close: AtomicU64,
    cleanup_failed: AtomicBool,
}

struct CountedSource {
    counts: Arc<Counts>,
    fail_cleanup: bool,
}
impl RequestSource for CountedSource {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        self.counts.next.fetch_add(1, Ordering::SeqCst);
        // Endless synthetic input exposes both prefetch slots without EOF/replay.
        Box::pin(async { Ok(Some(Bytes::from_static(b"synthetic-admission-frame"))) })
    }

    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        self.counts.close.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if self.fail_cleanup {
                self.counts.cleanup_failed.store(true, Ordering::SeqCst);
                Err(SourceError::new(std::io::Error::other(
                    "synthetic cleanup failure",
                )))
            } else {
                Ok(())
            }
        })
    }
}

#[derive(Default)]
struct GatedMetadataFailure {
    entered: CancellationToken,
    release: CancellationToken,
    calls: AtomicU64,
    failed: AtomicBool,
}
impl MetadataProvider for GatedMetadataFailure {
    fn metadata(&self) -> MetadataFuture<'_> {
        Box::pin(async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.cancel();
            self.release.cancelled().await;
            self.failed.store(true, Ordering::SeqCst);
            Err(TransportError::Credentials)
        })
    }
}

// A local peer that observes zero request HEADERS. Peer work has its own deadline
// even if an assertion or the outer scenario deadline drops its JoinHandle.
async fn idle_peer() -> (GrpcTransport, JoinHandle<u64>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let ready = CancellationToken::new();
    let announced = ready.clone();
    let peer = tokio::spawn(async move {
        tokio::time::timeout(PEER_LIMIT, async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut connection = h2::server::handshake(tcp).await.unwrap();
            announced.cancel();
            let mut requests = 0;
            while let Some(request) = connection.accept().await {
                match request {
                    Ok((_request, mut response)) => {
                        requests += 1;
                        response.send_reset(h2::Reason::CANCEL);
                    }
                    // Local driver shutdown may arrive as EOF or an I/O error.
                    Err(_) => break,
                }
            }
            requests
        })
        .await
        .expect("idle peer exceeded its bounded lifetime")
    });
    let transport = GrpcTransport::connect(TransportConfig::loopback(address).unwrap())
        .await
        .unwrap();
    ready.cancelled().await;
    (transport, peer)
}

struct OpeningObservation {
    pending_next: u64,
    total_next: u64,
    close: u64,
    cleanup_failed: bool,
    selected: TransportError,
}

async fn metadata_failure_observation(fail_cleanup: bool) -> OpeningObservation {
    let (transport, peer) = idle_peer().await;
    let counts = Arc::new(Counts::default());
    let provider = Arc::new(GatedMetadataFailure::default());
    let mut session = transport
        .start_with_provider(
            CountedSource {
                counts: counts.clone(),
                fail_cleanup,
            },
            provider.clone(),
            RPC_LIMIT,
        )
        .unwrap();

    // This is an actual provider poll, not a scheduler delay. The producer must
    // still be waiting for the private local-admission gate.
    provider.entered.cancelled().await;
    assert!(!provider.failed.load(Ordering::SeqCst));
    let pending_next = counts.next.load(Ordering::SeqCst);
    provider.release.cancel();
    let selected = session.finish().await.unwrap_err();
    let completion = session.completion();
    let observation = OpeningObservation {
        pending_next,
        total_next: counts.next.load(Ordering::SeqCst),
        close: counts.close.load(Ordering::SeqCst),
        cleanup_failed: counts.cleanup_failed.load(Ordering::SeqCst),
        selected,
    };
    // Record the actual selected final outcome as well as both failure facts.
    eprintln!(
        "admission metadata: pending_next={} total_next={} close={} metadata_failed={} cleanup_failed={} selected={:?} requests_produced={} requests_polled={}",
        observation.pending_next,
        observation.total_next,
        observation.close,
        provider.failed.load(Ordering::SeqCst),
        observation.cleanup_failed,
        observation.selected,
        completion.requests_produced,
        completion.requests_polled,
    );
    transport.close().await.unwrap();
    let peer_requests = peer.await.unwrap();
    assert_eq!(peer_requests, 0, "metadata failure must not open a stream");
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        1,
        "no provider retry"
    );
    assert!(provider.failed.load(Ordering::SeqCst));
    assert!(completion.source_closed);
    assert_eq!(completion.requests_polled, 0);
    observation
}

#[tokio::test]
async fn admission_metadata_pending_then_failure_never_reads_source() {
    tokio::time::timeout(SCENARIO_LIMIT, async {
        let observed = metadata_failure_observation(false).await;
        assert!(matches!(observed.selected, TransportError::Credentials));
        assert_eq!(
            observed.close, 1,
            "close exactly once after opening failure"
        );
        assert_eq!(
            observed.pending_next, 0,
            "source read while metadata pending"
        );
        assert_eq!(
            observed.total_next, 0,
            "source read despite failed admission"
        );
    })
    .await
    .expect("bounded metadata admission scenario");
}

#[tokio::test]
async fn admission_known_goaway_before_start_never_reads_source() {
    tokio::time::timeout(SCENARIO_LIMIT, async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            tokio::time::timeout(PEER_LIMIT, async move {
                let (mut tcp, _) = listener.accept().await.unwrap();
                let mut preface = [0; 24];
                tcp.read_exact(&mut preface).await.unwrap();
                assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
                // Empty SETTINGS, then GOAWAY(last=0, PROTOCOL_ERROR).
                tcp.write_all(&[0, 0, 0, 4, 0, 0, 0, 0, 0])
                    .await
                    .unwrap();
                tcp.write_all(&[0, 0, 8, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])
                    .await
                    .unwrap();
                // Require actual client EOF, not a sleep after sending GOAWAY.
                let mut buffer = [0; 1024];
                while tcp.read(&mut buffer).await.unwrap() != 0 {}
            })
            .await
            .expect("GOAWAY peer did not observe client EOF before its deadline");
        });
        let transport = GrpcTransport::connect(TransportConfig::loopback(address).unwrap())
            .await
            .unwrap();
        peer.await.unwrap();
        let channel = transport
            .inner
            .lifecycle
            .lock()
            .unwrap()
            .channel
            .as_ref()
            .unwrap()
            .clone();
        let terminal = channel.ready().await.unwrap_err();
        assert!(terminal.is_go_away() && terminal.is_remote());
        assert_eq!(terminal.reason(), Some(h2::Reason::PROTOCOL_ERROR));
        // Joining the existing driver also proves its terminal error was not
        // replaced by local cancellation. This does not close the lifecycle.
        assert!(matches!(
            transport.inner.network.shutdown().await,
            Err(TransportError::Connect)
        ));
        assert!(!transport.inner.lifecycle.lock().unwrap().closed);
        eprintln!("admission known-dead premise: peer_client_eof=true h2_ready=remote_GOAWAY_PROTOCOL_ERROR driver_join=Connect lifecycle_closed=false; source not started yet");

        let counts = Arc::new(Counts::default());
        let mut session = transport
            .start(
                CountedSource {
                    counts: counts.clone(),
                    fail_cleanup: false,
                },
                Metadata::default(),
                RPC_LIMIT,
            )
            .unwrap();
        let selected = session.finish().await.unwrap_err();
        let completion = session.completion();
        let next = counts.next.load(Ordering::SeqCst);
        let close = counts.close.load(Ordering::SeqCst);
        eprintln!(
            "admission known-dead result: next={next} close={close} selected={selected:?} requests_produced={} requests_polled={}",
            completion.requests_produced, completion.requests_polled,
        );
        transport.close().await.unwrap();
        assert!(matches!(selected, TransportError::Grpc(Code::Internal)));
        assert!(completion.source_closed);
        assert_eq!(completion.requests_polled, 0);
        assert_eq!(close, 1, "close exactly once on the already-dead connection");
        assert_eq!(next, 0, "source read after h2 terminal state was proven before start");
    })
    .await
    .expect("bounded known-dead admission scenario; premise failure is harness uncertainty");
}

#[tokio::test]
async fn admission_opening_and_cleanup_failure_records_both_without_prefetch() {
    tokio::time::timeout(SCENARIO_LIMIT, async {
        let observed = metadata_failure_observation(true).await;
        assert!(
            observed.cleanup_failed,
            "cleanup failure must actually occur"
        );
        assert_eq!(
            observed.close, 1,
            "one cleanup attempt despite both failures"
        );
        let TransportError::PrimaryAndCleanup(failure) = &observed.selected else {
            panic!("opening failure was lost: {:?}", observed.selected);
        };
        assert!(matches!(failure.primary(), TransportError::Credentials));
        assert!(failure.cleanup().original().is::<std::io::Error>());
        assert_eq!(
            observed.pending_next, 0,
            "source read while metadata pending"
        );
        assert_eq!(
            observed.total_next, 0,
            "source read despite opening failure"
        );
    })
    .await
    .expect("bounded opening-plus-cleanup admission scenario");
}

#[test]
fn selected_primary_precedence_never_nests_or_invents_an_rpc_failure() {
    let cleanup = SourceError::new(std::io::Error::other("private cleanup"));
    for selected in [
        Ok(()),
        Err(TransportError::Credentials),
        Err(TransportError::Timeout),
        Err(TransportError::Cancelled),
    ] {
        for primary in [
            None,
            Some(TransportError::MessageTooLarge),
            Some(TransportError::Source(SourceError::new(
                std::io::Error::other("private read"),
            ))),
        ] {
            let expected = primary.clone().or_else(|| selected.clone().err());
            let result = ProducerOutcome {
                primary,
                cleanup: Some(cleanup.clone()),
            }
            .compose(selected.clone())
            .unwrap_err();
            match (expected, result) {
                (None | Some(TransportError::Cancelled), TransportError::Cleanup(actual)) => {
                    assert!(std::ptr::eq(actual.original(), cleanup.original()));
                }
                (Some(expected), TransportError::PrimaryAndCleanup(actual)) => {
                    assert_eq!(
                        std::mem::discriminant(actual.primary()),
                        std::mem::discriminant(&expected)
                    );
                    assert!(std::ptr::eq(
                        actual.cleanup().original(),
                        cleanup.original()
                    ));
                    assert!(!matches!(
                        actual.primary(),
                        TransportError::Cancelled
                            | TransportError::Cleanup(_)
                            | TransportError::PrimaryAndCleanup(_)
                    ));
                }
                (_, other) => panic!("unexpected composition: {other:?}"),
            }
        }
    }
}

#[tokio::test]
async fn pending_metadata_deadline_and_cancel_both_close_without_reads() {
    tokio::time::timeout(SCENARIO_LIMIT, async {
        for deadline in [false, true] {
            let (transport, peer) = idle_peer().await;
            let counts = Arc::new(Counts::default());
            let provider = Arc::new(GatedMetadataFailure::default());
            let mut session = transport
                .start_with_provider(
                    CountedSource {
                        counts: counts.clone(),
                        fail_cleanup: true,
                    },
                    provider.clone(),
                    if deadline {
                        Duration::from_millis(30)
                    } else {
                        RPC_LIMIT
                    },
                )
                .unwrap();
            provider.entered.cancelled().await;
            assert_eq!(counts.next.load(Ordering::SeqCst), 0);
            if !deadline {
                session.request_cancel();
            }
            let error = session.finish().await.unwrap_err();
            if deadline {
                assert!(matches!(error, TransportError::PrimaryAndCleanup(failure)
                    if matches!(failure.primary(), TransportError::Timeout)));
            } else {
                assert!(matches!(error, TransportError::Cleanup(_)));
            }
            assert_eq!(counts.next.load(Ordering::SeqCst), 0);
            assert_eq!(counts.close.load(Ordering::SeqCst), 1);
            assert_eq!(session.completion().requests_produced, 0);
            assert!(!session.completion().source_exhausted);
            transport.close().await.unwrap();
            assert_eq!(peer.await.unwrap(), 0);
        }
    })
    .await
    .expect("bounded pending admission shutdown");
}
