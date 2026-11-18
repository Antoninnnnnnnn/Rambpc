#![allow(dead_code)]
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use rambler_transport::{GrpcTransport, RawBytesCodec, TransportConfig};
use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
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

pub type Replies = Pin<Box<dyn Stream<Item = Result<Bytes, Status>> + Send>>;
type HandlerFuture = Pin<Box<dyn Future<Output = Result<Response<Replies>, Status>> + Send>>;
type Handler = Arc<dyn Fn(Request<Streaming<Bytes>>) -> HandlerFuture + Send + Sync>;
#[derive(Clone)]
struct RawService(Handler);
impl NamedService for RawService {
    const NAME: &'static str = "speech.s3.S3StubbyClientConnectorService";
}
impl<B> Service<http::Request<B>> for RawService
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
        let handler = self.0.clone();
        Box::pin(async move {
            struct Method(Handler);
            impl StreamingService<Bytes> for Method {
                type Response = Bytes;
                type ResponseStream = Replies;
                type Future = HandlerFuture;
                fn call(&mut self, request: Request<Streaming<Bytes>>) -> Self::Future {
                    (self.0)(request)
                }
            }
            Ok(Grpc::new(RawBytesCodec)
                .streaming(Method(handler), request)
                .await)
        })
    }
}
pub struct TestServer {
    pub address: std::net::SocketAddr,
    accepted: Arc<AtomicUsize>,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
}
impl TestServer {
    pub async fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(Request<Streaming<Bytes>>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response<Replies>, Status>> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = RawService(Arc::new(move |request| Box::pin(handler(request))));
        let stop = CancellationToken::new();
        let done = stop.clone();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        let incoming = TcpListenerStream::new(listener).inspect(move |socket| {
            if let Ok(socket) = socket {
                // Tonic's builder cannot set this on a caller-supplied listener.
                socket.set_nodelay(true).unwrap();
                counter.fetch_add(1, Ordering::Relaxed);
            }
        });
        let task = tokio::spawn(async move {
            Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(incoming, done.cancelled_owned())
                .await
                .unwrap();
        });
        Self {
            address,
            accepted,
            stop,
            task: Some(task),
        }
    }
    pub fn accepted_connections(&self) -> usize {
        self.accepted.load(Ordering::Relaxed)
    }
    pub async fn transport(&self) -> GrpcTransport {
        GrpcTransport::connect(TransportConfig::loopback(self.address).unwrap())
            .await
            .unwrap()
    }
    pub async fn close(mut self) {
        self.stop.cancel();
        self.task.take().unwrap().await.unwrap();
    }
}
impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
pub fn replies(
    stream: impl Stream<Item = Result<Bytes, Status>> + Send + 'static,
) -> Response<Replies> {
    Response::new(Box::pin(stream))
}
pub async fn bounded<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("test deadline")
}
pub async fn drain(mut request: Streaming<Bytes>) -> Vec<Bytes> {
    let mut data = Vec::new();
    while let Some(payload) = request.message().await.unwrap() {
        data.push(payload);
    }
    data
}
pub async fn complete(request: Request<Streaming<Bytes>>) -> Result<Response<Replies>, Status> {
    drain(request.into_inner()).await;
    Ok(replies(futures_util::stream::iter([Ok(
        Bytes::from_static(b"\x08\x01"),
    )])))
}
