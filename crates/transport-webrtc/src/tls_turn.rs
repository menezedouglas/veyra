//! Adapter for an existing TURN/TCP backend that lacks real TURN/TLS.
//! No TURN/WebRTC protocol is implemented here: standard TLS library plus a
//! bounded byte-stream copy. A verified TLS handshake precedes local exposure.
use super::{Result, TransportError};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, ServerName},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};
use tokio_rustls::{TlsConnector, client::TlsStream};

pub struct TlsTurnBridge {
    pub local_port: u16,
    task: JoinHandle<()>,
    verified: Arc<AtomicUsize>,
}
impl Drop for TlsTurnBridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn verified_stream(
    host: String,
    port: u16,
    config: Arc<ClientConfig>,
) -> Result<TlsStream<TcpStream>> {
    let name = ServerName::try_from(host.clone()).map_err(|_| TransportError::Configuration)?;
    tokio::time::timeout(Duration::from_secs(10), async {
        let tcp = TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|_| TransportError::TlsConnection)?;
        let tls = TlsConnector::from(config)
            .connect(name, tcp)
            .await
            .map_err(|error| {
                if matches!(
                    error
                        .get_ref()
                        .and_then(|e| e.downcast_ref::<rustls::Error>()),
                    Some(rustls::Error::InvalidCertificate(_))
                ) {
                    TransportError::TlsCertificate
                } else {
                    TransportError::TlsHandshake
                }
            })?;
        if tls.get_ref().1.protocol_version().is_none()
            || tls.get_ref().1.peer_certificates().is_none()
        {
            return Err(TransportError::Backend);
        }
        Ok(tls)
    })
    .await
    .map_err(|_| TransportError::Timeout)?
}

fn tls_config(extra_roots: &[Vec<u8>]) -> Result<Arc<ClientConfig>> {
    if extra_roots.len() > 4 || extra_roots.iter().any(|c| c.len() > 8 * 1024) {
        return Err(TransportError::Configuration);
    }
    let mut roots = RootCertStore::empty();
    for certificate in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(certificate);
    }
    for certificate in extra_roots {
        roots
            .add(CertificateDer::from(certificate.clone()))
            .map_err(|_| TransportError::Configuration)?;
    }
    if roots.is_empty() {
        return Err(TransportError::Configuration);
    }
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|_| TransportError::Backend)?
            .with_root_certificates(roots)
            .with_no_client_auth();
    Ok(Arc::new(config))
}

impl TlsTurnBridge {
    pub fn verified_sessions(&self) -> usize {
        self.verified.load(Ordering::Acquire)
    }
    pub async fn start(host: String, port: u16, roots: &[Vec<u8>]) -> Result<Self> {
        Self::start_with_config(host, port, tls_config(roots)?).await
    }
    async fn start_with_config(host: String, port: u16, config: Arc<ClientConfig>) -> Result<Self> {
        // Failure here prevents creation of any plaintext-facing local bridge.
        let first = verified_stream(host.clone(), port, config.clone()).await?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| TransportError::Backend)?;
        let local_port = listener
            .local_addr()
            .map_err(|_| TransportError::Backend)?
            .port();
        let verified = Arc::new(AtomicUsize::new(1));
        let counter = verified.clone();
        let task = tokio::spawn(async move {
            let mut first = Some(first);
            let mut workers = JoinSet::new();
            // Finite allocation budget and finite bridge lifetime. A new peer
            // connection must create a fresh adapter and Noise session.
            for _ in 0..8 {
                while workers.len() >= 4 {
                    workers.join_next().await;
                }
                let Ok(Ok((mut local, address))) =
                    tokio::time::timeout(Duration::from_secs(300), listener.accept()).await
                else {
                    break;
                };
                if !address.ip().is_loopback() {
                    continue;
                }
                let remote = match first.take() {
                    Some(remote) => remote,
                    None => match verified_stream(host.clone(), port, config.clone()).await {
                        Ok(remote) => {
                            counter.fetch_add(1, Ordering::AcqRel);
                            remote
                        }
                        Err(_) => continue,
                    },
                };
                workers.spawn(async move {
                    let mut remote = remote;
                    // Standard Tokio copy supplies backpressure; no unbounded
                    // queues, framing, crypto, retries or plaintext fallback.
                    let _ = tokio::time::timeout(
                        Duration::from_secs(600),
                        tokio::io::copy_bidirectional_with_sizes(
                            &mut local,
                            &mut remote,
                            16 * 1024,
                            16 * 1024,
                        ),
                    )
                    .await;
                });
            }
            while workers.join_next().await.is_some() {}
        });
        Ok(Self {
            local_port,
            task,
            verified,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::TlsAcceptor;

    fn server() -> (Arc<ServerConfig>, Vec<u8>) {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let der = cert.cert.der().to_vec();
        let key = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![CertificateDer::from(der.clone())], key.into())
                .unwrap();
        (Arc::new(config), der)
    }
    async fn launch() -> (u16, Vec<u8>, JoinHandle<bool>) {
        let (config, root) = server();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let Ok(mut tls) = TlsAcceptor::from(config).accept(socket).await else {
                return false;
            };
            if tls.get_ref().1.protocol_version().is_none() {
                return false;
            }
            let mut message = [0; 4];
            if tls.read_exact(&mut message).await.is_err() || message != *b"test" {
                return false;
            }
            tls.write_all(b"okay").await.is_ok()
        });
        (port, root, task)
    }
    #[tokio::test]
    async fn real_tls_handshake_and_bounded_tcp_adapter() {
        let (port, root, server) = launch().await;
        let bridge = TlsTurnBridge::start("localhost".into(), port, &[root])
            .await
            .unwrap();
        assert_eq!(bridge.verified_sessions(), 1);
        let mut tcp = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, bridge.local_port))
            .await
            .unwrap();
        tcp.write_all(b"test").await.unwrap();
        let mut reply = [0; 4];
        tcp.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"okay");
        assert!(server.await.unwrap());
    }
    #[tokio::test]
    async fn untrusted_certificate_is_rejected_before_local_listener() {
        let (port, _, server) = launch().await;
        assert!(
            TlsTurnBridge::start("localhost".into(), port, &[])
                .await
                .is_err()
        );
        assert!(!server.await.unwrap());
    }
    #[tokio::test]
    async fn incorrect_hostname_is_rejected() {
        let (port, root, server) = launch().await;
        assert!(
            TlsTurnBridge::start("127.0.0.1".into(), port, &[root])
                .await
                .is_err()
        );
        assert!(!server.await.unwrap());
    }
}
