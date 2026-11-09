use std::path::Path;
use veyra_daemon::VaultClient;
use veyra_ipc::{VaultRequest as Req, VaultResponse as Resp};
use veyra_protocol::{Frame, IdentityBinding};

async fn worker() -> VaultClient {
    VaultClient::spawn(Path::new(env!("CARGO_BIN_EXE_veyra-vault")))
        .await
        .unwrap()
}
async fn identity(w: &mut VaultClient) -> (IdentityBinding, Vec<u8>) {
    let Resp::Identity { binding, .. } = w.request(&Req::Status {}).await.unwrap() else {
        panic!("identity response")
    };
    let Frame::IdentityBinding(b) = Frame::decode(&binding).unwrap() else {
        panic!("identity frame")
    };
    (b, binding)
}
#[tokio::test]
async fn isolated_vaults_exchange_ciphertext_and_consume_once() {
    let (mut alice, mut bob) = (worker().await, worker().await);
    let (a, ab) = identity(&mut alice).await;
    let (b, bb) = identity(&mut bob).await;
    assert_ne!(a.runtime_id, b.runtime_id);
    assert!(matches!(
        alice
            .request(&Req::VerifyPeer {
                binding: bb.clone(),
                approved: false
            })
            .await
            .unwrap(),
        Resp::Refused { .. }
    ));
    assert!(matches!(
        alice
            .request(&Req::VerifyPeer {
                binding: bb,
                approved: true
            })
            .await
            .unwrap(),
        Resp::Ok
    ));
    assert!(matches!(
        bob.request(&Req::VerifyPeer {
            binding: ab,
            approved: true
        })
        .await
        .unwrap(),
        Resp::Ok
    ));
    let Resp::Frame { frame: offer } = bob
        .request(&Req::CreateOffer {
            peer: a.identity,
            ttl_ms: 5000,
        })
        .await
        .unwrap()
    else {
        panic!("offer")
    };
    let Resp::Frame { frame: sealed } = alice
        .request(&Req::Seal {
            offer,
            text: "fixture de leitura única".into(),
        })
        .await
        .unwrap()
    else {
        panic!("sealed")
    };
    assert!(!sealed.windows(7).any(|bytes| bytes == b"fixture"));
    let Frame::Message(envelope) = Frame::decode(&sealed).unwrap() else {
        panic!("envelope")
    };
    assert!(matches!(
        bob.request(&Req::AcceptEnvelope {
            frame: sealed.clone()
        })
        .await
        .unwrap(),
        Resp::Frame { .. }
    ));
    let Resp::Pending { messages } = bob.request(&Req::ListPending {}).await.unwrap() else {
        panic!("pending")
    };
    assert_eq!(messages.len(), 1);
    assert!(matches!(
        bob.request(&Req::OpenOnce {
            message_id: envelope.header.message_id,
            confirmed: false
        })
        .await
        .unwrap(),
        Resp::Refused { .. }
    ));
    let Resp::Opened {
        body,
        display_for_ms,
    } = bob
        .request(&Req::OpenOnce {
            message_id: envelope.header.message_id,
            confirmed: true,
        })
        .await
        .unwrap()
    else {
        panic!("opened")
    };
    assert_eq!(body, "fixture de leitura única");
    assert!(display_for_ms > 0 && display_for_ms <= 5000);
    assert!(matches!(
        bob.request(&Req::OpenOnce {
            message_id: envelope.header.message_id,
            confirmed: true
        })
        .await
        .unwrap(),
        Resp::Refused { .. }
    ));
    assert!(matches!(
        bob.request(&Req::AcceptEnvelope { frame: sealed })
            .await
            .unwrap(),
        Resp::Frame { .. }
    ));
    let Resp::Pending { messages } = bob.request(&Req::ListPending {}).await.unwrap() else {
        panic!("pending")
    };
    assert!(messages.is_empty());
    alice.end().await;
    bob.end().await;
}
#[tokio::test]
async fn a_new_worker_is_a_new_runtime() {
    let mut first = worker().await;
    let (a, _) = identity(&mut first).await;
    first.end().await;
    let mut second = worker().await;
    let (b, _) = identity(&mut second).await;
    assert_ne!(a.identity, b.identity);
    assert_ne!(a.runtime_id, b.runtime_id);
    second.end().await;
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn cancellation_invalidates_the_vault_response_stream() {
    // Real inherited-pipe worker; a cancelled request must not leave a response
    // that a later operation can accidentally receive under another type/ID.
    let mut vault = worker().await;
    // Pause only our owned synthetic worker, guaranteeing the response is not
    // ready before cancelling the request. The next request must kill it.
    assert_eq!(
        unsafe { libc::kill(vault.process_id().unwrap() as i32, libc::SIGSTOP) },
        0
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(10),
        vault.request(&Req::Status {}),
    )
    .await;
    assert!(result.is_err());
    assert!(vault.request(&Req::Status {}).await.is_err());
    vault.end().await;
}

#[tokio::test]
async fn vault_registers_only_a_public_transport_key_without_replacing_identity() {
    let mut vault = worker().await;
    let (before, _) = identity(&mut vault).await;
    let Resp::Identity { binding, .. } = vault
        .request(&Req::BindTransport {
            noise_public: [7; 32],
        })
        .await
        .unwrap()
    else {
        panic!("public binding refused")
    };
    let Frame::IdentityBinding(after) = Frame::decode(&binding).unwrap() else {
        panic!("public identity binding")
    };
    assert_eq!(after.identity, before.identity);
    assert_eq!(after.runtime_id, before.runtime_id);
    assert_eq!(after.noise_public, [7; 32]);
    veyra_identity::verify_binding(&after, &after.noise_public).unwrap();
    assert!(matches!(
        vault
            .request(&Req::BindTransport {
                noise_public: [8; 32]
            })
            .await
            .unwrap(),
        Resp::Refused { .. }
    ));
    assert_eq!(identity(&mut vault).await.0, after);
    vault.end().await;
}
