//! Raw gRPC over h2, with explicit per-stream reset ownership. No protobuf parsing.
use crate::{MetadataProvider, Progress, TransportConfig, TransportError, Upload};
use bytes::{Buf, Bytes};
use futures_util::{StreamExt, future::poll_fn};
use std::sync::{Arc, Mutex};
use tokio::{sync::mpsc, time::Instant};
use tonic::{Code, Request};

mod deframer;
use deframer::Deframer;

// Keep prefix and original payload together without a contiguous envelope copy.
pub(crate) type Envelope = bytes::buf::Chain<Bytes, Bytes>;

struct Reset(Arc<Mutex<h2::SendStream<Envelope>>>);
impl Drop for Reset {
    fn drop(&mut self) {
        self.0.lock().unwrap().send_reset(h2::Reason::CANCEL);
    }
}
// Keep the cause on the error actually selected, not in sticky shared state.
// In particular, poll_reset(Ok(reason)) has already lost GOAWAY vs RST origin.
// These are deliberately distinct until the supervisor redacts its final result.
pub(crate) enum RpcError {
    Transport(TransportError),
    H2(h2::Error),
    ResetReason(h2::Reason),
}
impl From<TransportError> for RpcError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}
impl From<h2::Error> for RpcError {
    fn from(error: h2::Error) -> Self {
        Self::H2(error)
    }
}
impl RpcError {
    pub(crate) fn into_transport(self) -> TransportError {
        match self {
            Self::Transport(error) => error,
            Self::H2(error) => net_error(error),
            // This maps only the gRPC code; it must never infer error identity.
            Self::ResetReason(reason) => net_error(reason.into()),
        }
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

// The caller grants a positive count no greater than the envelope remaining.
fn split_envelope(prefix: &mut Bytes, body: &mut Bytes, count: usize) -> Envelope {
    assert!(
        count > 0 && count <= prefix.len() + body.len(),
        "invalid envelope split count"
    );
    let head = prefix.split_to(count.min(prefix.len()));
    let tail = body.split_to(count - head.len());
    head.chain(tail)
}

async fn send_envelope(
    sender: &Mutex<h2::SendStream<Envelope>>,
    mut prefix: Bytes,
    mut body: Bytes,
) -> Result<(), RpcError> {
    while prefix.len() + body.len() != 0 {
        // Bound native outstanding writes as well as the source queue. Never
        // submit an entire multi-MiB message without respecting h2 flow control.
        // One submission spans both parts when credit permits; h2 does not
        // merge a separately submitted five-byte prefix with its payload.
        let claim = (prefix.len() + body.len()).min(16 * 1024);
        sender.lock().unwrap().reserve_capacity(claim);
        let capacity = match poll_fn(|cx| sender.lock().unwrap().poll_capacity(cx)).await {
            Some(result) => result?,
            None => {
                // A bare reason may be GOAWAY or RST_STREAM: do not fabricate
                // a typed h2 error, and do not acknowledge any driver failure.
                return Err(
                    match poll_fn(|cx| sender.lock().unwrap().poll_reset(cx)).await {
                        Ok(reason) => RpcError::ResetReason(reason),
                        Err(error) => RpcError::H2(error),
                    },
                );
            }
        };
        if capacity == 0 {
            continue;
        }
        let count = capacity.min(claim);
        sender
            .lock()
            .unwrap()
            .send_data(split_envelope(&mut prefix, &mut body, count), false)?;
    }
    sender.lock().unwrap().reserve_capacity(0);
    Ok(())
}
async fn upload(
    mut input: Upload,
    sender: Arc<Mutex<h2::SendStream<Envelope>>>,
) -> Result<(), RpcError> {
    while let Some(bytes) = input.next().await {
        // Exact original payload; the only bytes added are gRPC's envelope.
        let len = u32::try_from(bytes.len()).map_err(|_| TransportError::MessageTooLarge)?;
        send_envelope(&sender, crate::grpc_prefix::grpc_prefix(len), bytes).await?;
        input.progress.upload.lock().unwrap().record_request(len);
    }
    sender
        .lock()
        .unwrap()
        .send_data(Bytes::new().chain(Bytes::new()), true)?;
    input.progress.upload.lock().unwrap().half_close = Some(Instant::now());
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
    let base = b"application/grpc";
    if types.next().is_some()
        || !(value
            .get(..base.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(base))
            && matches!(value.get(base.len()), None | Some(b'+' | b';')))
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

async fn read(
    response: h2::client::ResponseFuture,
    limit: usize,
    responses: mpsc::Sender<(Bytes, Instant)>,
    progress: Arc<Progress>,
) -> Result<(), RpcError> {
    let response = response.await?;
    let http_status = response.status();
    let initial_status = grpc_status(response.headers())?;
    let framed = match response_headers(response.headers()) {
        Ok(()) => true,
        // HTTP error pages need not have a gRPC content type. Drain them without
        // treating their HTML/text as gRPC, retaining a trailer-status override.
        Err(TransportError::Protocol) if http_status != http::StatusCode::OK => false,
        Err(error) => return Err(error.into()),
    };
    let mut stream = response.into_body();
    let mut decoder = Deframer::new(limit);
    while let Some(chunk) = stream.data().await {
        let mut next = {
            let chunk = chunk?;
            // Trailers-Only ends with the initial HEADERS. Even zero-length DATA
            // is another frame, not proof that those HEADERS ended the stream.
            if initial_status.is_some() {
                return Err(TransportError::Protocol.into());
            }
            // Preserve credit release before parser errors or a blocked send.
            stream.flow_control().release_capacity(chunk.len())?;
            if !framed {
                continue;
            }
            decoder.frame(&chunk)?
        }; // Drop h2's backing before awaiting the response queue.
        while let Some(bytes) = next {
            progress
                .responses
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            responses
                .send((bytes, Instant::now()))
                .await
                .map_err(|_| TransportError::Cancelled)?;
            next = decoder.message()?;
        }
    }
    if !decoder.is_empty() {
        return Err(TransportError::Protocol.into());
    }
    let trailers = stream.trailers().await?;
    let trailing_status = trailers.as_ref().map(grpc_status).transpose()?.flatten();
    if initial_status.is_some() && trailers.is_some() {
        return Err(TransportError::Protocol.into());
    }
    let status = trailing_status
        .or(initial_status)
        .unwrap_or_else(|| http_fallback(http_status));
    *progress.terminal.lock().unwrap() = Some(status);
    if status == Code::Ok && !framed {
        return Err(TransportError::Protocol.into());
    }
    if status == Code::Ok {
        Ok(())
    } else {
        Err(TransportError::Grpc(status).into())
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn rpc(
    channel: h2::client::SendRequest<Envelope>,
    config: TransportConfig,
    provider: Arc<dyn MetadataProvider>,
    upload_stream: Upload,
    deadline: Instant,
    responses: mpsc::Sender<(Bytes, Instant)>,
    progress: Arc<Progress>,
    admitted: tokio::sync::oneshot::Sender<()>,
) -> Result<(), RpcError> {
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
    let mut channel = channel.ready().await?;
    let (response, sender) = channel.send_request(request, false)?;
    let reset = Reset(Arc::new(Mutex::new(sender)));
    // Local admission only, not peer/auth acceptance or network delivery.
    let _ = admitted.send(());
    let write = upload(upload_stream, reset.0.clone());
    let read = read(response, config.max_message_bytes, responses, progress);
    tokio::pin!(write, read);
    let result = tokio::select! {
        // If upload has an already observable failure, it wins even when the
        // server's OK trailers become ready in the same supervisor poll.
        biased;
        result = &mut write => match result {
            Ok(()) => read.await,
            Err(error) => {
                let lost_origin = match &error {
                    RpcError::ResetReason(_) => true,
                    RpcError::H2(error) => !error.is_io() && !error.is_go_away() && !error.is_reset(),
                    RpcError::Transport(_) => false,
                };
                if lost_origin {
                    // ResponseFuture/RecvStream retains the actual typed cause
                    // that poll_reset(Ok(reason)) and send_data(UserError) lost.
                    // Poll once, never await: read may be blocked on our bounded
                    // response queue. Do not turn an upload failure into a hang,
                    // discard the write failure for remote OK, or infer identity.
                    if let std::task::Poll::Ready(Err(RpcError::H2(cause))) = futures_util::poll!(read.as_mut()) {
                        return Err(RpcError::H2(cause));
                    }
                }
                Err(error)
            }
        },
        result = &mut read => result,
    };
    drop(reset); // Also stops incomplete upload after an early remote terminal.
    result
}

#[cfg(test)]
mod envelope_tests {
    use super::*;
    use std::io::IoSlice;

    #[test]
    fn every_positive_split_preserves_envelope_bytes_and_backing_slices() {
        for length in [0usize, 1, 4, 250, 251, 255, 256, 16_379, 16_380] {
            let original_prefix = crate::grpc_prefix::grpc_prefix(length as u32);
            let original_body = Bytes::from(
                (0..length)
                    .map(|offset| ((offset * 13 + 37) % 256) as u8)
                    .collect::<Vec<_>>(),
            );
            let n = length as u32;
            let header = [0, (n >> 24) as u8, (n >> 16) as u8, (n >> 8) as u8, n as u8];
            let total = 5 + length;
            for grant in 1..=total {
                let mut prefix = original_prefix.clone();
                let mut body = original_body.clone();
                let mut consumed = 0;
                while consumed < total {
                    let count = grant.min(total - consumed);
                    let piece = split_envelope(&mut prefix, &mut body, count);
                    assert_eq!(piece.remaining(), count);
                    assert!(piece.has_remaining());
                    assert!(piece.remaining() <= grant);
                    let head_length = (5usize.saturating_sub(consumed)).min(count);
                    let prefix_offset = consumed.min(5);
                    assert_eq!(
                        piece.first_ref().as_ref(),
                        &header[prefix_offset..prefix_offset + head_length]
                    );
                    let body_offset = consumed.saturating_sub(5);
                    let tail_length = count - head_length;
                    assert_eq!(
                        piece.last_ref().as_ref(),
                        &original_body[body_offset..body_offset + tail_length]
                    );
                    // split_to(0) may return the canonical empty Bytes: it has
                    // no payload bytes whose address could be checked.
                    if tail_length != 0 {
                        assert_eq!(
                            piece.last_ref().as_ptr(),
                            original_body[body_offset..].as_ptr()
                        );
                        let start = piece.last_ref().as_ptr() as usize;
                        let original = original_body.as_ptr() as usize;
                        assert!(start >= original);
                        assert!(start + tail_length <= original + length);
                    }
                    if grant == total {
                        assert_eq!(piece.first_ref().as_ptr(), original_prefix.as_ptr());
                        if length < 256 {
                            assert_eq!(
                                piece.first_ref().as_ptr(),
                                crate::grpc_prefix::grpc_prefix(length as u32).as_ptr()
                            );
                        }
                        assert_eq!(piece.last_ref().as_ptr(), original_body.as_ptr());
                        assert_eq!(piece.last_ref().len(), length);
                    }
                    consumed += count;
                    assert_eq!(prefix.len() + body.len(), total - consumed);
                }
                assert!(prefix.is_empty() && body.is_empty());
            }
        }
    }

    #[test]
    fn three_byte_grants_cross_from_cached_prefix_into_original_body() {
        let original = Bytes::from_static(b"\x00\xff\x42\xa5");
        let mut body = original.clone();
        let mut prefix = crate::grpc_prefix::grpc_prefix(4);
        let first = split_envelope(&mut prefix, &mut body, 3);
        assert_eq!(first.first_ref().as_ref(), &[0, 0, 0]);
        assert!(first.last_ref().is_empty());
        let second = split_envelope(&mut prefix, &mut body, 3);
        assert_eq!(second.first_ref().as_ref(), &[0, 4]);
        assert_eq!(second.last_ref().as_ref(), &original[..1]);
        assert_eq!(second.last_ref().as_ptr(), original.as_ptr());
        let third = split_envelope(&mut prefix, &mut body, 3);
        assert!(third.first_ref().is_empty());
        assert_eq!(third.last_ref().as_ref(), &original[1..]);
        assert_eq!(third.last_ref().as_ptr(), original[1..].as_ptr());
        assert!(prefix.is_empty() && body.is_empty());
    }

    #[test]
    fn chain_and_take_advance_and_vectored_chunks_cross_the_prefix_boundary() {
        fn assert_driver_traits<T: Send + Sync + Unpin + 'static>(_: std::marker::PhantomData<T>) {}
        assert_driver_traits(std::marker::PhantomData::<Envelope>);
        let original = Bytes::from_static(b"\x00\xff\x42\xa5");
        for advanced in 0..=9 {
            let mut prefix = crate::grpc_prefix::grpc_prefix(4);
            let mut body = original.clone();
            let mut envelope = split_envelope(&mut prefix, &mut body, 9);
            envelope.advance(advanced);
            assert_eq!(envelope.remaining(), 9 - advanced);
            let expected = b"\x00\x00\x00\x00\x04\x00\xff\x42\xa5";
            let mut slices = [IoSlice::new(&[]); 2];
            let count = envelope.chunks_vectored(&mut slices);
            let mut cursor = advanced;
            for slice in &slices[..count] {
                assert_eq!(slice.as_ref(), &expected[cursor..cursor + slice.len()]);
                cursor += slice.len();
            }
            assert_eq!(cursor, 9);
            if advanced < 5 {
                assert_eq!(envelope.chunk(), &expected[advanced..5]);
            } else {
                assert_eq!(envelope.chunk(), &expected[advanced..]);
            }
        }
        let mut prefix = crate::grpc_prefix::grpc_prefix(4);
        let mut body = original.clone();
        let mut limited = split_envelope(&mut prefix, &mut body, 9).take(6);
        let mut slices = [IoSlice::new(&[]); 2];
        assert_eq!(limited.chunks_vectored(&mut slices), 2);
        assert_eq!(slices[0].as_ref(), &[0, 0, 0, 0, 4]);
        assert_eq!(slices[1].as_ref(), &original[..1]);
        limited.advance(5);
        assert_eq!(limited.remaining(), 1);
        assert_eq!(limited.chunk(), &original[..1]);
        limited.advance(1);
        assert!(!limited.has_remaining());
        let rest = limited.into_inner();
        assert_eq!(rest.remaining(), 3);
        assert_eq!(rest.chunk(), &original[1..]);
    }

    #[test]
    #[should_panic(expected = "invalid envelope split count")]
    fn zero_split_is_rejected_before_an_empty_nonfinal_submission() {
        split_envelope(
            &mut crate::grpc_prefix::grpc_prefix(0),
            &mut Bytes::new(),
            0,
        );
    }

    #[test]
    #[should_panic(expected = "invalid envelope split count")]
    fn split_beyond_remaining_is_rejected() {
        split_envelope(
            &mut crate::grpc_prefix::grpc_prefix(0),
            &mut Bytes::new(),
            6,
        );
    }
}

#[cfg(test)]
mod cause_tests {
    use super::*;
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[tokio::test]
    async fn blocked_upload_preserves_typed_receive_goaway_before_redaction() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut tcp, _) = listener.accept().await.unwrap();
                let mut preface = [0; 24];
                tcp.read_exact(&mut preface).await.unwrap();
                // SETTINGS_INITIAL_WINDOW_SIZE=0, then wait for HEADERS.
                tcp.write_all(&[0, 0, 6, 4, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0])
                    .await
                    .unwrap();
                loop {
                    let mut header = [0; 9];
                    tcp.read_exact(&mut header).await.unwrap();
                    let size = (usize::from(header[0]) << 16)
                        | (usize::from(header[1]) << 8)
                        | usize::from(header[2]);
                    let mut data = vec![0; size];
                    tcp.read_exact(&mut data).await.unwrap();
                    if header[3] == 1 {
                        break;
                    }
                }
                // GOAWAY(last=0, PROTOCOL_ERROR). Keep TCP open until h2 exits.
                tcp.write_all(&[0, 0, 8, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])
                    .await
                    .unwrap();
                let mut buffer = [0; 1024];
                while tcp.read(&mut buffer).await.is_ok_and(|n| n != 0) {}
            });
            let config = TransportConfig::loopback(address).unwrap();
            let connected = crate::connection::connect_with_timeout(&config, false)
                .await
                .unwrap();
            let progress = Arc::new(Progress::default());
            let (tx, receiver) = mpsc::channel(1);
            tx.send(Bytes::from(vec![0; 1024 * 1024])).await.unwrap();
            drop(tx);
            let (responses, _rx) = mpsc::channel(1);
            let result = rpc(
                connected.sender,
                config,
                Arc::new(crate::Metadata::default()),
                Upload {
                    receiver,
                    progress: progress.clone(),
                },
                Instant::now() + Duration::from_secs(3),
                responses,
                progress,
                tokio::sync::oneshot::channel().0,
            )
            .await;
            match result {
                Err(RpcError::H2(error)) => {
                    assert!(error.is_go_away() && error.is_remote());
                    assert_eq!(error.reason(), Some(h2::Reason::PROTOCOL_ERROR));
                }
                _ => panic!("typed receive GOAWAY was replaced by a bare reason or other outcome"),
            }
            assert!(matches!(
                connected.driver.shutdown().await,
                Err(TransportError::Connect)
            ));
            server.await.unwrap();
        })
        .await
        .expect("bounded typed-cause regression");
    }
}
