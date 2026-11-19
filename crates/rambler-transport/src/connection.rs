//! Shared HTTP/2 connection with a caller-owned, joined driver and OS-root TLS.
use crate::{TransportConfig, TransportError, wire::Envelope};
use futures_util::{StreamExt, stream::FuturesUnordered};
use std::{
    collections::VecDeque,
    future::{Future, poll_fn},
    io,
    net::SocketAddr,
    sync::Arc,
    task::Poll,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    sync::Mutex,
    task::JoinHandle,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{self, pki_types::ServerName},
};
use tokio_util::sync::CancellationToken;

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

pub(crate) struct Connected {
    pub sender: h2::client::SendRequest<Envelope>,
    pub driver: Driver,
}
pub(crate) struct Driver {
    stop: CancellationToken,
    handle: Mutex<Option<JoinHandle<Result<(), h2::Error>>>>,
}
impl Driver {
    pub async fn shutdown(&self) -> Result<(), TransportError> {
        self.stop.cancel();
        let mut slot = self.handle.lock().await;
        let result = if let Some(handle) = slot.as_mut() {
            match handle.await {
                Ok(result) => result.map_err(|_| TransportError::Connect),
                Err(_) => Err(TransportError::Supervisor),
            }
        } else {
            Ok(())
        };
        slot.take();
        result
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(handle) = self.handle.get_mut().take() {
            handle.abort();
        }
    }
}

async fn drive(
    connection: impl std::future::Future<Output = Result<(), h2::Error>>,
    stopped: CancellationToken,
) -> Result<(), h2::Error> {
    // Poll the connection first: shutdown must not replace an already ready
    // terminal failure with a successful local cancellation.
    tokio::select! { biased; result = connection => result, _ = stopped.cancelled() => Ok(()) }
}

const TCP_STAGGER: Duration = Duration::from_millis(250);
// RFC 8305 §5: even accelerated attempts MUST be at least 10ms apart.
const TCP_MIN_SPACING: Duration = Duration::from_millis(10);

/// Alternate families from the first answer, keeping every address and each
/// family's resolver order (including duplicates).
fn alternate_families(addresses: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let mut v4 = VecDeque::new();
    let mut v6 = VecDeque::new();
    let mut first_is_v6 = None;
    for address in addresses {
        first_is_v6.get_or_insert(address.is_ipv6());
        if address.is_ipv6() {
            v6.push_back(address);
        } else {
            v4.push_back(address);
        }
    }
    let mut use_v6 = first_is_v6.unwrap_or(false);
    let mut ordered = Vec::with_capacity(v4.len() + v6.len());
    while !v4.is_empty() || !v6.is_empty() {
        let address = if use_v6 {
            v6.pop_front().or_else(|| v4.pop_front())
        } else {
            v4.pop_front().or_else(|| v6.pop_front())
        };
        ordered.push(address.expect("at least one address remains"));
        use_v6 = !use_v6;
    }
    ordered
}

/// Race TCP only: returning or cancellation drops every losing future/stream
/// before the caller starts TLS. Never retry TLS or replay application data.
/// With the default 10s outer timeout and 250ms stagger, stalled attempts number
/// about 41 at most (also limited by DNS length). This is NOT a fixed bound for
/// arbitrary user timeouts; failed attempts are removed as they complete.
async fn race_tcp<T, Dial, Attempt, Now, Delay, Sleep>(
    addresses: impl IntoIterator<Item = SocketAddr>,
    mut dial: Dial,
    mut now: Now,
    mut delay: Delay,
) -> Result<T, TransportError>
where
    Dial: FnMut(SocketAddr) -> Attempt,
    Attempt: Future<Output = io::Result<T>>,
    Now: FnMut() -> tokio::time::Instant,
    Delay: FnMut(Duration) -> Sleep,
    Sleep: Future<Output = ()>,
{
    let mut addresses = addresses.into_iter().peekable();
    let mut address = addresses.next().ok_or(TransportError::Resolve)?;
    let mut attempts = FuturesUnordered::new();
    loop {
        let mut attempt = Box::pin(dial(address));
        // Poll ONCE with the caller's real waker, without a Pending boundary
        // between construction and initiation. Moving the Box into the set does
        // not move the !Unpin future. For Tokio's numeric SocketAddr connect,
        // this first poll initiates TCP; it does not measure kernel SYN timing.
        let initial = poll_fn(|cx| Poll::Ready(attempt.as_mut().poll(cx))).await;
        // Timestamp AFTER the first poll: even time spent initiating cannot
        // consume the spacing floor before the next application connect starts.
        let last_start = now();
        let mut next_due = last_start + TCP_STAGGER;
        match initial {
            Poll::Ready(Ok(stream)) => return Ok(stream),
            Poll::Ready(Err(_)) => {
                drop(attempt);
                if addresses.peek().is_none() && attempts.is_empty() {
                    return Err(TransportError::Connect);
                }
                next_due = next_due.min((last_start + TCP_MIN_SPACING).max(now()));
            }
            Poll::Pending => attempts.push(attempt),
        }
        let next_start = delay(next_due.saturating_duration_since(now()));
        tokio::pin!(next_start);
        // A long list of immediately-ready errors must not monopolize the
        // executor and prevent the enclosing connect timeout from being polled.
        tokio::task::yield_now().await;
        loop {
            tokio::select! {
                biased;
                result = attempts.next(), if !attempts.is_empty() => {
                    match result.expect("an attempt remains") {
                        Ok(stream) => return Ok(stream),
                        Err(_) => {
                            if addresses.peek().is_some() {
                                // Measure the floor from the LAST initiation,
                                // not the failed attempt. Never postpone a due.
                                let accelerated = (last_start + TCP_MIN_SPACING).max(now());
                                if accelerated < next_due {
                                    next_due = accelerated;
                                    next_start.set(delay(next_due.saturating_duration_since(now())));
                                }
                            } else if attempts.is_empty() {
                                return Err(TransportError::Connect);
                            }
                        }
                    }
                },
                _ = &mut next_start, if addresses.peek().is_some() => {
                    address = addresses.next().expect("an address remains");
                    break;
                },
            }
        }
    }
}

/// Aborting prevents a queued blocking job from starting, but cannot interrupt
/// one already running. Dropping its handle discards that job's eventual result.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
struct AbortOnDrop<T>(JoinHandle<T>);
#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn blocking_tls_setup<T: Send + 'static>(
    prepare: impl FnOnce() -> Result<T, TransportError> + Send + 'static,
) -> Result<T, TransportError> {
    let mut job = AbortOnDrop(tokio::task::spawn_blocking(prepare));
    (&mut job.0).await.map_err(|_| TransportError::Supervisor)?
}

fn tls_builder(
    provider: Arc<rustls::crypto::CryptoProvider>,
) -> Result<rustls::ConfigBuilder<rustls::ClientConfig, rustls::WantsVerifier>, TransportError> {
    rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| TransportError::Connect)
}

fn with_h2_alpn(mut tls: rustls::ClientConfig) -> rustls::ClientConfig {
    tls.alpn_protocols = vec![b"h2".to_vec()];
    tls
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn build_tls_config(
    load: impl FnOnce() -> rustls_native_certs::CertificateResult,
) -> Result<rustls::ClientConfig, TransportError> {
    let certs = load();
    let mut roots = rustls::RootCertStore::empty();
    let (accepted, _) = roots.add_parsable_certificates(certs.certs);
    if accepted == 0 {
        return Err(TransportError::TrustUnavailable);
    }
    let tls = tls_builder(Arc::new(rustls::crypto::ring::default_provider()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(with_h2_alpn(tls))
}

#[cfg(target_os = "ios")]
fn build_ios_tls_config() -> Result<rustls::ClientConfig, TransportError> {
    use rustls_platform_verifier::BuilderVerifierExt;

    let tls = tls_builder(Arc::new(rustls::crypto::ring::default_provider()))?
        .with_platform_verifier()
        .map_err(|_| TransportError::TrustUnavailable)?
        .with_no_client_auth();
    Ok(with_h2_alpn(tls))
}

#[cfg(target_os = "android")]
fn build_android_tls_config(host: &str) -> Result<rustls::ClientConfig, TransportError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = crate::android_tls::Verifier::new(provider.clone(), host.to_owned())?;
    let mut tls = tls_builder(provider)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    // Always ask the Android policy for a new connection. Do not retain TLS
    // authentication across changes to the host app's certificate policy.
    tls.resumption = rustls::client::Resumption::disabled();
    Ok(with_h2_alpn(tls))
}

fn tls_connect_error(error: io::Error) -> TransportError {
    // tokio-rustls embeds the typed rustls error directly in the I/O error.
    // Do not expose raw TLS/OS errors, or infer a cause from error strings.
    match error
        .get_ref()
        .and_then(|e| e.downcast_ref::<rustls::Error>())
    {
        Some(rustls::Error::InvalidCertificate(_) | rustls::Error::NoCertificatesPresented) => {
            TransportError::CertificateRejected
        }
        #[cfg(target_os = "android")]
        Some(rustls::Error::Other(other))
            if other.0.is::<crate::android_tls::PlatformTrustUnavailable>() =>
        {
            TransportError::TrustUnavailable
        }
        _ => TransportError::Connect,
    }
}

async fn connect_tls(
    tls: rustls::ClientConfig,
    name: ServerName<'static>,
    tcp: TcpStream,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, TransportError> {
    let stream = TlsConnector::from(Arc::new(tls))
        .connect(name, tcp)
        .await
        .map_err(tls_connect_error)?;
    if stream.get_ref().1.alpn_protocol() != Some(b"h2") {
        return Err(TransportError::Connect);
    }
    Ok(stream)
}

fn authority_host(authority: &http::uri::Authority) -> &str {
    let host = authority.host();
    host.strip_prefix('[')
        .and_then(|unbracketed| unbracketed.strip_suffix(']'))
        .unwrap_or(host)
}

#[cfg(any(target_os = "android", test))]
fn android_tls_names(host: &str) -> Result<(&str, ServerName<'static>), TransportError> {
    let identity =
        ServerName::try_from(host.to_owned()).map_err(|_| TransportError::InvalidConfig)?;
    Ok((host, identity))
}

fn discard_late_connect<T>(
    deadline: Option<tokio::time::Instant>,
    result: Result<T, TransportError>,
) -> Result<T, TransportError> {
    // Tokio can return Ready without polling its timer after synchronous
    // platform verification. Do not use a late success or failure; dropping the
    // result closes the socket or aborts an h2 driver that became ready.
    if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
        Err(TransportError::Timeout)
    } else {
        result
    }
}

pub(crate) async fn connect_with_timeout(
    config: &TransportConfig,
    wait_for_peer: bool,
) -> Result<Connected, TransportError> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    if let Some(deadline) = tokio::time::Instant::now().checked_add(config.connect_timeout) {
        let result =
            tokio::time::timeout_at(deadline, connect(config, wait_for_peer, Some(deadline)))
                .await
                .map_err(|_| TransportError::Timeout)?;
        return discard_late_connect(Some(deadline), result);
    }
    // Preserve desktop behavior and Tokio's handling of a mobile timeout
    // too large to represent as an absolute Instant.
    tokio::time::timeout(config.connect_timeout, connect(config, wait_for_peer, None))
        .await
        .map_err(|_| TransportError::Timeout)?
}

async fn connect(
    config: &TransportConfig,
    wait_for_peer: bool,
    deadline: Option<tokio::time::Instant>,
) -> Result<Connected, TransportError> {
    let authority: http::uri::Authority = config
        .target
        .parse()
        .map_err(|_| TransportError::InvalidConfig)?;
    let host = authority_host(&authority);
    // Construct the Android verifier before DNS/TCP so an app that has not
    // initialized trust fails deterministically without touching the network.
    #[cfg(target_os = "android")]
    let android_tls = if config.plaintext_loopback {
        None
    } else {
        let (policy_host, name) = android_tls_names(host)?;
        Some((build_android_tls_config(policy_host)?, name))
    };
    // Resolve once inside the existing outer DNS/TCP/TLS/h2 timeout. Keep the
    // original host/port for authority and verified TLS, not an IP substitute.
    let addresses = tokio::net::lookup_host((host, config.port))
        .await
        .map_err(|_| TransportError::Resolve)?;
    let tcp = race_tcp(
        alternate_families(addresses),
        TcpStream::connect,
        tokio::time::Instant::now,
        tokio::time::sleep,
    )
    .await?;
    tcp.set_nodelay(true).map_err(|_| TransportError::Connect)?;
    let io: Box<dyn Io> = if config.plaintext_loopback {
        Box::new(tcp)
    } else {
        #[cfg(not(target_os = "android"))]
        let (tls, name) = {
            #[cfg(target_os = "ios")]
            let tls = build_ios_tls_config()?;
            // Keeping the socket here lets the timeout close it even when an
            // OS root loader is still running in the blocking pool.
            #[cfg(not(target_os = "ios"))]
            let tls =
                blocking_tls_setup(|| build_tls_config(rustls_native_certs::load_native_certs))
                    .await?;
            let name =
                ServerName::try_from(host.to_owned()).map_err(|_| TransportError::InvalidConfig)?;
            (tls, name)
        };
        #[cfg(target_os = "android")]
        let (tls, name) = android_tls.ok_or(TransportError::TrustUnavailable)?;
        Box::new(discard_late_connect(
            deadline,
            connect_tls(tls, name, tcp).await,
        )?)
    };
    let (sender, mut connection) = h2::client::Builder::new()
        // Separate bounded native connection credit from capacity-one app queues.
        // One paused stream keeps its 65,535-byte limit without exhausting the
        // entire connection, but 1MiB does not promise arbitrary sibling isolation.
        .initial_connection_window_size(1024 * 1024)
        .initial_window_size(65_535)
        .handshake::<_, Envelope>(io)
        .await
        .map_err(|_| TransportError::Connect)?;
    // Client handshake only initializes the local end; it doesn't await peer
    // SETTINGS. A PONG confirms the peer is processing h2, without an RPC.
    let mut ping = if wait_for_peer {
        Some(connection.ping_pong().ok_or(TransportError::Connect)?)
    } else {
        None
    };
    let stop = CancellationToken::new();
    let stopped = stop.clone();
    let handle = tokio::spawn(drive(connection, stopped));
    let connected = Connected {
        sender,
        driver: Driver {
            stop,
            handle: Mutex::new(Some(handle)),
        },
    };
    // Own the driver across the await: cancelling/timing out connect drops it
    // and aborts the task rather than leaving a detached connection behind.
    if let Some(ping) = &mut ping {
        ping.ping(h2::Ping::opaque())
            .await
            .map_err(|_| TransportError::Connect)?;
    }
    Ok(connected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{pin::Pin, sync::Mutex as StdMutex};
    use tokio::sync::watch;

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    use std::sync::{
        Condvar,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    };
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    use tokio::sync::oneshot;

    // Every intentionally blocked worker has an independent OS-thread watchdog.
    // It releases even if the runtime is blocked or unwinding during shutdown.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    struct BlockingGate {
        open: Arc<(StdMutex<bool>, Condvar)>,
        release: Option<mpsc::Sender<()>>,
        watchdog: Option<std::thread::JoinHandle<()>>,
        expired: Arc<AtomicBool>,
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    impl BlockingGate {
        fn new() -> Self {
            let open = Arc::new((StdMutex::new(false), Condvar::new()));
            let expired = Arc::new(AtomicBool::new(false));
            let (release, wait) = mpsc::channel();
            let state = open.clone();
            let fired = expired.clone();
            let watchdog = std::thread::spawn(move || {
                if matches!(
                    wait.recv_timeout(Duration::from_secs(5)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    fired.store(true, Ordering::SeqCst);
                }
                *state.0.lock().unwrap() = true;
                state.1.notify_all();
            });
            Self {
                open,
                release: Some(release),
                watchdog: Some(watchdog),
                expired,
            }
        }
        fn blocker(&self) -> (oneshot::Receiver<()>, impl FnOnce() + Send + 'static) {
            let open = self.open.clone();
            let (started, wait) = oneshot::channel();
            (wait, move || {
                let _ = started.send(());
                let mut ready = open.0.lock().unwrap();
                while !*ready {
                    ready = open.1.wait(ready).unwrap();
                }
            })
        }
        fn release(&mut self) {
            if let Some(release) = self.release.take() {
                let _ = release.send(());
            }
            if let Some(watchdog) = self.watchdog.take() {
                watchdog.join().unwrap();
            }
        }
        fn assert_not_expired(&self) {
            assert!(
                !self.expired.load(Ordering::SeqCst),
                "out-of-runtime watchdog had to release the worker"
            );
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    impl Drop for BlockingGate {
        fn drop(&mut self) {
            self.release();
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn tls_test(future: impl Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(3), future)
                    .await
                    .expect("TLS setup test exceeded its runtime deadline");
            });
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    struct DropSignal(Option<oneshot::Sender<()>>);
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(signal) = self.0.take() {
                let _ = signal.send(());
            }
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn fake_roots() -> rustls_native_certs::CertificateResult {
        // Minimal synthetic v1 trust-anchor DER accepted by the root parser.
        // Parser/configuration fixture ONLY: not a usable TLS certificate, and
        // not evidence of certificate/hostname verification or a TLS handshake.
        const SYNTHETIC_ANCHOR: &[u8] = b"\x30\x14\x30\x0d\x02\x01\x01\x30\x00\x30\x00\x30\x00\x30\x00\x30\x00\x30\x00\x03\x01\x00";
        let mut roots = rustls_native_certs::CertificateResult::default();
        roots.certs.push(SYNTHETIC_ANCHOR.into());
        roots
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn fake_provider_error() -> rustls_native_certs::Error {
        rustls_native_certs::Error {
            context: "injected provider error",
            kind: rustls_native_certs::ErrorKind::Os(Box::new(io::Error::other("fake provider"))),
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "reads host public roots; run explicitly with SSL_CERT_FILE and SSL_CERT_DIR absent"]
    async fn tls_setup_native_public_roots_smoke() {
        // Read only the OS public trust roots through the production loader:
        // no private keys/client credentials, network I/O, or trust changes.
        // This does not qualify a TLS handshake or trust rotation.
        for name in ["SSL_CERT_FILE", "SSL_CERT_DIR"] {
            assert!(
                std::env::var_os(name).is_none(),
                "native-root smoke requires {name} to be absent in the child environment"
            );
        }
        let tls = blocking_tls_setup(|| build_tls_config(rustls_native_certs::load_native_certs))
            .await
            .expect("native public-root TLS configuration must succeed");
        assert_eq!(tls.alpn_protocols, [b"h2".to_vec()]);
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn tls_setup_keeps_current_thread_heartbeat_live_while_loader_blocks() {
        let mut gate = BlockingGate::new();
        let (started, block) = gate.blocker();
        tls_test(async {
            let mut setup = Box::pin(blocking_tls_setup(move || {
                build_tls_config(|| {
                    block();
                    fake_roots()
                })
            }));
            assert!(futures_util::poll!(setup.as_mut()).is_pending());
            started.await.unwrap();
            let mut beats = 0;
            for _ in 0..3 {
                tokio::time::sleep(Duration::from_millis(1)).await;
                beats += 1;
                assert!(futures_util::poll!(setup.as_mut()).is_pending());
            }
            assert_eq!(beats, 3);
            gate.release();
            assert_eq!(setup.await.unwrap().alpn_protocols, [b"h2".to_vec()]);
        });
        gate.assert_not_expired();
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn tls_setup_outer_timeout_closes_caller_owned_socket_before_loader_returns() {
        use tokio::{io::AsyncReadExt, net::TcpListener};

        let mut gate = BlockingGate::new();
        let (started, block) = gate.blocker();
        tls_test(async {
            let listener = TcpListener::bind(v4(0)).await.unwrap();
            let (tcp, peer) = tokio::join!(
                TcpStream::connect(listener.local_addr().unwrap()),
                listener.accept()
            );
            let tcp = tcp.unwrap();
            let (mut peer, _) = peer.unwrap();
            let after_setup = Arc::new(AtomicUsize::new(0));
            let continued = after_setup.clone();
            // Mirror connect's ownership boundary without calling the OS loader
            // or attempting TLS. The socket NEVER enters the blocking closure.
            let mut setup = Box::pin(async move {
                let tls = blocking_tls_setup(move || {
                    build_tls_config(|| {
                        block();
                        fake_roots()
                    })
                })
                .await?;
                continued.fetch_add(1, Ordering::SeqCst);
                drop((tls, tcp));
                Ok::<_, TransportError>(())
            });
            assert!(futures_util::poll!(setup.as_mut()).is_pending());
            started.await.unwrap();
            let result = tokio::time::timeout(Duration::from_millis(20), setup)
                .await
                .map_err(|_| TransportError::Timeout);
            assert!(matches!(result, Err(TransportError::Timeout)));
            assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
            assert_eq!(after_setup.load(Ordering::SeqCst), 0);
            gate.release();
            // A one-thread blocking pool makes this a completion fence.
            tokio::task::spawn_blocking(|| ()).await.unwrap();
            assert_eq!(after_setup.load(Ordering::SeqCst), 0);
        });
        gate.assert_not_expired();
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn tls_setup_drop_aborts_queued_loader_in_single_worker_pool() {
        let mut gate = BlockingGate::new();
        let (started, block) = gate.blocker();
        tls_test(async {
            let occupied = tokio::task::spawn_blocking(block);
            started.await.unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let called = calls.clone();
            let (discarded, wait) = oneshot::channel();
            let guard = DropSignal(Some(discarded));
            let mut setup = Box::pin(blocking_tls_setup(move || {
                let _guard = guard;
                called.fetch_add(1, Ordering::SeqCst);
                build_tls_config(fake_roots)
            }));
            assert!(futures_util::poll!(setup.as_mut()).is_pending());
            drop(setup);
            gate.release();
            occupied.await.unwrap();
            wait.await.unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        });
        gate.assert_not_expired();
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn tls_setup_drop_discards_started_result_without_continuation() {
        struct TrackedConfig {
            _config: rustls::ClientConfig,
            _discarded: DropSignal,
        }

        let mut gate = BlockingGate::new();
        let (started, block) = gate.blocker();
        tls_test(async {
            let (discarded, wait) = oneshot::channel();
            let after_setup = Arc::new(AtomicUsize::new(0));
            let continued = after_setup.clone();
            let mut setup = Box::pin(async move {
                let _tls = blocking_tls_setup(move || {
                    let config = build_tls_config(|| {
                        block();
                        fake_roots()
                    })?;
                    Ok(TrackedConfig {
                        _config: config,
                        _discarded: DropSignal(Some(discarded)),
                    })
                })
                .await?;
                // Neither TLS nor any source work belongs in the blocking job.
                continued.fetch_add(1, Ordering::SeqCst);
                Ok::<_, TransportError>(())
            });
            assert!(futures_util::poll!(setup.as_mut()).is_pending());
            started.await.unwrap();
            drop(setup);
            assert_eq!(after_setup.load(Ordering::SeqCst), 0);
            gate.release();
            wait.await.unwrap();
            assert_eq!(after_setup.load(Ordering::SeqCst), 0);
        });
        gate.assert_not_expired();
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn tls_setup_panic_is_fixed_supervisor_error() {
        tls_test(async {
            let error =
                blocking_tls_setup(|| build_tls_config(|| panic!("injected root loader panic")))
                    .await
                    .unwrap_err();
            assert!(matches!(error, TransportError::Supervisor));
            assert_eq!(error.to_string(), "session supervisor failed");
        });
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn tls_setup_preserves_fresh_roots_partial_success_and_provider_errors() {
        tls_test(async {
            let calls = Arc::new(AtomicUsize::new(0));
            let runtime_thread = std::thread::current().id();
            for _ in 0..2 {
                let called = calls.clone();
                let tls = blocking_tls_setup(move || {
                    build_tls_config(|| {
                        assert_ne!(std::thread::current().id(), runtime_thread);
                        called.fetch_add(1, Ordering::SeqCst);
                        let mut roots = fake_roots();
                        roots.errors.push(fake_provider_error());
                        roots.certs.push(vec![0xff].into());
                        roots
                    })
                })
                .await
                .unwrap();
                assert_eq!(tls.alpn_protocols, [b"h2".to_vec()]);
                assert!(!tls.enable_early_data);
            }
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            for invalid in [false, true] {
                let error = blocking_tls_setup(move || {
                    build_tls_config(|| {
                        let mut roots = rustls_native_certs::CertificateResult::default();
                        roots.errors.push(fake_provider_error());
                        if invalid {
                            roots.certs.push(vec![0xff].into());
                        }
                        roots
                    })
                })
                .await
                .unwrap_err();
                assert!(matches!(error, TransportError::TrustUnavailable));
                assert_eq!(error.to_string(), "transport TLS trust unavailable");
            }
        });
    }

    #[cfg(not(target_os = "android"))]
    fn loopback_tls_acceptor(h2: bool) -> tokio_rustls::TlsAcceptor {
        use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};

        const LEAF: &[u8] = include_bytes!("../tests/fixtures/tls-loopback-leaf.der");
        const KEY: &[u8] = include_bytes!("../tests/fixtures/tls-loopback-key.pkcs8");
        let mut server = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(LEAF)],
            PrivatePkcs8KeyDer::from(KEY).into(),
        )
        .unwrap();
        if h2 {
            server.alpn_protocols = vec![b"h2".to_vec()];
        }
        tokio_rustls::TlsAcceptor::from(Arc::new(server))
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn tls_loopback_requires_trusted_name_and_h2() {
        use rustls::pki_types::CertificateDer;
        use tokio::net::TcpListener;

        const CA: &[u8] = include_bytes!("../tests/fixtures/tls-loopback-ca.der");
        let trusted = || {
            let mut result = rustls_native_certs::CertificateResult::default();
            result.certs.push(CertificateDer::from(CA));
            result
        };
        tls_test(async {
            for (host, trust, h2) in [
                ("localhost", true, true),
                ("127.0.0.1", true, true),
                ("localhost", false, true),
                ("localhost", true, false),
            ] {
                let acceptor = loopback_tls_acceptor(h2);
                let listener = TcpListener::bind(v4(0)).await.unwrap();
                let address = listener.local_addr().unwrap();
                let (outcome, accepted) = tokio::join!(
                    async {
                        let tcp = TcpStream::connect(address).await.unwrap();
                        let tls = build_tls_config(|| if trust { trusted() } else { fake_roots() })
                            .unwrap();
                        let name = ServerName::try_from(host).unwrap();
                        connect_tls(tls, name, tcp).await
                    },
                    async {
                        let (tcp, _) = listener.accept().await.unwrap();
                        acceptor.accept(tcp).await
                    },
                );
                match (host, trust, h2) {
                    ("localhost", true, true) => {
                        assert!(outcome.is_ok());
                        assert!(accepted.is_ok());
                    }
                    ("localhost", true, false) => {
                        assert!(matches!(outcome, Err(TransportError::Connect)));
                    }
                    _ => assert!(matches!(outcome, Err(TransportError::CertificateRejected))),
                }
            }
        });
    }

    #[cfg(target_os = "ios")]
    #[tokio::test]
    async fn ios_tls_rejects_untrusted_loopback_certificate() {
        let listener = tokio::net::TcpListener::bind(v4(0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = TransportConfig::tls(format!("localhost:{}", address.port())).unwrap();
        let peer = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            loopback_tls_acceptor(true).accept(tcp).await
        });

        let Err(error) = connect_with_timeout(&config, false).await else {
            panic!("an untrusted certificate must prevent connection");
        };
        assert!(
            matches!(error, TransportError::CertificateRejected),
            "{error}"
        );
        assert!(peer.await.unwrap().is_err());
    }

    #[cfg(target_os = "android")]
    #[tokio::test]
    async fn tls_without_android_app_init_reports_trust_unavailable_before_networking() {
        let config = TransportConfig::tls("127.0.0.1:9").unwrap();
        assert!(matches!(
            connect_with_timeout(&config, true).await,
            Err(TransportError::TrustUnavailable)
        ));
        let marker = rustls::Error::Other(rustls::OtherError(Arc::new(
            crate::android_tls::PlatformTrustUnavailable,
        )));
        assert!(matches!(
            tls_connect_error(io::Error::other(marker)),
            TransportError::TrustUnavailable
        ));
    }

    #[test]
    fn authority_host_preserves_spelling_for_android_policy() {
        let authority: http::uri::Authority = "[2001:0DB8:0000:0:0:0:0:0010]:443".parse().unwrap();
        let (policy_host, identity) = android_tls_names(authority_host(&authority)).unwrap();
        assert_eq!(policy_host, "2001:0DB8:0000:0:0:0:0:0010");
        assert_eq!(identity.to_str(), "2001:db8::10");
        let authority: http::uri::Authority = "Example.COM.:443".parse().unwrap();
        assert_eq!(
            android_tls_names(authority_host(&authority)).unwrap().0,
            "Example.COM."
        );
    }

    #[test]
    fn a_late_android_connect_result_is_discarded() {
        let now = tokio::time::Instant::now();
        let future = now + Duration::from_secs(60);
        let payload = Arc::new(());
        let outcome = discard_late_connect(Some(now), Ok(payload.clone()));
        assert!(matches!(outcome, Err(TransportError::Timeout)));
        assert_eq!(Arc::strong_count(&payload), 1);
        let rejected = || Err::<(), _>(TransportError::CertificateRejected);
        assert!(matches!(
            discard_late_connect(Some(now), rejected()),
            Err(TransportError::Timeout)
        ));
        assert!(matches!(
            discard_late_connect(Some(future), rejected()),
            Err(TransportError::CertificateRejected)
        ));
    }

    // No Tokio test-util dependency or paused time: this clock affects only
    // injected, no-I/O attempts, never the numeric-loopback test below.
    #[derive(Clone)]
    struct Clock(watch::Sender<Duration>, tokio::time::Instant);
    impl Clock {
        fn new() -> Self {
            Self(
                watch::channel(Duration::ZERO).0,
                tokio::time::Instant::now(),
            )
        }
        fn instant(&self) -> tokio::time::Instant {
            self.1 + self.now()
        }
        fn now(&self) -> Duration {
            *self.0.borrow()
        }
        fn advance(&self, duration: Duration) {
            self.0.send_modify(|now| *now += duration);
        }
        fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send + 'static {
            let deadline = self.now() + duration;
            let mut ticks = self.0.subscribe();
            async move {
                loop {
                    if *ticks.borrow_and_update() >= deadline {
                        return;
                    }
                    ticks.changed().await.unwrap();
                }
            }
        }
    }

    #[derive(Clone)]
    struct Attempts {
        clock: Clock,
        started: Arc<StdMutex<Vec<(SocketAddr, Duration)>>>,
        dropped: Arc<StdMutex<Vec<SocketAddr>>>,
    }
    struct DropAttempt(Attempts, SocketAddr);
    impl Drop for DropAttempt {
        fn drop(&mut self) {
            self.0.dropped.lock().unwrap().push(self.1);
        }
    }
    impl Attempts {
        fn new(clock: &Clock) -> Self {
            Self {
                clock: clock.clone(),
                started: Arc::default(),
                dropped: Arc::default(),
            }
        }
        fn dial(
            &self,
            address: SocketAddr,
            outcome: Option<io::Result<SocketAddr>>,
        ) -> impl Future<Output = io::Result<SocketAddr>> + Send + 'static {
            self.dial_with_timing(address, outcome, Duration::ZERO, Duration::ZERO)
        }
        fn dial_with_timing(
            &self,
            address: SocketAddr,
            outcome: Option<io::Result<SocketAddr>>,
            first_poll_cost: Duration,
            completion_delay: Duration,
        ) -> impl Future<Output = io::Result<SocketAddr>> + Send + 'static {
            // Even an unpolled attempt owns a guard, but initiation is recorded
            // ONLY on its first poll. These counters prove future drops, not EOF.
            let guard = DropAttempt(self.clone(), address);
            let attempts = self.clone();
            async move {
                let _guard = guard;
                attempts
                    .started
                    .lock()
                    .unwrap()
                    .push((address, attempts.clock.now()));
                attempts.clock.advance(first_poll_cost);
                attempts.clock.sleep(completion_delay).await;
                match outcome {
                    Some(result) => result,
                    None => std::future::pending().await,
                }
            }
        }
        fn starts(&self) -> Vec<(SocketAddr, Duration)> {
            self.started.lock().unwrap().clone()
        }
        fn drops(&self) -> Vec<SocketAddr> {
            self.dropped.lock().unwrap().clone()
        }
    }

    fn v4(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }
    fn v6(port: u16) -> SocketAddr {
        SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port))
    }
    fn failed() -> Option<io::Result<SocketAddr>> {
        Some(Err(io::Error::from(io::ErrorKind::ConnectionRefused)))
    }
    async fn settle_pending<F: Future>(mut future: Pin<&mut F>) {
        for _ in 0..8 {
            assert!(futures_util::poll!(future.as_mut()).is_pending());
            tokio::task::yield_now().await;
        }
    }

    #[test]
    fn family_order_keeps_all_addresses_and_duplicates() {
        assert_eq!(
            alternate_families([v6(1), v6(2), v4(3), v4(4), v4(4), v6(5), v4(6)]),
            [v6(1), v4(3), v6(2), v4(4), v6(5), v4(4), v4(6)]
        );
        assert_eq!(
            alternate_families([v4(1), v4(2), v6(3), v6(4), v6(5)]),
            [v4(1), v6(3), v4(2), v6(4), v6(5)]
        );
        assert_eq!(alternate_families([v4(1), v4(2)]), [v4(1), v4(2)]);
        assert!(alternate_families([]).is_empty());
    }

    #[tokio::test]
    async fn pending_ipv6_falls_back_to_ipv4_at_250ms_and_drops_loser() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let race = race_tcp(
            alternate_families([v6(1), v4(2)]),
            |address| attempts.dial(address, (address == v4(2)).then_some(Ok(address))),
            || clock.instant(),
            |duration| clock.sleep(duration),
        );
        tokio::pin!(race);
        settle_pending(race.as_mut()).await;
        clock.advance(Duration::from_millis(249));
        settle_pending(race.as_mut()).await;
        assert_eq!(attempts.starts(), [(v6(1), Duration::ZERO)]);
        clock.advance(Duration::from_millis(1));
        assert_eq!(race.await.unwrap(), v4(2));
        assert_eq!(
            attempts.starts(),
            [(v6(1), Duration::ZERO), (v4(2), TCP_STAGGER)]
        );
        let mut dropped = attempts.drops();
        dropped.sort();
        let mut expected = [v6(1), v4(2)];
        expected.sort();
        assert_eq!(dropped, expected);
    }

    #[tokio::test]
    async fn executor_delay_before_repoll_cannot_collapse_first_poll_spacing() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let race = race_tcp(
            [v4(1), v4(2)],
            |address| attempts.dial(address, if address.port() == 1 { failed() } else { None }),
            || clock.instant(),
            |duration| clock.sleep(duration),
        );
        tokio::pin!(race);
        // Stop at the parent's FIRST suspension, then delay its executor 20ms.
        assert!(futures_util::poll!(race.as_mut()).is_pending());
        clock.advance(Duration::from_millis(20));
        for _ in 0..4 {
            assert!(futures_util::poll!(race.as_mut()).is_pending());
        }
        let starts = attempts.starts();
        assert_eq!(starts.len(), 2);
        assert!(
            starts[1].1 - starts[0].1 >= TCP_MIN_SPACING,
            "first-poll initiation gap collapsed below 10ms: {starts:?}"
        );
    }

    #[tokio::test]
    async fn pending_first_poll_is_not_deferred_until_after_executor_delay() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let race = race_tcp(
            [v4(1), v4(2)],
            |address| attempts.dial(address, None),
            || clock.instant(),
            |duration| clock.sleep(duration),
        );
        tokio::pin!(race);
        assert!(futures_util::poll!(race.as_mut()).is_pending());
        clock.advance(TCP_STAGGER + Duration::from_millis(20));
        for _ in 0..4 {
            assert!(futures_util::poll!(race.as_mut()).is_pending());
        }
        let starts = attempts.starts();
        assert_eq!(starts.len(), 2);
        assert!(
            starts[1].1 - starts[0].1 >= TCP_STAGGER,
            "pending first-poll initiation gap collapsed: {starts:?}"
        );
    }

    async fn first_poll_cost_is_excluded_from_next_spacing(first_fails: bool) {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let cost = Duration::from_millis(20);
        let spacing = if first_fails {
            TCP_MIN_SPACING
        } else {
            TCP_STAGGER
        };
        let race = race_tcp(
            [v4(1), v4(2)],
            |address| {
                attempts.dial_with_timing(
                    address,
                    if address.port() == 1 {
                        if first_fails { failed() } else { None }
                    } else {
                        Some(Ok(address))
                    },
                    if address.port() == 1 {
                        cost
                    } else {
                        Duration::ZERO
                    },
                    Duration::ZERO,
                )
            },
            || clock.instant(),
            |duration| clock.sleep(duration),
        );
        tokio::pin!(race);
        settle_pending(race.as_mut()).await;
        assert_eq!(clock.now(), cost);
        clock.advance(spacing - Duration::from_millis(1));
        settle_pending(race.as_mut()).await;
        assert_eq!(attempts.starts(), [(v4(1), Duration::ZERO)]);
        clock.advance(Duration::from_millis(1));
        assert_eq!(race.await.unwrap(), v4(2));
        assert_eq!(
            attempts.starts(),
            [(v4(1), Duration::ZERO), (v4(2), cost + spacing)]
        );
        assert_eq!(attempts.drops().len(), 2);
    }

    #[tokio::test]
    async fn ready_error_floor_starts_after_first_poll_returns() {
        first_poll_cost_is_excluded_from_next_spacing(true).await;
    }

    #[tokio::test]
    async fn pending_stagger_starts_after_first_poll_returns() {
        first_poll_cost_is_excluded_from_next_spacing(false).await;
    }

    #[tokio::test]
    async fn first_poll_ready_success_wins_without_yield_or_another_dial() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let race = race_tcp(
            [v4(1), v4(2)],
            |address| attempts.dial(address, Some(Ok(address))),
            || clock.instant(),
            |duration| clock.sleep(duration),
        );
        tokio::pin!(race);
        assert!(
            matches!(futures_util::poll!(race.as_mut()), std::task::Poll::Ready(Ok(address)) if address == v4(1))
        );
        assert_eq!(attempts.starts(), [(v4(1), Duration::ZERO)]);
        assert_eq!(attempts.drops(), [v4(1)]);
    }

    #[tokio::test]
    async fn dropping_race_after_two_first_polls_drops_both_pending_futures() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let mut race = Box::pin(race_tcp(
            [v4(1), v4(2), v4(3)],
            |address| attempts.dial(address, None),
            || clock.instant(),
            |duration| clock.sleep(duration),
        ));
        settle_pending(race.as_mut()).await;
        clock.advance(TCP_STAGGER);
        settle_pending(race.as_mut()).await;
        assert_eq!(
            attempts.starts(),
            [(v4(1), Duration::ZERO), (v4(2), TCP_STAGGER)]
        );
        assert!(attempts.drops().is_empty());
        drop(race);
        let mut dropped = attempts.drops();
        dropped.sort();
        assert_eq!(dropped, [v4(1), v4(2)]);
    }

    #[tokio::test]
    async fn fast_failure_respects_10ms_floor_before_next_start() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let race = race_tcp(
            [v6(1), v4(2)],
            |address| {
                attempts.dial(
                    address,
                    if address == v6(1) {
                        failed()
                    } else {
                        Some(Ok(address))
                    },
                )
            },
            || clock.instant(),
            |duration| clock.sleep(duration),
        );
        tokio::pin!(race);
        settle_pending(race.as_mut()).await;
        clock.advance(Duration::from_millis(9));
        settle_pending(race.as_mut()).await;
        assert_eq!(attempts.starts(), [(v6(1), Duration::ZERO)]);
        clock.advance(Duration::from_millis(1));
        assert_eq!(race.await.unwrap(), v4(2));
        assert_eq!(
            attempts.starts(),
            [(v6(1), Duration::ZERO), (v4(2), Duration::from_millis(10))]
        );
        assert_eq!(attempts.drops().len(), 2);
    }

    #[tokio::test]
    async fn later_fast_failure_uses_last_launch_floor_while_older_attempt_is_pending() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let race = race_tcp(
            [v6(1), v4(2), v6(3)],
            |address| {
                let outcome = match address.port() {
                    1 => None,
                    2 => failed(),
                    _ => Some(Ok(address)),
                };
                attempts.dial(address, outcome)
            },
            || clock.instant(),
            |duration| clock.sleep(duration),
        );
        tokio::pin!(race);
        settle_pending(race.as_mut()).await;
        clock.advance(TCP_STAGGER);
        settle_pending(race.as_mut()).await;
        clock.advance(TCP_MIN_SPACING - Duration::from_millis(1));
        settle_pending(race.as_mut()).await;
        assert_eq!(attempts.starts().len(), 2);
        clock.advance(Duration::from_millis(1));
        assert_eq!(race.await.unwrap(), v6(3));
        assert_eq!(
            attempts.starts(),
            [
                (v6(1), Duration::ZERO),
                (v4(2), TCP_STAGGER),
                (v6(3), TCP_STAGGER + TCP_MIN_SPACING)
            ]
        );
        assert_eq!(attempts.drops().len(), 3);
    }

    async fn delayed_first_failure(
        failure_at_ms: u64,
        winner_port: u16,
        advances_ms: &[u64],
        expected: &[(SocketAddr, Duration)],
    ) {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let race = race_tcp(
            (1..=winner_port).map(v4),
            |address| {
                attempts.dial_with_timing(
                    address,
                    if address.port() == 1 {
                        failed()
                    } else {
                        (address.port() == winner_port).then_some(Ok(address))
                    },
                    Duration::ZERO,
                    if address.port() == 1 {
                        Duration::from_millis(failure_at_ms)
                    } else {
                        Duration::ZERO
                    },
                )
            },
            || clock.instant(),
            |duration| clock.sleep(duration),
        );
        tokio::pin!(race);
        settle_pending(race.as_mut()).await;
        let (last, earlier) = advances_ms.split_last().unwrap();
        for &advance in earlier {
            clock.advance(Duration::from_millis(advance));
            settle_pending(race.as_mut()).await;
        }
        clock.advance(Duration::from_millis(*last));
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), race)
                .await
                .expect("next attempt was postponed")
                .unwrap(),
            v4(winner_port)
        );
        assert_eq!(attempts.starts(), expected);
        assert_eq!(attempts.drops().len(), usize::from(winner_port));
    }

    #[tokio::test]
    async fn failure_at_200ms_accelerates_next_start_to_now() {
        delayed_first_failure(
            200,
            2,
            &[199, 1],
            &[(v4(1), Duration::ZERO), (v4(2), Duration::from_millis(200))],
        )
        .await;
    }

    #[tokio::test]
    async fn older_attempt_failure_uses_most_recent_launch_for_10ms_floor() {
        delayed_first_failure(
            255,
            3,
            &[250, 5, 4, 1],
            &[
                (v4(1), Duration::ZERO),
                (v4(2), TCP_STAGGER),
                (v4(3), Duration::from_millis(260)),
            ],
        )
        .await;
    }

    #[tokio::test]
    async fn late_failure_cannot_postpone_an_already_programmed_start() {
        // The executor observes the first failure at 600ms, after the 500ms
        // due time. Biased failure polling must preserve that ready timer.
        delayed_first_failure(
            600,
            3,
            &[250, 350],
            &[
                (v4(1), Duration::ZERO),
                (v4(2), TCP_STAGGER),
                (v4(3), Duration::from_millis(600)),
            ],
        )
        .await;
    }

    #[tokio::test]
    async fn ninth_address_can_win_after_eight_pending_attempts() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let race = race_tcp(
            (1..=9).map(v4),
            |address| attempts.dial(address, (address.port() == 9).then_some(Ok(address))),
            || clock.instant(),
            |duration| clock.sleep(duration),
        );
        tokio::pin!(race);
        settle_pending(race.as_mut()).await;
        for _ in 1..8 {
            clock.advance(TCP_STAGGER);
            settle_pending(race.as_mut()).await;
        }
        assert_eq!(attempts.starts().len(), 8);
        clock.advance(TCP_STAGGER);
        assert_eq!(race.await.unwrap(), v4(9));
        assert_eq!(
            attempts.starts(),
            (1..=9)
                .map(|port| (v4(port), TCP_STAGGER * u32::from(port - 1)))
                .collect::<Vec<_>>()
        );
        assert_eq!(clock.now(), Duration::from_secs(2));
        assert_eq!(attempts.drops().len(), 9);
    }

    #[tokio::test]
    async fn entire_fast_failure_list_is_traversed_and_empty_list_is_resolve() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let race = race_tcp(
            (1..=129).map(v4),
            |address| attempts.dial(address, failed()),
            || clock.instant(),
            |duration| clock.sleep(duration),
        );
        tokio::pin!(race);
        settle_pending(race.as_mut()).await;
        for count in 1..128 {
            assert_eq!(attempts.starts().len(), count);
            clock.advance(TCP_MIN_SPACING);
            settle_pending(race.as_mut()).await;
        }
        assert_eq!(attempts.starts().len(), 128);
        clock.advance(TCP_MIN_SPACING - Duration::from_millis(1));
        settle_pending(race.as_mut()).await;
        assert_eq!(attempts.starts().len(), 128);
        clock.advance(Duration::from_millis(1));
        assert!(matches!(race.await, Err(TransportError::Connect)));
        assert_eq!(
            attempts.starts(),
            (1..=129)
                .map(|port| (v4(port), TCP_MIN_SPACING * u32::from(port - 1)))
                .collect::<Vec<_>>()
        );
        assert_eq!(clock.now(), Duration::from_millis(1280));
        assert_eq!(attempts.drops().len(), 129);
        let result = race_tcp(
            [],
            |address| attempts.dial(address, failed()),
            || clock.instant(),
            |duration| clock.sleep(duration),
        )
        .await;
        assert!(matches!(result, Err(TransportError::Resolve)));
        assert_eq!(attempts.starts().len(), 129);
    }

    #[tokio::test]
    async fn fake_outer_deadline_drops_every_pending_attempt() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        // Model the existing outer timeout's cancellation/drop boundary with
        // fake time. The public timeout mapping in lib.rs is unchanged.
        let deadline = clock.sleep(Duration::from_millis(750));
        let timed = async {
            tokio::select! {
                biased;
                _ = deadline => Err(TransportError::Timeout),
                result = race_tcp((1..=20).map(v4), |address| attempts.dial(address, None), || clock.instant(), |duration| clock.sleep(duration)) => result,
            }
        };
        tokio::pin!(timed);
        settle_pending(timed.as_mut()).await;
        for _ in 0..2 {
            clock.advance(TCP_STAGGER);
            settle_pending(timed.as_mut()).await;
        }
        assert_eq!(attempts.starts().len(), 3);
        assert!(attempts.drops().is_empty());
        clock.advance(TCP_STAGGER);
        assert!(matches!(timed.await, Err(TransportError::Timeout)));
        assert_eq!(attempts.drops().len(), 3);
    }

    #[tokio::test]
    async fn ready_failures_yield_so_an_outer_deadline_is_polled() {
        let clock = Clock::new();
        let attempts = Attempts::new(&clock);
        let deadline = clock.sleep(Duration::from_millis(81));
        // Simulate the executor consuming each pacing wait before the race can
        // yield. Every timer is already ready: explicit yields, not a Pending
        // pacing timer, must let the enclosing deadline run after each launch.
        let elapsed_sleep = |duration| {
            let sleep = clock.sleep(duration);
            clock.advance(duration);
            sleep
        };
        let timed = async {
            tokio::select! {
                biased;
                _ = deadline => Err(TransportError::Timeout),
                result = race_tcp((1..=129).map(v4), |address| attempts.dial(address, failed()), || clock.instant(), elapsed_sleep) => result,
            }
        };
        tokio::pin!(timed);
        for count in 1..=9 {
            assert!(futures_util::poll!(timed.as_mut()).is_pending());
            assert_eq!(attempts.starts().len(), count);
        }
        assert_eq!(clock.now(), Duration::from_millis(90));
        assert!(matches!(timed.await, Err(TransportError::Timeout)));
        assert_eq!(attempts.starts().len(), 9);
        assert_eq!(attempts.drops().len(), 9);
    }

    #[tokio::test]
    async fn numeric_loopback_losing_connected_socket_is_closed_before_return() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio::sync::oneshot;

        tokio::time::timeout(Duration::from_secs(10), async {
            let listener = TcpListener::bind(v4(0)).await.unwrap();
            let address = listener.local_addr().unwrap();
            let (loser, peer) = tokio::join!(TcpStream::connect(address), listener.accept());
            let loser = loser.unwrap();
            let (mut loser_peer, _) = peer.unwrap();
            let (winner, peer) = tokio::join!(TcpStream::connect(address), listener.accept());
            let winner = winner.unwrap();
            let (mut winner_peer, _) = peer.unwrap();
            let (gate, wait) = oneshot::channel::<()>();
            let mut streams = VecDeque::from([(loser, Some(wait)), (winner, None)]);
            let mut won = race_tcp(
                [address, address],
                |_| {
                    let (stream, wait) = streams.pop_front().unwrap();
                    async move {
                        if let Some(wait) = wait {
                            let _ = wait.await;
                        }
                        Ok(stream)
                    }
                },
                tokio::time::Instant::now,
                tokio::time::sleep,
            )
            .await
            .unwrap();
            assert!(gate.send(()).is_err(), "loser future was not dropped");
            let mut byte = [0];
            assert_eq!(
                loser_peer.read(&mut byte).await.unwrap(),
                0,
                "loser peer did not observe OS socket EOF"
            );
            won.write_all(b"W").await.unwrap();
            winner_peer.read_exact(&mut byte).await.unwrap();
            assert_eq!(&byte, b"W");
            drop(won);
            assert_eq!(winner_peer.read(&mut byte).await.unwrap(), 0);
        })
        .await
        .expect("numeric-loopback test exceeded its real-time deadline");
    }

    #[tokio::test]
    async fn shutdown_cannot_mask_ready_driver_failure_but_still_stops_pending_io() {
        let stopped = CancellationToken::new();
        stopped.cancel();
        let result = drive(
            std::future::ready(Err(h2::Reason::PROTOCOL_ERROR.into())),
            stopped.clone(),
        )
        .await;
        assert_eq!(
            result.unwrap_err().reason(),
            Some(h2::Reason::PROTOCOL_ERROR)
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            drive(std::future::pending(), stopped),
        )
        .await
        .unwrap()
        .unwrap();
    }
}
