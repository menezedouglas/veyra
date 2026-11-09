//! Four real product processes, with manually exchanged SDP invitations.
//! No WSS operator, STUN server, TURN relay, or TLS fixture participates.
#![cfg(any(target_os = "linux", windows))]

use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use veyra_connectivity::{Route, TransportConfig};
use veyra_daemon::{
    DaemonClient, VaultClient,
    wire::{Command, Peer, Reply},
};
use veyra_ipc::{PendingMessage, VaultRequest as Request, VaultResponse as Response};

const TEXT: &str = "mensagem sintética direta sem servidor";

struct Person {
    vault: Arc<Mutex<VaultClient>>,
    network: DaemonClient,
}

fn binary(variable: &str, compiled: &str) -> PathBuf {
    let result = std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(compiled));
    assert!(
        result.is_absolute(),
        "product binaries require absolute paths"
    );
    result
}

fn assert_ok(reply: Reply) {
    match reply {
        Reply::Ok {} => {}
        Reply::Refused { reason } => panic!("operation refused: {reason}"),
        _ => panic!("unexpected product reply"),
    }
}

fn invitation(reply: Reply) -> String {
    match reply {
        Reply::Invite {
            token,
            expires_in_ms,
        } => {
            assert!(token.starts_with("veyra2:"));
            assert!(expires_in_ms > 0 && expires_in_ms <= 300_000);
            token
        }
        Reply::Refused { reason } => panic!("direct invitation refused: {reason}"),
        _ => panic!("expected a direct invitation or answer"),
    }
}

impl Person {
    async fn start() -> Self {
        let vault = Arc::new(Mutex::new(
            VaultClient::spawn(&binary(
                "VEYRA_TEST_VAULT_BIN",
                env!("CARGO_BIN_EXE_veyra-vault"),
            ))
            .await
            .unwrap(),
        ));
        let mut network = DaemonClient::spawn(
            &binary("VEYRA_TEST_DAEMON_BIN", env!("CARGO_BIN_EXE_veyra-daemon")),
            vault.clone(),
        )
        .await
        .unwrap();
        // Only loopback candidates are gathered. Both ICE server lists and the
        // signaling endpoint are empty; the network needs no other process.
        assert_ok(
            network
                .request(Command::Start {
                    endpoint: String::new(),
                    transport: TransportConfig {
                        ice_servers: Vec::new(),
                        relay_only: false,
                        bind_addrs: vec!["127.0.0.1:0".into()],
                        ..Default::default()
                    },
                })
                .await
                .unwrap(),
        );
        Self { vault, network }
    }

    async fn peers(&mut self) -> Vec<Peer> {
        match self.network.request(Command::Peers {}).await.unwrap() {
            Reply::Peers { peers } => peers,
            Reply::Refused { reason } => panic!("peers refused: {reason}"),
            _ => panic!("unexpected peer response"),
        }
    }

    async fn end(self) {
        self.network.end().await;
        let vault = tokio::time::timeout(Duration::from_secs(2), async move {
            let mut shared = self.vault;
            loop {
                match Arc::try_unwrap(shared) {
                    Ok(vault) => return vault.into_inner(),
                    Err(vault) => shared = vault,
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("network task did not release its restricted vault port");
        vault.end().await;
    }
}

async fn paired(a: &mut Person, b: &mut Person) -> (Peer, Peer) {
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            let mut left = a.peers().await;
            let mut right = b.peers().await;
            if left.len() == 1
                && right.len() == 1
                && left[0].online
                && right[0].online
                && left[0].verification_code == right[0].verification_code
            {
                return (left.remove(0), right.remove(0));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("direct pairing timed out")
}

async fn approve(person: &mut Person, peer: &Peer) {
    assert!(matches!(
        person
            .vault
            .lock()
            .await
            .request(&Request::VerifyPeer {
                binding: peer.binding_frame.clone(),
                approved: true,
            })
            .await
            .unwrap(),
        Response::Ok
    ));
    assert_ok(
        person
            .network
            .request(Command::ApprovePeer {
                peer_id: peer.id.clone(),
                code: peer.verification_code.clone(),
            })
            .await
            .unwrap(),
    );
}

async fn wait_offers(person: &mut Person) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if person
                .peers()
                .await
                .iter()
                .any(|peer| peer.verified && peer.available_offers > 0)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("verified direct peer did not provide disposable keys");
}

async fn send(person: &mut Person, peer: &str) -> [u8; 16] {
    let Reply::Frame { frame: offer } = person
        .network
        .request(Command::TakeOffer {
            peer_id: peer.into(),
        })
        .await
        .unwrap()
    else {
        panic!("disposable key unavailable")
    };
    let Response::Frame { frame } = person
        .vault
        .lock()
        .await
        .request(&Request::Seal {
            offer,
            text: TEXT.into(),
        })
        .await
        .unwrap()
    else {
        panic!("vault refused to seal a verified message")
    };
    assert!(
        !frame
            .windows(TEXT.len())
            .any(|window| window == TEXT.as_bytes())
    );
    let veyra_protocol::Frame::Message(envelope) = veyra_protocol::Frame::decode(&frame).unwrap()
    else {
        panic!("expected HPKE envelope")
    };
    let id = envelope.header.message_id;
    assert_ok(
        person
            .network
            .request(Command::SendEnvelope { frame })
            .await
            .unwrap(),
    );
    id
}

async fn pending(person: &Person) -> Vec<PendingMessage> {
    match person
        .vault
        .lock()
        .await
        .request(&Request::ListPending {})
        .await
        .unwrap()
    {
        Response::Pending { messages } => messages,
        _ => panic!("expected pending message metadata"),
    }
}

async fn wait_pending(person: &Person, id: [u8; 16]) {
    let id = veyra_daemon::hex(&id);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if pending(person).await.iter().any(|message| message.id == id) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("sealed message did not reach recipient vault");
}

async fn consume(person: &Person, id: [u8; 16]) {
    assert!(matches!(
        person
            .vault
            .lock()
            .await
            .request(&Request::OpenOnce {
                message_id: id,
                confirmed: false,
            })
            .await
            .unwrap(),
        Response::Refused { .. }
    ));
    let Response::Opened {
        body,
        display_for_ms,
    } = person
        .vault
        .lock()
        .await
        .request(&Request::OpenOnce {
            message_id: id,
            confirmed: true,
        })
        .await
        .unwrap()
    else {
        panic!("first confirmed opening refused")
    };
    assert_eq!(body, TEXT);
    assert!(display_for_ms > 0 && display_for_ms <= 30_000);
    assert!(matches!(
        person
            .vault
            .lock()
            .await
            .request(&Request::OpenOnce {
                message_id: id,
                confirmed: true,
            })
            .await
            .unwrap(),
        Response::Refused { .. }
    ));
    assert!(pending(person).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_product_session_without_external_services() {
    let (mut a, mut b) = (Person::start().await, Person::start().await);
    let offer = invitation(a.network.request(Command::CreateInvite {}).await.unwrap());
    assert!(matches!(
        a.network
            .request(Command::JoinInvite {
                token: offer.clone(),
            })
            .await
            .unwrap(),
        Reply::Refused { .. }
    ));
    let mut corrupted = offer.clone().into_bytes();
    let last = corrupted.last_mut().unwrap();
    *last = if *last == b'0' { b'1' } else { b'0' };
    assert!(matches!(
        b.network
            .request(Command::JoinInvite {
                token: String::from_utf8(corrupted).unwrap(),
            })
            .await
            .unwrap(),
        Reply::Refused { .. }
    ));
    let answer = invitation(
        b.network
            .request(Command::JoinInvite { token: offer })
            .await
            .unwrap(),
    );
    // Manual exchange must tolerate a person taking longer than the former
    // 15-second opening wait and 30-second ICE failure deadline to answer.
    tokio::time::sleep(Duration::from_secs(35)).await;
    assert_ok(
        a.network
            .request(Command::JoinInvite {
                token: answer.clone(),
            })
            .await
            .unwrap(),
    );
    assert!(matches!(
        a.network
            .request(Command::JoinInvite { token: answer })
            .await
            .unwrap(),
        Reply::Refused { .. }
    ));

    let (ap, bp) = paired(&mut a, &mut b).await;
    assert!(ap.online && bp.online);
    assert!(!ap.verified && !bp.verified);
    assert_eq!(ap.verification_code, bp.verification_code);
    for (person, peer) in [(&mut a, &ap), (&mut b, &bp)] {
        assert!(matches!(
            person
                .network
                .request(Command::TakeOffer {
                    peer_id: peer.id.clone(),
                })
                .await
                .unwrap(),
            Reply::Refused { .. }
        ));
        approve(person, peer).await;
    }
    wait_offers(&mut a).await;
    wait_offers(&mut b).await;
    let id = send(&mut a, &ap.id).await;
    wait_pending(&b, id).await;
    consume(&b, id).await;
    let id = send(&mut b, &bp.id).await;
    wait_pending(&a, id).await;
    consume(&a, id).await;
    for person in [&mut a, &mut b] {
        let peer = person.peers().await.remove(0);
        assert!(peer.online && peer.verified);
        assert_eq!(peer.route, Route::Direct);
    }

    let id = send(&mut a, &ap.id).await;
    wait_pending(&b, id).await;
    let veyra_protocol::Frame::IdentityBinding(binding) =
        veyra_protocol::Frame::decode(&bp.binding_frame).unwrap()
    else {
        panic!("expected signed identity binding")
    };
    assert!(matches!(
        b.vault
            .lock()
            .await
            .request(&Request::Revoke {
                peer: binding.identity,
            })
            .await
            .unwrap(),
        Response::Ok
    ));
    assert_ok(
        b.network
            .request(Command::RevokePeer {
                peer_id: bp.id.clone(),
            })
            .await
            .unwrap(),
    );
    assert!(pending(&b).await.is_empty());
    assert!(matches!(
        b.vault
            .lock()
            .await
            .request(&Request::OpenOnce {
                message_id: id,
                confirmed: true,
            })
            .await
            .unwrap(),
        Response::Refused { .. }
    ));

    // The surviving identity may pair again once its former channel is
    // offline. The new channel has a new transcript and requires approval.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if a.peers().await.iter().any(|peer| !peer.online) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("revoked remote connection did not become offline");
    let offer = invitation(a.network.request(Command::CreateInvite {}).await.unwrap());
    let answer = invitation(
        b.network
            .request(Command::JoinInvite { token: offer })
            .await
            .unwrap(),
    );
    assert_ok(
        a.network
            .request(Command::JoinInvite { token: answer })
            .await
            .unwrap(),
    );
    let (new_ap, new_bp) = paired(&mut a, &mut b).await;
    assert_eq!(new_ap.id, ap.id);
    assert_eq!(new_bp.id, bp.id);
    assert!(!new_ap.verified && !new_bp.verified);
    assert_eq!(new_ap.verification_code, new_bp.verification_code);
    assert_ne!(new_ap.verification_code, ap.verification_code);
    assert!(matches!(
        a.network
            .request(Command::ApprovePeer {
                peer_id: new_ap.id.clone(),
                code: ap.verification_code,
            })
            .await
            .unwrap(),
        Reply::Refused { .. }
    ));
    approve(&mut a, &new_ap).await;
    approve(&mut b, &new_bp).await;
    wait_offers(&mut a).await;
    wait_offers(&mut b).await;
    let id = send(&mut a, &new_ap.id).await;
    wait_pending(&b, id).await;
    consume(&b, id).await;
    a.end().await;
    b.end().await;
    println!(
        "Direct SDP exchange + two daemons + two isolated vaults: verified, bidirectional HPKE, one opening, revocation; no external services"
    );
}
