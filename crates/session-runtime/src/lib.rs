//! Network-only runtime. Custody is a capability-limited port over inherited IPC.
//! This crate has no command to verify a person, seal plaintext or open a message.
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::{
    net::TcpStream,
    sync::{Mutex, mpsc, oneshot, watch},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use veyra_connectivity::{Route, TransportConfig};
use veyra_core::Outbox;
use veyra_crypto::{NoiseChannel, NoiseHandshake};
use veyra_domain::{Deadline, MAX_PEERS};
use veyra_identity::{fingerprint, verification_code, verify_binding, verify_envelope};
use veyra_protocol::{
    Envelope, Frame, Id, Identity, IdentityBinding, KeyOffer, Receipt, SignalDocument,
    noise_prologue,
};
use veyra_signaling::SignalPacket;
use veyra_transport::{RTCSessionDescription, WebRtcPeer};
use zeroize::{Zeroize, Zeroizing};

const MAX_INVITES: usize = 4;
const MAX_EVENTS: usize = 128;
const INVITE_SECONDS: u64 = 300;
const OFFER_TTL: u32 = 300_000;
const MAX_SIGNAL: usize = 64 * 1024;
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Error)]
pub enum Error {
    #[error("infraestrutura não configurada ou endpoint WSS inválido")]
    Configuration,
    #[error("conexão ou operador indisponível")]
    Connection,
    #[error("convite inválido, expirado ou já utilizado")]
    Invitation,
    #[error("identidade ou autorização não verificada")]
    Authentication,
    #[error("limite de recursos atingido")]
    Capacity,
    #[error("oferta válida indisponível; redija novamente quando o contato estiver disponível")]
    NoOffer,
    #[error("operação recusada pelo cofre")]
    Custody,
    #[error("sessão encerrada")]
    Closed,
}
/// Implementations must forward ONLY these typed operations to the coordinator.
/// They must not pass the vault pipe handle, OpenOnce or VerifyPeer to this process.
#[async_trait]
pub trait VaultNetworkPort: Send + Sync + 'static {
    async fn binding(&self) -> Result<IdentityBinding>;
    async fn prepare_transport(&self) -> Result<Zeroizing<[u8; 32]>>;
    async fn sign_signal(&self, document: SignalDocument) -> Result<[u8; 64]>;
    async fn create_offer(&self, peer: Identity, ttl_ms: u32) -> Result<KeyOffer>;
    async fn accept_envelope(&self, envelope: Envelope) -> Result<Receipt>;
}
#[derive(Clone)]
pub struct RuntimeConfig {
    pub endpoint: String,
    pub transport: TransportConfig,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub token: String,
    pub expires_unix: u64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerStatus {
    pub id: String,
    pub binding_frame: Vec<u8>,
    pub fingerprint: String,
    pub verification_code: String,
    pub verified: bool,
    pub online: bool,
    pub route: Route,
    pub available_offers: usize,
}
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeEvent {
    PeerChanged {
        peer_id: String,
    },
    Delivery {
        message_id: String,
        status: &'static str,
    },
    ConnectionFailed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InviteToken {
    version: u8,
    endpoint: String,
    room: String,
    expires_unix: u64,
    binding: String,
    fingerprint: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attached {
    status: String,
    turn: Option<TurnGrant>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnGrant {
    urls: Vec<String>,
    username: String,
    credential: String,
    expires_unix: u64,
}
impl Drop for TurnGrant {
    fn drop(&mut self) {
        self.username.zeroize();
        self.credential.zeroize();
    }
}
fn apply_turn_grant(
    mut config: TransportConfig,
    grant: Option<TurnGrant>,
) -> Result<TransportConfig> {
    // Preserve an explicit operator/fallback selection. The default client gets
    // short-lived TURN credentials over authenticated TLS without an operator secret.
    if config.ice_servers.is_empty()
        && let Some(mut g) = grant
    {
        let now = unix()?;
        if g.expires_unix <= now || g.expires_unix.saturating_sub(now) > 300 {
            return Err(Error::Configuration);
        }
        config.ice_servers.push(veyra_connectivity::IceServer {
            urls: std::mem::take(&mut g.urls),
            username: std::mem::take(&mut g.username),
            credential: std::mem::take(&mut g.credential),
        });
    }
    veyra_transport::validate_config(&config).map_err(|_| Error::Configuration)?;
    Ok(config)
}

struct PublicOffer {
    offer: KeyOffer,
    deadline: Deadline,
}
struct PeerHandle {
    status: PeerStatus,
    offers: VecDeque<PublicOffer>,
    command: mpsc::Sender<PeerCommand>,
}
enum PeerCommand {
    Approve {
        code: String,
        reply: oneshot::Sender<Result<()>>,
    },
    Send {
        envelope: Box<Envelope>,
        reply: oneshot::Sender<Result<()>>,
    },
    Close,
}
struct PairingSignal {
    socket: Option<Socket>,
    room: [u8; 32],
    expires: u64,
    role: u8,
}
struct State {
    peers: HashMap<String, PeerHandle>,
    invites: usize,
    events: VecDeque<RuntimeEvent>,
}
pub struct Runtime {
    config: RuntimeConfig,
    binding: IdentityBinding,
    port: Arc<dyn VaultNetworkPort>,
    state: Mutex<State>,
    ended: AtomicBool,
    cancellation: watch::Sender<bool>,
}
fn boot() -> Result<u64> {
    veyra_secure_memory::monotonic_ms().map_err(|_| Error::Closed)
}
fn unix() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|x| x.as_secs())
        .map_err(|_| Error::Invitation)
}
fn endpoint_valid(endpoint: &str) -> bool {
    let Ok(u) = url::Url::parse(endpoint) else {
        return false;
    };
    u.scheme() == "wss"
        && u.host_str().is_some()
        && u.username().is_empty()
        && u.password().is_none()
        && u.query().is_none()
        && u.fragment().is_none()
        && u.path() == "/v1/rendezvous"
        && endpoint.len() <= 512
}
impl Runtime {
    pub async fn new(config: RuntimeConfig, port: Arc<dyn VaultNetworkPort>) -> Result<Arc<Self>> {
        let mut gathering_config = config.transport.clone();
        if gathering_config.ice_servers.is_empty() {
            gathering_config.relay_only = false;
        }
        let valid_transport = veyra_transport::validate_config(&gathering_config).is_ok();
        if !endpoint_valid(&config.endpoint) || !valid_transport {
            return Err(Error::Configuration);
        }
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            return Err(Error::Configuration);
        }
        let binding = port.binding().await?;
        verify_binding(&binding, &binding.noise_public).map_err(|_| Error::Authentication)?;
        let (cancellation, _) = watch::channel(false);
        Ok(Arc::new(Self {
            config,
            binding,
            port,
            state: Mutex::new(State {
                peers: HashMap::new(),
                invites: 0,
                events: VecDeque::new(),
            }),
            ended: AtomicBool::new(false),
            cancellation,
        }))
    }
    pub fn binding(&self) -> IdentityBinding {
        self.binding.clone()
    }
    fn active(&self) -> Result<()> {
        if self.ended.load(Ordering::Acquire) {
            Err(Error::Closed)
        } else {
            Ok(())
        }
    }
    async fn cancelled(&self) {
        let mut receiver = self.cancellation.subscribe();
        let ended = *receiver.borrow_and_update();
        if !ended {
            let _ = receiver.changed().await;
        }
    }
    async fn event(&self, e: RuntimeEvent) -> Result<()> {
        self.active()?;
        let mut s = self.state.lock().await;
        if s.events.len() >= MAX_EVENTS {
            self.ended.store(true, Ordering::Release);
            self.cancellation.send_replace(true);
            return Err(Error::Capacity);
        }
        s.events.push_back(e);
        Ok(())
    }
    pub async fn drain_events(&self) -> Vec<RuntimeEvent> {
        self.state.lock().await.events.drain(..).collect()
    }
    pub async fn peers(&self) -> Vec<PeerStatus> {
        let mut s = self.state.lock().await;
        let now = boot().unwrap_or(u64::MAX);
        s.peers
            .values_mut()
            .map(|p| {
                p.offers.retain(|o| !o.deadline.expired(now));
                p.status.available_offers = p.offers.len();
                p.status.clone()
            })
            .collect()
    }
    async fn reserve(&self) -> Result<()> {
        self.active()?;
        let mut s = self.state.lock().await;
        if s.invites >= MAX_INVITES || s.peers.len() + s.invites >= MAX_PEERS {
            return Err(Error::Capacity);
        }
        s.invites += 1;
        Ok(())
    }
    pub async fn create_invite(self: &Arc<Self>) -> Result<Invite> {
        self.reserve().await?;
        let result = self.create_inner().await;
        if result.is_err() {
            let mut s = self.state.lock().await;
            s.invites = s.invites.saturating_sub(1)
        }
        result
    }
    async fn create_inner(self: &Arc<Self>) -> Result<Invite> {
        let random = veyra_identity::random_secret().map_err(|_| Error::Invitation)?;
        let room = *random;
        let expires = unix()?
            .checked_add(INVITE_SECONDS)
            .ok_or(Error::Invitation)?;
        let (mut ws, transport) = self.attach(room, expires, 0, [0; 32]).await?;
        let token = InviteToken {
            version: 1,
            endpoint: self.config.endpoint.clone(),
            room: hex::encode(room),
            expires_unix: expires,
            binding: hex::encode(Frame::IdentityBinding(self.binding.clone()).encode()),
            fingerprint: fingerprint(&self.binding.identity),
        };
        let encoded = format!(
            "veyra1:{}",
            hex::encode(serde_json::to_vec(&token).map_err(|_| Error::Invitation)?)
        );
        let this = self.clone();
        tokio::spawn(async move {
            let result = tokio::select! {_ = this.cancelled()=>Ok(Err(Error::Closed)),result=tokio::time::timeout(Duration::from_secs(INVITE_SECONDS), async {
                let join = receive_signal(&mut ws).await?;
                let remote =
                    validate_signal(&join, room, expires, 1, 0, 1, &this.binding.identity, None)?;
                this.establish(ws, room, expires, true, remote,transport).await
            })=>result};
            {
                let mut s = this.state.lock().await;
                s.invites = s.invites.saturating_sub(1)
            }
            if !matches!(result, Ok(Ok(()))) {
                let _ = this.event(RuntimeEvent::ConnectionFailed).await;
            }
        });
        Ok(Invite {
            token: encoded,
            expires_unix: expires,
        })
    }
    pub async fn join_invite(self: &Arc<Self>, token: &str) -> Result<()> {
        self.active()?;
        if token.len() > 4096 {
            return Err(Error::Invitation);
        }
        let token = token.strip_prefix("veyra1:").ok_or(Error::Invitation)?;
        let bytes = hex::decode(token).map_err(|_| Error::Invitation)?;
        let token: InviteToken = serde_json::from_slice(&bytes).map_err(|_| Error::Invitation)?;
        if token.version != 1
            || token.endpoint != self.config.endpoint
            || !endpoint_valid(&token.endpoint)
            || token.expires_unix <= unix()?
            || token.expires_unix.saturating_sub(unix()?) > INVITE_SECONDS
        {
            return Err(Error::Invitation);
        }
        let room: [u8; 32] = hex::decode(&token.room)
            .map_err(|_| Error::Invitation)?
            .try_into()
            .map_err(|_| Error::Invitation)?;
        let Frame::IdentityBinding(remote) =
            Frame::decode(&hex::decode(&token.binding).map_err(|_| Error::Invitation)?)
                .map_err(|_| Error::Invitation)?
        else {
            return Err(Error::Invitation);
        };
        if fingerprint(&remote.identity) != token.fingerprint
            || remote.identity == self.binding.identity
        {
            return Err(Error::Authentication);
        }
        verify_binding(&remote, &remote.noise_public).map_err(|_| Error::Authentication)?;
        self.reserve().await?;
        let result = self
            .attach(room, token.expires_unix, 1, remote.identity)
            .await;
        let (ws, transport) = match result {
            Ok(ws) => ws,
            Err(e) => {
                let mut s = self.state.lock().await;
                s.invites = s.invites.saturating_sub(1);
                return Err(e);
            }
        };
        let this = self.clone();
        tokio::spawn(async move {
            let result = tokio::select! {_ = this.cancelled()=>Ok(Err(Error::Closed)),result=tokio::time::timeout(
                Duration::from_secs(INVITE_SECONDS),
                this.establish(ws, room, token.expires_unix, false, remote,transport),
            )=>result};
            {
                let mut s = this.state.lock().await;
                s.invites = s.invites.saturating_sub(1)
            }
            if !matches!(result, Ok(Ok(()))) {
                let _ = this.event(RuntimeEvent::ConnectionFailed).await;
            }
        });
        Ok(())
    }
    async fn attach(
        &self,
        room: [u8; 32],
        expires: u64,
        role: u8,
        recipient: Identity,
    ) -> Result<(Socket, TransportConfig)> {
        let mut request = self
            .config
            .endpoint
            .as_str()
            .into_client_request()
            .map_err(|_| Error::Configuration)?;
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            "veyra-signal-v1"
                .parse()
                .map_err(|_| Error::Configuration)?,
        );
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_SIGNAL))
            .max_frame_size(Some(MAX_SIGNAL))
            .write_buffer_size(16 * 1024)
            .max_write_buffer_size(128 * 1024);
        let (mut ws, response) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async_with_config(request, Some(config), false),
        )
        .await
        .map_err(|_| Error::Connection)?
        .map_err(|_| Error::Connection)?;
        if response
            .headers()
            .get("Sec-WebSocket-Protocol")
            .and_then(|h| h.to_str().ok())
            != Some("veyra-signal-v1")
        {
            return Err(Error::Connection);
        }
        let packet = self
            .packet(room, expires, role, role, 0, recipient, String::new())
            .await?;
        send_signal(&mut ws, packet).await?;
        let message = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .map_err(|_| Error::Connection)?
            .ok_or(Error::Connection)?
            .map_err(|_| Error::Connection)?;
        let Message::Text(text) = message else {
            return Err(Error::Connection);
        };
        let attached: Attached = serde_json::from_str(&text).map_err(|_| Error::Connection)?;
        if attached.status != "attached" {
            return Err(Error::Connection);
        }
        let transport = apply_turn_grant(self.config.transport.clone(), attached.turn)?;
        Ok((ws, transport))
    }
    #[allow(clippy::too_many_arguments)]
    async fn packet(
        &self,
        room: [u8; 32],
        expires: u64,
        role: u8,
        operation: u8,
        sequence: u32,
        recipient: Identity,
        payload: String,
    ) -> Result<SignalPacket> {
        let document = SignalDocument {
            room,
            recipient,
            sequence,
            expires_unix: expires,
            role,
            operation,
            payload,
        };
        let signature = self.port.sign_signal(document.clone()).await?;
        Ok(SignalPacket {
            version: 1,
            room: hex::encode(room),
            binding: hex::encode(Frame::IdentityBinding(self.binding.clone()).encode()),
            recipient: hex::encode(recipient),
            sequence,
            expires_unix: expires,
            role,
            operation,
            payload: document.payload,
            signature: hex::encode(signature),
        })
    }
    async fn establish(
        self: &Arc<Self>,
        mut ws: Socket,
        room: [u8; 32],
        expires: u64,
        initiator: bool,
        remote: IdentityBinding,
        transport: TransportConfig,
    ) -> Result<()> {
        self.active()?;
        let peer = Arc::new(
            WebRtcPeer::new(transport)
                .await
                .map_err(|_| Error::Connection)?,
        );
        let exchange = async {
            if initiator {
                let offer = peer.offer().await.map_err(|_| Error::Connection)?;
                let payload = serde_json::to_string(&offer).map_err(|_| Error::Connection)?;
                send_signal(
                    &mut ws,
                    self.packet(room, expires, 0, 2, 1, remote.identity, payload)
                        .await?,
                )
                .await?;
                let answer = receive_signal(&mut ws).await?;
                validate_signal(
                    &answer,
                    room,
                    expires,
                    1,
                    1,
                    3,
                    &self.binding.identity,
                    Some(&remote),
                )?;
                let answer: RTCSessionDescription =
                    serde_json::from_str(&answer.payload).map_err(|_| Error::Connection)?;
                peer.accept_answer(answer)
                    .await
                    .map_err(|_| Error::Connection)?;
            } else {
                let offer = receive_signal(&mut ws).await?;
                validate_signal(
                    &offer,
                    room,
                    expires,
                    0,
                    1,
                    2,
                    &self.binding.identity,
                    Some(&remote),
                )?;
                let offer: RTCSessionDescription =
                    serde_json::from_str(&offer.payload).map_err(|_| Error::Connection)?;
                let answer = peer.answer(offer).await.map_err(|_| Error::Connection)?;
                let payload = serde_json::to_string(&answer).map_err(|_| Error::Connection)?;
                send_signal(
                    &mut ws,
                    self.packet(room, expires, 1, 3, 1, remote.identity, payload)
                        .await?,
                )
                .await?;
            }
            peer.wait_open().await.map_err(|_| Error::Connection)?;
            let (i, r) = if initiator {
                (&self.binding, &remote)
            } else {
                (&remote, &self.binding)
            };
            let prologue = noise_prologue(i, r, &Sha256::digest(room).into());
            let secret = self.port.prepare_transport().await?;
            let mut handshake = NoiseHandshake::from_private(&*secret, initiator, &prologue)
                .map_err(|_| Error::Authentication)?;
            drop(secret);
            if initiator {
                peer.send(&handshake.write().map_err(|_| Error::Authentication)?)
                    .await
                    .map_err(|_| Error::Connection)?;
                handshake
                    .read(&peer.recv().await.map_err(|_| Error::Connection)?)
                    .map_err(|_| Error::Authentication)?;
                peer.send(&handshake.write().map_err(|_| Error::Authentication)?)
                    .await
                    .map_err(|_| Error::Connection)?;
            } else {
                handshake
                    .read(&peer.recv().await.map_err(|_| Error::Connection)?)
                    .map_err(|_| Error::Authentication)?;
                peer.send(&handshake.write().map_err(|_| Error::Authentication)?)
                    .await
                    .map_err(|_| Error::Connection)?;
                handshake
                    .read(&peer.recv().await.map_err(|_| Error::Connection)?)
                    .map_err(|_| Error::Authentication)?;
            }
            let (noise, transcript) = handshake
                .finish(&remote)
                .map_err(|_| Error::Authentication)?;
            let code = verification_code(&transcript, i, r);
            Ok::<_, Error>((noise, code))
        }
        .await;
        let (noise, code) = match exchange {
            Ok(v) => v,
            Err(e) => {
                let _ = peer.close().await;
                return Err(e);
            }
        };
        let id = hex::encode(remote.identity);
        let (tx, rx) = mpsc::channel(8);
        {
            let mut s = self.state.lock().await;
            if s.peers.contains_key(&id) || s.peers.len() >= MAX_PEERS {
                let _ = peer.close().await;
                return Err(Error::Capacity);
            }
            s.peers.insert(
                id.clone(),
                PeerHandle {
                    status: PeerStatus {
                        id: id.clone(),
                        binding_frame: Frame::IdentityBinding(remote.clone()).encode(),
                        fingerprint: fingerprint(&remote.identity),
                        verification_code: code.clone(),
                        verified: false,
                        online: true,
                        route: peer.route().await,
                        available_offers: 0,
                    },
                    offers: VecDeque::new(),
                    command: tx,
                },
            );
        }
        self.event(RuntimeEvent::PeerChanged {
            peer_id: id.clone(),
        })
        .await?;
        // Keep rendezvous alive until BOTH explicit local approvals have crossed
        // the authenticated Noise channel. An unfinished pairing keeps its original deadline.
        let signaling = PairingSignal {
            socket: Some(ws),
            room,
            expires,
            role: if initiator { 0 } else { 1 },
        };
        let this = self.clone();
        tokio::spawn(async move {
            let _ = this
                .peer_loop(id.clone(), remote, peer.clone(), noise, code, rx, signaling)
                .await;
            let _ = peer.close().await;
            {
                let mut s = this.state.lock().await;
                if let Some(p) = s.peers.get_mut(&id) {
                    p.status.online = false;
                    p.status.verified = false;
                    p.offers.clear();
                    p.status.available_offers = 0;
                }
            }
            let _ = this.event(RuntimeEvent::PeerChanged { peer_id: id }).await;
        });
        Ok(())
    }
    pub async fn approve_peer(&self, id: &str, code: &str) -> Result<()> {
        self.active()?;
        let tx = self
            .state
            .lock()
            .await
            .peers
            .get(id)
            .ok_or(Error::Authentication)?
            .command
            .clone();
        let (reply, rx) = oneshot::channel();
        tx.try_send(PeerCommand::Approve {
            code: code.to_owned(),
            reply,
        })
        .map_err(|_| Error::Capacity)?;
        tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .map_err(|_| Error::Connection)?
            .map_err(|_| Error::Connection)?
    }
    pub async fn take_offer(&self, id: &str) -> Result<KeyOffer> {
        self.active()?;
        let now = boot()?;
        let mut s = self.state.lock().await;
        let p = s.peers.get_mut(id).ok_or(Error::NoOffer)?;
        if !p.status.verified || !p.status.online {
            return Err(Error::Authentication);
        }
        while let Some(o) = p.offers.pop_front() {
            if !o.deadline.expired(now) {
                p.status.available_offers = p.offers.len();
                return Ok(o.offer);
            }
        }
        p.status.available_offers = 0;
        Err(Error::NoOffer)
    }
    pub async fn send_envelope(&self, envelope: Envelope) -> Result<()> {
        self.active()?;
        let id = hex::encode(envelope.header.recipient);
        let tx = self
            .state
            .lock()
            .await
            .peers
            .get(&id)
            .ok_or(Error::Connection)?
            .command
            .clone();
        let (reply, rx) = oneshot::channel();
        tx.try_send(PeerCommand::Send {
            envelope: Box::new(envelope),
            reply,
        })
        .map_err(|_| Error::Capacity)?;
        tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .map_err(|_| Error::Connection)?
            .map_err(|_| Error::Connection)?
    }
    pub async fn revoke_peer(&self, id: &str) -> Result<()> {
        if let Some(p) = self.state.lock().await.peers.remove(id) {
            let _ = p.command.try_send(PeerCommand::Close);
        }
        Ok(())
    }
    pub async fn end(&self) {
        self.ended.store(true, Ordering::Release);
        self.cancellation.send_replace(true);
        let mut s = self.state.lock().await;
        for (_, p) in s.peers.drain() {
            let _ = p.command.try_send(PeerCommand::Close);
        }
        s.events.clear();
    }
    async fn send_frame(peer: &WebRtcPeer, noise: &mut NoiseChannel, frame: Frame) -> Result<()> {
        let packet = noise
            .encrypt(&frame.encode())
            .map_err(|_| Error::Authentication)?;
        peer.send(&packet).await.map_err(|_| Error::Connection)
    }
    async fn publish_offer(
        &self,
        peer: &WebRtcPeer,
        noise: &mut NoiseChannel,
        remote: Identity,
        issued: &mut HashMap<Id, Deadline>,
    ) -> Result<()> {
        let offer = self.port.create_offer(remote, OFFER_TTL).await?;
        let deadline = Deadline::new(boot()?, offer.ttl_ms).map_err(|_| Error::Custody)?;
        issued.insert(offer.key_id, deadline);
        Self::send_frame(peer, noise, Frame::KeyOffer(offer)).await
    }
    #[allow(clippy::too_many_arguments)]
    async fn peer_loop(
        &self,
        id: String,
        remote: IdentityBinding,
        peer: Arc<WebRtcPeer>,
        mut noise: NoiseChannel,
        code: String,
        mut commands: mpsc::Receiver<PeerCommand>,
        mut signaling: PairingSignal,
    ) -> Result<()> {
        let remaining = signaling
            .expires
            .saturating_sub(unix()?)
            .saturating_mul(1000)
            .min(300_000) as u32;
        let pairing_deadline = Deadline::new(boot()?, remaining).map_err(|_| Error::Invitation)?;
        let mut local_approved = false;
        let mut remote_approved = false;
        let mut offered = false;
        let mut outbox = Outbox::default();
        let mut received: HashMap<Id, Deadline> = HashMap::new();
        let mut issued: HashMap<Id, Deadline> = HashMap::new();
        let mut next_offer_refresh = 0u64;
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut retries: HashMap<Id, (Envelope, Deadline, u64)> = HashMap::new();
        loop {
            if self.active().is_err() {
                return Err(Error::Closed);
            }
            if !offered && pairing_deadline.expired(boot()?) {
                return Err(Error::Invitation);
            }
            tokio::select! {
             command=commands.recv()=>match command{
              Some(PeerCommand::Close)|None=>return Ok(()),
              Some(PeerCommand::Approve{code:approved,reply})=>{
               if approved!=code||local_approved{let _=reply.send(Err(Error::Authentication));continue}
               local_approved=true;let sent=Self::send_frame(&peer,&mut noise,Frame::Hello(self.binding.runtime_id)).await;
               if sent.is_ok(){let mut s=self.state.lock().await;if let Some(p)=s.peers.get_mut(&id){p.status.verified=true;}}
               let failed=sent.is_err();let _=reply.send(sent);if failed{return Err(Error::Connection)}
              },
              Some(PeerCommand::Send{envelope,reply})=>{
               let envelope=*envelope;
               let h=&envelope.header;
               if !local_approved||!remote_approved||h.sender!=self.binding.identity||h.sender_runtime!=self.binding.runtime_id||h.recipient!=remote.identity||h.recipient_runtime!=remote.runtime_id||verify_envelope(&envelope).is_err(){let _=reply.send(Err(Error::Authentication));continue}
               let now=boot()?;let message=h.message_id;let deadline=Deadline::new(now,h.ttl_ms).map_err(|_|Error::NoOffer)?;
               if outbox.enqueue(envelope.clone(),now).is_err(){let _=reply.send(Err(Error::Capacity));continue}
               let sent=Self::send_frame(&peer,&mut noise,Frame::Message(envelope.clone())).await;
               if sent.is_ok(){outbox.mark_sent(&message).map_err(|_|Error::Connection)?;retries.insert(message,(envelope,deadline,now));self.event(RuntimeEvent::Delivery{message_id:hex::encode(message),status:"sent"}).await?;}
               let failed=sent.is_err();let _=reply.send(sent);if failed{return Err(Error::Connection)}
              }
             },
             packet=peer.recv()=>{
              let packet=packet.map_err(|_|Error::Connection)?;let clear=noise.decrypt(&packet).map_err(|_|Error::Authentication)?;let frame=Frame::decode(&clear).map_err(|_|Error::Authentication)?;
              match frame{
               Frame::Hello(runtime)=>{if runtime!=remote.runtime_id||remote_approved{return Err(Error::Authentication)}remote_approved=true;},
               Frame::Close(_)=>return Ok(()),
               Frame::KeyOffer(offer)=>{
                if !local_approved||!remote_approved||offer.sender!=self.binding.identity||offer.recipient!=remote.identity||offer.sender_runtime!=self.binding.runtime_id||offer.recipient_runtime!=remote.runtime_id||veyra_identity::verify_offer(&offer).is_err(){return Err(Error::Authentication)}
                let deadline=Deadline::new(boot()?,offer.ttl_ms).map_err(|_|Error::NoOffer)?;
                let mut s=self.state.lock().await;let p=s.peers.get_mut(&id).ok_or(Error::Closed)?;
                if p.offers.len()>=16||p.offers.iter().any(|o|o.offer.key_id==offer.key_id){return Err(Error::Capacity)}p.offers.push_back(PublicOffer{offer,deadline});p.status.available_offers=p.offers.len();
               },
               Frame::Message(envelope)=>{
                if !local_approved||!remote_approved||envelope.header.sender!=remote.identity{return Err(Error::Authentication)}
                let message_id=envelope.header.message_id;let key_id=envelope.header.key_id;let ttl=envelope.header.ttl_ms;let first=!received.contains_key(&message_id);
                let original_deadline=issued.get(&key_id).copied();
                if first&&received.len()>=16{return Err(Error::Capacity)}
                let receipt=self.port.accept_envelope(envelope).await?;
                if first{issued.remove(&key_id);received.insert(message_id,original_deadline.unwrap_or(Deadline::new(boot()?,ttl).map_err(|_|Error::Authentication)?));}
                Self::send_frame(&peer,&mut noise,Frame::Received(receipt)).await?;
                if first{let _=self.publish_offer(&peer,&mut noise,remote.identity,&mut issued).await;}
                self.event(RuntimeEvent::PeerChanged{peer_id:id.clone()}).await?;
               },
               Frame::Received(receipt)=>{
                if !local_approved||!remote_approved{return Err(Error::Authentication)}
                if retries.contains_key(&receipt.message_id){outbox.acknowledge(&receipt).map_err(|_|Error::Authentication)?;retries.remove(&receipt.message_id);self.event(RuntimeEvent::Delivery{message_id:hex::encode(receipt.message_id),status:"acknowledged"}).await?;}
               },_=>return Err(Error::Authentication)
              }
             },
             _=tick.tick()=>{
              let now=boot()?;received.retain(|_,d|!d.expired(now));issued.retain(|_,d|!d.expired(now));let expired:Vec<_>=retries.iter().filter(|(_,(_,d,_))|d.expired(now)).map(|(id,_)|*id).collect();
              for message in expired{retries.remove(&message);self.event(RuntimeEvent::Delivery{message_id:hex::encode(message),status:"delivery_unconfirmed"}).await?;}outbox.expire(now);
              for(envelope,deadline,last)in retries.values_mut(){if !deadline.expired(now)&&now.saturating_sub(*last)>=2000{Self::send_frame(&peer,&mut noise,Frame::Message(envelope.clone())).await?;*last=now;}}
              let route=peer.route().await;let mut s=self.state.lock().await;if let Some(p)=s.peers.get_mut(&id){p.status.route=route;p.offers.retain(|o|!o.deadline.expired(now));p.status.available_offers=p.offers.len();}
             }
            }
            if local_approved && remote_approved && !offered {
                if pairing_deadline.expired(boot()?) {
                    return Err(Error::Invitation);
                }
                if let Some(mut ws) = signaling.socket.take() {
                    let paired = self
                        .packet(
                            signaling.room,
                            signaling.expires,
                            signaling.role,
                            5,
                            2,
                            remote.identity,
                            String::new(),
                        )
                        .await?;
                    // Both approvals are authenticated by Noise already. Loss of
                    // this public rendezvous notification cannot change that trust;
                    // disconnect also consumes the honest server's room tombstone.
                    let _ =
                        tokio::time::timeout(Duration::from_secs(1), send_signal(&mut ws, paired))
                            .await;
                    let _ = ws.close(None).await;
                }
                // Four independent one-use offers per peer. No message plaintext is queued.
                for _ in 0..4 {
                    self.publish_offer(&peer, &mut noise, remote.identity, &mut issued)
                        .await?;
                }
                offered = true;
            }
            let now = boot()?;
            if offered && issued.len() < 4 && now >= next_offer_refresh {
                next_offer_refresh = now.saturating_add(5000);
                for _ in issued.len()..4 {
                    if self
                        .publish_offer(&peer, &mut noise, remote.identity, &mut issued)
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    }
}
async fn send_signal(ws: &mut Socket, packet: SignalPacket) -> Result<()> {
    let text = serde_json::to_string(&packet).map_err(|_| Error::Connection)?;
    if text.len() > MAX_SIGNAL {
        return Err(Error::Capacity);
    }
    tokio::time::timeout(Duration::from_secs(10), ws.send(Message::Text(text.into())))
        .await
        .map_err(|_| Error::Connection)?
        .map_err(|_| Error::Connection)
}
async fn receive_signal(ws: &mut Socket) -> Result<SignalPacket> {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(30), ws.next())
            .await
            .map_err(|_| Error::Connection)?
            .ok_or(Error::Connection)?
            .map_err(|_| Error::Connection)?;
        match message {
            Message::Text(text) => {
                if text.len() > MAX_SIGNAL {
                    return Err(Error::Capacity);
                }
                return serde_json::from_str(&text).map_err(|_| Error::Authentication);
            }
            Message::Ping(data) => {
                ws.send(Message::Pong(data))
                    .await
                    .map_err(|_| Error::Connection)?;
            }
            _ => return Err(Error::Connection),
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn validate_signal(
    packet: &SignalPacket,
    room: [u8; 32],
    expires: u64,
    role: u8,
    sequence: u32,
    operation: u8,
    recipient: &Identity,
    expected: Option<&IdentityBinding>,
) -> Result<IdentityBinding> {
    let binding = packet
        .validate(unix()?)
        .map_err(|_| Error::Authentication)?;
    if packet.room != hex::encode(room)
        || packet.expires_unix != expires
        || packet.role != role
        || packet.sequence != sequence
        || packet.operation != operation
        || packet.recipient != hex::encode(recipient)
        || expected.is_some_and(|e| e != &binding)
    {
        return Err(Error::Authentication);
    }
    Ok(binding)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_requires_wss_and_has_no_url_credentials_or_tokens() {
        assert!(endpoint_valid("wss://localhost:8443/v1/rendezvous"));
        for e in [
            "ws://localhost/v1/rendezvous",
            "wss://localhost/v1/rendezvous?token=secret",
            "wss://u:p@localhost/v1/rendezvous",
            "wss://localhost/v1/rendezvous#x",
            "https://localhost/v1/rendezvous",
        ] {
            assert!(!endpoint_valid(e));
        }
    }
    #[test]
    fn typed_vault_signaling_matches_server_canonical_contract() {
        let identity = veyra_identity::RuntimeIdentity::generate().unwrap();
        let now = unix().unwrap();
        let document = SignalDocument {
            room: [7; 32],
            recipient: [0; 32],
            sequence: 0,
            expires_unix: now + 60,
            role: 0,
            operation: 0,
            payload: String::new(),
        };
        let signature = identity.sign_signal(&document).unwrap();
        let mut packet = SignalPacket {
            version: 1,
            room: hex::encode(document.room),
            binding: hex::encode(Frame::IdentityBinding(identity.binding()).encode()),
            recipient: hex::encode(document.recipient),
            sequence: 0,
            expires_unix: document.expires_unix,
            role: 0,
            operation: 0,
            payload: String::new(),
            signature: hex::encode(signature),
        };
        assert_eq!(
            packet.signing_bytes().unwrap(),
            document.signing_bytes(&identity.binding()).unwrap()
        );
        packet.validate(now).unwrap();
        packet.payload = "tampered".into();
        assert!(packet.validate(now).is_err());
    }
}

#[cfg(test)]
mod turn_grant_tests {
    use super::*;
    fn grant(expires_unix: u64) -> TurnGrant {
        TurnGrant {
            urls: vec!["turn:localhost:3478?transport=udp".into()],
            username: "synthetic".into(),
            credential: "synthetic-laboratory-only".into(),
            expires_unix,
        }
    }
    #[test]
    fn automatic_grant_is_bounded_and_expired_grant_is_refused() {
        let cfg = TransportConfig {
            relay_only: true,
            ..Default::default()
        };
        assert!(apply_turn_grant(cfg.clone(), Some(grant(unix().unwrap()))).is_err());
        assert!(apply_turn_grant(cfg.clone(), Some(grant(unix().unwrap() + 301))).is_err());
        let cfg = apply_turn_grant(cfg, Some(grant(unix().unwrap() + 60))).unwrap();
        assert_eq!(cfg.ice_servers.len(), 1);
        assert_eq!(cfg.ice_servers[0].username, "synthetic");
    }
    #[test]
    fn configured_operator_is_not_replaced_by_a_grant() {
        let cfg = TransportConfig {
            ice_servers: vec![veyra_connectivity::IceServer {
                urls: vec!["turn:localhost:3479?transport=udp".into()],
                username: "selected".into(),
                credential: "synthetic".into(),
            }],
            ..Default::default()
        };
        let cfg = apply_turn_grant(cfg, Some(grant(unix().unwrap() + 60))).unwrap();
        assert_eq!(cfg.ice_servers[0].username, "selected");
        assert_eq!(
            cfg.ice_servers[0].urls[0],
            "turn:localhost:3479?transport=udp"
        );
    }
}
