//! Shared HTTP/2 connection with a caller-owned, joined driver and OS-root TLS.
use crate::{TransportConfig, TransportError};
use bytes::Bytes;
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
    pub sender: h2::client::SendRequest<Bytes>,
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
    let mut address = addresses.next().ok_or(TransportError::Connect)?;
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

pub(crate) async fn connect(config: &TransportConfig) -> Result<Connected, TransportError> {
    let authority: http::uri::Authority = config
        .target
        .parse()
        .map_err(|_| TransportError::InvalidConfig)?;
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']');
    // Resolve once inside the existing outer DNS/TCP/TLS/h2 timeout. Keep the
    // original host/port for authority and verified TLS, not an IP substitute.
    let addresses = tokio::net::lookup_host((host, config.port))
        .await
        .map_err(|_| TransportError::Connect)?;
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
        let certs = rustls_native_certs::load_native_certs();
        let mut roots = rustls::RootCertStore::empty();
        let (accepted, _) = roots.add_parsable_certificates(certs.certs);
        if accepted == 0 {
            return Err(TransportError::Connect);
        }
        let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| TransportError::Connect)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        tls.alpn_protocols = vec![b"h2".to_vec()];
        let name =
            ServerName::try_from(host.to_owned()).map_err(|_| TransportError::InvalidConfig)?;
        let stream = TlsConnector::from(Arc::new(tls))
            .connect(name, tcp)
            .await
            .map_err(|_| TransportError::Connect)?;
        if stream.get_ref().1.alpn_protocol() != Some(b"h2") {
            return Err(TransportError::Connect);
        }
        Box::new(stream)
    };
    let (sender, connection) = h2::client::Builder::new()
        // Separate bounded native connection credit from capacity-one app queues.
        // One paused stream keeps its 65,535-byte limit without exhausting the
        // entire connection, but 1MiB does not promise arbitrary sibling isolation.
        .initial_connection_window_size(1024 * 1024)
        .initial_window_size(65_535)
        .handshake::<_, Bytes>(io)
        .await
        .map_err(|_| TransportError::Connect)?;
    let stop = CancellationToken::new();
    let stopped = stop.clone();
    let handle = tokio::spawn(drive(connection, stopped));
    Ok(Connected {
        sender,
        driver: Driver {
            stop,
            handle: Mutex::new(Some(handle)),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{pin::Pin, sync::Mutex as StdMutex};
    use tokio::sync::watch;

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
    async fn entire_fast_failure_list_is_traversed_and_empty_list_is_connect() {
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
        assert!(matches!(result, Err(TransportError::Connect)));
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
