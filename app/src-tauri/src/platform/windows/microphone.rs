use super::pcm::Converter;
use crate::{
    model::MicChoice,
    ports::{
        FaultCallback, MicError, MicInfo, MicStream, Microphone, PcmRate, PcmSink, SinkFactory,
    },
};
use cpal::{
    Data, Device, ErrorKind, HostId, SampleFormat, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::Win32::Foundation::E_ACCESSDENIED;

const OPEN_TIMEOUT: Duration = Duration::from_secs(3);
const START_POLL: Duration = Duration::from_millis(3);
const FAULT_POLL: Duration = Duration::from_millis(10);

#[derive(Default)]
pub struct WasapiMicrophone {
    // A bounded caller timeout must not detach a driver still negotiating or
    // unwinding. Do not start a second open while this worker holds the device.
    late_open: Mutex<Option<JoinHandle<()>>>,
}

/// Audio format only, for the local diagnostic harness.
#[derive(Clone, Copy, Debug)]
pub struct CaptureFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: SampleFormat,
    pub output_rate: PcmRate,
}

impl WasapiMicrophone {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn join_late_open(&self) {
        let worker = self
            .late_open
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }

    pub fn open_with_format(
        &self,
        choice: &MicChoice,
        sink_for: SinkFactory,
        on_fault: FaultCallback,
    ) -> Result<(Box<dyn MicStream>, CaptureFormat), MicError> {
        let mut late_open = self
            .late_open
            .try_lock()
            .map_err(|_| MicError::OpenTimeout)?;
        if late_open
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return Err(MicError::OpenTimeout);
        }
        if let Some(worker) = late_open.take() {
            let _ = worker.join();
        }
        let deadline = Instant::now() + OPEN_TIMEOUT;
        // A rendezvous acknowledges a running stream only if the caller still waits.
        let (ready, started) = mpsc::sync_channel(0);
        let (stop_tx, stop_rx) = mpsc::channel();
        let status = Arc::new(Status::default());
        let worker_status = status.clone();
        let choice = choice.clone();
        let thread = thread::Builder::new()
            .name("sambler-wasapi".into())
            .spawn(move || {
                run(
                    &choice,
                    sink_for,
                    on_fault,
                    &worker_status,
                    ready,
                    stop_rx,
                    deadline,
                );
            })
            .map_err(|_| MicError::Other)?;
        match started.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(format)) => Ok((
                Box::new(Running {
                    stop: stop_tx,
                    thread: Some(thread),
                }),
                format,
            )),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                status.cancelled.store(true, Ordering::Release);
                drop(started);
                drop(stop_tx);
                *late_open = Some(thread);
                Err(match error {
                    mpsc::RecvTimeoutError::Timeout => MicError::OpenTimeout,
                    _ => MicError::Other,
                })
            }
        }
    }
}

fn wasapi() -> Result<cpal::Host, MicError> {
    cpal::host_from_id(HostId::Wasapi).map_err(|_| MicError::Other)
}

impl Microphone for WasapiMicrophone {
    fn inputs(&self) -> Result<Vec<MicInfo>, MicError> {
        let host = wasapi()?;
        let default = host
            .default_input_device()
            .and_then(|device| device.id().ok());
        // CPAL 0.18.2's WASAPI supports_input() checks endpoint direction,
        // independent of the loopback capability of a render endpoint.
        let inputs = host
            .input_devices()
            .map_err(|error| start_error(&error, MicError::NoInput, None))?;
        Ok(inputs
            .filter_map(|device| {
                let id = device.id().ok()?;
                let name = device.description().ok()?.name().to_owned();
                Some(MicInfo {
                    is_default: default.as_ref() == Some(&id),
                    id: id.to_string(),
                    name,
                })
            })
            .collect())
    }

    fn open(
        &self,
        choice: &MicChoice,
        sink_for: SinkFactory,
        on_fault: FaultCallback,
    ) -> Result<Box<dyn MicStream>, MicError> {
        self.open_with_format(choice, sink_for, on_fault)
            .map(|(stream, _)| stream)
    }
}

fn resolve(choice: &MicChoice) -> Result<(Device, MicError), MicError> {
    let host = wasapi()?;
    let (id, missing) = match choice {
        // Freeze the current default's real ID. The CPAL default object is a
        // dynamic handle; never let it resolve differently between negotiation and start.
        MicChoice::SystemDefault => (
            host.default_input_device()
                .ok_or(MicError::NoInput)?
                .id()
                .map_err(|error| start_error(&error, MicError::NoInput, None))?,
            MicError::NoInput,
        ),
        MicChoice::Named { id, .. } => (
            id.parse().map_err(|_| MicError::NotFound)?,
            MicError::NotFound,
        ),
    };
    let device = host
        .device_by_id(&id)
        .filter(DeviceTrait::supports_input)
        .ok_or_else(|| missing.clone())?;
    Ok((device, missing))
}

fn accepted(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::I32 | SampleFormat::U16
    )
}

fn negotiate(device: &Device, missing: &MicError) -> Result<SupportedStreamConfig, MicError> {
    let mut best = None;
    let mut first = None;
    let ranges = device
        .supported_input_configs()
        .map_err(|error| start_error(&error, missing.clone(), None))?;
    for range in ranges {
        first.get_or_insert_with(|| {
            format!(
                "{}–{} Hz, {} ch, {}",
                range.min_sample_rate(),
                range.max_sample_rate(),
                range.channels(),
                range.sample_format()
            )
        });
        if !accepted(range.sample_format()) || range.channels() == 0 {
            continue;
        }
        let Some(config) = [48_000, 16_000, 44_100, range.max_sample_rate()]
            .into_iter()
            .filter(|rate| (4_000..=384_000).contains(rate))
            .find_map(|rate| range.try_with_sample_rate(rate))
        else {
            continue;
        };
        let rate_rank = match config.sample_rate() {
            48_000 => 0,
            16_000 => 1,
            44_100 => 2,
            _ => 3,
        };
        // Prefer a preferred rate combined with mono, then a preferred rate
        // with multiple channels; otherwise pick the smallest channel count.
        let score = (rate_rank > 1, config.channels(), rate_rank);
        if best.as_ref().is_none_or(|(key, _)| score < *key) {
            best = Some((score, config));
        }
    }
    best.map(|(_, config)| config)
        .ok_or_else(|| MicError::UnsupportedFormat {
            format: first.unwrap_or_else(|| "no shared capture format".into()),
        })
}

// CPAL's WASAPI adapter stringifies std::io::Error from the HRESULT and
// currently marks E_ACCESSDENIED as BackendError. Match only the stable OS
// error suffix, not localized prose, and never propagate the CPAL message.
fn access_denied(error: &cpal::Error) -> bool {
    const DENIED: &str = "(os error -2147024891)";
    debug_assert_eq!(E_ACCESSDENIED.0, -2147024891);
    error.kind() == ErrorKind::PermissionDenied
        || error
            .message()
            .is_some_and(|message| message.ends_with(DENIED))
}

fn start_error(error: &cpal::Error, missing: MicError, format: Option<CaptureFormat>) -> MicError {
    if access_denied(error) {
        return MicError::AccessDenied;
    }
    match error.kind() {
        ErrorKind::DeviceNotAvailable | ErrorKind::DeviceChanged => missing,
        ErrorKind::StreamInvalidated => MicError::Disconnected,
        ErrorKind::UnsupportedConfig | ErrorKind::UnsupportedOperation => {
            MicError::UnsupportedFormat {
                format: format.map_or_else(
                    || "no shared capture format".into(),
                    |f| {
                        format!(
                            "{} Hz, {} ch, {}",
                            f.sample_rate, f.channels, f.sample_format
                        )
                    },
                ),
            }
        }
        _ => MicError::Other,
    }
}

const NO_FAULT: u8 = 0;
const DEVICE_FAULT: u8 = 1;
const DENIED_FAULT: u8 = 2;
const OTHER_FAULT: u8 = 3;

#[derive(Default)]
struct Status {
    first_frames: AtomicBool,
    cancelled: AtomicBool,
    fault: AtomicU8,
}
impl Status {
    fn fault(&self) -> u8 {
        self.fault.load(Ordering::Acquire)
    }
    fn latch_error(&self, error: &cpal::Error) {
        // Some drivers flag the first packet after Start as discontinuous even
        // with a nonzero device position. There is no captured prefix to lose.
        // Once the first packet arrives, any discontinuity aborts the take.
        if error.kind() == ErrorKind::Xrun && !self.first_frames.load(Ordering::Acquire) {
            return;
        }
        let code = if access_denied(error) {
            DENIED_FAULT
        } else {
            match error.kind() {
                ErrorKind::DeviceNotAvailable
                | ErrorKind::StreamInvalidated
                | ErrorKind::DeviceChanged => DEVICE_FAULT,
                _ => OTHER_FAULT,
            }
        };
        let _ = self
            .fault
            .compare_exchange(NO_FAULT, code, Ordering::AcqRel, Ordering::Acquire);
    }
}

#[derive(Clone, Copy)]
enum End {
    Finish,
    Abort,
}

struct Running {
    stop: mpsc::Sender<End>,
    thread: Option<JoinHandle<()>>,
}
impl Running {
    fn shutdown(&mut self, end: End) {
        if let Some(thread) = self.thread.take() {
            let _ = self.stop.send(end);
            if thread.thread().id() != std::thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}
impl MicStream for Running {
    fn stop(mut self: Box<Self>) {
        self.shutdown(End::Finish);
    }
    fn abort(mut self: Box<Self>) {
        self.shutdown(End::Abort);
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.shutdown(End::Abort);
    }
}

struct Capture {
    converter: Converter,
    sink: Option<Box<dyn PcmSink>>,
}
impl Capture {
    fn push(&mut self, data: &Data, format: SampleFormat) {
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        macro_rules! convert {
            ($sample:ty) => {
                if let Some(samples) = data.as_slice::<$sample>() {
                    self.converter.push(samples, |part| sink.push(part));
                }
            };
        }
        match format {
            SampleFormat::F32 => convert!(f32),
            SampleFormat::I16 => convert!(i16),
            SampleFormat::I32 => convert!(i32),
            SampleFormat::U16 => convert!(u16),
            _ => {}
        }
    }

    fn end(mut self, end: End) {
        if let Some(mut sink) = self.sink.take() {
            match end {
                End::Finish => {
                    self.converter.finish(|part| sink.push(part));
                    sink.finish();
                }
                End::Abort => sink.abort(),
            }
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.abort();
        }
    }
}

// The data closure owns the converter and sink uniquely. On teardown only, it
// returns them to the owner. CPAL WASAPI's Stream::drop joins its callback thread.
struct CallbackCapture {
    data: Option<Capture>,
    reclaim: SyncSender<Capture>,
}
impl CallbackCapture {
    fn push(&mut self, data: &Data, format: SampleFormat) {
        if let Some(capture) = &mut self.data {
            capture.push(data, format);
        }
    }
}
impl Drop for CallbackCapture {
    fn drop(&mut self) {
        if let Some(capture) = self.data.take() {
            let _ = self.reclaim.try_send(capture);
        }
    }
}

fn start(
    choice: &MicChoice,
    sink_for: SinkFactory,
    status: &Arc<Status>,
    deadline: Instant,
) -> Result<(cpal::Stream, Receiver<Capture>, CaptureFormat, MicError), MicError> {
    let (device, missing) = resolve(choice)?;
    let config = negotiate(&device, &missing)?;
    let format = CaptureFormat {
        sample_rate: config.sample_rate(),
        channels: config.channels(),
        sample_format: config.sample_format(),
        output_rate: if config.sample_rate() == 48_000 {
            PcmRate::Hz48000
        } else {
            PcmRate::Hz16000
        },
    };
    let still_waiting = || !status.cancelled.load(Ordering::Acquire) && Instant::now() < deadline;
    let converter = Converter::new(format.channels, format.sample_rate, format.output_rate);
    if !still_waiting() {
        return Err(MicError::OpenTimeout);
    }
    let (reclaim, returned) = mpsc::sync_channel(1);
    let mut callback = CallbackCapture {
        reclaim,
        data: Some(Capture {
            converter,
            sink: Some(sink_for(format.output_rate)),
        }),
    };
    if !still_waiting() {
        return Err(MicError::OpenTimeout);
    }
    let audio_status = status.clone();
    let error_status = status.clone();
    let stream = device
        .build_input_stream_raw(
            config.config(),
            format.sample_format,
            move |data, _| {
                if data.len() != 0
                    && !audio_status.cancelled.load(Ordering::Acquire)
                    && audio_status.fault() == NO_FAULT
                {
                    callback.push(data, format.sample_format);
                    audio_status.first_frames.store(true, Ordering::Release);
                }
            },
            move |error| error_status.latch_error(&error),
            None,
        )
        .map_err(|error| start_error(&error, missing.clone(), Some(format)))?;
    if !still_waiting() {
        return Err(MicError::OpenTimeout);
    }
    stream
        .play()
        .map_err(|error| start_error(&error, missing.clone(), Some(format)))?;
    Ok((stream, returned, format, missing))
}

fn run(
    choice: &MicChoice,
    sink_for: SinkFactory,
    on_fault: FaultCallback,
    status: &Arc<Status>,
    ready: SyncSender<Result<CaptureFormat, MicError>>,
    stop: Receiver<End>,
    deadline: Instant,
) {
    let (stream, returned, format, missing) = match start(choice, sink_for, status, deadline) {
        Ok(started) => started,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let result = loop {
        if status.cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            break Err(MicError::OpenTimeout);
        }
        match status.fault() {
            NO_FAULT => {}
            DEVICE_FAULT => break Err(missing),
            DENIED_FAULT => break Err(MicError::AccessDenied),
            _ => break Err(MicError::Other),
        }
        // WASAPI play() merely queues a command. The first actual data callback
        // is the acknowledgement, including a callback carrying silent samples.
        if status.first_frames.load(Ordering::Acquire) {
            break Ok(format);
        }
        if !matches!(
            stop.recv_timeout(START_POLL),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            break Err(MicError::OpenTimeout);
        }
    };
    let active = result.is_ok() && ready.send(result.clone()).is_ok();
    let end = if active {
        loop {
            if status.fault() != NO_FAULT {
                break End::Abort;
            }
            match stop.recv_timeout(FAULT_POLL) {
                Ok(end) => break end,
                Err(mpsc::RecvTimeoutError::Disconnected) => break End::Abort,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    } else {
        End::Abort
    };
    drop(stream);
    let fault = status.fault();
    if let Ok(capture) = returned.try_recv() {
        capture.end(if fault == NO_FAULT { end } else { End::Abort });
    }
    if !active && result.is_err() {
        let _ = ready.send(result);
    }
    if active && fault != NO_FAULT {
        let error = match fault {
            DEVICE_FAULT => MicError::Disconnected,
            DENIED_FAULT => MicError::AccessDenied,
            _ => MicError::Other,
        };
        on_fault(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pre_capture_xrun_is_ignored_and_faults_are_latched_once() {
        let status = Status::default();
        status.latch_error(&ErrorKind::Xrun.into());
        assert_eq!(status.fault(), NO_FAULT);
        status.first_frames.store(true, Ordering::Release);
        status.latch_error(&ErrorKind::StreamInvalidated.into());
        status.latch_error(&ErrorKind::Xrun.into());
        assert_eq!(status.fault(), DEVICE_FAULT);
        let status = Status::default();
        status.first_frames.store(true, Ordering::Release);
        status.latch_error(&ErrorKind::Xrun.into());
        assert_eq!(status.fault(), OTHER_FAULT);
    }

    #[test]
    fn sink_ends_exactly_once_on_finish_abort_or_drop() {
        struct Sink(mpsc::Sender<bool>);
        impl PcmSink for Sink {
            fn push(&mut self, _: &[i16]) {}
            fn finish(self: Box<Self>) {
                self.0.send(true).unwrap();
            }
            fn abort(self: Box<Self>) {
                self.0.send(false).unwrap();
            }
        }
        for end in [Some(End::Finish), Some(End::Abort), None] {
            let (tx, rx) = mpsc::channel();
            let capture = Capture {
                converter: Converter::new(1, 48_000, PcmRate::Hz48000),
                sink: Some(Box::new(Sink(tx))),
            };
            if let Some(end) = end {
                capture.end(end);
            } else {
                drop(capture);
            }
            assert_eq!(rx.try_recv(), Ok(matches!(end, Some(End::Finish))));
            assert_eq!(rx.try_recv(), Err(mpsc::TryRecvError::Disconnected));
        }
    }

    #[test]
    fn preserves_hresult_access_denied_without_leaking_backend_messages() {
        let message = std::io::Error::from_raw_os_error(E_ACCESSDENIED.0).to_string();
        let error = cpal::Error::with_message(ErrorKind::BackendError, message);
        assert_eq!(
            start_error(&error, MicError::NotFound, None),
            MicError::AccessDenied
        );
    }
}
