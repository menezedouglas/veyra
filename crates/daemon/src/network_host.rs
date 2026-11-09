use crate::{
    network_client,
    wire::{self, Command, NetworkVaultRequest, Peer, Reply, Wire},
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, mpsc, oneshot};
use veyra_ipc::VaultResponse;
use veyra_protocol::{
    Envelope, Frame, Identity, IdentityBinding, KeyOffer, Receipt, SignalDocument,
};
use veyra_secure_memory::LockedBuffer;
use veyra_session_runtime::{Error, Result, Runtime, RuntimeConfig, VaultNetworkPort};
use zeroize::Zeroizing;

struct Custody {
    output: mpsc::Sender<Zeroizing<Vec<u8>>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<VaultResponse>>>>,
    sequence: AtomicU64,
    noise_private: Mutex<LockedBuffer>,
    noise_public: [u8; 32],
    binding: Mutex<Option<IdentityBinding>>,
}
impl Custody {
    fn new(
        output: mpsc::Sender<Zeroizing<Vec<u8>>>,
        pending: Arc<Mutex<HashMap<u64, oneshot::Sender<VaultResponse>>>>,
    ) -> io::Result<Self> {
        let keypair = snow::Builder::new(
            veyra_identity::NOISE_PATTERN
                .parse()
                .map_err(|_| io::Error::other("Noise parameters"))?,
        )
        .generate_keypair()
        .map_err(|_| io::Error::other("transport entropy"))?;
        let private = Zeroizing::new(keypair.private);
        if private.len() != 32 {
            return Err(io::Error::other("Noise key length"));
        }
        let noise_public = keypair
            .public
            .try_into()
            .map_err(|_| io::Error::other("Noise public key length"))?;
        let mut locked = LockedBuffer::new(32)?;
        locked.as_mut_slice().copy_from_slice(&private);
        Ok(Self {
            output,
            pending,
            sequence: AtomicU64::new(1),
            noise_private: Mutex::new(locked),
            noise_public,
            binding: Mutex::new(None),
        })
    }
    async fn call(&self, request: NetworkVaultRequest) -> Result<VaultResponse> {
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            if pending.len() >= 8 {
                return Err(Error::Capacity);
            }
            pending.insert(id, tx);
        }
        let bytes = match wire::encode(Wire::Custody { id, request }) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.pending.lock().await.remove(&id);
                return Err(Error::Custody);
            }
        };
        if self.output.try_send(bytes).is_err() {
            self.pending.lock().await.remove(&id);
            return Err(Error::Capacity);
        }
        let result = tokio::time::timeout(Duration::from_secs(5), rx).await;
        self.pending.lock().await.remove(&id);
        result
            .map_err(|_| Error::Custody)?
            .map_err(|_| Error::Custody)
    }
}
#[async_trait]
impl VaultNetworkPort for Custody {
    async fn binding(&self) -> Result<IdentityBinding> {
        let mut cached = self.binding.lock().await;
        if let Some(binding) = cached.as_ref() {
            return Ok(binding.clone());
        }
        let VaultResponse::Identity { binding, .. } = self
            .call(NetworkVaultRequest::BindTransport {
                noise_public: self.noise_public,
            })
            .await?
        else {
            return Err(Error::Custody);
        };
        let Frame::IdentityBinding(binding) =
            Frame::decode(&binding).map_err(|_| Error::Custody)?
        else {
            return Err(Error::Custody);
        };
        veyra_identity::verify_binding(&binding, &self.noise_public).map_err(|_| Error::Custody)?;
        *cached = Some(binding.clone());
        Ok(binding)
    }
    async fn prepare_transport(&self) -> Result<Zeroizing<[u8; 32]>> {
        // This operation is local to the network process. No private key is
        // sent through custody IPC or through the desktop coordinator.
        let locked = self.noise_private.lock().await;
        let mut secret = Zeroizing::new([0; 32]);
        secret.copy_from_slice(locked.as_slice());
        Ok(secret)
    }
    async fn sign_signal(&self, document: SignalDocument) -> Result<[u8; 64]> {
        let VaultResponse::SignalSigned { signature } = self
            .call(NetworkVaultRequest::SignSignal { document })
            .await?
        else {
            return Err(Error::Custody);
        };
        signature.try_into().map_err(|_| Error::Custody)
    }
    async fn create_offer(&self, peer: Identity, ttl_ms: u32) -> Result<KeyOffer> {
        let VaultResponse::Frame { frame } = self
            .call(NetworkVaultRequest::CreateOffer { peer, ttl_ms })
            .await?
        else {
            return Err(Error::Custody);
        };
        let Frame::KeyOffer(k) = Frame::decode(&frame).map_err(|_| Error::Custody)? else {
            return Err(Error::Custody);
        };
        Ok(k)
    }
    async fn accept_envelope(&self, envelope: Envelope) -> Result<Receipt> {
        let VaultResponse::Frame { frame } = self
            .call(NetworkVaultRequest::AcceptEnvelope {
                frame: Frame::Message(envelope).encode(),
            })
            .await?
        else {
            return Err(Error::Custody);
        };
        let Frame::Received(r) = Frame::decode(&frame).map_err(|_| Error::Custody)? else {
            return Err(Error::Custody);
        };
        Ok(r)
    }
}
pub async fn run() -> io::Result<()> {
    veyra_secure_memory::harden_process()?;
    veyra_ipc::validate_inherited_stdio()?;
    // Bootstrap before async stdin/stdout tasks take ownership.
    veyra_ipc::child_authenticate(&mut std::io::stdin().lock(), &mut std::io::stdout().lock())?;
    let (output, mut outgoing) = mpsc::channel::<Zeroizing<Vec<u8>>>(16);
    let pending = Arc::new(Mutex::new(
        HashMap::<u64, oneshot::Sender<VaultResponse>>::new(),
    ));
    let port = Arc::new(Custody::new(output.clone(), pending.clone())?);
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(bytes) = outgoing.recv().await {
            if network_client::write(&mut stdout, &bytes).await.is_err() {
                break;
            }
        }
    });
    let (commands, mut incoming) = mpsc::channel(8);
    let reader = tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        while let Ok(wire) = network_client::read(&mut stdin).await {
            match wire {
                Wire::Command { id, command } => {
                    if commands.try_send((id, command)).is_err() {
                        break;
                    }
                }
                Wire::CustodyReply { id, mut response } => {
                    if let Some(channel) = pending.lock().await.remove(&id) {
                        if let Err(mut response) = channel.send(response) {
                            wire::wipe_response(&mut response)
                        }
                    } else {
                        wire::wipe_response(&mut response)
                    }
                }
                _ => break,
            }
        }
    });
    let mut runtime: Option<Arc<Runtime>> = None;
    while let Some((id, command)) = incoming.recv().await {
        let ending = matches!(command, Command::End {});
        let response = execute(&mut runtime, port.clone(), command)
            .await
            .unwrap_or_else(|e| Reply::Refused {
                reason: e.to_string(),
            });
        let bytes = wire::encode(Wire::Reply { id, response })?;
        if output.send(bytes).await.is_err() {
            break;
        }
        if ending {
            break;
        }
    }
    if let Some(runtime) = runtime {
        runtime.end().await;
    }
    reader.abort();
    drop(output);
    drop(port);
    let mut writer = writer;
    if tokio::time::timeout(Duration::from_secs(1), &mut writer)
        .await
        .is_err()
    {
        writer.abort();
    }
    Ok(())
}
async fn execute(
    runtime: &mut Option<Arc<Runtime>>,
    port: Arc<Custody>,
    command: Command,
) -> Result<Reply> {
    if let Command::Start {
        endpoint,
        transport,
    } = command
    {
        if runtime.is_some() {
            return Err(Error::Closed);
        }
        *runtime = Some(
            Runtime::new(
                RuntimeConfig {
                    endpoint,
                    transport,
                },
                port,
            )
            .await?,
        );
        return Ok(Reply::Ok {});
    }
    let r = runtime.as_ref().ok_or(Error::Closed)?;
    match command {
        Command::CreateInvite {} => {
            let invite = r.create_invite().await?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::Closed)?
                .as_secs();
            Ok(Reply::Invite {
                token: invite.token,
                expires_in_ms: invite.expires_unix.saturating_sub(now) * 1000,
            })
        }
        Command::JoinInvite { token } => match r.exchange_invite(&token).await? {
            Some(invite) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| Error::Closed)?
                    .as_secs();
                Ok(Reply::Invite {
                    token: invite.token,
                    expires_in_ms: invite.expires_unix.saturating_sub(now) * 1000,
                })
            }
            None => Ok(Reply::Ok {}),
        },
        Command::Peers {} => Ok(Reply::Peers {
            peers: r
                .peers()
                .await
                .into_iter()
                .map(|p| Peer {
                    id: p.id,
                    binding_frame: p.binding_frame,
                    fingerprint: p.fingerprint,
                    verification_code: p.verification_code,
                    verified: p.verified,
                    online: p.online,
                    route: p.route,
                    available_offers: p.available_offers,
                })
                .collect(),
        }),
        Command::Events {} => {
            let mut deliveries = Vec::new();
            let mut connection_failed = false;
            for event in r.drain_events().await {
                match event {
                    veyra_session_runtime::RuntimeEvent::Delivery { message_id, status } => {
                        deliveries.push(wire::DeliveryUpdate {
                            message_id,
                            state: status.into(),
                        })
                    }
                    veyra_session_runtime::RuntimeEvent::ConnectionFailed => {
                        connection_failed = true
                    }
                    _ => {}
                }
            }
            Ok(Reply::Events {
                deliveries,
                connection_failed,
            })
        }
        Command::ApprovePeer { peer_id, code } => {
            r.approve_peer(&peer_id, &code).await?;
            Ok(Reply::Ok {})
        }
        Command::TakeOffer { peer_id } => Ok(Reply::Frame {
            frame: Frame::KeyOffer(r.take_offer(&peer_id).await?).encode(),
        }),
        Command::SendEnvelope { frame } => {
            let Frame::Message(e) = Frame::decode(&frame).map_err(|_| Error::Authentication)?
            else {
                return Err(Error::Authentication);
            };
            r.send_envelope(e).await?;
            Ok(Reply::Ok {})
        }
        Command::RevokePeer { peer_id } => {
            r.revoke_peer(&peer_id).await?;
            Ok(Reply::Ok {})
        }
        Command::End {} => {
            r.end().await;
            Ok(Reply::Ok {})
        }
        Command::Start { .. } => Err(Error::Closed),
    }
}

#[cfg(test)]
mod local_transport_tests {
    use super::*;
    #[tokio::test]
    async fn transport_private_key_is_local_and_never_uses_custody_wire() {
        let (output, mut requests) = mpsc::channel(1);
        let custody = Custody::new(output, Arc::new(Mutex::new(HashMap::new()))).unwrap();
        let first = custody.prepare_transport().await.unwrap();
        let second = custody.prepare_transport().await.unwrap();
        assert!(first.iter().any(|byte| *byte != 0));
        assert!(*first == *second);
        assert!(matches!(
            requests.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        let document = wire::encode(Wire::Custody {
            id: 1,
            request: NetworkVaultRequest::BindTransport {
                noise_public: custody.noise_public,
            },
        })
        .unwrap();
        assert!(
            !document
                .windows(b"noise_private".len())
                .any(|part| part == b"noise_private")
        );
    }
}
