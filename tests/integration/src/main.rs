//! Laboratory harness peer. Public SDP/binding exchange uses inherited stdio;
//! every application frame travels through real WebRTC + DTLS + Noise + HPKE.
//! Automatic approval belongs exclusively to this synthetic-data test harness.
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Read, Write},
    time::Instant,
};
use veyra_connectivity::{Route, TransportConfig};
use veyra_core::Outbox;
use veyra_crypto::{NoiseChannel, NoiseHandshake};
use veyra_identity::{fingerprint, verification_code};
use veyra_protocol::{noise_prologue, Frame, IdentityBinding};
use veyra_transport::{RTCSessionDescription, WebRtcPeer};
use veyra_vault::{LocalAuthority, Vault};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const SYNTHETIC_TEXT: &str = "Veyra: conteúdo sintético de integração, sem dados pessoais.";
fn now(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}
fn control() -> Result<Value> {
    let mut line = String::new();
    let n = io::stdin().lock().take(98_305).read_line(&mut line)?;
    if n == 0 || n > 98_304 {
        return Err("control closed or oversize".into());
    }
    Ok(serde_json::from_str(&line)?)
}
fn report(value: Value) -> Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, &value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
fn binding_frame(value: &Value) -> Result<IdentityBinding> {
    let bytes: Vec<u8> = serde_json::from_value(value["frame"].clone())?;
    match Frame::decode(&bytes)? {
        Frame::IdentityBinding(b) => Ok(b),
        _ => Err("expected identity".into()),
    }
}
async fn transmit(peer: &WebRtcPeer, noise: &mut NoiseChannel, frame: Frame) -> Result<()> {
    peer.send(&noise.encrypt(&frame.encode())?).await?;
    Ok(())
}
async fn receive(peer: &WebRtcPeer, noise: &mut NoiseChannel) -> Result<Frame> {
    Ok(Frame::decode(&noise.decrypt(&peer.recv().await?)?)?)
}
async fn transfer(
    sender: bool,
    vault: &mut Vault,
    authority: &LocalAuthority,
    remote: &IdentityBinding,
    peer: &WebRtcPeer,
    noise: &mut NoiseChannel,
    start: Instant,
) -> Result<()> {
    if sender {
        let offer = match receive(peer, noise).await? {
            Frame::KeyOffer(o) => o,
            _ => return Err("expected key offer".into()),
        };
        let envelope = vault.seal(&offer, SYNTHETIC_TEXT)?;
        let id = envelope.header.message_id;
        let mut outbox = Outbox::default();
        outbox.enqueue(envelope.clone(), now(start))?;
        transmit(peer, noise, Frame::Message(envelope.clone())).await?;
        outbox.mark_sent(&id)?;
        let ack = match receive(peer, noise).await? {
            Frame::Received(r) => r,
            _ => return Err("expected receipt".into()),
        };
        outbox.acknowledge(&ack)?;
        if !outbox.is_empty() {
            return Err("ACK did not discard envelope".into());
        }
        // Same signed envelope, newly encrypted with fresh Noise message counter.
        transmit(peer, noise, Frame::Message(envelope)).await?;
        let duplicate_ack = match receive(peer, noise).await? {
            Frame::Received(r) => r,
            _ => return Err("expected duplicate receipt".into()),
        };
        if duplicate_ack != ack {
            return Err("duplicate disclosed read-state".into());
        }
    } else {
        let offer = vault.create_offer(&remote.identity, now(start), 30_000)?;
        transmit(peer, noise, Frame::KeyOffer(offer)).await?;
        let envelope = match receive(peer, noise).await? {
            Frame::Message(m) => m,
            _ => return Err("expected envelope".into()),
        };
        let id = envelope.header.message_id;
        let ack = vault.accept_envelope(envelope, now(start))?;
        transmit(peer, noise, Frame::Received(ack.clone())).await?;
        let plaintext = vault.open_once(authority.authorize_open(id), now(start))?;
        if plaintext.as_str() != SYNTHETIC_TEXT {
            return Err("wrong plaintext".into());
        }
        drop(plaintext);
        if vault
            .open_once(authority.authorize_open(id), now(start))
            .is_ok()
        {
            return Err("opened twice".into());
        }
        let duplicate = match receive(peer, noise).await? {
            Frame::Message(m) => m,
            _ => return Err("expected duplicate envelope".into()),
        };
        let duplicate_ack = vault.accept_envelope(duplicate, now(start))?;
        if ack != duplicate_ack || !vault.list_pending(now(start)).is_empty() {
            return Err("replay restored consumed message".into());
        }
        transmit(peer, noise, Frame::Received(duplicate_ack)).await?;
    }
    Ok(())
}
async fn run() -> Result<()> {
    veyra_secure_memory::harden_process()?;
    let init = control()?;
    let initiator = init["initiator"].as_bool().ok_or("missing role")?;
    let config: TransportConfig = serde_json::from_value(init["config"].clone())?;
    let expected_route = if config.relay_only {
        Route::Relay
    } else {
        Route::Direct
    };
    let (mut vault, authority) = Vault::new()?;
    let local = vault.binding();
    report(json!({"kind":"binding","frame":Frame::IdentityBinding(local.clone()).encode()}))?;
    let remote = binding_frame(&control()?)?;
    let peer = WebRtcPeer::new(config).await?;
    if initiator {
        let sdp = peer
            .offer()
            .await
            .map_err(|e| format!("gather_offer: {e}"))?;
        report(json!({"kind":"offer","description":sdp}))?;
        let answer: RTCSessionDescription =
            serde_json::from_value(control()?["description"].clone())?;
        peer.accept_answer(answer).await?;
    } else {
        let offer: RTCSessionDescription =
            serde_json::from_value(control()?["description"].clone())?;
        let answer = peer
            .answer(offer)
            .await
            .map_err(|e| format!("gather_answer: {e}"))?;
        report(json!({"kind":"answer","description":answer}))?;
    }
    peer.wait_open()
        .await
        .map_err(|e| format!("wait_open: {e}"))?;
    let (i, r) = if initiator {
        (&local, &remote)
    } else {
        (&remote, &local)
    };
    let prologue = noise_prologue(i, r, &[41; 32]); // Fixed synthetic invitation digest for this laboratory only.
    let mut handshake = NoiseHandshake::new(vault.identity(), initiator, &prologue)?;
    if initiator {
        peer.send(&handshake.write()?).await?;
        handshake.read(&peer.recv().await?)?;
        peer.send(&handshake.write()?).await?;
    } else {
        handshake.read(&peer.recv().await?)?;
        peer.send(&handshake.write()?).await?;
        handshake.read(&peer.recv().await?)?;
    }
    let (mut noise, transcript) = handshake.finish(&remote)?;
    let code = verification_code(&transcript, i, r);
    report(json!({"kind":"verification","code":code,"fingerprint":fingerprint(&remote.identity)}))?;
    let approval = control()?;
    if approval["kind"] != "approve"
        || approval["code"].as_str() != Some(code.as_str())
        || approval["approved"] != true
    {
        return Err("explicit verification refused".into());
    }
    vault.verify_peer(remote.clone(), &authority)?;
    let start = Instant::now();
    transfer(
        initiator, &mut vault, &authority, &remote, &peer, &mut noise, start,
    )
    .await?;
    transfer(
        !initiator, &mut vault, &authority, &remote, &peer, &mut noise, start,
    )
    .await?;
    let route = peer.route().await;
    if route != expected_route {
        return Err("selected route differs from required route".into());
    }
    // Final two-way barrier prevents a close racing the other peer's last ACK.
    transmit(&peer, &mut noise, Frame::Close(0)).await?;
    if receive(&peer, &mut noise).await? != Frame::Close(0) {
        return Err("close barrier".into());
    }
    report(
        json!({"kind":"result","route":route,"bidirectional":true,"at_most_once":true,"duplicate_ack_idempotent":true,"pending":vault.list_pending(now(start)).len()}),
    )?;
    peer.close().await?;
    Ok(())
}
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Veyra synthetic integration failed: {error}");
        std::process::exit(1)
    }
}
