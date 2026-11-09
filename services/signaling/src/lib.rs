//! Ephemeral rendezvous. Public identities and SDP are visible to the server.
//! Signatures authenticate their source; they do not encrypt signaling.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, Semaphore, mpsc};
use veyra_protocol::{Frame, IdentityBinding};
pub mod turn;
pub const MAX_SIGNAL: usize = 64 * 1024;
pub const MAX_ROOMS: usize = 128;
pub const MAX_CONNECTIONS: usize = 64;

/// Closed schema. Operations: 0 create, 1 join, 2 offer, 3 answer,
/// 4 ICE, 5 paired, 6 close. The payload is a bounded string, never a JSON tree.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignalPacket {
    pub version: u8,
    pub room: String,
    /// Hex of canonical veyra-protocol IDENTITY_BINDING frame.
    pub binding: String,
    pub recipient: String,
    pub sequence: u32,
    pub expires_unix: u64,
    pub role: u8,
    pub operation: u8,
    pub payload: String,
    pub signature: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Malformed,
    Authentication,
    Expired,
    Capacity,
    State,
    Replay,
}
fn unhex<const N: usize>(input: &str) -> Result<[u8; N], Rejection> {
    if input.len() != N * 2 {
        return Err(Rejection::Malformed);
    }
    let mut output = [0; N];
    hex::decode_to_slice(input, &mut output).map_err(|_| Rejection::Malformed)?;
    Ok(output)
}
impl SignalPacket {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, Rejection> {
        if self.version != 1
            || self.role > 1
            || self.operation > 6
            || self.payload.len() > 48 * 1024
            || self.binding.len() > 1024
        {
            return Err(Rejection::Malformed);
        }
        let room = unhex::<32>(&self.room)?;
        let recipient = unhex::<32>(&self.recipient)?;
        let binding = hex::decode(&self.binding).map_err(|_| Rejection::Malformed)?;
        let mut enc = minicbor::Encoder::new(Vec::new());
        enc.array(10)
            .unwrap()
            .str("Veyra/signaling/v1")
            .unwrap()
            .u8(self.version)
            .unwrap()
            .bytes(&room)
            .unwrap()
            .bytes(&binding)
            .unwrap()
            .bytes(&recipient)
            .unwrap()
            .u32(self.sequence)
            .unwrap()
            .u64(self.expires_unix)
            .unwrap()
            .u8(self.role)
            .unwrap()
            .u8(self.operation)
            .unwrap()
            .str(&self.payload)
            .unwrap();
        Ok(enc.into_writer())
    }
    pub fn validate(&self, now_unix: u64) -> Result<IdentityBinding, Rejection> {
        if self.expires_unix <= now_unix || self.expires_unix.saturating_sub(now_unix) > 300 {
            return Err(Rejection::Expired);
        }
        let bytes = self.signing_bytes()?;
        let signature = unhex::<64>(&self.signature)?;
        let binding_bytes = hex::decode(&self.binding).map_err(|_| Rejection::Malformed)?;
        let Frame::IdentityBinding(binding) =
            Frame::decode(&binding_bytes).map_err(|_| Rejection::Malformed)?
        else {
            return Err(Rejection::Malformed);
        };
        veyra_identity::verify_binding(&binding, &binding.noise_public)
            .map_err(|_| Rejection::Authentication)?;
        veyra_identity::verify(&binding.identity, &bytes, &signature)
            .map_err(|_| Rejection::Authentication)?;
        Ok(binding)
    }
}
struct Peer {
    identity: [u8; 32],
    runtime: [u8; 16],
    last_sequence: u32,
    output: mpsc::Sender<SignalPacket>,
}
struct Room {
    deadline: Instant,
    expiry_unix: u64,
    peers: [Option<Peer>; 2],
    paired: [bool; 2],
    tombstone: bool,
}
#[derive(Clone)]
pub struct Rendezvous {
    rooms: Arc<Mutex<HashMap<[u8; 32], Room>>>,
    pub connections: Arc<Semaphore>,
    turn: Option<Arc<turn::TurnIssuer>>,
}
impl Default for Rendezvous {
    fn default() -> Self {
        Self {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            turn: None,
        }
    }
}
impl Rendezvous {
    pub fn with_turn(mut self, issuer: turn::TurnIssuer) -> Self {
        self.turn = Some(Arc::new(issuer));
        self
    }
    /// Called once, after authenticated attach reserves a bounded room.
    pub fn turn_credentials(&self) -> Result<Option<turn::TurnGrant>, &'static str> {
        self.turn.as_ref().map(|issuer| issuer.issue()).transpose()
    }
    pub async fn cleanup(&self) {
        self.rooms
            .lock()
            .await
            .retain(|_, room| room.deadline > Instant::now());
    }
    pub async fn attach(
        &self,
        packet: &SignalPacket,
        output: mpsc::Sender<SignalPacket>,
    ) -> Result<(), Rejection> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Rejection::Expired)?
            .as_secs();
        let binding = packet.validate(now)?;
        if packet.sequence != 0 || !packet.payload.is_empty() || packet.operation != packet.role {
            return Err(Rejection::State);
        }
        let key = unhex::<32>(&packet.room)?;
        let recipient = unhex::<32>(&packet.recipient)?;
        let peer = Peer {
            identity: binding.identity,
            runtime: binding.runtime_id,
            last_sequence: 0,
            output,
        };
        let mut rooms = self.rooms.lock().await;
        rooms.retain(|_, room| room.deadline > Instant::now());
        if packet.role == 0 {
            if recipient != [0; 32] {
                return Err(Rejection::State);
            }
            if rooms.contains_key(&key) {
                return Err(Rejection::Replay);
            }
            if rooms.len() >= MAX_ROOMS {
                return Err(Rejection::Capacity);
            }
            rooms.insert(
                key,
                Room {
                    deadline: Instant::now() + Duration::from_secs(packet.expires_unix - now),
                    expiry_unix: packet.expires_unix,
                    peers: [Some(peer), None],
                    paired: [false; 2],
                    tombstone: false,
                },
            );
        } else {
            let room = rooms.get_mut(&key).ok_or(Rejection::State)?;
            if room.tombstone || room.peers[1].is_some() || packet.expires_unix != room.expiry_unix
            {
                return Err(Rejection::State);
            }
            let owner = room.peers[0].as_ref().ok_or(Rejection::State)?;
            if recipient != owner.identity || binding.identity == owner.identity {
                return Err(Rejection::Authentication);
            }
            owner
                .output
                .try_send(packet.clone())
                .map_err(|_| Rejection::Capacity)?;
            room.peers[1] = Some(peer);
        }
        Ok(())
    }
    pub async fn forward(
        &self,
        packet: SignalPacket,
        pinned: &IdentityBinding,
        room_key: &[u8; 32],
        role: u8,
    ) -> Result<(), Rejection> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Rejection::Expired)?
            .as_secs();
        let binding = packet.validate(now)?;
        if &unhex::<32>(&packet.room)? != room_key
            || role > 1
            || packet.role != role
            || binding != *pinned
        {
            return Err(Rejection::Authentication);
        }
        if packet.operation < 2
            || (packet.operation == 2 && role != 0)
            || (packet.operation == 3 && role != 1)
        {
            return Err(Rejection::State);
        }
        let mut rooms = self.rooms.lock().await;
        let room = rooms.get_mut(room_key).ok_or(Rejection::State)?;
        if room.tombstone
            || room.deadline <= Instant::now()
            || packet.expires_unix != room.expiry_unix
        {
            return Err(Rejection::Expired);
        }
        let sender = room.peers[role as usize].as_ref().ok_or(Rejection::State)?;
        if sender.identity != pinned.identity
            || sender.runtime != pinned.runtime_id
            || sender.last_sequence.checked_add(1) != Some(packet.sequence)
        {
            return Err(Rejection::Replay);
        }
        let recipient = room.peers[(1 - role) as usize]
            .as_ref()
            .ok_or(Rejection::State)?;
        if recipient.identity != unhex::<32>(&packet.recipient)? {
            return Err(Rejection::Authentication);
        }
        let operation = packet.operation;
        let sequence = packet.sequence;
        recipient
            .output
            .try_send(packet)
            .map_err(|_| Rejection::Capacity)?;
        room.peers[role as usize].as_mut().unwrap().last_sequence = sequence;
        if operation == 5 {
            room.paired[role as usize] = true;
        }
        if operation == 6 || room.paired == [true; 2] {
            room.peers = [None, None];
            room.tombstone = true;
        }
        Ok(())
    }
    /// Keep only a replay tombstone until the ORIGINAL deadline after disconnect.
    pub async fn detach(&self, room_key: &[u8; 32]) {
        if let Some(room) = self.rooms.lock().await.get_mut(room_key) {
            room.peers = [None, None];
            room.tombstone = true;
        }
    }
}
pub fn room_key(packet: &SignalPacket) -> Result<[u8; 32], Rejection> {
    unhex(&packet.room)
}
#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    fn packet() -> SignalPacket {
        let signing = SigningKey::from_bytes(&[7; 32]);
        let mut binding = IdentityBinding {
            identity: signing.verifying_key().to_bytes(),
            noise_public: [8; 32],
            runtime_id: [9; 16],
            signature: [0; 64],
        };
        binding.signature = signing.sign(&binding.signing_bytes()).to_bytes();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut packet = SignalPacket {
            version: 1,
            room: hex::encode([3; 32]),
            binding: hex::encode(Frame::IdentityBinding(binding).encode()),
            recipient: hex::encode([0; 32]),
            sequence: 0,
            expires_unix: now + 60,
            role: 0,
            operation: 0,
            payload: String::new(),
            signature: String::new(),
        };
        packet.signature = hex::encode(signing.sign(&packet.signing_bytes().unwrap()).to_bytes());
        packet
    }
    #[test]
    fn signature_covers_role_sequence_recipient_and_payload() {
        let original = packet();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(original.validate(now).is_ok());
        let mut changed = original.clone();
        changed.payload = "replacement".into();
        assert_eq!(changed.validate(now), Err(Rejection::Authentication));
        let mut changed = original.clone();
        changed.sequence = 1;
        assert_eq!(changed.validate(now), Err(Rejection::Authentication));
        let mut changed = original.clone();
        changed.role = 1;
        assert_eq!(changed.validate(now), Err(Rejection::Authentication));
    }
    #[tokio::test]
    async fn consumed_room_cannot_be_recreated_before_expiry() {
        let server = Rendezvous::default();
        let packet = packet();
        let (tx, _) = mpsc::channel(8);
        server.attach(&packet, tx.clone()).await.unwrap();
        server.detach(&room_key(&packet).unwrap()).await;
        assert_eq!(server.attach(&packet, tx).await, Err(Rejection::Replay));
    }
}
