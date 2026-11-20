//! Voluntary microphone level test from the settings. Local only: no voice is
//! sent anywhere, and it works before consent or while Rambler is unavailable.

use crate::{
    model::MicChoice,
    ports::{MicError, Microphone, PcmSink},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, oneshot, watch};

/// The newest request owns its cancellation sender. Each worker retains only
/// its receiver, and owns its stream until it has joined the driver.
#[derive(Default)]
pub(super) struct MicTest {
    request: Mutex<Request>,
    pub(super) serial: AsyncMutex<()>,
}

#[derive(Default)]
struct Request {
    closed: bool,
    active: Option<watch::Sender<()>>,
}

impl MicTest {
    pub(super) fn request(&self) -> Option<watch::Receiver<()>> {
        let mut request = self.request.lock().unwrap_or_else(|e| e.into_inner());
        if request.closed {
            return None;
        }
        let (stop, stopped) = watch::channel(());
        request.active = Some(stop);
        Some(stopped)
    }

    pub(super) fn stop(&self) {
        self.request
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active = None;
    }

    pub(super) fn close(&self) {
        let mut request = self.request.lock().unwrap_or_else(|e| e.into_inner());
        request.closed = true;
        request.active = None;
    }

    pub(super) async fn run(
        &self,
        microphone: Arc<dyn Microphone>,
        choice: MicChoice,
        mut stopped: watch::Receiver<()>,
        opened: oneshot::Sender<Result<(), MicError>>,
        emit: impl Fn(f32),
    ) {
        let _serial = self.serial.lock().await;
        if stopped.has_changed().is_err() {
            let _ = opened.send(Ok(()));
            return;
        }
        let level = Level::default();
        let sink_level = level.clone();
        let (fault, failed) = oneshot::channel();
        let stream = tokio::task::spawn_blocking(move || {
            microphone.open(
                &choice,
                Box::new(move |_| Box::new(LevelOnlySink::new(sink_level))),
                Box::new(move |_| {
                    let _ = fault.send(());
                }),
            )
        })
        .await
        .unwrap_or(Err(MicError::Other));
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                let _ = opened.send(Err(error));
                return;
            }
        };
        let _ = opened.send(Ok(()));
        let meter = async {
            loop {
                emit(level.get());
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };
        tokio::select! {
            biased;
            _ = stopped.changed() => (),
            _ = failed => (),
            _ = tokio::time::sleep(Duration::from_secs(60)) => (),
            _ = meter => (),
        }
        let _ = tokio::task::spawn_blocking(move || stream.abort()).await;
        emit(0.0);
    }

    pub(super) async fn drain(&self) {
        // Any worker still waiting after this barrier sees its request cancelled
        // before it opens the device.
        let _serial = self.serial.lock().await;
    }
}

/// Latest level in 0..=1 (dBFS −60..0 mapped linearly), stored as f32 bits.
#[derive(Clone, Default)]
pub struct Level(Arc<AtomicU32>);

impl Level {
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

pub fn level_of(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum / samples.len() as f64).sqrt() / 32768.0;
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0) as f32
}

pub struct LevelOnlySink {
    level: Level,
}

impl LevelOnlySink {
    pub fn new(level: Level) -> Self {
        Self { level }
    }
}

impl PcmSink for LevelOnlySink {
    fn push(&mut self, samples: &[i16]) {
        self.level.set(level_of(samples));
    }
    fn finish(self: Box<Self>) {
        self.level.set(0.0);
    }
    fn abort(self: Box<Self>) {
        self.level.set(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stop_or_replacement_only_cancels_the_current_generation() {
        let test = MicTest::default();
        let mut first = test.request().unwrap();
        let mut second = test.request().unwrap();
        assert!(first.changed().await.is_err());
        assert!(matches!(second.has_changed(), Ok(false)));
        test.stop();
        assert!(second.changed().await.is_err());
        let mut last = test.request().unwrap();
        test.close();
        assert!(last.changed().await.is_err());
        assert!(test.request().is_none());
    }

    #[tokio::test]
    async fn stop_during_open_still_aborts_and_drains_the_late_stream() {
        use crate::ports::{FaultCallback, MicInfo, MicStream, PcmRate, SinkFactory};
        use std::sync::atomic::AtomicBool;

        struct Stream(Box<dyn PcmSink>, Arc<AtomicBool>, FaultCallback);
        impl MicStream for Stream {
            fn stop(self: Box<Self>) {
                panic!("a cancelled test cannot finish normally")
            }
            fn abort(self: Box<Self>) {
                self.0.abort();
                self.1.store(true, Ordering::Release);
                drop(self.2);
            }
        }
        struct Mic {
            entered: tokio::sync::Notify,
            resume: Mutex<std::sync::mpsc::Receiver<()>>,
            aborted: Arc<AtomicBool>,
        }
        impl Microphone for Mic {
            fn inputs(&self) -> Result<Vec<MicInfo>, MicError> {
                Ok(vec![])
            }
            fn open(
                &self,
                _: &MicChoice,
                factory: SinkFactory,
                fault: FaultCallback,
            ) -> Result<Box<dyn MicStream>, MicError> {
                self.entered.notify_one();
                self.resume
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap();
                Ok(Box::new(Stream(
                    factory(PcmRate::Hz16000),
                    self.aborted.clone(),
                    fault,
                )))
            }
        }
        let (resume, unblock) = std::sync::mpsc::channel();
        let mic = Arc::new(Mic {
            entered: tokio::sync::Notify::new(),
            resume: Mutex::new(unblock),
            aborted: Arc::new(AtomicBool::new(false)),
        });
        let test = Arc::new(MicTest::default());
        let stop_signal = test.request().unwrap();
        let (opened, reply) = oneshot::channel();
        let task = tokio::spawn({
            let (test, mic) = (test.clone(), mic.clone());
            async move {
                test.run(mic, MicChoice::SystemDefault, stop_signal, opened, |_| {})
                    .await;
            }
        });
        tokio::time::timeout(Duration::from_secs(2), mic.entered.notified())
            .await
            .unwrap();
        test.close();
        assert!(!mic.aborted.load(Ordering::Acquire));
        resume.send(()).unwrap();
        test.drain().await;
        assert!(mic.aborted.load(Ordering::Acquire));
        assert!(reply.await.unwrap().is_ok());
        task.await.unwrap();
    }

    #[test]
    fn silence_is_zero_and_full_scale_is_one() {
        assert_eq!(level_of(&[0; 160]), 0.0);
        assert!((level_of(&[i16::MAX; 160]) - 1.0).abs() < 1e-3);
        let quiet = level_of(&[33; 160]); // about -60 dBFS
        assert!(quiet < 0.02);
    }
}
