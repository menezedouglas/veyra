//! Ordered, reliable binary WebRTC. Application authentication is the caller's
//! Noise layer; this module deliberately never interprets user messages.
use bytes::BytesMut;
use rtc::{
    ice::mdns::MulticastDnsMode, peer_connection::configuration::setting_engine::SctpMaxMessageSize,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, mpsc, watch};
use veyra_connectivity::{Route, TransportConfig};
pub use webrtc::peer_connection::RTCSessionDescription;

#[cfg(feature = "native-libnice")]
mod native;
#[cfg(feature = "native-libnice")]
mod tls_turn;
use webrtc::{
    data_channel::{DataChannel, DataChannelEvent, RTCDataChannelInit},
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder,
        RTCIceCandidateType, RTCIceGatheringState, RTCIceServer, RTCIceTransportPolicy,
        RTCPeerConnectionState, SettingEngineBuilder,
    },
};

pub const MAX_WIRE_FRAME: usize = 16 * 1024 + 16;
const TIMEOUT: Duration = Duration::from_secs(15);
const LABEL: &str = "veyra-noise-v1";
const PROTOCOL: &str = "veyra/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    Configuration,
    UnsupportedTurnTcpTls,
    Timeout,
    Closed,
    InvalidChannel,
    Oversize,
    Backend,
    TlsCertificate,
    TlsHandshake,
    TlsConnection,
}
impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TransportError {}
type Result<T> = std::result::Result<T, TransportError>;

/// Operator sessions use the configured backend. Manual sessions use the Rust
/// UDP backend on both platforms so their invitation deadline controls initial
/// ICE attempts without relying on libnice's unexposed connection timers.
pub enum WebRtcPeer {
    Rust(RustWebRtcPeer),
    #[cfg(feature = "native-libnice")]
    Native(native::NativeWebRtcPeer),
}

impl WebRtcPeer {
    pub async fn new(config: TransportConfig) -> Result<Self> {
        #[cfg(feature = "native-libnice")]
        {
            native::NativeWebRtcPeer::new(config)
                .await
                .map(Self::Native)
        }
        #[cfg(not(feature = "native-libnice"))]
        {
            RustWebRtcPeer::new(config).await.map(Self::Rust)
        }
    }

    pub async fn new_manual(config: TransportConfig, budget: Duration) -> Result<Self> {
        if !config.ice_servers.is_empty()
            || !config.tls_roots_der.is_empty()
            || config.relay_only
            || budget.is_zero()
            || budget > Duration::from_secs(300)
        {
            return Err(TransportError::Configuration);
        }
        RustWebRtcPeer::new_with_manual_budget(config, Some(budget))
            .await
            .map(Self::Rust)
    }

    pub fn tls_session_count(&self) -> usize {
        match self {
            Self::Rust(peer) => peer.tls_session_count(),
            #[cfg(feature = "native-libnice")]
            Self::Native(peer) => peer.tls_session_count(),
        }
    }

    pub async fn offer(&self) -> Result<RTCSessionDescription> {
        match self {
            Self::Rust(peer) => peer.offer().await,
            #[cfg(feature = "native-libnice")]
            Self::Native(peer) => peer.offer().await,
        }
    }
    pub async fn answer(&self, offer: RTCSessionDescription) -> Result<RTCSessionDescription> {
        match self {
            Self::Rust(peer) => peer.answer(offer).await,
            #[cfg(feature = "native-libnice")]
            Self::Native(peer) => peer.answer(offer).await,
        }
    }
    pub async fn accept_answer(&self, answer: RTCSessionDescription) -> Result<()> {
        match self {
            Self::Rust(peer) => peer.accept_answer(answer).await,
            #[cfg(feature = "native-libnice")]
            Self::Native(peer) => peer.accept_answer(answer).await,
        }
    }
    pub async fn wait_open(&self) -> Result<()> {
        match self {
            Self::Rust(peer) => peer.wait_open().await,
            #[cfg(feature = "native-libnice")]
            Self::Native(peer) => peer.wait_open().await,
        }
    }
    pub async fn wait_open_for(&self, timeout: Duration) -> Result<()> {
        match self {
            Self::Rust(peer) => peer.wait_open_for(timeout).await,
            #[cfg(feature = "native-libnice")]
            Self::Native(peer) => peer.wait_open_for(timeout).await,
        }
    }
    pub async fn send(&self, frame: &[u8]) -> Result<()> {
        match self {
            Self::Rust(peer) => peer.send(frame).await,
            #[cfg(feature = "native-libnice")]
            Self::Native(peer) => peer.send(frame).await,
        }
    }
    pub async fn recv(&self) -> Result<Vec<u8>> {
        match self {
            Self::Rust(peer) => peer.recv().await,
            #[cfg(feature = "native-libnice")]
            Self::Native(peer) => peer.recv().await,
        }
    }
    pub async fn route(&self) -> Route {
        match self {
            Self::Rust(peer) => peer.route().await,
            #[cfg(feature = "native-libnice")]
            Self::Native(peer) => peer.route().await,
        }
    }
    pub async fn close(&self) -> Result<()> {
        match self {
            Self::Rust(peer) => peer.close().await,
            #[cfg(feature = "native-libnice")]
            Self::Native(peer) => peer.close().await,
        }
    }
}

/// Structural validation for the actually selected backend, without making
/// network calls or creating plaintext queues.
pub fn validate_config(config: &TransportConfig) -> Result<()> {
    #[cfg(feature = "native-libnice")]
    let result = config.validate_native();
    #[cfg(not(feature = "native-libnice"))]
    let result = config.validate();
    result.map_err(|error| match error {
        veyra_connectivity::ConfigError::UnsupportedTurnTcpTls => {
            TransportError::UnsupportedTurnTcpTls
        }
        _ => TransportError::Configuration,
    })
}

/// Select only transports implemented by this build from an automatic operator
/// grant. Validate the complete grant first, including URLs we cannot use. An
/// explicit local configuration still goes through `validate_config` unchanged.
/// URLs are never rewritten, so a TLS endpoint cannot become cleartext TURN.
pub fn select_automatic_config(config: TransportConfig) -> Result<TransportConfig> {
    config
        .validate_native()
        .map_err(|_| TransportError::Configuration)?;
    #[cfg(not(feature = "native-libnice"))]
    let config = {
        let mut config = config;
        let advertised_turn = config.ice_servers.iter().any(|server| {
            server
                .urls
                .iter()
                .any(|url| url.starts_with("turn:") || url.starts_with("turns:"))
        });
        for server in &mut config.ice_servers {
            server
                .urls
                .retain(|url| !url.starts_with("turns:") && !url.contains("transport=tcp"));
        }
        config.ice_servers.retain(|server| !server.urls.is_empty());
        if advertised_turn
            && !config
                .ice_servers
                .iter()
                .any(|server| server.urls.iter().any(|url| url.starts_with("turn:")))
        {
            return Err(TransportError::UnsupportedTurnTcpTls);
        }
        config
    };
    validate_config(&config)?;
    Ok(config)
}

struct Handler {
    gathered: watch::Sender<bool>,
    channel: mpsc::Sender<Arc<dyn DataChannel>>,
    accepted: AtomicBool,
    opened: watch::Sender<bool>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            self.gathered.send_replace(true);
        }
    }
    async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
        if self.accepted.swap(true, Ordering::AcqRel)
            || self.channel.try_send(channel.clone()).is_err()
        {
            tokio::spawn(async move {
                let _ = channel.close().await;
            });
        }
    }
    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        if matches!(
            state,
            RTCPeerConnectionState::Disconnected
                | RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Closed
        ) {
            self.opened.send_replace(false);
        }
    }
}

/// Owns exactly one DataChannel and at most eight received frames. Drop alone
/// does not constitute verified secret erasure; call `close` for orderly stop.
pub struct RustWebRtcPeer {
    pc: Arc<dyn PeerConnection>,
    gathered: watch::Receiver<bool>,
    remote_channel: Mutex<mpsc::Receiver<Arc<dyn DataChannel>>>,
    channel: Mutex<Option<Arc<dyn DataChannel>>>,
    incoming: Mutex<mpsc::Receiver<Vec<u8>>>,
    incoming_tx: mpsc::Sender<Vec<u8>>,
    opened_tx: watch::Sender<bool>,
    opened: watch::Receiver<bool>,
    writer: Mutex<()>,
}

impl RustWebRtcPeer {
    pub fn tls_session_count(&self) -> usize {
        0
    }
    pub async fn new(config: TransportConfig) -> Result<Self> {
        Self::new_with_manual_budget(config, None).await
    }
    async fn new_with_manual_budget(
        config: TransportConfig,
        manual_budget: Option<Duration>,
    ) -> Result<Self> {
        config.validate().map_err(|e| match e {
            veyra_connectivity::ConfigError::UnsupportedTurnTcpTls => {
                TransportError::UnsupportedTurnTcpTls
            }
            _ => TransportError::Configuration,
        })?;
        let (gathered_tx, gathered) = watch::channel(false);
        let (channel_tx, remote_channel) = mpsc::channel(1);
        let (incoming_tx, incoming) = mpsc::channel(8);
        let (opened_tx, opened) = watch::channel(false);
        let servers = config
            .ice_servers
            .iter()
            .map(|s| RTCIceServer {
                urls: s.urls.clone(),
                username: s.username.clone(),
                credential: s.credential.clone(),
            })
            .collect();
        let rtc_config = RTCConfigurationBuilder::new()
            .with_ice_servers(servers)
            .with_ice_transport_policy(if config.relay_only {
                RTCIceTransportPolicy::Relay
            } else {
                RTCIceTransportPolicy::All
            })
            .build();
        let mut settings = SettingEngineBuilder::default()
            .with_multicast_dns_mode(MulticastDnsMode::Disabled)
            .with_sctp_max_message_size(SctpMaxMessageSize::Bounded(MAX_WIRE_FRAME as u32))
            .with_sctp_max_receive_buffer_size(256 * 1024)
            .with_discard_local_candidates_during_ice_restart(true);
        if let Some(budget) = manual_budget {
            settings = settings
                .with_ice_timeouts(Some(Duration::from_secs(5)), Some(budget), None)
                .with_ice_connection_attempts(
                    Some(Duration::from_millis(200)),
                    Some((budget.as_millis().div_ceil(200) + 5).min(u16::MAX as u128) as u16),
                );
        }
        let settings = settings.build();
        let pc = PeerConnectionBuilder::new()
            .with_configuration(rtc_config)
            .with_setting_engine(settings)
            .with_handler(Arc::new(Handler {
                gathered: gathered_tx,
                channel: channel_tx,
                accepted: AtomicBool::new(false),
                opened: opened_tx.clone(),
            }))
            .with_udp_addrs(config.bind_addrs.clone())
            .with_data_channel_send_buffer_limit(64 * 1024)
            .build()
            .await
            .map_err(|_| TransportError::Backend)?;
        Ok(Self {
            pc: Arc::new(pc),
            gathered,
            remote_channel: Mutex::new(remote_channel),
            channel: Mutex::new(None),
            incoming: Mutex::new(incoming),
            incoming_tx,
            opened_tx,
            opened,
            writer: Mutex::new(()),
        })
    }

    async fn install_channel(&self, dc: Arc<dyn DataChannel>) -> Result<()> {
        if dc.label().await.map_err(|_| TransportError::Closed)? != LABEL
            || !dc.ordered().await.map_err(|_| TransportError::Closed)?
            || dc
                .max_packet_life_time()
                .await
                .map_err(|_| TransportError::Closed)?
                .is_some()
            || dc
                .max_retransmits()
                .await
                .map_err(|_| TransportError::Closed)?
                .is_some()
            || dc.protocol().await.map_err(|_| TransportError::Closed)? != PROTOCOL
        {
            let _ = dc.close().await;
            return Err(TransportError::InvalidChannel);
        }
        let mut channel = self.channel.lock().await;
        if channel.is_some() {
            return Err(TransportError::InvalidChannel);
        }
        *channel = Some(dc.clone());
        drop(channel);
        let tx = self.incoming_tx.clone();
        let opened = self.opened_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = dc.poll().await {
                match event {
                    DataChannelEvent::OnOpen => {
                        opened.send_replace(true);
                    }
                    DataChannelEvent::OnMessage(message) => {
                        if message.is_string
                            || message.data.is_empty()
                            || message.data.len() > MAX_WIRE_FRAME
                        {
                            break;
                        }
                        if !matches!(
                            tokio::time::timeout(
                                Duration::from_secs(5),
                                tx.send(message.data.to_vec())
                            )
                            .await,
                            Ok(Ok(()))
                        ) {
                            break;
                        }
                    }
                    DataChannelEvent::OnClose | DataChannelEvent::OnError => break,
                    _ => {}
                }
            }
            opened.send_replace(false);
            let _ = dc.close().await;
        });
        Ok(())
    }

    async fn gather(&self) -> Result<RTCSessionDescription> {
        let mut gathered = self.gathered.clone();
        tokio::time::timeout(TIMEOUT, async {
            while !*gathered.borrow_and_update() {
                gathered
                    .changed()
                    .await
                    .map_err(|_| TransportError::Closed)?;
            }
            self.pc
                .local_description()
                .await
                .ok_or(TransportError::Backend)
        })
        .await
        .map_err(|_| TransportError::Timeout)?
    }

    pub async fn offer(&self) -> Result<RTCSessionDescription> {
        let dc = self
            .pc
            .create_data_channel(
                LABEL,
                Some(RTCDataChannelInit {
                    protocol: PROTOCOL.into(),
                    ..Default::default()
                }),
            )
            .await
            .map_err(|_| TransportError::Backend)?;
        self.install_channel(dc).await?;
        let offer = self
            .pc
            .create_offer(None)
            .await
            .map_err(|_| TransportError::Backend)?;
        self.pc
            .set_local_description(offer)
            .await
            .map_err(|_| TransportError::Backend)?;
        self.gather().await
    }

    pub async fn answer(&self, offer: RTCSessionDescription) -> Result<RTCSessionDescription> {
        if offer.sdp.len() > 64 * 1024 {
            return Err(TransportError::Oversize);
        }
        self.pc
            .set_remote_description(offer)
            .await
            .map_err(|_| TransportError::Backend)?;
        let answer = self
            .pc
            .create_answer(None)
            .await
            .map_err(|_| TransportError::Backend)?;
        self.pc
            .set_local_description(answer)
            .await
            .map_err(|_| TransportError::Backend)?;
        self.gather().await
    }

    pub async fn accept_answer(&self, answer: RTCSessionDescription) -> Result<()> {
        if answer.sdp.len() > 64 * 1024 {
            return Err(TransportError::Oversize);
        }
        self.pc
            .set_remote_description(answer)
            .await
            .map_err(|_| TransportError::Backend)
    }

    pub async fn wait_open(&self) -> Result<()> {
        self.wait_open_for(TIMEOUT).await
    }

    /// A manually exchanged answer can take longer than normal signaling.
    /// The caller must cap this budget by the original invitation deadline.
    pub async fn wait_open_for(&self, timeout: Duration) -> Result<()> {
        tokio::time::timeout(timeout, async {
            if self.channel.lock().await.is_none() {
                let dc = self
                    .remote_channel
                    .lock()
                    .await
                    .recv()
                    .await
                    .ok_or(TransportError::Closed)?;
                self.install_channel(dc).await?;
            }
            let mut opened = self.opened.clone();
            while !*opened.borrow_and_update() {
                opened.changed().await.map_err(|_| TransportError::Closed)?;
            }
            Ok(())
        })
        .await
        .map_err(|_| TransportError::Timeout)?
    }

    pub async fn send(&self, frame: &[u8]) -> Result<()> {
        if frame.is_empty() || frame.len() > MAX_WIRE_FRAME {
            return Err(TransportError::Oversize);
        }
        let _writer = self.writer.lock().await;
        let dc = self
            .channel
            .lock()
            .await
            .clone()
            .ok_or(TransportError::Closed)?;
        tokio::time::timeout(TIMEOUT, dc.send(BytesMut::from(frame)))
            .await
            .map_err(|_| TransportError::Timeout)?
            .map_err(|_| TransportError::Closed)
    }

    pub async fn recv(&self) -> Result<Vec<u8>> {
        let mut opened = self.opened.clone();
        tokio::time::timeout(TIMEOUT, async {
            if !*opened.borrow_and_update() {
                return Err(TransportError::Closed);
            }
            let mut incoming = self.incoming.lock().await;
            tokio::select! {
                frame = incoming.recv() => frame.ok_or(TransportError::Closed),
                _ = opened.changed() => Err(TransportError::Closed),
            }
        })
        .await
        .map_err(|_| TransportError::Timeout)?
    }

    /// Classify only the selected candidate pair. Gathering a relay candidate
    /// alone must never be reported to the interface as a relayed connection.
    pub async fn route(&self) -> Route {
        let Some(sctp) = self.pc.sctp().await else {
            return Route::Unknown;
        };
        let Ok(Some(pair)) = sctp
            .transport()
            .ice_transport()
            .get_selected_candidate_pair()
            .await
        else {
            return Route::Unknown;
        };
        if pair.local().typ == RTCIceCandidateType::Relay
            || pair.remote().typ == RTCIceCandidateType::Relay
        {
            Route::Relay
        } else {
            Route::Direct
        }
    }

    pub async fn close(&self) -> Result<()> {
        self.opened_tx.send_replace(false);
        self.pc.close().await.map_err(|_| TransportError::Backend)
    }
}
