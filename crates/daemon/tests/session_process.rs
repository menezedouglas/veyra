//! Requires the TLS operator laboratory. See tests/integration/run_product.sh.
//! Four child processes: two network daemons, two isolated vaults. The test
//! coordinator represents local UI actions and never hands its vault pipe to a daemon.
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use veyra_connectivity::{Route, TransportConfig};
use veyra_daemon::{
    DaemonClient, VaultClient,
    wire::{Command, Peer, Reply},
};
use veyra_ipc::{VaultRequest as Request, VaultResponse as Response};

fn client_binary(variable: &str, compiled: &str) -> PathBuf {
    let path = std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(compiled));
    assert!(
        path.is_absolute(),
        "client executable must be an absolute path"
    );
    path
}
struct Person {
    vault: Arc<Mutex<VaultClient>>,
    network: DaemonClient,
}
impl Person {
    async fn start(endpoint: &str, relay: bool) -> Self {
        let vault_path = client_binary("VEYRA_TEST_VAULT_BIN", env!("CARGO_BIN_EXE_veyra-vault"));
        let daemon_path =
            client_binary("VEYRA_TEST_DAEMON_BIN", env!("CARGO_BIN_EXE_veyra-daemon"));
        let vault = Arc::new(Mutex::new(VaultClient::spawn(&vault_path).await.unwrap()));
        let mut network = DaemonClient::spawn(&daemon_path, vault.clone())
            .await
            .unwrap();
        let transport = if relay {
            // No TURN credentials in the client fixture. The WSS operator must
            // issue a real short-lived REST credential grant after attachment.
            TransportConfig {
                ice_servers: Vec::new(),
                relay_only: true,
                bind_addrs: vec!["0.0.0.0:0".into()],
                ..Default::default()
            }
        } else {
            TransportConfig {
                bind_addrs: vec!["0.0.0.0:0".into()],
                ..Default::default()
            }
        };
        let reply = network
            .request(Command::Start {
                endpoint: endpoint.to_owned(),
                transport,
            })
            .await
            .unwrap();
        assert_ok(reply);
        Self { vault, network }
    }
    async fn peers(&mut self) -> Vec<Peer> {
        match self.network.request(Command::Peers {}).await.unwrap() {
            Reply::Peers { peers } => peers,
            Reply::Refused { reason } => panic!("peers refused: {reason}"),
            _ => panic!("unexpected peer reply"),
        }
    }
    async fn end(self) {
        self.network.end().await;
        let vault = tokio::time::timeout(Duration::from_secs(1), async move {
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
        .expect("network custody task did not release the vault client");
        vault.end().await;
    }
}
fn assert_ok(reply: Reply) {
    match reply {
        Reply::Ok {} => {}
        Reply::Refused { reason } => panic!("operation refused: {reason}"),
        _ => panic!("unexpected reply"),
    }
}
async fn wait_peers(a: &mut Person, b: &mut Person) -> (Peer, Peer) {
    for _ in 0..400 {
        let mut ap = a.peers().await;
        let mut bp = b.peers().await;
        if ap.len() == 1 && bp.len() == 1 {
            return (ap.remove(0), bp.remove(0));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("pairing timeout")
}
async fn known_routes(a: &mut Person, b: &mut Person) -> (Route, Route) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let a_peer = a.peers().await.remove(0);
            let b_peer = b.peers().await.remove(0);
            assert!(
                a_peer.online && b_peer.online,
                "transport disconnected before route telemetry became available"
            );
            if a_peer.route != Route::Unknown && b_peer.route != Route::Unknown {
                return (a_peer.route, b_peer.route);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("selected ICE route remained unknown for three seconds")
}
async fn wait_offers(a: &mut Person) {
    for _ in 0..100 {
        if a.peers().await.iter().any(|p| p.available_offers > 0) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("offer timeout")
}
async fn send(a: &mut Person, peer: &str) -> String {
    let Reply::Frame { frame: offer } = a
        .network
        .request(Command::TakeOffer {
            peer_id: peer.into(),
        })
        .await
        .unwrap()
    else {
        panic!("key offer unavailable")
    };
    let Response::Frame { frame } = a
        .vault
        .lock()
        .await
        .request(&Request::Seal {
            offer,
            text: "conteúdo sintético de quatro processos".into(),
        })
        .await
        .unwrap()
    else {
        panic!("vault seal refused")
    };
    let veyra_protocol::Frame::Message(envelope) = veyra_protocol::Frame::decode(&frame).unwrap()
    else {
        panic!("envelope")
    };
    assert!(
        !frame
            .windows(b"conte\xc3\xbado".len())
            .any(|w| w == b"conte\xc3\xbado")
    );
    let id = veyra_daemon::hex(&envelope.header.message_id);
    assert_ok(
        a.network
            .request(Command::SendEnvelope { frame })
            .await
            .unwrap(),
    );
    id
}
fn message_id(s: &str) -> [u8; 16] {
    let mut out = [0; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}
async fn pending(a: &Person) -> Vec<veyra_ipc::PendingMessage> {
    let Response::Pending { messages } = a
        .vault
        .lock()
        .await
        .request(&Request::ListPending {})
        .await
        .unwrap()
    else {
        panic!("pending")
    };
    messages
}
async fn wait_pending(a: &Person, id: &str) {
    for _ in 0..100 {
        if pending(a).await.iter().any(|m| m.id == id) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("message acceptance timeout")
}
async fn consume(a: &Person, id: &str) {
    let id = message_id(id);
    assert!(matches!(
        a.vault
            .lock()
            .await
            .request(&Request::OpenOnce {
                message_id: id,
                confirmed: false
            })
            .await
            .unwrap(),
        Response::Refused { .. }
    ));
    let Response::Opened {
        body,
        display_for_ms,
    } = a
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
        panic!("first opening refused")
    };
    assert_eq!(body, "conteúdo sintético de quatro processos");
    assert!(display_for_ms > 0 && display_for_ms <= 30_000);
    assert!(matches!(
        a.vault
            .lock()
            .await
            .request(&Request::OpenOnce {
                message_id: id,
                confirmed: true
            })
            .await
            .unwrap(),
        Response::Refused { .. }
    ));
    assert!(pending(a).await.is_empty());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a real WSS operator with trusted TLS certificate; run tests/integration/run_product.sh"]
async fn product_session_bidirectional() {
    if std::env::var_os("VEYRA_TEST_REQUIRE_EMPTY_PATH").is_some() {
        assert!(
            std::env::var_os("PATH").is_some_and(|path| path.is_empty()),
            "client PATH must be empty"
        );
        assert!(std::env::var_os("VEYRA_TEST_DAEMON_BIN").is_some());
        assert!(std::env::var_os("VEYRA_TEST_VAULT_BIN").is_some());
        println!("Core clients use explicit external paths and inherit an empty PATH");
    }
    let endpoint =
        std::env::var("VEYRA_TEST_WSS_ENDPOINT").expect("public laboratory WSS endpoint");
    let relay = std::env::var("VEYRA_TEST_RELAY").ok().as_deref() == Some("1");
    let (mut a, mut b) = (
        Person::start(&endpoint, relay).await,
        Person::start(&endpoint, relay).await,
    );
    let Reply::Invite {
        token,
        expires_in_ms,
    } = a.network.request(Command::CreateInvite {}).await.unwrap()
    else {
        panic!("invite creation refused")
    };
    assert!(expires_in_ms > 0 && expires_in_ms <= 300_000);
    assert_ok(
        b.network
            .request(Command::JoinInvite { token })
            .await
            .unwrap(),
    );
    let (ap, bp) = wait_peers(&mut a, &mut b).await;
    assert!(!ap.verified && !bp.verified);
    assert_eq!(ap.verification_code, bp.verification_code);
    assert!(matches!(
        a.network
            .request(Command::TakeOffer {
                peer_id: ap.id.clone()
            })
            .await
            .unwrap(),
        Reply::Refused { .. }
    ));
    // Human approval is represented explicitly by the trusted coordinator, never by network code.
    for (person, peer) in [(&mut a, &ap), (&mut b, &bp)] {
        assert!(matches!(
            person
                .vault
                .lock()
                .await
                .request(&Request::VerifyPeer {
                    binding: peer.binding_frame.clone(),
                    approved: true
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
    wait_offers(&mut a).await;
    wait_offers(&mut b).await;
    let id = send(&mut a, &ap.id).await;
    wait_pending(&b, &id).await;
    consume(&b, &id).await;
    let id = send(&mut b, &bp.id).await;
    wait_pending(&a, &id).await;
    consume(&a, &id).await;
    let expected = if relay { Route::Relay } else { Route::Direct };
    let (a_route, b_route) = known_routes(&mut a, &mut b).await;
    assert_eq!(a_route, expected);
    assert_eq!(b_route, expected);
    // Revoke discards a locally pending envelope and its opening key.
    let id = send(&mut a, &ap.id).await;
    wait_pending(&b, &id).await;
    let frame = veyra_protocol::Frame::decode(&bp.binding_frame).unwrap();
    let veyra_protocol::Frame::IdentityBinding(binding) = frame else {
        panic!("binding")
    };
    assert!(matches!(
        b.vault
            .lock()
            .await
            .request(&Request::Revoke {
                peer: binding.identity
            })
            .await
            .unwrap(),
        Response::Ok
    ));
    assert_ok(
        b.network
            .request(Command::RevokePeer { peer_id: bp.id })
            .await
            .unwrap(),
    );
    assert!(pending(&b).await.is_empty());
    a.end().await;
    b.end().await;
    println!(
        "WSS + two network daemons + two isolated vaults: bidirectional, verified, one opening, revocation; selected route {expected:?}"
    );
}
