//! Shared HTTP/2 connection with a caller-owned, joined driver and OS-root TLS.
use crate::{TransportConfig, TransportError};
use bytes::Bytes;
use std::sync::Arc;
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

pub(crate) async fn connect(config: &TransportConfig) -> Result<Connected, TransportError> {
    let authority: http::uri::Authority = config
        .target
        .parse()
        .map_err(|_| TransportError::InvalidConfig)?;
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']');
    // Resolved and validated at construction; explicit bad ports never become 443.
    let tcp = TcpStream::connect((host, config.port))
        .await
        .map_err(|_| TransportError::Connect)?;
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
        .handshake::<_, Bytes>(io)
        .await
        .map_err(|_| TransportError::Connect)?;
    let stop = CancellationToken::new();
    let stopped = stop.clone();
    let handle = tokio::spawn(async move {
        tokio::select! { biased; _ = stopped.cancelled() => Ok(()), result = connection => result }
    });
    Ok(Connected {
        sender,
        driver: Driver {
            stop,
            handle: Mutex::new(Some(handle)),
        },
    })
}
