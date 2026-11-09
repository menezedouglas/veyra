use std::io;
use veyra_ipc::{Refusal, VaultRequest as Request, VaultResponse as Response};
use veyra_protocol::Frame;
use veyra_vault::{LocalAuthority, Vault};

fn main() {
    // No panic payloads, diagnostics, plaintext, addresses or tokens on stderr.
    std::panic::set_hook(Box::new(|_| {}));
    if run().is_err() {
        std::process::exit(78);
    }
}
fn run() -> io::Result<()> {
    veyra_secure_memory::harden_process()?;
    veyra_ipc::validate_inherited_stdio()?;
    let (mut input, mut output) = (io::stdin().lock(), io::stdout().lock());
    veyra_ipc::child_authenticate(&mut input, &mut output)?;
    drop(input);
    let (mut vault, authority) = Vault::new().map_err(|_| io::Error::other("memory protection"))?;
    let origin = veyra_secure_memory::monotonic_ms()?;
    // Bounded reader separates blocking pipe I/O from timed key destruction.
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            let request = veyra_ipc::receive::<Request>(&mut input);
            let failed = request.is_err();
            if sender.send(request).is_err() || failed {
                break;
            }
        }
    });
    loop {
        let now = veyra_secure_memory::monotonic_ms()?
            .checked_sub(origin)
            .ok_or_else(|| io::Error::other("clock"))?;
        vault.tick(now);
        let request = match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(request) => request?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Ok(()),
        };
        let now = veyra_secure_memory::monotonic_ms()?
            .checked_sub(origin)
            .ok_or_else(|| io::Error::other("clock"))?;
        let ending = matches!(request, Request::End {});
        let mut response = handle(&mut vault, &authority, request, now);
        let sent = veyra_ipc::send(&mut output, &response);
        if let Response::Opened { body, .. } = &mut response {
            zeroize::Zeroize::zeroize(body);
        }
        sent?;
        if ending {
            return Ok(());
        }
    }
}
fn handle(v: &mut Vault, a: &LocalAuthority, r: Request, now: u64) -> Response {
    let result: Result<Response, ()> = (|| match r {
        Request::BindTransport { noise_public } => {
            let b = v.bind_transport(noise_public).map_err(|_| ())?;
            Ok(Response::Identity {
                fingerprint: veyra_identity::fingerprint(&b.identity),
                runtime_id: veyra_daemon::hex(&b.runtime_id),
                binding: Frame::IdentityBinding(b).encode(),
            })
        }
        Request::SignSignal { document } => Ok(Response::SignalSigned {
            signature: v
                .identity()
                .sign_signal(&document)
                .map_err(|_| ())?
                .to_vec(),
        }),
        Request::Status {} => {
            let b = v.binding();
            Ok(Response::Identity {
                fingerprint: veyra_identity::fingerprint(&b.identity),
                runtime_id: veyra_daemon::hex(&b.runtime_id),
                binding: Frame::IdentityBinding(b).encode(),
            })
        }
        Request::VerifyPeer { binding, approved } => {
            if !approved {
                return Err(());
            }
            let Frame::IdentityBinding(b) = Frame::decode(&binding).map_err(|_| ())? else {
                return Err(());
            };
            v.verify_peer(b, a).map_err(|_| ())?;
            Ok(Response::Ok)
        }
        Request::CreateOffer { peer, ttl_ms } => Ok(Response::Frame {
            frame: Frame::KeyOffer(v.create_offer(&peer, now, ttl_ms).map_err(|_| ())?).encode(),
        }),
        Request::Seal { offer, text } => {
            let text = zeroize::Zeroizing::new(text);
            let Frame::KeyOffer(offer) = Frame::decode(&offer).map_err(|_| ())? else {
                return Err(());
            };
            Ok(Response::Frame {
                frame: Frame::Message(v.seal(&offer, &text).map_err(|_| ())?).encode(),
            })
        }
        Request::AcceptEnvelope { frame } => {
            let Frame::Message(e) = Frame::decode(&frame).map_err(|_| ())? else {
                return Err(());
            };
            Ok(Response::Frame {
                frame: Frame::Received(v.accept_envelope(e, now).map_err(|_| ())?).encode(),
            })
        }
        Request::ListPending {} => Ok(Response::Pending {
            messages: v
                .list_pending(now)
                .into_iter()
                .map(|m| veyra_ipc::PendingMessage {
                    id: veyra_daemon::hex(&m.message_id),
                    peer_id: veyra_daemon::hex(&m.sender),
                    expires_in_ms: m.remaining_ms,
                })
                .collect(),
        }),
        Request::OpenOnce {
            message_id,
            confirmed,
        } => {
            if !confirmed {
                return Err(());
            }
            let remaining = v
                .list_pending(now)
                .into_iter()
                .find(|m| m.message_id == message_id)
                .ok_or(())?
                .remaining_ms;
            let body = v
                .open_once(a.authorize_open(message_id), now)
                .map_err(|_| ())?;
            Ok(Response::Opened {
                body: body.to_string(),
                display_for_ms: remaining.min(30_000),
            })
        }
        Request::Discard { message_id } => {
            v.discard_message(&message_id);
            Ok(Response::Ok)
        }
        Request::Revoke { peer } => {
            v.revoke_peer(&peer);
            Ok(Response::Ok)
        }
        Request::End {} => Ok(Response::Ok),
    })();
    result.unwrap_or(Response::Refused {
        code: Refusal::Unauthorized,
    })
}
