//! Desktop coordinator. Vault pipes are never shared with network processes.
mod network_client;
pub mod network_host;
pub mod wire;
mod worker_process;
pub use network_client::DaemonClient;
pub use veyra_session_runtime::{invitation_endpoint, signaling_endpoint_valid};
/// Derive the UI identity from the signed document, never a network-child label.
pub fn validate_peer_metadata(peer: &wire::Peer) -> io::Result<veyra_protocol::IdentityBinding> {
    use veyra_protocol::Frame;
    let Frame::IdentityBinding(binding) =
        Frame::decode(&peer.binding_frame).map_err(|_| unavailable())?
    else {
        return Err(unavailable());
    };
    veyra_identity::verify_binding(&binding, &binding.noise_public).map_err(|_| unavailable())?;
    let code = peer.verification_code.as_bytes();
    if code.len() != 19
        || code.iter().enumerate().any(|(i, b)| {
            if [4, 9, 14].contains(&i) {
                *b != b'-'
            } else {
                !b.is_ascii_hexdigit()
            }
        })
    {
        return Err(unavailable());
    }
    if peer.id != hex(&binding.identity)
        || peer.fingerprint != veyra_identity::fingerprint(&binding.identity)
    {
        return Err(unavailable());
    }
    Ok(binding)
}
/// Bind sealing to the recipient selected by the local user, before plaintext is
/// handed to custody. A compromised network child cannot swap an authorized offer.
pub fn validate_recipient_offer(
    frame: &[u8],
    recipient: &[u8; 32],
) -> io::Result<veyra_protocol::KeyOffer> {
    let veyra_protocol::Frame::KeyOffer(offer) =
        veyra_protocol::Frame::decode(frame).map_err(|_| unavailable())?
    else {
        return Err(unavailable());
    };
    if &offer.recipient != recipient {
        return Err(unavailable());
    }
    veyra_identity::verify_offer(&offer).map_err(|_| unavailable())?;
    Ok(offer)
}
use serde::Serialize;
use std::{io, path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Child,
    time::timeout,
};
use veyra_ipc::{MAX_IPC_FRAME, VaultRequest, VaultResponse};
use zeroize::Zeroizing;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSetup {
    pub configured: bool,
    pub source: &'static str,
    pub reason: Option<String>,
}
pub const NO_CONNECTION_SERVICE: &str = "Esta versão ainda não inclui um serviço de conexão. Você pode receber um convite para usar o serviço informado por ele. A conexão padrão deve ser configurada na distribuição do aplicativo.";
impl Default for ConnectionSetup {
    fn default() -> Self {
        Self {
            configured: false,
            source: "unconfigured",
            reason: Some(NO_CONNECTION_SERVICE.into()),
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub phase: &'static str,
    pub runtime_id: Option<String>,
    pub fingerprint: Option<String>,
    pub infrastructure_configured: bool,
    pub connection_setup: ConnectionSetup,
    pub reason: Option<String>,
    pub peers: Vec<PeerSummary>,
    pub pending_count: usize,
    pub memory_protection: &'static str,
    pub deliveries: Vec<DeliveryStatus>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryStatus {
    pub message_id: String,
    pub peer_id: String,
    pub state: &'static str,
    pub reason: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerSummary {
    pub id: String,
    pub label: String,
    pub fingerprint: String,
    pub verified: bool,
    pub route: &'static str,
    pub verification_code: Option<String>,
}
impl Default for SessionStatus {
    fn default() -> Self {
        Self {
            phase: "inactive",
            runtime_id: None,
            fingerprint: None,
            infrastructure_configured: false,
            connection_setup: ConnectionSetup::default(),
            reason: Some(NO_CONNECTION_SERVICE.into()),
            peers: Vec::new(),
            pending_count: 0,
            memory_protection: "not-checked",
            deliveries: Vec::new(),
        }
    }
}

pub struct VaultClient {
    child: Child,
    input: worker_process::WorkerInput,
    output: worker_process::WorkerOutput,
    in_flight: bool,
}
struct SensitiveRequest(VaultRequest);
impl Drop for SensitiveRequest {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        if let VaultRequest::Seal { text, .. } = &mut self.0 {
            text.zeroize();
        }
    }
}
impl VaultClient {
    /// Local supervisor metadata; never exposed as a WebView command.
    pub fn process_id(&self) -> Option<u32> {
        self.child.id()
    }
    /// The coordinator owns this plaintext operation; network custody omits it.
    pub async fn seal(&mut self, offer: Vec<u8>, text: &str) -> io::Result<VaultResponse> {
        let request = SensitiveRequest(VaultRequest::Seal {
            offer,
            text: text.to_owned(),
        });
        self.request(&request.0).await
    }
    pub async fn spawn(path: &Path) -> io::Result<Self> {
        let (mut child, mut input, mut output) = worker_process::spawn(path).await?;
        // Bootstrap uses the exact inherited handle, never an endpoint name.
        let mut challenge = Vec::new();
        let mut reply = Vec::new();
        // Generate through the shared IPC helper and in-memory captured duplex.
        // Use explicit challenge helpers below to avoid blocking pipe I/O.
        veyra_ipc::bootstrap_challenge(&mut challenge, &mut reply)?;
        let challenge = Zeroizing::new(challenge);
        let expected = Zeroizing::new(reply);
        let auth = async {
            input.write_all(&challenge).await?;
            input.flush().await?;
            let mut actual = [0; 32];
            output.read_exact(&mut actual).await?;
            if !veyra_ipc::bootstrap_matches(&actual, &expected) {
                return Err(unavailable());
            }
            Ok::<(), io::Error>(())
        };
        if !matches!(timeout(Duration::from_secs(5), auth).await, Ok(Ok(()))) {
            let _ = child.kill().await;
            return Err(unavailable());
        }
        Ok(Self {
            child,
            input,
            output,
            in_flight: false,
        })
    }
    pub async fn request(&mut self, request: &VaultRequest) -> io::Result<VaultResponse> {
        // Cancellation leaves this true. Never associate a late pipe response
        // with another typed request: invalidate the worker before reuse.
        if self.in_flight {
            let _ = self.child.kill().await;
            return Err(unavailable());
        }
        self.in_flight = true;
        let result = timeout(Duration::from_secs(5), self.exchange(request)).await;
        match result {
            Ok(Ok(reply)) => {
                self.in_flight = false;
                Ok(reply)
            }
            _ => {
                let _ = self.child.kill().await;
                Err(unavailable())
            }
        }
    }
    async fn exchange(&mut self, request: &VaultRequest) -> io::Result<VaultResponse> {
        if self.child.try_wait()?.is_some() {
            return Err(unavailable());
        }
        let frame = veyra_ipc::encode(request)?;
        self.input
            .write_all(&(frame.len() as u32).to_be_bytes())
            .await?;
        self.input.write_all(&frame).await?;
        self.input.flush().await?;
        let n = self.output.read_u32().await? as usize;
        if n == 0 || n > MAX_IPC_FRAME {
            return Err(unavailable());
        }
        let mut bytes = Zeroizing::new(vec![0; n]);
        self.output.read_exact(&mut bytes).await?;
        veyra_ipc::decode(&bytes)
    }
    pub async fn end(mut self) {
        let _ = self.request(&VaultRequest::End {}).await;
        let _ = self.child.kill().await;
    }
}
fn unavailable() -> io::Error {
    io::Error::other("vault unavailable; session invalidated")
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_does_not_invent_connectivity() {
        let s = SessionStatus::default();
        assert!(!s.infrastructure_configured);
        assert!(s.peers.is_empty());
        assert!(s.fingerprint.is_none());
    }
    #[test]
    fn network_metadata_cannot_substitute_a_displayed_identity() {
        let identity = veyra_identity::RuntimeIdentity::generate().unwrap();
        let b = identity.binding();
        let mut peer = wire::Peer {
            id: hex(&b.identity),
            fingerprint: veyra_identity::fingerprint(&b.identity),
            binding_frame: veyra_protocol::Frame::IdentityBinding(b.clone()).encode(),
            verification_code: "ABCD-EF01-2345-6789".into(),
            verified: false,
            online: true,
            route: veyra_connectivity::Route::Direct,
            available_offers: 0,
        };
        assert!(validate_peer_metadata(&peer).is_ok());
        peer.id = hex(&[0; 32]);
        assert!(validate_peer_metadata(&peer).is_err());
        peer.id = hex(&b.identity);
        peer.fingerprint = "outra identidade".into();
        assert!(validate_peer_metadata(&peer).is_err());
    }
    #[test]
    fn network_cannot_redirect_sealing_to_another_authorized_recipient() {
        let recipient = veyra_identity::RuntimeIdentity::generate().unwrap();
        let mut offer = veyra_protocol::KeyOffer {
            sender: [7; 32],
            recipient: recipient.public(),
            sender_runtime: [9; 16],
            recipient_runtime: recipient.runtime_id(),
            key_id: [1; 16],
            public_key: [2; 32],
            ttl_ms: 1000,
            signature: [0; 64],
        };
        recipient.sign_offer(&mut offer).unwrap();
        let frame = veyra_protocol::Frame::KeyOffer(offer).encode();
        assert!(validate_recipient_offer(&frame, &recipient.public()).is_ok());
        assert!(validate_recipient_offer(&frame, &[3; 32]).is_err());
    }
}
