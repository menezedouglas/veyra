//! Network child authority. There is deliberately no opening, sealing or trust
//! approval command in the custody request schema the child can send upward.
use serde::{Deserialize, Serialize};
use veyra_connectivity::{Route, TransportConfig};
use veyra_ipc::{VaultRequest, VaultResponse};
use veyra_protocol::SignalDocument;
use zeroize::{Zeroize, Zeroizing};

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkVaultRequest {
    BindTransport { noise_public: [u8; 32] },
    SignSignal { document: SignalDocument },
    CreateOffer { peer: [u8; 32], ttl_ms: u32 },
    AcceptEnvelope { frame: Vec<u8> },
}
impl NetworkVaultRequest {
    pub fn into_vault(self) -> VaultRequest {
        match self {
            Self::BindTransport { noise_public } => VaultRequest::BindTransport { noise_public },
            Self::SignSignal { document } => VaultRequest::SignSignal { document },
            Self::CreateOffer { peer, ttl_ms } => VaultRequest::CreateOffer { peer, ttl_ms },
            Self::AcceptEnvelope { frame } => VaultRequest::AcceptEnvelope { frame },
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Start {
        endpoint: String,
        transport: TransportConfig,
    },
    CreateInvite {},
    JoinInvite {
        token: String,
    },
    Peers {},
    Events {},
    ApprovePeer {
        peer_id: String,
        code: String,
    },
    TakeOffer {
        peer_id: String,
    },
    SendEnvelope {
        frame: Vec<u8>,
    },
    RevokePeer {
        peer_id: String,
    },
    End {},
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Peer {
    pub id: String,
    pub binding_frame: Vec<u8>,
    pub fingerprint: String,
    pub verification_code: String,
    pub verified: bool,
    pub online: bool,
    pub route: Route,
    pub available_offers: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Ok {},
    Invite {
        token: String,
        expires_in_ms: u64,
    },
    Peers {
        peers: Vec<Peer>,
    },
    Events {
        deliveries: Vec<DeliveryUpdate>,
        connection_failed: bool,
    },
    Frame {
        frame: Vec<u8>,
    },
    Refused {
        reason: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryUpdate {
    pub message_id: String,
    pub state: String,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Wire {
    Command {
        id: u64,
        command: Command,
    },
    Reply {
        id: u64,
        response: Reply,
    },
    Custody {
        id: u64,
        request: NetworkVaultRequest,
    },
    CustodyReply {
        id: u64,
        response: VaultResponse,
    },
}
pub fn wipe_response(response: &mut VaultResponse) {
    if let VaultResponse::Opened { body, .. } = response {
        body.zeroize();
    }
}
pub fn encode(mut wire: Wire) -> std::io::Result<Zeroizing<Vec<u8>>> {
    let result = veyra_ipc::encode(&wire);
    if let Wire::CustodyReply { response, .. } = &mut wire {
        wipe_response(response)
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_child_cannot_request_open_trust_or_plaintext() {
        for operation in [
            "prepare_transport",
            "open_once",
            "verify_peer",
            "seal",
            "end",
            "discard",
            "revoke",
        ] {
            let text = format!(r#"{{"operation":"{operation}"}}"#);
            assert!(serde_json::from_str::<NetworkVaultRequest>(&text).is_err());
        }
    }
}
