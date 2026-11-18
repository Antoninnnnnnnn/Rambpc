//! Application-level recovery from a real TCP interruption on owned loopback.
//! Both sides are synthetic; there is no provider, microphone, or replay.
mod common;

use bytes::Bytes;
use common::{TestServer, bounded, replies};
use futures_util::{StreamExt, stream};
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, ContextProvider, EditingContext, Engine,
    EngineError, SessionOptions, SourceError, SourceFuture, collect_dictation,
    collect_dictation_with_observer,
};
use rambler_protocol::{
    AUDIO_DATA, RECOGNITION_RESPONSE, end_request,
    wire::{Value, bytes_field as b, parse_fields, string_field as s, varint_field as v},
};
use rambler_transport::{TransportConfig, TransportError};
use std::{
    future::pending,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
    task::JoinSet,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cut {
    AwaitingMoreAudio,
    LateContext,
    AwaitingTrailers,
}

#[derive(Default)]
struct SourceState {
    reads: AtomicUsize,
    closes: AtomicUsize,
}

struct Input {
    state: Arc<SourceState>,
    first: Option<Bytes>,
    pause_after_first: Option<Arc<Notify>>,
}
impl Input {
    fn new(
        data: &'static [u8],
        pause_after_first: Option<Arc<Notify>>,
    ) -> (Self, Arc<SourceState>) {
        let state = Arc::new(SourceState::default());
        (
            Self {
                state: state.clone(),
                first: Some(Bytes::from_static(data)),
                pause_after_first,
            },
            state,
        )
    }
}
impl AudioSource for Input {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            self.state.reads.fetch_add(1, Ordering::SeqCst);
            if let Some(first) = self.first.take() {
                return Ok(Some(first));
            }
            if let Some(gate) = &self.pause_after_first {
                gate.notify_one();
                pending().await
            } else {
                Ok(None)
            }
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.state.closes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

struct WaitingContext(Arc<Notify>);
impl ContextProvider for WaitingContext {
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
        Box::pin(async move {
            self.0.notify_one();
            pending().await
        })
    }
}

fn check_audio(message: &Bytes, expected: &[u8]) {
    let fields = parse_fields(message).unwrap();
    let data = fields.iter().find(|f| f.number == AUDIO_DATA).unwrap();
    let Value::Bytes(data) = &data.value else {
        panic!("expected an audio wrapper");
    };
    let nested = parse_fields(data).unwrap();
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].number, 1);
    assert!(matches!(&nested[0].value, Value::Bytes(bytes) if bytes.as_slice() == expected));
}

fn final_message(text: &str) -> Bytes {
    let hypothesis = b(3, &s(1, text).unwrap()).unwrap();
    let recognition = [v(1, 1).unwrap(), b(3, &hypothesis).unwrap()].concat();
    Bytes::from(b(RECOGNITION_RESPONSE, &b(1, &recognition).unwrap()).unwrap())
}

async fn case(cut: Cut) {
    let calls = Arc::new(AtomicUsize::new(0));
    let peer_calls = calls.clone();
    let saw_upload = Arc::new(Notify::new());
    let peer_upload = saw_upload.clone();
    let peer = TestServer::new(move |request| {
        let index = peer_calls.fetch_add(1, Ordering::SeqCst);
        let peer_upload = peer_upload.clone();
        async move {
            assert!(!request.metadata().contains_key("x-goog-api-key"));
            let mut body = request.into_inner();
            let init = body.message().await.unwrap().unwrap();
            assert!(
                !parse_fields(&init)
                    .unwrap()
                    .iter()
                    .any(|f| f.number == AUDIO_DATA)
            );
            let audio = body.message().await.unwrap().unwrap();
            if index == 0 {
                check_audio(&audio, b"interrupted audio");
                if cut == Cut::AwaitingTrailers {
                    assert_eq!(
                        body.message().await.unwrap().unwrap().as_ref(),
                        end_request()
                    );
                    assert!(body.message().await.unwrap().is_none());
                }
                peer_upload.notify_one();
                // A final-looking recognition must not commit text. Keeping the
                // input stream live also avoids an accidental early half-close.
                let responses = stream::iter([Ok(final_message("Do not commit this."))]).chain(
                    stream::once(async move {
                        let _held_body = body;
                        pending().await
                    }),
                );
                Ok(replies(responses))
            } else {
                assert_eq!(
                    index, 1,
                    "only the new explicit dictation may reach this peer"
                );
                check_audio(&audio, b"fresh audio");
                assert_eq!(
                    body.message().await.unwrap().unwrap().as_ref(),
                    end_request()
                );
                assert!(body.message().await.unwrap().is_none());
                Ok(replies(stream::iter([
                    Ok(final_message("Fresh capture only.")),
                    Ok(Bytes::from_static(b"\x08\x01")),
                ])))
            }
        }
    })
    .await;

    let listener = Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap());
    let address = listener.local_addr().unwrap();
    let backend = peer.address;
    let first_listener = listener.clone();
    let (cut_now, cut_requested) = oneshot::channel();
    let mut owners = JoinSet::new();
    owners.spawn(async move {
        let (mut client, _) = first_listener.accept().await.unwrap();
        let mut upstream = TcpStream::connect(backend).await.unwrap();
        tokio::select! {
            result = copy_bidirectional(&mut client, &mut upstream) => panic!("ended before controlled cut: {result:?}"),
            _ = cut_requested => (),
        }
        // This endpoint owns both sockets; dropping them actually breaks TCP.
    });
    let config = TransportConfig::loopback(address).unwrap();
    let old = Engine::connect_ready(config.clone()).await.unwrap();
    let waiting = Arc::new(Notify::new());
    let observed = Arc::new(Notify::new());
    let (input, source_state) = Input::new(
        b"interrupted audio",
        (cut == Cut::AwaitingMoreAudio).then(|| waiting.clone()),
    );
    let mut options = SessionOptions::default();
    if cut == Cut::LateContext {
        options.context = ContextInput::Provider(Arc::new(WaitingContext(waiting.clone())));
    }
    let mut observe = |observation: rambler_engine::DictationObservation<'_>| {
        assert_eq!(observation.metrics.responses_received, 1);
        observed.notify_one();
    };
    let collector = collect_dictation_with_observer(
        &old,
        Box::new(input),
        options,
        CollectorLimits::default(),
        &mut observe,
    );
    let cut_after_gates = async {
        saw_upload.notified().await;
        if cut != Cut::AwaitingTrailers {
            waiting.notified().await;
        }
        observed.notified().await;
        cut_now.send(()).unwrap();
    };
    let (result, ()) = tokio::join!(collector, cut_after_gates);
    let terminal = result.unwrap_err();
    assert!(
        matches!(terminal, EngineError::Transport(_)),
        "{cut:?}: {terminal:?}"
    );
    assert_eq!(source_state.reads.load(Ordering::SeqCst), 2);
    assert_eq!(source_state.closes.load(Ordering::SeqCst), 1);
    owners.join_next().await.unwrap().unwrap();

    // Starting another source on the dead Engine cannot silently reconnect or
    // consume it. A busy new producer and a newly made Engine are separate things.
    let (still_disconnected, rejected) = Input::new(b"must not upload", None);
    assert!(
        collect_dictation(
            &old,
            Box::new(still_disconnected),
            SessionOptions::default(),
            CollectorLimits::default(),
        )
        .await
        .is_err()
    );
    assert_eq!(rejected.reads.load(Ordering::SeqCst), 0);
    assert_eq!(rejected.closes.load(Ordering::SeqCst), 1);
    assert!(matches!(
        old.close().await,
        Ok(()) | Err(EngineError::Transport(TransportError::Connect))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // A locally queued TCP connect would otherwise look like the new explicit one.
    assert!(
        tokio::time::timeout(Duration::from_millis(35), listener.accept())
            .await
            .is_err()
    );

    owners.spawn(async move {
        let (mut client, _) = listener.accept().await.unwrap();
        let mut upstream = TcpStream::connect(backend).await.unwrap();
        let _ = copy_bidirectional(&mut client, &mut upstream).await;
    });
    let fresh = Engine::connect_ready(config).await.unwrap();
    let (audio, fresh_state) = Input::new(b"fresh audio", None);
    let text = collect_dictation(
        &fresh,
        Box::new(audio),
        SessionOptions::default(),
        CollectorLimits::default(),
    )
    .await
    .unwrap()
    .text()
    .unwrap();
    assert_eq!(text, "Fresh capture only.");
    assert_eq!(fresh_state.closes.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(peer.accepted_connections(), 2);
    fresh.close().await.unwrap();
    owners.join_next().await.unwrap().unwrap();
    peer.close().await;
}

#[tokio::test]
async fn explicit_recovery_after_tcp_cut_never_replays_or_commits_an_incomplete_dictation() {
    for cut in [
        Cut::AwaitingMoreAudio,
        Cut::LateContext,
        Cut::AwaitingTrailers,
    ] {
        bounded(case(cut)).await;
    }
}
