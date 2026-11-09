use axum::{
    Router,
    extract::{
        OriginalUri, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use axum_server::tls_rustls::RustlsConfig;
use futures_util::SinkExt;
use std::{
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;
use veyra_signaling::{MAX_SIGNAL, Rendezvous, SignalPacket, room_key};
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!(
            "Sinalização indisponível: configure endereço, certificado e chave TLS do operador."
        );
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "TLS provider already configured")?;
    let mut args = std::env::args().skip(1);
    let address: SocketAddr = args.next().ok_or("listen address required")?.parse()?;
    let certificate = args.next().ok_or("TLS certificate path required")?;
    let key = args.next().ok_or("TLS key path required")?;
    let turn_configuration = args.next();
    if args.next().is_some() {
        return Err("unexpected configuration".into());
    }
    let tls = RustlsConfig::from_pem_file(certificate, key).await?;
    let mut state = Rendezvous::default();
    if let Some(path) = turn_configuration {
        state = state.with_turn(veyra_signaling::turn::TurnIssuer::load(
            std::path::Path::new(&path),
        )?);
    }
    let cleanup = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            cleanup.cleanup().await;
        }
    });
    let app = Router::new()
        .route("/v1/rendezvous", get(upgrade))
        .with_state(state);
    axum_server::bind_rustls(address, tls)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}
async fn upgrade(
    State(state): State<Rendezvous>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    if headers.contains_key("origin") || uri.query().is_some() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(permit) = state.connections.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.protocols(["veyra-signal-v1"])
        .read_buffer_size(8 * 1024)
        .write_buffer_size(16 * 1024)
        .max_write_buffer_size(128 * 1024)
        .max_message_size(MAX_SIGNAL)
        .max_frame_size(MAX_SIGNAL)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            serve(socket, state).await;
        })
}
async fn serve(mut socket: WebSocket, state: Rendezvous) {
    let first = tokio::time::timeout(Duration::from_secs(10), socket.recv()).await;
    let Ok(Some(Ok(Message::Text(first)))) = first else {
        return;
    };
    let Ok(packet) = serde_json::from_str::<SignalPacket>(&first) else {
        return;
    };
    let Ok(key) = room_key(&packet) else {
        return;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(u64::MAX);
    let Ok(pinned) = packet.validate(now) else {
        return;
    };
    let role = packet.role;
    let (tx, mut rx) = mpsc::channel(8);
    if state.attach(&packet, tx).await.is_err() {
        return;
    }
    let Ok(turn) = state.turn_credentials() else {
        state.detach(&key).await;
        return;
    };
    #[derive(serde::Serialize)]
    struct Attached {
        status: &'static str,
        turn: Option<veyra_signaling::turn::TurnGrant>,
    }
    let response = Attached {
        status: "attached",
        turn,
    };
    let attached = serde_json::to_string(&response);
    drop(response);
    let Ok(attached) = attached else {
        state.detach(&key).await;
        return;
    };
    if socket.send(Message::Text(attached.into())).await.is_err() {
        state.detach(&key).await;
        return;
    }
    let lifetime = tokio::time::sleep(Duration::from_secs(
        packet.expires_unix.saturating_sub(now).min(300),
    ));
    tokio::pin!(lifetime);
    let mut count = 0u16;
    loop {
        tokio::select! {
            _ = &mut lifetime => break,
            outgoing = rx.recv() => {
                let Some(outgoing) = outgoing else { break; };
                let Ok(json) = serde_json::to_string(&outgoing) else { break; };
                if !matches!(tokio::time::timeout(Duration::from_secs(5), socket.send(Message::Text(json.into()))).await, Ok(Ok(()))) { break; }
            }
            incoming = socket.recv() => {
                count = count.saturating_add(1); if count > 128 { break; }
                let Some(Ok(Message::Text(incoming))) = incoming else { break; };
                let Ok(packet) = serde_json::from_str::<SignalPacket>(&incoming) else { break; };
                if state.forward(packet, &pinned, &key, role).await.is_err() { break; }
            }
        }
    }
    state.detach(&key).await;
    let _ = socket.close().await;
}
