//! Signed manual signaling for a direct WebRTC channel. No signaling socket,
//! discovery service, STUN server or TURN relay is created in this mode.
use super::*;

/// Hex encoding keeps a complete direct exchange within the 32 KiB IPC frame.
pub const MAX_DIRECT_TOKEN: usize = 16_000;
const MAX_SEEN: usize = 128;

pub(super) struct PendingOffer {
    pub(super) peer: Arc<WebRtcPeer>,
    expires: u64,
    deadline: Deadline,
}

#[derive(Default)]
pub(super) struct DirectState {
    pub(super) offers: HashMap<[u8; 32], PendingOffer>,
    pub(super) seen: HashMap<[u8; 32], Deadline>,
}

pub(super) struct DirectToken {
    packet: SignalPacket,
    remote: IdentityBinding,
    room: [u8; 32],
    description: RTCSessionDescription,
}

pub(super) fn parse_token(token: &str, now: u64) -> Result<DirectToken> {
    if token.len() > MAX_DIRECT_TOKEN {
        return Err(Error::Invitation);
    }
    let encoded = token.strip_prefix("veyra2:").ok_or(Error::Invitation)?;
    let packet: SignalPacket =
        serde_json::from_slice(&hex::decode(encoded).map_err(|_| Error::Invitation)?)
            .map_err(|_| Error::Invitation)?;
    let remote = packet.validate(now).map_err(|_| Error::Authentication)?;
    let room: [u8; 32] = hex::decode(&packet.room)
        .map_err(|_| Error::Invitation)?
        .try_into()
        .map_err(|_| Error::Invitation)?;
    let recipient: [u8; 32] = hex::decode(&packet.recipient)
        .map_err(|_| Error::Invitation)?
        .try_into()
        .map_err(|_| Error::Invitation)?;
    let offer = packet.role == 0 && packet.operation == 2 && recipient == [0; 32];
    let answer = packet.role == 1 && packet.operation == 3 && recipient != [0; 32];
    if packet.sequence != 1 || !(offer || answer) {
        return Err(Error::Invitation);
    }
    let description: RTCSessionDescription =
        serde_json::from_str(&packet.payload).map_err(|_| Error::Invitation)?;
    let mut candidates = 0;
    for line in description
        .sdp
        .lines()
        .filter(|line| line.starts_with("a=candidate:"))
    {
        candidates += 1;
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        if fields.get(6) != Some(&"typ")
            || fields.get(7) != Some(&"host")
            || !fields
                .get(2)
                .is_some_and(|transport| transport.eq_ignore_ascii_case("udp"))
            || !fields
                .get(4)
                .is_some_and(|address| address.parse::<std::net::IpAddr>().is_ok())
            || !fields
                .get(5)
                .is_some_and(|port| port.parse::<u16>().is_ok_and(|port| port > 0))
        {
            return Err(Error::Invitation);
        }
    }
    if description.sdp_type.to_string() != if offer { "offer" } else { "answer" }
        || description.sdp.is_empty()
        || candidates == 0
    {
        return Err(Error::Invitation);
    }
    Ok(DirectToken {
        packet,
        remote,
        room,
        description,
    })
}

fn encode_token(packet: &SignalPacket) -> Result<String> {
    let token = format!(
        "veyra2:{}",
        hex::encode(serde_json::to_vec(packet).map_err(|_| Error::Invitation)?)
    );
    if token.len() > MAX_DIRECT_TOKEN {
        return Err(Error::Capacity);
    }
    Ok(token)
}

fn deadline_for(expires: u64) -> Result<Deadline> {
    let remaining = expires
        .saturating_sub(unix()?)
        .saturating_mul(1000)
        .min(INVITE_SECONDS * 1000);
    Deadline::new(boot()?, remaining as u32).map_err(|_| Error::Invitation)
}

impl Runtime {
    pub(super) async fn create_direct_invite(self: &Arc<Self>) -> Result<Invite> {
        self.active()?;
        let room = *veyra_identity::random_secret().map_err(|_| Error::Invitation)?;
        let expires = unix()?
            .checked_add(INVITE_SECONDS)
            .ok_or(Error::Invitation)?;
        let deadline = deadline_for(expires)?;
        let peer = Arc::new(
            WebRtcPeer::new_manual(
                self.config.transport.clone(),
                Duration::from_millis(deadline.remaining_ms(boot()?)),
            )
            .await
            .map_err(|_| Error::Connection)?,
        );
        let built = async {
            let offer = peer.offer().await.map_err(|_| Error::Connection)?;
            let packet = self
                .packet(
                    room,
                    expires,
                    0,
                    2,
                    1,
                    [0; 32],
                    serde_json::to_string(&offer).map_err(|_| Error::Invitation)?,
                )
                .await?;
            let token = encode_token(&packet)?;
            self.active()?;
            if deadline.expired(boot()?) {
                return Err(Error::Invitation);
            }
            let mut direct = self.direct.lock().await;
            if direct.offers.len() >= MAX_INVITES || direct.offers.contains_key(&room) {
                return Err(Error::Capacity);
            }
            direct.offers.insert(
                room,
                PendingOffer {
                    peer: peer.clone(),
                    expires,
                    deadline,
                },
            );
            Ok(Invite {
                token,
                expires_unix: expires,
            })
        }
        .await;
        if built.is_err() {
            let _ = peer.close().await;
        } else {
            let this = self.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                loop {
                    let expired = tokio::select! {
                        _ = this.cancelled() => true,
                        _ = tick.tick() => deadline.expired(boot().unwrap_or(u64::MAX)),
                    };
                    let pending = {
                        let mut direct = this.direct.lock().await;
                        if !direct.offers.contains_key(&room) {
                            break;
                        }
                        if expired {
                            direct.offers.remove(&room)
                        } else {
                            None
                        }
                    };
                    if let Some(pending) = pending {
                        let _ = pending.peer.close().await;
                        let mut state = this.state.lock().await;
                        state.invites = state.invites.saturating_sub(1);
                        break;
                    }
                }
            });
        }
        built
    }

    pub(super) async fn exchange_direct_invite(
        self: &Arc<Self>,
        token: &str,
    ) -> Result<Option<Invite>> {
        self.active()?;
        if !self.config.endpoint.is_empty() {
            return Err(Error::Configuration);
        }
        // Signature, source binding, recipient, schema and lifetime are checked
        // before reserving resources or changing a one-use pending offer.
        let DirectToken {
            packet,
            remote,
            room,
            description,
        } = parse_token(token, unix()?)?;
        if remote.identity == self.binding.identity {
            return Err(Error::Authentication);
        }
        if packet.role == 1 {
            if packet.recipient != hex::encode(self.binding.identity) {
                return Err(Error::Authentication);
            }
            let pending = {
                let mut direct = self.direct.lock().await;
                let pending = direct.offers.get(&room).ok_or(Error::Invitation)?;
                if pending.expires != packet.expires_unix || pending.deadline.expired(boot()?) {
                    return Err(Error::Invitation);
                }
                direct.offers.remove(&room).ok_or(Error::Invitation)?
            };
            if pending.peer.accept_answer(description).await.is_err() {
                let _ = pending.peer.close().await;
                let mut state = self.state.lock().await;
                state.invites = state.invites.saturating_sub(1);
                return Err(Error::Connection);
            }
            self.start_direct_finish(
                pending.peer,
                room,
                pending.expires,
                true,
                remote,
                pending.deadline,
            );
            return Ok(None);
        }

        let deadline = deadline_for(packet.expires_unix)?;
        self.reserve().await?;
        let result = self
            .answer_direct_offer(room, packet.expires_unix, deadline, remote, description)
            .await;
        if result.is_err() {
            let mut state = self.state.lock().await;
            state.invites = state.invites.saturating_sub(1);
        }
        result.map(Some)
    }

    async fn answer_direct_offer(
        self: &Arc<Self>,
        room: [u8; 32],
        expires: u64,
        deadline: Deadline,
        remote: IdentityBinding,
        description: RTCSessionDescription,
    ) -> Result<Invite> {
        {
            let mut direct = self.direct.lock().await;
            let now = boot()?;
            direct.seen.retain(|_, deadline| !deadline.expired(now));
            if direct.seen.contains_key(&room) {
                return Err(Error::Invitation);
            }
            if direct.seen.len() >= MAX_SEEN {
                return Err(Error::Capacity);
            }
            direct.seen.insert(room, deadline);
        }
        let peer = Arc::new(
            WebRtcPeer::new_manual(
                self.config.transport.clone(),
                Duration::from_millis(deadline.remaining_ms(boot()?)),
            )
            .await
            .map_err(|_| Error::Connection)?,
        );
        let result = async {
            let answer = peer
                .answer(description)
                .await
                .map_err(|_| Error::Connection)?;
            let packet = self
                .packet(
                    room,
                    expires,
                    1,
                    3,
                    1,
                    remote.identity,
                    serde_json::to_string(&answer).map_err(|_| Error::Invitation)?,
                )
                .await?;
            let token = encode_token(&packet)?;
            self.active()?;
            if deadline.expired(boot()?) {
                return Err(Error::Invitation);
            }
            Ok(Invite {
                token,
                expires_unix: expires,
            })
        }
        .await;
        match result {
            Ok(invite) => {
                // Return the answer now: the other participant needs this token
                // before their channel can open or the Noise exchange can run.
                self.start_direct_finish(peer, room, expires, false, remote, deadline);
                Ok(invite)
            }
            Err(error) => {
                let _ = peer.close().await;
                Err(error)
            }
        }
    }

    fn start_direct_finish(
        self: &Arc<Self>,
        peer: Arc<WebRtcPeer>,
        room: [u8; 32],
        expires: u64,
        initiator: bool,
        remote: IdentityBinding,
        deadline: Deadline,
    ) {
        let this = self.clone();
        tokio::spawn(async move {
            let result = {
                let operation = this.finish_establish(
                    peer.clone(),
                    None,
                    room,
                    expires,
                    initiator,
                    remote,
                    deadline,
                );
                tokio::pin!(operation);
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                loop {
                    tokio::select! {
                        _ = this.cancelled() => break Err(Error::Closed),
                        result = &mut operation => break result,
                        _ = tick.tick() => {
                            if deadline.expired(boot().unwrap_or(u64::MAX)) {
                                break Err(Error::Invitation);
                            }
                        }
                    }
                }
            };
            {
                let mut state = this.state.lock().await;
                state.invites = state.invites.saturating_sub(1);
            }
            if result.is_err() {
                let _ = peer.close().await;
                let _ = this.event(RuntimeEvent::ConnectionFailed).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_offer(candidate: &str) -> SignalPacket {
        let identity = veyra_identity::RuntimeIdentity::generate().unwrap();
        let payload = serde_json::json!({
            "type": "offer",
            "sdp": format!("v=0\r\n{candidate}\r\n"),
        })
        .to_string();
        let document = SignalDocument {
            room: [9; 32],
            recipient: [0; 32],
            sequence: 1,
            expires_unix: unix().unwrap() + 60,
            role: 0,
            operation: 2,
            payload,
        };
        SignalPacket {
            version: 1,
            room: hex::encode(document.room),
            binding: hex::encode(Frame::IdentityBinding(identity.binding()).encode()),
            recipient: hex::encode(document.recipient),
            sequence: document.sequence,
            expires_unix: document.expires_unix,
            role: document.role,
            operation: document.operation,
            payload: document.payload.clone(),
            signature: hex::encode(identity.sign_signal(&document).unwrap()),
        }
    }

    #[test]
    fn signed_direct_invitation_selects_no_operator_and_rejects_tampering() {
        let mut packet = signed_offer("a=candidate:1 1 udp 2130706431 192.168.1.2 50000 typ host");
        let token = encode_token(&packet).unwrap();
        assert_eq!(invitation_endpoint(&token).unwrap(), "");
        assert!(parse_token(&token, packet.expires_unix).is_err());
        packet.payload = packet.payload.replace("50000", "50001");
        assert!(invitation_endpoint(&encode_token(&packet).unwrap()).is_err());
        assert!(
            parse_token(
                &format!("veyra2:{}", "0".repeat(MAX_DIRECT_TOKEN)),
                unix().unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn direct_candidates_require_literal_host_addresses_and_valid_ports() {
        for candidate in [
            "",
            "a=candidate:1 1 udp 2130706431 operator.example 50000 typ host",
            "a=candidate:1 1 udp 2130706431 machine.local 50000 typ host",
            "a=candidate:1 1 udp 2130706431 192.168.1.2 0 typ host",
            "a=candidate:1 1 udp 2130706431 192.168.1.2 65536 typ host",
            "a=candidate:1 1 udp 2130706431 192.168.1.2 50000 typ srflx",
            "a=candidate:1 1 udp 2130706431 192.168.1.2 50000 typ relay",
            "a=candidate:1 1 unknown 2130706431 192.168.1.2 50000 typ host",
        ] {
            let packet = signed_offer(candidate);
            assert!(
                invitation_endpoint(&encode_token(&packet).unwrap()).is_err(),
                "{candidate}"
            );
        }
        let packet = signed_offer("a=candidate:1 1 udp 2130706431 2001:db8::1 50000 typ host");
        assert!(invitation_endpoint(&encode_token(&packet).unwrap()).is_ok());
    }
}
