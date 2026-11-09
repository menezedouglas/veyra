//! Laboratory program. Parent exchanges synthetic SDP over inherited pipes.
//! No user messages, invitations, identities or production credentials are used.
use std::io::{self, BufRead, Read, Write};
use veyra_connectivity::{Route, TransportConfig};
use veyra_transport::{RTCSessionDescription, WebRtcPeer};

fn read_json<T: serde::de::DeserializeOwned>() -> Result<T, Box<dyn std::error::Error>> {
    let mut line = Vec::new();
    let stdin = io::stdin();
    let mut input = stdin.lock().take(64 * 1024 + 1);
    input.read_until(b'\n', &mut line)?;
    if line.len() > 64 * 1024 {
        return Err("lab input limit".into());
    }
    Ok(serde_json::from_slice(&line)?)
}

fn emit(value: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, &value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        // Deliberately do not print errors originating from SDP/ICE libraries.
        eprintln!("Falha na prova sintética: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let initiator = match std::env::args().nth(1).as_deref() {
        Some("initiator") => true,
        Some("responder") => false,
        _ => return Err("role required".into()),
    };
    let config: TransportConfig = read_json()?;
    let expected = if config.relay_only {
        Route::Relay
    } else {
        Route::Direct
    };
    let expects_tls = config
        .ice_servers
        .iter()
        .any(|s| s.urls.iter().any(|u| u.starts_with("turns:")));
    let peer = WebRtcPeer::new(config).await?;
    if initiator {
        emit(serde_json::json!({"event":"description","description":peer.offer().await?}))?;
        let answer: RTCSessionDescription = read_json()?;
        peer.accept_answer(answer).await?;
    } else {
        let offer: RTCSessionDescription = read_json()?;
        emit(serde_json::json!({"event":"description","description":peer.answer(offer).await?}))?;
    }
    peer.wait_open().await?;
    if initiator {
        peer.send(b"veyra-synthetic-probe-v1").await?;
        if peer.recv().await? != b"veyra-synthetic-reply-v1" {
            return Err("probe mismatch".into());
        }
    } else {
        if peer.recv().await? != b"veyra-synthetic-probe-v1" {
            return Err("probe mismatch".into());
        }
        peer.send(b"veyra-synthetic-reply-v1").await?;
    }
    let route = peer.route().await;
    if route != expected {
        return Err("selected pair did not prove expected route".into());
    }
    if expects_tls && peer.tls_session_count() == 0 {
        return Err("no validated TLS session".into());
    }
    // Parent acknowledges result before either peer closes the shared transport.
    emit(
        serde_json::json!({"event":"verified","route":route,"ordered":true,"reliable":true,"binary":true,"validated_tls_sessions":peer.tls_session_count()}),
    )?;
    let _: serde_json::Value = read_json()?;
    peer.close().await?;
    Ok(())
}
