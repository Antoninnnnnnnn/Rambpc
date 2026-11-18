use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Point {
    Upload,
    Partials,
    AfterEnd,
    LateContext,
}

type Signal = oneshot::Sender<()>;

struct GateDrop(Option<Signal>);

impl Drop for GateDrop {
    fn drop(&mut self) {
        let _ = self.0.take().unwrap().send(());
    }
}

struct WaitingContext {
    gates: Mutex<Option<(Signal, Signal)>>,
}

impl ContextProvider for WaitingContext {
    fn context(&self) -> SourceFuture<'_, Result<Option<EditingContext>, SourceError>> {
        Box::pin(async move {
            let (entered, dropped) = self.gates.lock().unwrap().take().unwrap();
            let _drop = GateDrop(Some(dropped));
            entered.send(()).unwrap();
            pending().await
        })
    }
}

struct CancelCase {
    point: Point,
    name: String,
    send_upload: oneshot::Sender<Upload>,
    send_retired: Signal,
}

const STEPS: [(Point, bool); 6] = [
    (Point::Upload, false),
    (Point::Upload, true),
    (Point::Partials, false),
    (Point::Partials, true),
    (Point::AfterEnd, false),
    (Point::LateContext, false),
];

fn options(name: &str) -> SessionOptions {
    SessionOptions {
        config: ProtocolConfig {
            application_id: name.into(),
            ..Default::default()
        },
        context_timeout: SESSION_TIMEOUT,
        ..Default::default()
    }
}

async fn cancellation_peer(mut upload: Upload, case: CancelCase) -> Response<Replies> {
    assert_eq!(
        upload.required().await,
        initial_request(&options(&case.name).config, 0).unwrap()
    );
    // The cancellation point is after actual Opus media crosses the wire,
    // not merely after the app's acceptance into its PCM queue.
    let needed = 2 + if case.point == Point::AfterEnd || case.point == Point::LateContext {
        // One full input frame plus an encoder lookahead flush packet.
        2
    } else {
        1
    };
    for _ in 0..needed {
        let audio = upload.required().await;
        upload.page(&audio);
    }
    assert_eq!(upload.pages.eos, needed == 4);
    if case.point == Point::AfterEnd {
        upload.finish(None).await; // Peer has observed upload END and half-close.
    }
    assert!(case.send_upload.send(upload).is_ok());
    let frames = match case.point {
        Point::Upload | Point::LateContext => vec![],
        Point::Partials => (0..3)
            .map(|i| recognition(false, &format!("{} draft {i}", case.name)))
            .collect(),
        Point::AfterEnd => vec![recognition(true, &case.name), END],
    };
    let lifetime = GateDrop(Some(case.send_retired));
    // Keep the response genuinely open: only cancellation can retire it.
    let held = stream::poll_fn(move |_| {
        let _hold = &lifetime;
        std::task::Poll::Pending
    });
    replies(stream::iter(frames.into_iter().map(Ok)).chain(held))
}

#[tokio::test(flavor = "current_thread")]
async fn app_abort_retires_live_rpc_and_does_not_commit_or_replay() {
    timed(async {
        let mut cases = Vec::new();
        let mut gates = Vec::new();
        for (i, (point, abandon)) in STEPS.into_iter().enumerate() {
            let (send_upload, upload) = oneshot::channel();
            let (send_dropped, dropped) = oneshot::channel();
            let name = format!("cancel-{i}");
            gates.push((point, abandon, name.clone(), upload, dropped));
            cases.push(CancelCase {
                point,
                name,
                send_upload,
                send_retired: send_dropped,
            });
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let server = TestServer::new({
            let calls = calls.clone();
            let cases = Mutex::new(cases.into_iter());
            move |request| {
                let index = calls.fetch_add(1, SeqCst);
                let case = cases.lock().unwrap().next();
                async move {
                    let mut upload = Upload::new(request.into_inner());
                    if let Some(case) = case {
                        Ok(cancellation_peer(upload, case).await)
                    } else {
                        assert_eq!(index, STEPS.len(), "unexpected retry/replay");
                        assert_eq!(
                            upload.required().await,
                            initial_request(&options("healthy follow-up").config, 0).unwrap()
                        );
                        upload.finish(None).await;
                        Ok(replies(stream::iter([
                            Ok(recognition(true, "healthy follow-up")),
                            Ok(END),
                        ])))
                    }
                }
            }
        })
        .await;
        let engine = engine(&server).await;
        for (point, abandon, name, upload_ready, dropped) in gates {
            let mut opts = options(&name);
            let (entered, context_dropped) = if point == Point::LateContext {
                let (send_entered, entered) = oneshot::channel();
                let (send_dropped, dropped) = oneshot::channel();
                opts.context = ContextInput::Provider(Arc::new(WaitingContext {
                    gates: Mutex::new(Some((send_entered, send_dropped))),
                }));
                (Some(entered), Some(dropped))
            } else {
                (None, None)
            };
            let observed = Notify::new();
            let mut count = 0;
            let mut observe = |event: DictationObservation<'_>| {
                count += 1;
                assert_eq!(event.metrics.responses_received, count);
                assert!(event.metrics.eof_seconds.is_none());
                match point {
                    Point::Partials => {
                        assert_eq!(preview(&event), format!("{name} draft {}", count - 1));
                        assert!(count <= 3);
                        if count == 3 {
                            observed.notify_one();
                        }
                    }
                    Point::AfterEnd => {
                        if count == 1 {
                            assert_eq!(event.presentation.response.raw, recognition(true, &name));
                        } else {
                            assert_eq!(count, 2);
                            assert_eq!(event.presentation.response.raw, END);
                            observed.notify_one();
                        }
                    }
                    _ => panic!("unexpected reply before cancellation"),
                }
            };
            let lifetime = Arc::new(PcmLifetime::default());
            let (writer, result) = start_pcm_dictation_with_observer(
                &engine,
                opts,
                CollectorLimits::default(),
                &mut observe,
            );
            let mut result = Box::pin(result);
            let mut writer = Some(writer);
            lifetime
                .send(writer.as_mut().unwrap(), pcm(0, FRAME_SAMPLES, 0))
                .await;
            if matches!(point, Point::AfterEnd | Point::LateContext) {
                writer.take().unwrap().finish().unwrap();
            }
            let mut upload = tokio::select! {
                result = &mut result => panic!("completed before abort at {point:?}: {result:?}"),
                ready = async {
                    let upload = upload_ready.await.unwrap();
                    if matches!(point, Point::Partials | Point::AfterEnd) {
                        observed.notified().await;
                    }
                    if let Some(entered) = entered { entered.await.unwrap(); }
                    upload
                } => ready,
            };
            assert!(poll!(result.as_mut()).is_pending());
            if let Some(writer) = writer.as_mut() {
                // Current-thread execution and a ready one-slot permit make this
                // chunk still live at the instant of abort. Too small to add a page.
                lifetime.send(writer, vec![17, 0]).await;
                assert!((1..=2).contains(&lifetime.live.load(SeqCst)));
            } else {
                assert_eq!(
                    lifetime.live.load(SeqCst),
                    0,
                    "finished PCM retained across {point:?}"
                );
            }
            // Shorter than the SDK and late-context deadlines: timeouts alone
            // must not provide the cleanup that app cancellation is meant to trigger.
            common::bounded(async {
                if abandon {
                    drop(writer.take().unwrap());
                    let result = result.await;
                    let Err(EngineError::Audio(error)) = result else {
                        panic!("expected PCM abandonment at {point:?}, got {result:?}");
                    };
                    assert_eq!(
                        error.original().downcast_ref::<AudioError>(),
                        Some(&AudioError::InputAbandoned)
                    );
                } else if let Some(writer) = writer.as_mut() {
                    let mut waiting = Box::pin(writer.reserve());
                    assert!(poll!(waiting.as_mut()).is_pending());
                    drop(result);
                    assert!(matches!(waiting.await, Err(PcmInputError::Closed)));
                } else {
                    drop(result);
                }
                if let Some(dropped) = context_dropped {
                    dropped.await.unwrap();
                }
                assert!(
                    !matches!(upload.next().await, Ok(Some(_))),
                    "unexpected post-abort audio, context or END at {point:?}"
                );
                dropped.await.unwrap(); // Teardown reached the peer's still-open response.
            })
            .await;
            assert_eq!(lifetime.live.load(SeqCst), 0, "retained PCM at {point:?}");
            assert_eq!(
                count,
                match point {
                    Point::Partials => 3,
                    Point::AfterEnd => 2,
                    _ => 0,
                }
            );
        }
        let (writer, result) = start_pcm_dictation(
            &engine,
            options("healthy follow-up"),
            CollectorLimits::default(),
        );
        writer.finish().unwrap();
        assert_eq!(result.await.unwrap().text().unwrap(), "healthy follow-up");
        engine.close().await.unwrap(); // Observed audio errors and cancellation are not replayed here.
        assert_eq!(server.accepted_connections(), 1);
        server.close().await;
        assert_eq!(calls.load(SeqCst), STEPS.len() + 1);
    })
    .await;
}
