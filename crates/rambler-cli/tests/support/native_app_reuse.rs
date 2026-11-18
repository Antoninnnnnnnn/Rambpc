use super::*;
use rambler_engine::collect_dictation_with_observer;
use std::path::{Path, PathBuf};

const SEQUENTIAL: usize = 52;
const CONCURRENT: usize = 4;

#[derive(Clone, Copy)]
enum Input {
    Pcm16,
    Pcm48,
    Ogg,
}

struct Case {
    index: usize,
    input: Input,
    samples: usize,
    pcm: Vec<u8>,
    expected: Vec<Bytes>,
    path: Option<PathBuf>,
    context: Option<EditingContext>,
    initial: Vec<u8>,
}

fn protocol(index: usize) -> ProtocolConfig {
    ProtocolConfig {
        application_id: format!("native-reuse-{index}"),
        ..Default::default()
    }
}

impl Case {
    async fn new(index: usize, dir: &Path) -> Self {
        let input = match index % 3 {
            0 => Input::Pcm16,
            1 => Input::Pcm48,
            _ => Input::Ogg,
        };
        let wanted = 2 * FRAME_SAMPLES + 17 + index;
        let samples_in = if matches!(input, Input::Pcm48) {
            wanted * 3 - 1
        } else {
            wanted
        };
        let pcm = pcm(0, samples_in, index);
        let source = audio_common::Source::new([pcm.clone()]);
        let source: Box<dyn AudioSource> = if matches!(input, Input::Pcm48) {
            Box::new(Pcm16Opus::new(
                Pcm48kTo16k::new(source),
                EncoderConfig::default(),
            ))
        } else {
            Box::new(Pcm16Opus::new(source, EncoderConfig::default()))
        };
        let expected: Vec<Bytes> = encoded_pages(source).await;
        let path = if matches!(input, Input::Ogg) {
            let path = dir.join(format!("dictation-{index}.ogg"));
            tokio::fs::write(&path, expected.concat()).await.unwrap();
            Some(path)
        } else {
            None
        };
        let context = (!index.is_multiple_of(2)).then(|| EditingContext {
            before: format!("before {index}"),
            after: format!("after {index}"),
            cursor_aware: true,
            ..Default::default()
        });
        Self {
            index,
            input,
            samples: wanted,
            pcm,
            expected,
            path,
            context,
            initial: initial_request(&protocol(index), 0).unwrap(),
        }
    }

    fn draft(&self) -> String {
        format!("session {} draft", self.index)
    }

    fn final_text(&self) -> String {
        format!("session {} final", self.index)
    }
}

async fn encoded_pages(mut source: Box<dyn AudioSource>) -> Vec<Bytes> {
    let mut expected = Vec::new();
    while let Some(page) = source.next().await.unwrap() {
        expected.push(page);
    }
    source.close().await.unwrap();
    expected
}

#[derive(Default)]
struct PeerState {
    claims: HashSet<usize>,
    remote: Option<SocketAddr>,
    live: usize,
    peak: usize,
}

struct Peer {
    cases: Vec<Arc<Case>>,
    receipts: Vec<Mutex<Option<Receipt>>>,
    state: Mutex<PeerState>,
    concurrent_uploads: Barrier,
    retired: Notify,
}

struct RpcLifetime(Arc<Peer>);

impl Drop for RpcLifetime {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().live -= 1;
        self.0.retired.notify_one();
    }
}

impl Peer {
    async fn serve(
        self: Arc<Self>,
        request: Request<Streaming<Bytes>>,
    ) -> Result<Response<Replies>, Status> {
        let remote = request.remote_addr().unwrap();
        assert!(remote.ip().is_loopback());
        {
            let mut state = self.state.lock().unwrap();
            assert_eq!(*state.remote.get_or_insert(remote), remote);
            state.live += 1;
            state.peak = state.peak.max(state.live);
        }
        let lifetime = RpcLifetime(self.clone());
        let mut upload = Upload::new(request.into_inner());
        let init = upload.required().await;
        let Some(case) = self
            .cases
            .iter()
            .find(|case| case.initial == init.as_ref())
            .cloned()
        else {
            return Err(Status::failed_precondition("unexpected dictation init"));
        };
        assert!(
            self.state.lock().unwrap().claims.insert(case.index),
            "replayed RPC {}",
            case.index
        );
        let exact = matches!(case.input, Input::Ogg);
        for page in &case.expected[..3] {
            upload.expect_page(page, exact).await;
        }
        if case.index >= SEQUENTIAL {
            // All concurrent streams must have uploaded media before any is
            // allowed a response, preventing accidental serial qualification.
            self.concurrent_uploads.wait().await;
        }
        let draft = recognition(false, &case.draft());
        let tail = stream::once(async move {
            for page in &case.expected[3..] {
                upload.expect_page(page, exact).await;
            }
            upload.pages.assert_duration(case.samples);
            upload.finish(case.context.as_ref()).await;
            assert!(
                self.receipts[case.index]
                    .lock()
                    .unwrap()
                    .replace(upload.receipt)
                    .is_none()
            );
            Ok(recognition(true, &case.final_text()))
        });
        // Hold through the end of the response body, including on abort.
        let response = stream::iter([Ok(draft)])
            .chain(tail)
            .chain(stream::iter([Ok(END)]));
        let held = stream::unfold(
            (Box::pin(response), lifetime),
            |(mut response, lifetime)| async move {
                let frame = response.next().await?;
                Some((frame, (response, lifetime)))
            },
        );
        Ok(replies(held))
    }

    async fn idle(&self) {
        loop {
            let retired = self.retired.notified();
            if self.state.lock().unwrap().live == 0 {
                break;
            }
            retired.await;
        }
    }
}

async fn feed(mut writer: PcmSender, bytes: &[u8], draft: &Notify) {
    for fragment in bytes.chunks(319) {
        writer
            .reserve()
            .await
            .unwrap()
            .send(Bytes::copy_from_slice(fragment))
            .unwrap();
    }
    draft.notified().await;
    writer.finish().unwrap();
}

async fn run(engine: &Engine, case: &Case, peer: &Peer) -> u64 {
    let options = SessionOptions {
        config: protocol(case.index),
        context: case
            .context
            .clone()
            .map_or(ContextInput::None, ContextInput::Static),
        ..Default::default()
    };
    let limits = CollectorLimits::default();
    let draft = Notify::new();
    let mut seen = 0;
    let mut session_id = None;
    let mut bytes = 0;
    let mut observe = |event: DictationObservation<'_>| {
        seen += 1;
        bytes += event.presentation.response.raw.len() as u64;
        assert_eq!(event.metrics.responses_received, seen);
        assert_eq!(event.metrics.response_protobuf_bytes, bytes);
        assert_eq!(
            *session_id.get_or_insert(event.metrics.session_id),
            event.metrics.session_id
        );
        assert!(event.metrics.eof_seconds.is_none());
        match seen {
            1 => {
                assert_eq!(preview(&event), case.draft());
                draft.notify_one();
            }
            2 => assert_eq!(
                event.presentation.response.raw,
                recognition(true, &case.final_text())
            ),
            3 => assert_eq!(event.presentation.response.raw, END),
            _ => panic!("duplicate response for session {}", case.index),
        }
    };
    let result = match case.input {
        Input::Pcm16 => {
            let (writer, result) =
                start_pcm_dictation_with_observer(engine, options, limits, &mut observe);
            tokio::join!(feed(writer, &case.pcm, &draft), result).1
        }
        Input::Pcm48 => {
            let (writer, reader) = pcm_channel();
            let source = Pcm16Opus::new(Pcm48kTo16k::new(reader), EncoderConfig::default());
            let result = collect_dictation_with_observer(
                engine,
                Box::new(source),
                options,
                limits,
                &mut observe,
            );
            tokio::join!(feed(writer, &case.pcm, &draft), result).1
        }
        Input::Ogg => {
            let source = OggAudio::open(case.path.as_ref().unwrap()).await.unwrap();
            collect_dictation_with_observer(engine, Box::new(source), options, limits, &mut observe)
                .await
        }
    }
    .unwrap();
    let receipt = peer.receipts[case.index].lock().unwrap().take().unwrap();
    let metrics = assert_complete(&result, &receipt, 3);
    assert_eq!(metrics.response_protobuf_bytes, bytes);
    assert_eq!(Some(metrics.session_id), session_id);
    assert_eq!(seen, 3);
    assert_eq!(result.context, case.context);
    assert_eq!(result.final_updates.len(), 1);
    assert_eq!(result.text().unwrap(), case.final_text());
    metrics.session_id
}

fn task_count() -> usize {
    tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks()
}

async fn quiescent(peer: &Peer, max_tasks: usize) {
    common::bounded(async {
        peer.idle().await;
        // Task termination has no public notification. The peer receipt/retire
        // signals gate this check; yield until task accounting catches up.
        while task_count() > max_tasks {
            tokio::task::yield_now().await;
        }
    })
    .await;
}

#[tokio::test]
async fn shared_engine_isolates_52_sequential_and_four_inflight_dictations() {
    timed(async {
        let dir = tempfile::tempdir().unwrap();
        let cases = join_all((0..SEQUENTIAL + CONCURRENT).map(|i| Case::new(i, dir.path()))).await;
        let peer = Arc::new(Peer {
            cases: cases.into_iter().map(Arc::new).collect(),
            receipts: (0..SEQUENTIAL + CONCURRENT)
                .map(|_| Mutex::new(None))
                .collect(),
            state: Mutex::default(),
            concurrent_uploads: Barrier::new(CONCURRENT),
            retired: Notify::new(),
        });
        let server = TestServer::new({
            let peer = peer.clone();
            move |request| peer.clone().serve(request)
        })
        .await;
        let engine = engine(&server).await;
        let mut sessions = HashSet::new();
        assert!(sessions.insert(run(&engine, &peer.cases[0], &peer).await));
        peer.idle().await;
        let warm_tasks = task_count();
        for case in &peer.cases[1..SEQUENTIAL] {
            assert!(sessions.insert(run(&engine, case, &peer).await));
            quiescent(&peer, warm_tasks).await;
            assert_eq!(server.accepted_connections(), 1);
        }
        for id in join_all(
            peer.cases[SEQUENTIAL..]
                .iter()
                .map(|case| run(&engine, case, &peer)),
        )
        .await
        {
            assert!(sessions.insert(id));
        }
        quiescent(&peer, warm_tasks).await;
        assert_eq!(server.accepted_connections(), 1);
        engine.close().await.unwrap();
        assert_eq!(server.accepted_connections(), 1);
        server.close().await;
        quiescent(&peer, 0).await;
        let state = peer.state.lock().unwrap();
        assert_eq!(state.claims.len(), SEQUENTIAL + CONCURRENT);
        assert_eq!(state.peak, CONCURRENT);
        assert_eq!(state.live, 0);
        drop(state);
        dir.close().unwrap();
    })
    .await;
}
