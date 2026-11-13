//! Raw gRPC over h2, with explicit per-stream reset ownership. No protobuf parsing.
use crate::{MetadataProvider, Progress, TransportConfig, TransportError, Upload};
use bytes::{Buf, Bytes, BytesMut};
use futures_util::{StreamExt, future::poll_fn};
use std::sync::{Arc, Mutex};
use tokio::{sync::mpsc, time::Instant};
use tonic::{Code, Request};

struct Reset(Arc<Mutex<h2::SendStream<Bytes>>>);
impl Drop for Reset {
    fn drop(&mut self) {
        self.0.lock().unwrap().send_reset(h2::Reason::CANCEL);
    }
}
fn net_error(error: h2::Error) -> TransportError {
    // gRPC-over-HTTP/2 RST_STREAM mapping, matching tonic's public codes without
    // formatting or retaining the potentially private underlying error string.
    let code = match error.reason() {
        Some(
            h2::Reason::NO_ERROR
            | h2::Reason::PROTOCOL_ERROR
            | h2::Reason::INTERNAL_ERROR
            | h2::Reason::FLOW_CONTROL_ERROR
            | h2::Reason::SETTINGS_TIMEOUT
            | h2::Reason::FRAME_SIZE_ERROR
            | h2::Reason::COMPRESSION_ERROR
            | h2::Reason::CONNECT_ERROR,
        ) => Code::Internal,
        Some(h2::Reason::REFUSED_STREAM) => Code::Unavailable,
        Some(h2::Reason::CANCEL) => Code::Cancelled,
        Some(h2::Reason::ENHANCE_YOUR_CALM) => Code::ResourceExhausted,
        Some(h2::Reason::INADEQUATE_SECURITY) => Code::PermissionDenied,
        None if error.is_io() => Code::Unavailable,
        _ => Code::Unknown,
    };
    TransportError::Grpc(code)
}
fn http_fallback(status: http::StatusCode) -> Code {
    match status.as_u16() {
        400 => Code::Internal,
        401 => Code::Unauthenticated,
        403 => Code::PermissionDenied,
        404 => Code::Unimplemented,
        429 | 502 | 503 | 504 => Code::Unavailable,
        _ => Code::Unknown,
    }
}

async fn send_bytes(
    sender: &Mutex<h2::SendStream<Bytes>>,
    mut bytes: Bytes,
) -> Result<(), TransportError> {
    while !bytes.is_empty() {
        // Bound native outstanding writes as well as the source queue. Never
        // submit an entire multi-MiB message without respecting h2 flow control.
        let claim = bytes.len().min(16 * 1024);
        sender.lock().unwrap().reserve_capacity(claim);
        let capacity = poll_fn(|cx| sender.lock().unwrap().poll_capacity(cx))
            .await
            .ok_or(TransportError::Grpc(Code::Unavailable))?
            .map_err(net_error)?;
        if capacity == 0 {
            continue;
        }
        let count = capacity.min(claim);
        sender
            .lock()
            .unwrap()
            .send_data(bytes.split_to(count), false)
            .map_err(net_error)?;
    }
    sender.lock().unwrap().reserve_capacity(0);
    Ok(())
}
async fn upload(
    mut input: Upload,
    sender: Arc<Mutex<h2::SendStream<Bytes>>>,
) -> Result<(), TransportError> {
    while let Some(bytes) = input.next().await {
        // Exact original payload; the only bytes added are gRPC's envelope.
        let len = u32::try_from(bytes.len()).map_err(|_| TransportError::MessageTooLarge)?;
        let mut header = [0u8; 5];
        header[1..].copy_from_slice(&len.to_be_bytes());
        send_bytes(&sender, Bytes::copy_from_slice(&header)).await?;
        send_bytes(&sender, bytes).await?;
    }
    sender
        .lock()
        .unwrap()
        .send_data(Bytes::new(), true)
        .map_err(net_error)?;
    input
        .progress
        .eof
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

fn grpc_status(headers: &http::HeaderMap) -> Result<Option<Code>, TransportError> {
    let mut values = headers.get_all("grpc-status").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(TransportError::Protocol);
    }
    let value = value.as_bytes();
    if value.is_empty() || value.len() > 2 || !value.iter().all(u8::is_ascii_digit) {
        return Err(TransportError::Protocol);
    }
    let number = value.iter().fold(0u8, |n, digit| n * 10 + digit - b'0');
    if number > 16 {
        return Err(TransportError::Protocol);
    }
    Ok(Some(Code::from_i32(i32::from(number))))
}
fn response_headers(headers: &http::HeaderMap) -> Result<(), TransportError> {
    let mut types = headers.get_all(http::header::CONTENT_TYPE).iter();
    let Some(content_type) = types.next() else {
        return Err(TransportError::Protocol);
    };
    let value = content_type.as_bytes();
    if types.next().is_some()
        || !(value == b"application/grpc"
            || value.starts_with(b"application/grpc+")
            || value.starts_with(b"application/grpc;"))
    {
        return Err(TransportError::Protocol);
    }
    let mut encodings = headers.get_all("grpc-encoding").iter();
    if let Some(value) = encodings.next()
        && (encodings.next().is_some() || value != "identity")
    {
        return Err(TransportError::CompressionUnsupported);
    }
    let mut http_encodings = headers.get_all(http::header::CONTENT_ENCODING).iter();
    if let Some(value) = http_encodings.next()
        && (http_encodings.next().is_some() || value != "identity")
    {
        return Err(TransportError::CompressionUnsupported);
    }
    Ok(())
}

/// Incremental, bounded gRPC envelope decoder. Uses the raw tonic decoder for
/// payload identity on the test server side; this client never parses protobuf.
struct Deframer {
    buffer: BytesMut,
    limit: usize,
}
impl Deframer {
    fn new(limit: usize) -> Self {
        Self {
            buffer: BytesMut::new(),
            limit,
        }
    }
    fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }
    fn message(&mut self) -> Result<Option<Bytes>, TransportError> {
        if self.buffer.len() < 5 {
            return Ok(None);
        }
        match self.buffer[0] {
            0 => {}
            1 => return Err(TransportError::CompressionUnsupported),
            _ => return Err(TransportError::Protocol),
        }
        let size = u32::from_be_bytes(self.buffer[1..5].try_into().unwrap()) as usize;
        if size > self.limit {
            return Err(TransportError::Grpc(Code::ResourceExhausted));
        }
        if self.buffer.len() - 5 < size {
            return Ok(None);
        }
        self.buffer.advance(5);
        Ok(Some(self.buffer.split_to(size).freeze()))
    }
}
async fn read(
    response: h2::client::ResponseFuture,
    limit: usize,
    responses: mpsc::Sender<Bytes>,
    progress: Arc<Progress>,
) -> Result<(), TransportError> {
    let response = response.await.map_err(net_error)?;
    let http_status = response.status();
    let initial_status = grpc_status(response.headers())?;
    let framed = match response_headers(response.headers()) {
        Ok(()) => true,
        // HTTP error pages need not have a gRPC content type. Drain them without
        // treating their HTML/text as gRPC, retaining a trailer-status override.
        Err(TransportError::Protocol) if http_status != http::StatusCode::OK => false,
        Err(error) => return Err(error),
    };
    let mut stream = response.into_body();
    let mut decoder = Deframer::new(limit);
    let mut saw_data = false;
    while let Some(chunk) = stream.data().await {
        let chunk = chunk.map_err(net_error)?;
        saw_data |= !chunk.is_empty();
        if initial_status.is_some() && saw_data {
            return Err(TransportError::Protocol);
        }
        if framed {
            decoder.push(&chunk);
        }
        stream
            .flow_control()
            .release_capacity(chunk.len())
            .map_err(net_error)?;
        while let Some(bytes) = decoder.message()? {
            progress
                .responses
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            responses
                .send(bytes)
                .await
                .map_err(|_| TransportError::Cancelled)?;
        }
    }
    if !decoder.buffer.is_empty() {
        return Err(TransportError::Protocol);
    }
    let trailers = stream.trailers().await.map_err(net_error)?;
    let trailing_status = trailers.as_ref().map(grpc_status).transpose()?.flatten();
    if initial_status.is_some() && trailers.is_some() {
        return Err(TransportError::Protocol);
    }
    let status = trailing_status
        .or(initial_status)
        .unwrap_or_else(|| http_fallback(http_status));
    *progress.terminal.lock().unwrap() = Some(status);
    if status == Code::Ok && !framed {
        return Err(TransportError::Protocol);
    }
    if status == Code::Ok {
        Ok(())
    } else {
        Err(TransportError::Grpc(status))
    }
}

pub(crate) async fn rpc(
    channel: h2::client::SendRequest<Bytes>,
    config: TransportConfig,
    provider: Arc<dyn MetadataProvider>,
    upload_stream: Upload,
    deadline: Instant,
    responses: mpsc::Sender<Bytes>,
    progress: Arc<Progress>,
) -> Result<(), TransportError> {
    let metadata = provider
        .metadata()
        .await
        .map_err(|_| TransportError::Credentials)?;
    let mut credentials = Request::new(());
    *credentials.metadata_mut() = metadata.into_map();
    credentials.set_timeout(deadline.saturating_duration_since(Instant::now()));
    let scheme = if config.plaintext_loopback {
        "http"
    } else {
        "https"
    };
    let mut request = http::Request::builder()
        .method(http::Method::POST)
        .version(http::Version::HTTP_2)
        .uri(format!("{scheme}://{}{}", config.target, config.method))
        .body(())
        .map_err(|_| TransportError::InvalidConfig)?;
    *request.headers_mut() = credentials.into_parts().0.into_headers();
    request.headers_mut().insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/grpc"),
    );
    request
        .headers_mut()
        .insert(http::header::TE, http::HeaderValue::from_static("trailers"));
    let mut channel = channel.ready().await.map_err(net_error)?;
    let (response, sender) = channel.send_request(request, false).map_err(net_error)?;
    let reset = Reset(Arc::new(Mutex::new(sender)));
    let write = upload(upload_stream, reset.0.clone());
    let read = read(response, config.max_message_bytes, responses, progress);
    tokio::pin!(write, read);
    let result = tokio::select! {
        // If upload has an already observable failure, it wins even when the
        // server's OK trailers become ready in the same supervisor poll.
        biased;
        result = &mut write => match result { Ok(()) => read.await, Err(error) => Err(error) },
        result = &mut read => result,
    };
    drop(reset); // Also stops incomplete upload after an early remote terminal.
    result
}
