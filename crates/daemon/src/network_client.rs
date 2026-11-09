use crate::{
    VaultClient,
    wire::{self, Reply, Wire},
};
use std::{
    collections::HashMap,
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    process::Child,
    sync::{Mutex, mpsc, oneshot},
    time::timeout,
};
use veyra_ipc::{MAX_IPC_FRAME, Refusal, VaultResponse};
use zeroize::Zeroizing;

pub(crate) async fn read(r: &mut (impl AsyncRead + Unpin)) -> io::Result<Wire> {
    let n = r.read_u32().await? as usize;
    if n == 0 || n > MAX_IPC_FRAME {
        return Err(io::Error::other("IPC limit"));
    }
    let mut bytes = Zeroizing::new(vec![0; n]);
    r.read_exact(&mut bytes).await?;
    veyra_ipc::decode(&bytes)
}
pub(crate) async fn write(w: &mut (impl AsyncWrite + Unpin), bytes: &[u8]) -> io::Result<()> {
    w.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
    w.write_all(bytes).await?;
    w.flush().await
}
pub struct DaemonClient {
    child: Child,
    output: mpsc::Sender<Zeroizing<Vec<u8>>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Reply>>>>,
    sequence: AtomicU64,
    reader: tokio::task::JoinHandle<()>,
    writer: tokio::task::JoinHandle<()>,
}
impl DaemonClient {
    pub async fn spawn(path: &Path, vault: Arc<Mutex<VaultClient>>) -> io::Result<Self> {
        let (mut child, mut input, mut stream) = crate::worker_process::spawn(path).await?;
        let (mut challenge, mut expected) = (Vec::new(), Vec::new());
        veyra_ipc::bootstrap_challenge(&mut challenge, &mut expected)?;
        let (challenge, expected) = (Zeroizing::new(challenge), Zeroizing::new(expected));
        let auth = async {
            input.write_all(&challenge).await?;
            input.flush().await?;
            let mut actual = [0; 32];
            stream.read_exact(&mut actual).await?;
            if !veyra_ipc::bootstrap_matches(&actual, &expected) {
                return Err(io::Error::other("auth"));
            }
            Ok::<_, io::Error>(())
        };
        if !matches!(timeout(Duration::from_secs(5), auth).await, Ok(Ok(()))) {
            let _ = child.kill().await;
            return Err(io::Error::other("daemon auth"));
        }
        let (output, mut outgoing) = mpsc::channel::<Zeroizing<Vec<u8>>>(16);
        let writer = tokio::spawn(async move {
            while let Some(bytes) = outgoing.recv().await {
                if write(&mut input, &bytes).await.is_err() {
                    break;
                }
            }
        });
        let pending = Arc::new(Mutex::new(HashMap::<u64, oneshot::Sender<Reply>>::new()));
        let responses = pending.clone();
        let sender = output.clone();
        let reader = tokio::spawn(async move {
            while let Ok(message) = read(&mut stream).await {
                match message {
                    Wire::Reply { id, response } => {
                        if let Some(channel) = responses.lock().await.remove(&id) {
                            let _ = channel.send(response);
                        }
                    }
                    Wire::Custody { id, request } => {
                        // Closed enum is the authority boundary: no generic forwarding.
                        let request = request.into_vault();
                        let response = vault.lock().await.request(&request).await.unwrap_or(
                            VaultResponse::Refused {
                                code: Refusal::Unavailable,
                            },
                        );
                        let Ok(bytes) = wire::encode(Wire::CustodyReply { id, response }) else {
                            break;
                        };
                        if sender.send(bytes).await.is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            responses.lock().await.clear();
        });
        Ok(Self {
            child,
            output,
            pending,
            sequence: AtomicU64::new(1),
            reader,
            writer,
        })
    }
    pub async fn request(&mut self, command: wire::Command) -> io::Result<Reply> {
        if self.child.try_wait()?.is_some() {
            return Err(io::Error::other("daemon ended"));
        }
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().await;
            if map.len() >= 16 {
                return Err(io::Error::other("command limit"));
            }
            map.insert(id, tx);
        }
        let bytes = match wire::encode(Wire::Command { id, command }) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.pending.lock().await.remove(&id);
                return Err(error);
            }
        };
        if self.output.try_send(bytes).is_err() {
            self.pending.lock().await.remove(&id);
            return Err(io::Error::other("backpressure"));
        }
        match timeout(Duration::from_secs(40), rx).await {
            Ok(Ok(reply)) => Ok(reply),
            _ => {
                self.pending.lock().await.remove(&id);
                let _ = self.child.kill().await;
                Err(io::Error::other("daemon unavailable"))
            }
        }
    }
    pub async fn end(mut self) {
        let _ = timeout(Duration::from_secs(1), self.request(wire::Command::End {})).await;
        let _ = self.child.kill().await;
    }
}
impl Drop for DaemonClient {
    fn drop(&mut self) {
        self.reader.abort();
        self.writer.abort();
    }
}
