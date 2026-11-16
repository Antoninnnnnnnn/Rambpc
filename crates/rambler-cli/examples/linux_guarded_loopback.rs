//! Fixed public canary -> actual native codec/SDK -> plaintext loopback.
//! No arguments: silent no-op. Never supply credentials, private input, or audio.
#![forbid(unsafe_op_in_unsafe_fn)]

use bytes::Bytes;
use futures_util::{Stream, stream};
use rambler_audio::{EncoderConfig, Pcm16Opus};
use rambler_engine::{
    AudioSource, CollectorLimits, ContextInput, EditingContext, Engine, RenderPolicy,
    SessionOptions, SourceError, SourceFuture, collect_dictation,
};
use rambler_protocol::{
    ProtocolConfig, RECOGNITION_RESPONSE, audio_request, context_request, end_request,
    initial_request, wire,
};
use rambler_transport::{CredentialValue, GrpcTransport, Metadata, RawBytesCodec, TransportConfig};
use std::{
    convert::Infallible,
    ffi::OsStr,
    future::Future,
    io::{self, Cursor, Write},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::{
    Request, Response, Status, Streaming,
    body::Body,
    codegen::{Service, StdError},
    server::{Grpc, NamedService, StreamingService},
    transport::Server,
};

const MODE: &str = "--fixed-canary-v1";
const CANARY: &[u8; 26] = b"RAMBLER-PRECRED-CANARY-V1\n";
const READY: &[u8] = b"PRECRED_READY_V1\n";
const OK: &[u8] = b"PRECRED_CANARY_OK_V1\n";
const TEXT: &str = "Fixed synthetic loopback result.";
const HEADERS: [(&str, &str); 3] = [
    ("x-goog-api-key", "invented-public-canary-key"),
    ("x-android-package", "invalid.synthetic.canary"),
    ("x-android-cert", "invented-public-canary-cert"),
];
const DEADLINE: Duration = Duration::from_secs(5);

type Check = Result<(), Failure>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Harden,
    Verify,
    Traced,
    Fd,
    Env,
    Argv,
    Io,
    Payload,
    Sdk,
    Internal,
}
#[derive(Debug, PartialEq, Eq)]
enum SdkFailure {
    Other,
    LatePermissionDenied {
        peer_calls: usize,
        peer_drained: bool,
    },
}
impl From<Failure> for SdkFailure {
    fn from(_: Failure) -> Self {
        Self::Other
    }
}
impl Failure {
    fn line(self) -> &'static [u8] {
        match self {
            Self::Harden => b"PRECRED_FAIL_HARDEN\n",
            Self::Verify => b"PRECRED_FAIL_VERIFY\n",
            Self::Traced => b"PRECRED_FAIL_TRACED\n",
            Self::Fd => b"PRECRED_FAIL_FD\n",
            Self::Env => b"PRECRED_FAIL_ENV\n",
            Self::Argv => b"PRECRED_FAIL_ARGV\n",
            Self::Io => b"PRECRED_FAIL_IO\n",
            Self::Payload => b"PRECRED_REJECT_PAYLOAD\n",
            Self::Sdk => b"PRECRED_FAIL_SDK\n",
            Self::Internal => b"PRECRED_FAIL_INTERNAL\n",
        }
    }
}
fn require(value: bool, failure: Failure) -> Check {
    if value { Ok(()) } else { Err(failure) }
}

// Minimal injectable admission boundary. Native is NEVER instantiated by tests.
trait Admission {
    fn harden(&mut self) -> bool;
    fn dumpable(&mut self) -> i32;
    fn untraced(&mut self) -> bool;
    fn fds(&mut self) -> bool;
    fn env(&mut self) -> bool;
    fn args(&mut self) -> bool;
    fn read(&mut self, byte: &mut [u8]) -> Result<usize, ()>;
    fn write(&mut self, line: &'static [u8]) -> Result<(), ()>;
}
#[derive(Default)]
struct Output {
    attempts: u8,
    broken: bool,
}
impl Output {
    fn emit(&mut self, sys: &mut impl Admission, line: &'static [u8]) -> Check {
        require(!self.broken && self.attempts < 2, Failure::Io)?;
        self.attempts += 1;
        self.broken = true; // Never append to a failed/partially written line.
        sys.write(line).map_err(|_| Failure::Io)?;
        self.broken = false;
        Ok(())
    }
}
struct Admitted; // Constructed only after the fixed literal AND EOF.
fn admit(sys: &mut impl Admission, output: &mut Output) -> Result<Admitted, Failure> {
    require(sys.harden(), Failure::Harden)?;
    require(sys.dumpable() == 0, Failure::Verify)?;
    require(sys.untraced(), Failure::Traced)?;
    require(sys.fds(), Failure::Fd)?;
    require(sys.env(), Failure::Env)?;
    require(sys.args(), Failure::Argv)?;
    output.emit(sys, READY)?;
    require(sys.dumpable() == 0, Failure::Verify)?;
    let mut buffer = [0u8; 27]; // Public bytes only; no erasure/secret-reception claim.
    let mut used = 0;
    while used < buffer.len() {
        // Includes the EOF probe: sum of all requested read lengths <= 27.
        match sys.read(&mut buffer[used..used + 1]) {
            Ok(0) => {
                return require(&buffer[..used] == CANARY, Failure::Payload).map(|()| Admitted);
            }
            Ok(1) => used += 1,
            _ => return Err(Failure::Io),
        }
    }
    Err(Failure::Payload)
}
fn requested(
    has_args: bool,
    sys: &mut impl Admission,
    output: &mut Output,
    sdk: impl FnOnce(Admitted) -> Check,
) -> Check {
    if !has_args {
        return Ok(());
    }
    let admitted = admit(sys, output)?;
    sdk(admitted)?;
    output.emit(sys, OK)
}
#[cfg_attr(
    not(all(target_os = "linux", target_arch = "x86_64")),
    allow(dead_code)
)]
fn args_valid(mut args: impl Iterator<Item = std::ffi::OsString>) -> bool {
    args.next().is_some()
        && args.next().as_deref() == Some(OsStr::new(MODE))
        && args.next().is_none()
}
#[cfg_attr(
    not(all(target_os = "linux", target_arch = "x86_64")),
    allow(dead_code)
)]
fn env_key_allowed(key: &OsStr) -> bool {
    matches!(key.as_encoded_bytes(), b"PATH" | b"LC_ALL")
}
#[cfg_attr(
    not(all(target_os = "linux", target_arch = "x86_64")),
    allow(dead_code)
)]
fn tracer_zero(status: &[u8]) -> bool {
    let mut values = status
        .split(|b| *b == b'\n')
        .filter_map(|line| line.strip_prefix(b"TracerPid:"));
    values
        .next()
        .is_some_and(|value| value.trim_ascii() == b"0")
        && values.next().is_none()
}
#[cfg_attr(
    not(all(target_os = "linux", target_arch = "x86_64")),
    allow(dead_code)
)]
fn fd_names_valid(names: impl IntoIterator<Item = Result<std::ffi::OsString, ()>>) -> bool {
    let mut seen = [false; 4];
    for name in names {
        let Ok(name) = name else { return false };
        let index = match name.as_encoded_bytes() {
            b"0" => 0,
            b"1" => 1,
            b"2" => 2,
            b"3" => 3,
            _ => return false,
        };
        if seen[index] {
            return false;
        }
        seen[index] = true;
    }
    seen == [true; 4]
}
struct Native;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl Admission for Native {
    fn harden(&mut self) -> bool {
        // SAFETY: Linux prctl ABI, integer arguments only, no pointers.
        unsafe {
            libc::prctl(
                libc::PR_SET_DUMPABLE,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
            ) == 0
        }
    }
    fn dumpable(&mut self) -> i32 {
        // SAFETY: Linux prctl ABI, integer arguments only, no pointers.
        unsafe {
            libc::prctl(
                libc::PR_GET_DUMPABLE,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
            )
        }
    }
    fn untraced(&mut self) -> bool {
        use std::io::Read;
        let Ok(file) = std::fs::File::open("/proc/self/status") else {
            return false;
        };
        let mut bytes = Vec::new();
        // The status handle is dropped before fd enumeration. Require bounded EOF.
        file.take(65536).read_to_end(&mut bytes).is_ok()
            && bytes.len() < 65536
            && tracer_zero(&bytes)
    }
    fn fds(&mut self) -> bool {
        for fd in 0..=2 {
            // SAFETY: F_GETFD reads flags only; no pointer or third argument.
            if unsafe { libc::fcntl(fd, libc::F_GETFD) } == -1 {
                return false;
            }
        }
        let Ok(entries) = std::fs::read_dir("/proc/self/fd") else {
            return false;
        };
        fd_names_valid(entries.map(|entry| entry.map(|entry| entry.file_name()).map_err(|_| ())))
    }
    fn env(&mut self) -> bool {
        // Clear-env launch is REQUIRED: vars_os internally snapshots VALUES too.
        std::env::vars_os().all(|(key, _)| env_key_allowed(&key))
    }
    fn args(&mut self) -> bool {
        args_valid(std::env::args_os())
    }
    fn read(&mut self, byte: &mut [u8]) -> Result<usize, ()> {
        // SAFETY: writable slice, exact size, fd 0 only; no buffered read-ahead.
        let count = unsafe { libc::read(0, byte.as_mut_ptr().cast(), byte.len()) };
        if count < 0 {
            Err(())
        } else {
            Ok(count as usize)
        }
    }
    fn write(&mut self, line: &'static [u8]) -> Result<(), ()> {
        write_stdout(line)
    }
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
impl Admission for Native {
    fn harden(&mut self) -> bool {
        false
    }
    fn dumpable(&mut self) -> i32 {
        -1
    }
    fn untraced(&mut self) -> bool {
        false
    }
    fn fds(&mut self) -> bool {
        false
    }
    fn env(&mut self) -> bool {
        false
    }
    fn args(&mut self) -> bool {
        false
    }
    fn read(&mut self, _: &mut [u8]) -> Result<usize, ()> {
        Err(())
    }
    fn write(&mut self, line: &'static [u8]) -> Result<(), ()> {
        write_stdout(line)
    }
}
fn write_stdout(line: &'static [u8]) -> Result<(), ()> {
    let mut out = io::stdout().lock();
    out.write_all(line)
        .and_then(|()| out.flush())
        .map_err(|_| ())
}

// Same raw tonic local-service style as tests/common, but owns its task on every
// error/timeout/drop and propagates service-task failure instead of printing it.
type Replies = Pin<Box<dyn Stream<Item = Result<Bytes, Status>> + Send>>;
type ReplyFuture = Pin<Box<dyn Future<Output = Result<Response<Replies>, Status>> + Send>>;
#[derive(Default)]
struct Peer {
    calls: usize,
    drained: bool,
    requests: Vec<Bytes>,
}
#[derive(Clone)]
struct LocalService {
    peer: Arc<Mutex<Peer>>,
    late_failure: bool,
}
impl NamedService for LocalService {
    const NAME: &'static str = "speech.s3.S3StubbyClientConnectorService";
}
impl StreamingService<Bytes> for LocalService {
    type Response = Bytes;
    type ResponseStream = Replies;
    type Future = ReplyFuture;
    fn call(&mut self, request: Request<Streaming<Bytes>>) -> Self::Future {
        let state = self.clone();
        Box::pin(async move {
            {
                let mut peer = state.peer.lock().map_err(|_| Status::internal("fixed"))?;
                peer.calls += 1;
                if peer.calls != 1 {
                    return Err(Status::invalid_argument("fixed"));
                }
            }
            for (name, expected) in HEADERS {
                let values = request.metadata().get_all(name);
                let mut values = values.iter();
                if values.next().and_then(|v| v.to_str().ok()) != Some(expected)
                    || values.next().is_some()
                {
                    return Err(Status::invalid_argument("fixed"));
                }
            }
            let mut input = request.into_inner();
            let mut requests = Vec::new();
            let mut bytes = 0;
            while let Some(request) = input.message().await? {
                bytes += request.len();
                if requests.len() >= 32 || bytes > 65536 {
                    return Err(Status::resource_exhausted("fixed"));
                }
                requests.push(request);
            }
            {
                let mut peer = state.peer.lock().map_err(|_| Status::internal("fixed"))?;
                peer.requests = requests;
                peer.drained = true; // Independent HTTP upload EOF, not a local send counter.
            }
            let response = synthetic_response().map_err(|_| Status::internal("fixed"))?;
            let mut replies = vec![Ok(response), Ok(Bytes::from_static(b"\x08\x01"))];
            if state.late_failure {
                replies.push(Err(Status::permission_denied("fixed")));
            }
            let stream: Replies = Box::pin(stream::iter(replies));
            Ok(Response::new(stream))
        })
    }
}
impl<B> Service<http::Request<B>> for LocalService
where
    B: tonic::codegen::Body + Send + 'static,
    B::Error: Into<StdError> + Send + 'static,
{
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: http::Request<B>) -> Self::Future {
        let service = self.clone();
        Box::pin(async move { Ok(Grpc::new(RawBytesCodec).streaming(service, request).await) })
    }
}
struct LocalServer {
    stop: CancellationToken,
    task: Option<JoinHandle<Result<(), tonic::transport::Error>>>,
}
impl LocalServer {
    async fn close(&mut self) -> Check {
        self.stop.cancel();
        let task = self.task.as_mut().ok_or(Failure::Sdk)?;
        let result = tokio::time::timeout(DEADLINE, task).await;
        match result {
            Ok(Ok(Ok(()))) => {
                self.task.take();
                Ok(())
            }
            _ => Err(Failure::Sdk), // Drop still aborts if graceful shutdown hung.
        }
    }
}
impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
fn synthetic_response() -> Result<Bytes, Failure> {
    use wire::{bytes_field as b, string_field as s, varint_field as v};
    let make = || -> Result<Vec<u8>, rambler_protocol::Error> {
        let hypothesis = s(1, TEXT)?;
        let recognition = [v(1, 1)?, b(3, &b(3, &hypothesis)?)?].concat();
        b(RECOGNITION_RESPONSE, &b(1, &recognition)?)
    };
    make().map(Bytes::from).map_err(|_| Failure::Sdk)
}
#[derive(Default)]
struct PcmReceipt {
    bytes: usize,
    eof: bool,
    closes: usize,
}
struct SyntheticPcm {
    remaining: Option<Bytes>,
    receipt: Arc<Mutex<PcmReceipt>>,
}
impl AudioSource for SyntheticPcm {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<Bytes>, SourceError>> {
        Box::pin(async move {
            let output = self.remaining.take();
            let mut receipt = self.receipt.lock().unwrap();
            if let Some(bytes) = &output {
                receipt.bytes += bytes.len();
            } else {
                receipt.eof = true;
            }
            Ok(output)
        })
    }
    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.remaining.take();
            self.receipt.lock().unwrap().closes += 1;
            Ok(())
        })
    }
}
fn fixed_context() -> EditingContext {
    EditingContext {
        before: "fixed synthetic context".into(),
        ..Default::default()
    }
}
async fn sdk(admitted: Admitted, late_failure: bool) -> Check {
    sdk_observed(admitted, late_failure)
        .await
        .map_err(|_| Failure::Sdk)
}
async fn sdk_observed(admitted: Admitted, late_failure: bool) -> Result<(), SdkFailure> {
    let _ = admitted;
    // No caller-supplied credentials, endpoint, PCM, context, or response.
    let metadata = Metadata::new(HEADERS.map(|(k, v)| (k.into(), CredentialValue::Text(v.into()))))
        .map_err(|_| Failure::Sdk)?;
    let listener = TcpListener::bind(std::net::SocketAddrV4::new(
        std::net::Ipv4Addr::LOCALHOST,
        0,
    ))
    .await
    .map_err(|_| Failure::Sdk)?;
    let config = TransportConfig::loopback(listener.local_addr().map_err(|_| Failure::Sdk)?)
        .map_err(|_| Failure::Sdk)?;
    let peer = Arc::new(Mutex::new(Peer::default()));
    let stop = CancellationToken::new();
    let done = stop.clone();
    let service = LocalService {
        peer: peer.clone(),
        late_failure,
    };
    let mut server = LocalServer {
        stop,
        task: Some(tokio::spawn(async move {
            Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(
                    TcpListenerStream::new(listener),
                    done.cancelled_owned(),
                )
                .await
        })),
    };
    let transport = tokio::time::timeout(DEADLINE, GrpcTransport::connect(config))
        .await
        .map_err(|_| Failure::Sdk)?
        .map_err(|_| Failure::Sdk)?;
    let engine = Engine::new(transport.clone());
    let receipt = Arc::new(Mutex::new(PcmReceipt::default()));
    let pcm = (0..1017)
        .flat_map(|i| ((i as i16 % 97) * 100 - 4800).to_le_bytes())
        .collect::<Vec<_>>();
    let (source, progress) = Pcm16Opus::new_with_progress(
        SyntheticPcm {
            remaining: Some(Bytes::from(pcm)),
            receipt: receipt.clone(),
        },
        EncoderConfig::default(),
    );
    let result = tokio::time::timeout(
        DEADLINE,
        collect_dictation(
            &engine,
            Box::new(source),
            SessionOptions {
                context: ContextInput::Static(fixed_context()),
                credentials: Some(Arc::new(metadata)),
                timeout: DEADLINE,
                ..Default::default()
            },
            CollectorLimits {
                max_final_updates: 1,
                max_result_bytes: 1024,
            },
        ),
    )
    .await;
    // Always attempt all owner closures before interpreting any response/result.
    let engine_closed = tokio::time::timeout(DEADLINE, engine.close()).await;
    let transport_closed = tokio::time::timeout(DEADLINE, transport.close()).await;
    drop(engine);
    drop(transport);
    let server_closed = server.close().await;
    engine_closed
        .map_err(|_| Failure::Sdk)?
        .map_err(|_| Failure::Sdk)?;
    transport_closed
        .map_err(|_| Failure::Sdk)?
        .map_err(|_| Failure::Sdk)?;
    server_closed?;
    if matches!(
        &result,
        Ok(Err(rambler_engine::EngineError::Transport(
            rambler_transport::TransportError::Grpc(tonic::Code::PermissionDenied)
        )))
    ) {
        let peer = peer.lock().map_err(|_| Failure::Sdk)?;
        return Err(SdkFailure::LatePermissionDenied {
            peer_calls: peer.calls,
            peer_drained: peer.drained,
        });
    }
    let result = result
        .map_err(|_| Failure::Sdk)?
        .map_err(|_| Failure::Sdk)?;
    let rendered = result
        .render(RenderPolicy::Strict)
        .map_err(|_| Failure::Sdk)?;
    require(
        rendered.text == TEXT && result.context.as_ref() == Some(&fixed_context()),
        Failure::Sdk,
    )?;
    let pcm = progress.snapshot();
    require(
        pcm.input_bytes == 2034
            && pcm.encoded_input_samples == 1017
            && pcm.input_eof
            && pcm.output_eof,
        Failure::Sdk,
    )?;
    let receipt = receipt.lock().map_err(|_| Failure::Sdk)?;
    require(
        receipt.bytes == 2034 && receipt.eof && receipt.closes == 1,
        Failure::Sdk,
    )?;
    let peer = peer.lock().map_err(|_| Failure::Sdk)?;
    require(peer.calls == 1 && peer.drained, Failure::Sdk)?;
    let ogg = validate_requests(&peer.requests)?;
    validate_ogg(&ogg)?;
    let metrics = result.metrics.ok_or(Failure::Sdk)?;
    require(
        metrics.eof_seconds.is_some()
            && metrics.audio.eof
            && metrics.audio.cleanup_finished
            && metrics.audio.cleanup_succeeded
            && metrics.audio.bytes == ogg.len() as u64
            && metrics.audio.chunks == (peer.requests.len() - 3) as u64
            && metrics.requests_produced == peer.requests.len() as u64
            && metrics.request_protobuf_bytes
                == peer.requests.iter().map(|r| r.len() as u64).sum::<u64>()
            && metrics.responses_received == 2,
        Failure::Sdk,
    )
    .map_err(SdkFailure::from)
}
fn nested(payload: &[u8], number: u32) -> Result<Vec<u8>, Failure> {
    let fields = wire::parse_fields(payload).map_err(|_| Failure::Sdk)?;
    let mut fields = fields.iter().filter(|field| field.number == number);
    let Some(wire::Value::Bytes(bytes)) = fields.next().map(|field| &field.value) else {
        return Err(Failure::Sdk);
    };
    require(fields.next().is_none(), Failure::Sdk)?;
    Ok(bytes.clone())
}
fn elapsed(payload: &[u8]) -> Result<u64, Failure> {
    let fields = wire::parse_fields(&nested(payload, 293100)?).map_err(|_| Failure::Sdk)?;
    match fields.iter().find(|f| f.number == 9).map(|f| &f.value) {
        Some(wire::Value::Varint(value)) => Ok(*value),
        _ => Err(Failure::Sdk),
    }
}
fn validate_requests(requests: &[Bytes]) -> Result<Vec<u8>, Failure> {
    require(requests.len() >= 4, Failure::Sdk)?;
    require(
        requests[0].as_ref()
            == initial_request(&ProtocolConfig::default(), elapsed(&requests[0])?)
                .map_err(|_| Failure::Sdk)?,
        Failure::Sdk,
    )?;
    require(
        requests[requests.len() - 2].as_ref()
            == context_request(&fixed_context()).map_err(|_| Failure::Sdk)?,
        Failure::Sdk,
    )?;
    require(
        requests.last().ok_or(Failure::Sdk)?.as_ref() == end_request(),
        Failure::Sdk,
    )?;
    let mut ogg = Vec::new();
    for request in &requests[1..requests.len() - 2] {
        let bytes = nested(&nested(request, 293101)?, 1)?;
        require(
            !bytes.is_empty()
                && request.as_ref()
                    == audio_request(&bytes, elapsed(request)?).map_err(|_| Failure::Sdk)?,
            Failure::Sdk,
        )?;
        ogg.extend(bytes);
    }
    Ok(ogg)
}
fn validate_ogg(ogg: &[u8]) -> Check {
    // Exhaustive page boundaries disallow reader resynchronization/trailing junk.
    let (mut offset, mut sequence, mut page_serial) = (0, 0u32, None);
    let mut previous_granule = 0;
    while offset < ogg.len() {
        let header = ogg.get(offset..offset + 27).ok_or(Failure::Sdk)?;
        require(&header[..5] == b"OggS\0", Failure::Sdk)?;
        let lacing_end = offset + 27 + header[26] as usize;
        let lacing = ogg.get(offset + 27..lacing_end).ok_or(Failure::Sdk)?;
        let end = lacing_end + lacing.iter().map(|b| *b as usize).sum::<usize>();
        require(end <= ogg.len(), Failure::Sdk)?;
        let serial = u32::from_le_bytes(header[14..18].try_into().map_err(|_| Failure::Sdk)?);
        let actual_sequence =
            u32::from_le_bytes(header[18..22].try_into().map_err(|_| Failure::Sdk)?);
        let granule = u64::from_le_bytes(header[6..14].try_into().map_err(|_| Failure::Sdk)?);
        require(
            serial == *page_serial.get_or_insert(serial)
                && actual_sequence == sequence
                && header[5]
                    == if offset == 0 {
                        2
                    } else if end == ogg.len() {
                        4
                    } else {
                        0
                    },
            Failure::Sdk,
        )?;
        if sequence >= 2 {
            require(granule >= previous_granule, Failure::Sdk)?;
            previous_granule = granule;
        }
        sequence += 1;
        offset = end;
    }
    require(sequence >= 3, Failure::Sdk)?;
    // Independent reader validates CRC; real Opus decoder checks full-stream trim.
    let mut reader = ogg::reading::PacketReader::new(Cursor::new(ogg));
    let mut decoder = opus::Decoder::new(48000, opus::Channels::Mono).map_err(|_| Failure::Sdk)?;
    let (mut count, mut decoded, mut skip, mut eos, mut serial) = (0, 0usize, 0u64, None, None);
    while let Some(packet) = reader.read_packet().map_err(|_| Failure::Sdk)? {
        require(eos.is_none(), Failure::Sdk)?;
        require(
            packet.stream_serial() == *serial.get_or_insert(packet.stream_serial()),
            Failure::Sdk,
        )?;
        match count {
            0 => {
                require(
                    packet.first_in_stream()
                        && packet.data.len() == 19
                        && packet.data.starts_with(b"OpusHead")
                        && packet.data[8..10] == [1, 1]
                        && packet.data[12..16] == 16000u32.to_le_bytes()
                        && packet.data[16..19] == [0, 0, 0],
                    Failure::Sdk,
                )?;
                skip = u16::from_le_bytes([packet.data[10], packet.data[11]]) as u64;
            }
            1 => require(packet.data.starts_with(b"OpusTags"), Failure::Sdk)?,
            _ => {
                decoded += decoder
                    .decode(&packet.data, &mut [0i16; 960], false)
                    .map_err(|_| Failure::Sdk)?;
            }
        }
        if packet.last_in_stream() {
            eos = Some(packet.absgp_page());
        }
        count += 1;
    }
    let granule = eos.ok_or(Failure::Sdk)?;
    require(
        count >= 3
            && skip > 0
            && granule.checked_sub(skip) == Some(1017 * 3)
            && decoded as u64 >= granule
            && (decoded as u64 - granule) < 960,
        Failure::Sdk,
    )
}
fn main() -> std::process::ExitCode {
    // No global panic-hook changes occur in automatic example tests.
    std::panic::set_hook(Box::new(|_| {}));
    let mut sys = Native;
    let mut output = Output::default();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        requested(
            std::env::args_os().nth(1).is_some(),
            &mut sys,
            &mut output,
            |admitted| {
                // Runtime and all source/provider/socket work are behind admission.
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| Failure::Sdk)?;
                runtime.block_on(sdk(admitted, false))
            },
        )
    }))
    .unwrap_or(Err(Failure::Internal));
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(failure) => {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                output.emit(&mut sys, failure.line())
            }));
            std::process::ExitCode::from(1)
        }
    }
}

#[cfg(test)]
#[path = "support/linux_guarded_loopback_tests.rs"]
mod tests;
