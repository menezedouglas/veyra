//! Existing libdatachannel + libnice. Build with the checked-in CMake toolchain.
//! The binding requires a logger facade; all log levels are compiled out.
use super::tls_turn::TlsTurnBridge;
use super::{
    LABEL, MAX_WIRE_FRAME, PROTOCOL, RTCSessionDescription, Result, TIMEOUT, TransportError,
};
use datachannel::{
    DataChannelHandler, DataChannelInfo, DataChannelInit, GatheringState, PeerConnectionHandler,
    RtcConfig, RtcDataChannel, RtcPeerConnection, SdpType, SessionDescription, TransportPolicy,
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

#[derive(Clone)]
struct ChannelEvents {
    frames: mpsc::Sender<Vec<u8>>,
    opened: watch::Sender<bool>,
    failed: Arc<AtomicBool>,
}
impl DataChannelHandler for ChannelEvents {
    fn on_open(&mut self) {
        if !self.failed.load(Ordering::Acquire) {
            self.opened.send_replace(true);
        }
    }
    fn on_closed(&mut self) {
        self.failed.store(true, Ordering::Release);
        self.opened.send_replace(false);
    }
    fn on_error(&mut self, _error: &str) {
        self.failed.store(true, Ordering::Release);
        self.opened.send_replace(false);
    }
    fn on_message(&mut self, message: &[u8]) {
        // Callbacks cannot await. Overflow invalidates the entire channel; no
        // application ACK may be produced after this failure.
        if message.is_empty()
            || message.len() > MAX_WIRE_FRAME
            || self.frames.try_send(message.to_vec()).is_err()
        {
            self.failed.store(true, Ordering::Release);
            self.opened.send_replace(false);
        }
    }
}
struct ConnectionEvents {
    gathered: watch::Sender<bool>,
    channel: mpsc::Sender<Box<RtcDataChannel<ChannelEvents>>>,
    data: ChannelEvents,
    accepted: bool,
}
impl PeerConnectionHandler for ConnectionEvents {
    type DCH = ChannelEvents;
    fn data_channel_handler(&mut self, info: DataChannelInfo) -> Self::DCH {
        if info.label != LABEL
            || info.protocol.as_deref() != Some(PROTOCOL)
            || info.reliability.unordered
            || info.reliability.unreliable
        {
            self.data.failed.store(true, Ordering::Release);
        }
        self.data.clone()
    }
    fn on_gathering_state_change(&mut self, state: GatheringState) {
        if state == GatheringState::Complete {
            self.gathered.send_replace(true);
        }
    }
    fn on_connection_state_change(&mut self, state: datachannel::ConnectionState) {
        if matches!(
            state,
            datachannel::ConnectionState::Disconnected
                | datachannel::ConnectionState::Failed
                | datachannel::ConnectionState::Closed
        ) {
            self.data.failed.store(true, Ordering::Release);
            self.data.opened.send_replace(false);
        }
    }
    fn on_data_channel(&mut self, dc: Box<RtcDataChannel<Self::DCH>>) {
        if self.accepted || self.channel.try_send(dc).is_err() {
            self.data.failed.store(true, Ordering::Release);
        }
        self.accepted = true;
    }
}

pub struct NativeWebRtcPeer {
    pc: Mutex<Option<Box<RtcPeerConnection<ConnectionEvents>>>>,
    channel: Mutex<Option<Box<RtcDataChannel<ChannelEvents>>>>,
    remote_channel: Mutex<mpsc::Receiver<Box<RtcDataChannel<ChannelEvents>>>>,
    incoming: Mutex<mpsc::Receiver<Vec<u8>>>,
    gathered: watch::Receiver<bool>,
    opened: watch::Receiver<bool>,
    data: ChannelEvents,
    bridges: std::sync::Mutex<Vec<TlsTurnBridge>>,
}

impl NativeWebRtcPeer {
    pub async fn new(config: TransportConfig) -> Result<Self> {
        super::validate_config(&config)?;
        if config.ice_servers.len() > 4
            || config
                .ice_servers
                .iter()
                .any(|s| s.urls.len() > 4 || s.username.len() > 128 || s.credential.len() > 256)
        {
            return Err(TransportError::Configuration);
        }
        let mut urls = Vec::new();
        let mut bridges = Vec::new();
        let mut unavailable_tls = None;
        if config
            .ice_servers
            .iter()
            .map(|s| s.urls.len())
            .sum::<usize>()
            > 4
        {
            return Err(TransportError::Configuration);
        }
        for server in &config.ice_servers {
            for endpoint in &server.urls {
                if endpoint.len() > 512
                    || endpoint.contains('\0')
                    || endpoint.bytes().any(|b| b.is_ascii_whitespace())
                {
                    return Err(TransportError::Configuration);
                }
                if endpoint.starts_with("turn:") || endpoint.starts_with("turns:") {
                    if server.username.is_empty()
                        || server.credential.is_empty()
                        || server
                            .username
                            .bytes()
                            .any(|b| !b.is_ascii_alphanumeric() && !b":_-".contains(&b))
                        || server
                            .credential
                            .bytes()
                            .any(|b| !b.is_ascii_alphanumeric() && !b"+/=_-".contains(&b))
                    {
                        return Err(TransportError::Configuration);
                    }
                    let (scheme, location) = endpoint
                        .split_once(':')
                        .ok_or(TransportError::Configuration)?;
                    let username: String =
                        url::form_urlencoded::byte_serialize(server.username.as_bytes()).collect();
                    let credential: String =
                        url::form_urlencoded::byte_serialize(server.credential.as_bytes())
                            .collect();
                    if scheme == "turns" {
                        // libnice's TURN_TLS enum does not establish RFC TLS.
                        let authority = location.strip_suffix("?transport=tcp").unwrap_or(location);
                        let parsed = url::Url::parse(&format!("https://{authority}"))
                            .map_err(|_| TransportError::Configuration)?;
                        if !parsed.username().is_empty()
                            || parsed.password().is_some()
                            || parsed.path() != "/"
                            || parsed.query().is_some()
                            || parsed.fragment().is_some()
                        {
                            return Err(TransportError::Configuration);
                        }
                        let host = match parsed.host().ok_or(TransportError::Configuration)? {
                            url::Host::Domain(host) => host.to_owned(),
                            url::Host::Ipv4(host) => host.to_string(),
                            url::Host::Ipv6(host) => host.to_string(),
                        };
                        let bridge = TlsTurnBridge::start(
                            host,
                            parsed.port().unwrap_or(5349),
                            &config.tls_roots_der,
                        )
                        .await;
                        let bridge = match bridge {
                            Ok(bridge) => bridge,
                            Err(
                                error @ (TransportError::TlsConnection | TransportError::Timeout),
                            ) => {
                                unavailable_tls = Some(error);
                                continue;
                            }
                            // Certificate/hostname/handshake failure is never
                            // converted to TCP or accepted as another identity.
                            Err(error) => return Err(error),
                        };
                        urls.push(format!(
                            "turn:{username}:{credential}@127.0.0.1:{}?transport=tcp",
                            bridge.local_port
                        ));
                        bridges.push(bridge);
                    } else {
                        urls.push(format!("turn:{username}:{credential}@{location}"));
                    }
                } else if endpoint.starts_with("stun:") {
                    urls.push(endpoint.clone());
                } else {
                    return Err(TransportError::Configuration);
                }
            }
        }
        if config.relay_only
            && !urls
                .iter()
                .any(|u| u.starts_with("turn:") || u.starts_with("turns:"))
        {
            return Err(unavailable_tls.unwrap_or(TransportError::Configuration));
        }
        let rtc_config = RtcConfig::new(&urls)
            .max_message_size(MAX_WIRE_FRAME as i32)
            .disable_auto_negotiation()
            .ice_transport_policy(if config.relay_only {
                TransportPolicy::Relay
            } else {
                TransportPolicy::All
            });
        let (gathered_tx, gathered) = watch::channel(false);
        let (opened_tx, opened) = watch::channel(false);
        let (channel_tx, remote_channel) = mpsc::channel(1);
        let (frames, incoming) = mpsc::channel(8);
        let data = ChannelEvents {
            frames,
            opened: opened_tx,
            failed: Arc::new(AtomicBool::new(false)),
        };
        let pc = RtcPeerConnection::new(
            &rtc_config,
            ConnectionEvents {
                gathered: gathered_tx,
                channel: channel_tx,
                data: data.clone(),
                accepted: false,
            },
        )
        .map_err(|_| TransportError::Backend)?;
        Ok(Self {
            pc: Mutex::new(Some(pc)),
            channel: Mutex::new(None),
            remote_channel: Mutex::new(remote_channel),
            incoming: Mutex::new(incoming),
            gathered,
            opened,
            data,
            bridges: std::sync::Mutex::new(bridges),
        })
    }
    pub fn tls_session_count(&self) -> usize {
        self.bridges
            .lock()
            .map(|bridges| bridges.iter().map(TlsTurnBridge::verified_sessions).sum())
            .unwrap_or(0)
    }
    async fn gathered_description(&self) -> Result<RTCSessionDescription> {
        let mut gathered = self.gathered.clone();
        tokio::time::timeout(TIMEOUT, async {
            while !*gathered.borrow_and_update() {
                gathered
                    .changed()
                    .await
                    .map_err(|_| TransportError::Closed)?;
            }
            let guard = self.pc.lock().await;
            let description = guard
                .as_ref()
                .ok_or(TransportError::Closed)?
                .local_description()
                .ok_or(TransportError::Backend)?;
            let json = serde_json::to_value(description).map_err(|_| TransportError::Backend)?;
            serde_json::from_value(json).map_err(|_| TransportError::Backend)
        })
        .await
        .map_err(|_| TransportError::Timeout)?
    }
    pub async fn offer(&self) -> Result<RTCSessionDescription> {
        {
            let mut guard = self.pc.lock().await;
            let pc = guard.as_mut().ok_or(TransportError::Closed)?;
            let channel = pc
                .create_data_channel_ex(
                    LABEL,
                    self.data.clone(),
                    &DataChannelInit::default().protocol(PROTOCOL),
                )
                .map_err(|_| TransportError::Backend)?;
            *self.channel.lock().await = Some(channel);
            pc.set_local_description(SdpType::Offer)
                .map_err(|_| TransportError::Backend)?;
        }
        self.gathered_description().await
    }
    pub async fn answer(&self, offer: RTCSessionDescription) -> Result<RTCSessionDescription> {
        if offer.sdp.len() > 64 * 1024 {
            return Err(TransportError::Oversize);
        }
        let description: SessionDescription = serde_json::from_value(
            serde_json::to_value(offer).map_err(|_| TransportError::Backend)?,
        )
        .map_err(|_| TransportError::Backend)?;
        {
            let mut guard = self.pc.lock().await;
            let pc = guard.as_mut().ok_or(TransportError::Closed)?;
            pc.set_remote_description(&description)
                .map_err(|_| TransportError::Backend)?;
            pc.set_local_description(SdpType::Answer)
                .map_err(|_| TransportError::Backend)?;
        }
        self.gathered_description().await
    }
    pub async fn accept_answer(&self, answer: RTCSessionDescription) -> Result<()> {
        if answer.sdp.len() > 64 * 1024 {
            return Err(TransportError::Oversize);
        }
        let description: SessionDescription = serde_json::from_value(
            serde_json::to_value(answer).map_err(|_| TransportError::Backend)?,
        )
        .map_err(|_| TransportError::Backend)?;
        self.pc
            .lock()
            .await
            .as_mut()
            .ok_or(TransportError::Closed)?
            .set_remote_description(&description)
            .map_err(|_| TransportError::Backend)
    }
    pub async fn wait_open(&self) -> Result<()> {
        self.wait_open_for(TIMEOUT).await
    }
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
                *self.channel.lock().await = Some(dc);
            }
            let mut opened = self.opened.clone();
            while !*opened.borrow_and_update() {
                if self.data.failed.load(Ordering::Acquire) {
                    return Err(TransportError::Closed);
                }
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
        if self.data.failed.load(Ordering::Acquire) {
            return Err(TransportError::Closed);
        }
        tokio::time::timeout(TIMEOUT, async {
            let mut guard = self.channel.lock().await;
            let dc = guard.as_mut().ok_or(TransportError::Closed)?;
            while dc.buffered_amount() > 64 * 1024 - frame.len() {
                if self.data.failed.load(Ordering::Acquire) {
                    return Err(TransportError::Closed);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            dc.send(frame).map_err(|_| TransportError::Closed)
        })
        .await
        .map_err(|_| TransportError::Timeout)?
    }
    pub async fn recv(&self) -> Result<Vec<u8>> {
        let mut opened = self.opened.clone();
        tokio::time::timeout(TIMEOUT, async {
            if self.data.failed.load(Ordering::Acquire) || !*opened.borrow_and_update() { return Err(TransportError::Closed); }
            let mut incoming = self.incoming.lock().await;
            tokio::select! {
                frame = incoming.recv() => { if self.data.failed.load(Ordering::Acquire) { return Err(TransportError::Closed); } frame.ok_or(TransportError::Closed) },
                _ = opened.changed() => Err(TransportError::Closed),
            }
        }).await.map_err(|_| TransportError::Timeout)?
    }
    pub async fn route(&self) -> Route {
        let guard = self.pc.lock().await;
        let Some(pc) = guard.as_ref() else {
            return Route::Unknown;
        };
        let Some(pair) = pc.selected_candidate_pair() else {
            return Route::Unknown;
        };
        if pair.local.contains(" typ relay") || pair.remote.contains(" typ relay") {
            Route::Relay
        } else if pair.local.contains(" typ ") && pair.remote.contains(" typ ") {
            Route::Direct
        } else {
            Route::Unknown
        }
    }
    pub async fn close(&self) -> Result<()> {
        self.data.failed.store(true, Ordering::Release);
        self.data.opened.send_replace(false);
        self.channel.lock().await.take();
        self.pc.lock().await.take();
        self.bridges
            .lock()
            .map_err(|_| TransportError::Backend)?
            .clear();
        Ok(())
    }
}
