#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod contacts;
mod operator_config;

use contacts::ContactBook;
use operator_config::OperatorConfiguration;
use serde::Serialize;
use std::sync::Arc;
use tauri::{Manager, State, WebviewWindow};
use tokio::sync::Mutex;
use veyra_daemon::{
    DaemonClient, PeerSummary, SessionStatus, VaultClient,
    wire::{Command, Peer, Reply},
};
use veyra_ipc::{PendingMessage, VaultRequest as Req, VaultResponse as Resp};
struct Session {
    vault: Option<Arc<Mutex<VaultClient>>>,
    daemon: Option<DaemonClient>,
    status: SessionStatus,
    outgoing: std::collections::HashMap<String, (String, u64)>,
    approved: std::collections::HashMap<String, (Vec<u8>, String)>,
    network_attempted: bool,
}
impl Default for Session {
    fn default() -> Self {
        let setup = operator_config::resolve(None).setup;
        Self {
            vault: None,
            daemon: None,
            status: SessionStatus {
                reason: setup.reason.clone(),
                connection_setup: setup,
                ..SessionStatus::default()
            },
            outgoing: Default::default(),
            approved: Default::default(),
            network_attempted: false,
        }
    }
}
struct AppState(Mutex<Session>, Mutex<ContactState>);

struct ContactState {
    book: Option<ContactBook>,
    error: Option<String>,
}
impl ContactState {
    fn new(book: std::io::Result<ContactBook>) -> Self {
        match book {
            Ok(book) => Self {
                book: Some(book),
                error: None,
            },
            Err(_) => Self {
                book: None,
                error: Some("Não foi possível carregar os contatos salvos. O arquivo foi preservado; reinicie o aplicativo após corrigir o acesso aos dados locais.".into()),
            },
        }
    }

    fn sync(&mut self, peers: &[Peer]) {
        if let Some(book) = &mut self.book {
            self.error = book.sync(peers).err().map(|_| {
                "Não foi possível salvar os contatos. Eles podem ser perdidos ao fechar o aplicativo; confira o acesso aos dados locais e tente novamente.".into()
            });
        }
    }

    fn remove(&mut self, id: &str) -> Result<(), String> {
        let Some(book) = &mut self.book else {
            return Err(self
                .error
                .clone()
                .unwrap_or_else(|| "Contatos indisponíveis.".into()));
        };
        match book.remove(id) {
            Ok(()) => {
                self.error = None;
                Ok(())
            }
            Err(_) => {
                let reason = "Não foi possível remover o contato salvo. Confira o acesso aos dados locais e tente novamente.".to_string();
                self.error = Some(reason.clone());
                Err(reason)
            }
        }
    }

    fn status(&self, session: &SessionStatus) -> SessionStatus {
        let mut status = session.clone();
        status.contacts_error = self.error.clone();
        let mut saved = self
            .book
            .as_ref()
            .map(ContactBook::summaries)
            .unwrap_or_default();
        for peer in &status.peers {
            if let Some(contact) = saved.iter_mut().find(|contact| contact.id == peer.id) {
                let label = contact.label.clone();
                *contact = peer.clone();
                contact.label = label;
            } else {
                saved.push(peer.clone());
            }
        }
        status.peers = saved;
        status
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Invite {
    token: String,
    expires_in_ms: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Opened {
    body: String,
    display_for_ms: u64,
}
impl Drop for Opened {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.body.zeroize();
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Delivery {
    message_id: Option<String>,
    peer_id: String,
    state: &'static str,
    reason: Option<String>,
}
fn local(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Janela não autorizada.".into());
    }
    let url = window.url().map_err(|_| "Origem inválida.")?;
    if !trusted_origin(url.scheme(), url.host_str(), url.port()) {
        return Err("Origem não autorizada.".into());
    }
    Ok(())
}
fn trusted_origin(scheme: &str, host: Option<&str>, port: Option<u16>) -> bool {
    matches!(
        (scheme, host),
        ("tauri", Some("localhost")) | ("http", Some("tauri.localhost"))
    ) || (cfg!(debug_assertions)
        && scheme == "http"
        && host == Some("127.0.0.1")
        && port == Some(1420))
}
fn child_path(name: &str) -> Result<std::path::PathBuf, String> {
    let mut p = std::env::current_exe().map_err(|_| "Instalação inválida.")?;
    p.set_file_name(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    });
    Ok(p)
}
fn id<const N: usize>(s: &str) -> Result<[u8; N], String> {
    if s.len() != N * 2 || !s.is_ascii() {
        return Err("Identificador inválido.".into());
    }
    let mut b = [0; N];
    for (i, x) in b.iter_mut().enumerate() {
        *x = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(|_| "Identificador inválido.")?
    }
    Ok(b)
}
fn summary(
    p: &Peer,
    approved: &std::collections::HashMap<String, (Vec<u8>, String)>,
) -> PeerSummary {
    PeerSummary {
        id: p.id.clone(),
        label: format!("Peer {}", &p.id[..p.id.len().min(8)]),
        fingerprint: p.fingerprint.clone(),
        verified: p.online
            && p.verified
            && approved.get(&p.id).is_some_and(|(binding, code)| {
                binding == &p.binding_frame && code == &p.verification_code
            }),
        route: if !p.online {
            "disconnected"
        } else {
            match p.route {
                veyra_connectivity::Route::Direct => "direct",
                veyra_connectivity::Route::Relay => "relay",
                _ => "unknown",
            }
        },
        verification_code: Some(p.verification_code.clone()),
    }
}
async fn request(s: &mut Session, c: Command) -> Result<Reply, String> {
    let unavailable = s
        .status
        .reason
        .clone()
        .unwrap_or_else(|| veyra_daemon::NO_CONNECTION_SERVICE.into());
    let d = s.daemon.as_mut().ok_or(unavailable)?;
    match d.request(c).await {
        Ok(Reply::Refused { reason }) => Err(reason),
        Ok(r) => Ok(r),
        Err(_) => {
            s.daemon = None;
            s.status.peers.clear();
            s.status.infrastructure_configured = false;
            s.status.reason = Some(NETWORK_SESSION_ENDED.into());
            Err(NETWORK_SESSION_ENDED.into())
        }
    }
}
async fn peers(s: &mut Session) -> Result<Vec<Peer>, String> {
    let Reply::Peers { peers } = request(s, Command::Peers {}).await? else {
        return Err("Resposta inválida.".into());
    };
    let mut ids = std::collections::HashSet::new();
    if peers.len() > 16
        || peers
            .iter()
            .any(|p| !ids.insert(&p.id) || veyra_daemon::validate_peer_metadata(p).is_err())
    {
        s.daemon = None;
        s.status.peers.clear();
        s.status.infrastructure_configured = false;
        s.status.reason = Some(NETWORK_SESSION_ENDED.into());
        return Err("O daemon retornou uma identidade incoerente; conexões descartadas.".into());
    }
    Ok(peers)
}
#[tauri::command]
async fn connection_status(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<SessionStatus, String> {
    local(&window)?;
    let mut s = state.0.lock().await;
    let mut synchronized = Vec::new();
    s.status.deliveries.clear();
    if let Some(v) = s.vault.clone() {
        if v.lock().await.request(&Req::Status {}).await.is_err() {
            *s = Session::default();
            s.status.phase = "blocked";
            s.status.reason = Some("Cofre encerrado; sessão descartada.".into())
        } else if let Ok(Resp::Pending { messages }) =
            v.lock().await.request(&Req::ListPending {}).await
        {
            s.status.pending_count = messages.len();
        }
    }
    if s.daemon.is_some() {
        if let Ok(p) = peers(&mut s).await {
            s.approved.retain(|id, (binding, code)| {
                p.iter().any(|peer| {
                    &peer.id == id
                        && peer.online
                        && peer.verified
                        && &peer.binding_frame == binding
                        && &peer.verification_code == code
                })
            });
            s.status.peers = p.iter().map(|p| summary(p, &s.approved)).collect();
            synchronized = p;
        }
        if let Ok(Reply::Events {
            deliveries,
            connection_failed,
        }) = request(&mut s, Command::Events {}).await
        {
            for update in deliveries {
                if let Some((peer_id, _)) = s.outgoing.get(&update.message_id).cloned() {
                    let(state,reason)=match update.state.as_str(){
                        "sent"=>("sent",Some("Enviado ao transporte; recebimento ainda não confirmado.".into())),
                        "acknowledged"=>("acknowledged",Some("Aceita em RAM pelo destinatário. Não confirma leitura e pode ser perdida antes da abertura.".into())),
                        "delivery_unconfirmed"=>("delivery-unconfirmed",Some("Sem confirmação de recebimento. Isso não comprova falha de entrega.".into())),_=>continue,
                    };
                    if state != "sent" {
                        s.outgoing.remove(&update.message_id);
                    }
                    s.status.deliveries.push(veyra_daemon::DeliveryStatus {
                        message_id: update.message_id,
                        peer_id,
                        state,
                        reason,
                    });
                }
            }
            if connection_failed {
                s.status.reason=Some("Uma tentativa de conexão falhou ou expirou. Tente novamente com um novo convite.".into());
            }
        }
    }
    let now = veyra_secure_memory::monotonic_ms().unwrap_or(u64::MAX);
    let unconfirmed: Vec<_> = s
        .outgoing
        .iter()
        .filter(|(_, (peer, deadline))| {
            now >= *deadline
                || s.daemon.is_none()
                || s.status
                    .peers
                    .iter()
                    .any(|p| &p.id == peer && p.route == "disconnected")
        })
        .map(|(message, (peer, _))| (message.clone(), peer.clone()))
        .collect();
    for (message_id, peer_id) in unconfirmed {
        s.outgoing.remove(&message_id);
        s.status.deliveries.push(veyra_daemon::DeliveryStatus{message_id,peer_id,state:"delivery-unconfirmed",reason:Some("Não houve confirmação autenticada antes da expiração ou desconexão. A entrega pode ter ocorrido.".into())});
    }
    let mut contacts = state.1.lock().await;
    contacts.sync(&synchronized);
    Ok(contacts.status(&s.status))
}
#[tauri::command]
async fn start_session(
    window: WebviewWindow,
    state: State<'_, AppState>,
    allow_background: bool,
    signaling_endpoint: Option<String>,
) -> Result<SessionStatus, String> {
    local(&window)?;
    if allow_background {
        return Err("Recepção em segundo plano ainda não validada.".into());
    }
    let mut s = state.0.lock().await;
    if s.vault.is_some() {
        return Ok(state.1.lock().await.status(&s.status));
    }
    begin_session(
        &mut s,
        operator_config::resolve(signaling_endpoint.as_deref()),
    )
    .await?;
    Ok(state.1.lock().await.status(&s.status))
}

const NETWORK_SESSION_ENDED: &str = "O componente de conexão desta identidade foi encerrado. Encerre a sessão antes de iniciar outra conexão.";

fn session_platform(s: &mut Session) -> Result<(), String> {
    if !cfg!(any(target_os = "linux", windows)) {
        let reason = "Sessões nesta plataforma ainda não estão disponíveis: o canal IPC seguro não está implementado.";
        s.status.phase = "blocked";
        s.status.reason = Some(reason.into());
        return Err(reason.into());
    }
    Ok(())
}

fn network_start_allowed(s: &Session) -> Result<(), String> {
    if s.network_attempted {
        Err(NETWORK_SESSION_ENDED.into())
    } else {
        Ok(())
    }
}

async fn attach_daemon(s: &mut Session, endpoint: String) -> Result<(), String> {
    network_start_allowed(s)?;
    session_platform(s)?;
    let vault = s.vault.clone().ok_or("Sessão indisponível.")?;
    // The vault binds one transport Noise key for this identity. Even an
    // ambiguous startup failure must not launch a replacement network child.
    s.network_attempted = true;
    let result = async {
        let mut daemon = DaemonClient::spawn(&child_path("veyra-daemon")?, vault)
            .await
            .map_err(|_| "Não foi possível iniciar o componente de conexão.".to_string())?;
        match daemon
            .request(Command::Start {
                endpoint,
                transport: Default::default(),
            })
            .await
            .map_err(|_| "Não foi possível iniciar a conexão segura.".to_string())?
        {
            Reply::Ok {} => Ok(daemon),
            Reply::Refused { reason } => Err(reason),
            _ => Err("Resposta inválida do componente de conexão.".into()),
        }
    }
    .await;
    match result {
        Ok(daemon) => {
            s.daemon = Some(daemon);
            s.status.infrastructure_configured = true;
            s.status.reason = s.status.connection_setup.reason.clone().or_else(|| Some("Conexão preparada. A disponibilidade e a rota serão confirmadas no pareamento.".into()));
            Ok(())
        }
        Err(reason) => {
            s.status.infrastructure_configured = false;
            let reason = format!("{reason} Encerre esta sessão antes de tentar uma nova conexão.");
            s.status.reason = Some(reason.clone());
            Err(reason)
        }
    }
}

async fn begin_session(
    s: &mut Session,
    configuration: OperatorConfiguration,
) -> Result<(), String> {
    s.status.connection_setup = configuration.setup;
    session_platform(s)?;
    if !s.status.connection_setup.configured && s.status.connection_setup.source != "unconfigured" {
        let reason = s
            .status
            .connection_setup
            .reason
            .clone()
            .unwrap_or_else(|| "Configuração de conexão inválida.".into());
        s.status.phase = "blocked";
        s.status.reason = Some(reason.clone());
        return Err(reason);
    }
    if veyra_secure_memory::harden_process().is_err() {
        s.status.phase = "blocked";
        s.status.memory_protection = "unavailable";
        s.status.reason = Some("Proteção de memória indisponível; sessão recusada.".into());
        return Err("Proteção de memória indisponível; sessão recusada.".into());
    }
    let mut v = VaultClient::spawn(&child_path("veyra-vault")?)
        .await
        .map_err(|_| {
            let reason = "Não foi possível iniciar o cofre seguro; a sessão foi recusada.";
            s.status.phase = "blocked";
            s.status.reason = Some(reason.into());
            reason
        })?;
    let Resp::Identity {
        fingerprint,
        runtime_id,
        ..
    } = v
        .request(&Req::Status {})
        .await
        .map_err(|_| "Falha no cofre.")?
    else {
        return Err("Cofre recusou a sessão.".into());
    };
    s.status.phase = "active";
    s.status.fingerprint = Some(fingerprint);
    s.status.runtime_id = Some(runtime_id);
    s.status.memory_protection = "protected";
    s.status.reason = s.status.connection_setup.reason.clone();
    s.vault = Some(Arc::new(Mutex::new(v)));
    if let Some(endpoint) = configuration.endpoint {
        attach_daemon(s, endpoint).await?;
    }
    Ok(())
}
#[tauri::command]
async fn end_session(window: WebviewWindow, state: State<'_, AppState>) -> Result<(), String> {
    local(&window)?;
    let mut s = state.0.lock().await;
    if let Some(d) = s.daemon.take() {
        d.end().await
    }
    if let Some(v) = s.vault.take() {
        let _ = v.lock().await.request(&Req::End {}).await;
    }
    *s = Session::default();
    Ok(())
}
#[tauri::command]
async fn list_pending(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<PendingMessage>, String> {
    local(&window)?;
    let s = state.0.lock().await;
    let Some(v) = &s.vault else { return Ok(vec![]) };
    match v
        .lock()
        .await
        .request(&Req::ListPending {})
        .await
        .map_err(|_| "Cofre indisponível.")?
    {
        Resp::Pending { messages } => Ok(messages),
        _ => Err("Operação recusada.".into()),
    }
}
#[tauri::command]
async fn create_invite(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Invite, String> {
    local(&window)?;
    let Reply::Invite {
        token,
        expires_in_ms,
    } = request(&mut *state.0.lock().await, Command::CreateInvite {}).await?
    else {
        return Err("Convite indisponível.".into());
    };
    Ok(Invite {
        token,
        expires_in_ms,
    })
}
#[tauri::command]
async fn join_invite(
    window: WebviewWindow,
    state: State<'_, AppState>,
    invite: String,
    label: String,
) -> Result<Option<Invite>, String> {
    local(&window)?;
    if invite.len() > veyra_daemon::MAX_DIRECT_TOKEN
        || label.len() > 80
        || label.chars().any(char::is_control)
    {
        return Err("Convite ou nome inválido.".into());
    }
    // Resolve only a structurally and cryptographically valid invitation before
    // any child process or network connection can be started for it.
    let endpoint = veyra_daemon::invitation_endpoint(&invite).map_err(|error| error.to_string())?;
    let mut s = state.0.lock().await;
    if s.daemon.is_none() {
        network_start_allowed(&s)?;
        let configuration = OperatorConfiguration::invitation(endpoint.clone());
        if s.vault.is_none() {
            begin_session(&mut s, configuration).await?;
        } else {
            session_platform(&mut s)?;
            s.status.connection_setup = configuration.setup;
            attach_daemon(&mut s, endpoint).await?;
        }
    }
    match request(&mut s, Command::JoinInvite { token: invite }).await? {
        Reply::Ok {} => Ok(None),
        Reply::Invite {
            token,
            expires_in_ms,
        } => Ok(Some(Invite {
            token,
            expires_in_ms,
        })),
        _ => Err("Resposta inválida ao receber o convite.".into()),
    }
}
#[tauri::command]
async fn verify_peer(
    window: WebviewWindow,
    state: State<'_, AppState>,
    peer_id: String,
    code: String,
    approved: bool,
) -> Result<PeerSummary, String> {
    local(&window)?;
    id::<32>(&peer_id)?;
    if !approved || code.len() > 80 {
        return Err("Verificação não autorizada.".into());
    }
    let mut s = state.0.lock().await;
    let p = peers(&mut s)
        .await?
        .into_iter()
        .find(|p| p.id == peer_id)
        .ok_or("Contato indisponível.")?;
    if p.verification_code != code || !p.online {
        return Err("Código divergente.".into());
    }
    let approved_binding = (p.binding_frame.clone(), p.verification_code.clone());
    let v = s.vault.clone().ok_or("Sessão indisponível.")?;
    if !matches!(
        v.lock()
            .await
            .request(&Req::VerifyPeer {
                binding: p.binding_frame.clone(),
                approved: true
            })
            .await
            .map_err(|_| "Cofre indisponível.")?,
        Resp::Ok
    ) {
        return Err("Identidade recusada pelo cofre.".into());
    }
    request(
        &mut s,
        Command::ApprovePeer {
            peer_id: peer_id.clone(),
            code,
        },
    )
    .await?;
    s.approved.insert(peer_id.clone(), approved_binding);
    let p = peers(&mut s)
        .await?
        .into_iter()
        .find(|p| p.id == peer_id)
        .ok_or("Contato indisponível.")?;
    state.1.lock().await.sync(std::slice::from_ref(&p));
    Ok(summary(&p, &s.approved))
}
#[tauri::command]
async fn send_message(
    window: WebviewWindow,
    state: State<'_, AppState>,
    peer_ids: Vec<String>,
    text: String,
) -> Result<Vec<Delivery>, String> {
    let text = zeroize::Zeroizing::new(text);
    local(&window)?;
    veyra_domain::validate_text(&text).map_err(|e| e.to_string())?;
    if peer_ids.is_empty() || peer_ids.len() > 16 {
        return Err("Escolha de um a 16 destinatários.".into());
    }
    let mut unique = std::collections::HashSet::new();
    for p in &peer_ids {
        id::<32>(p)?;
        if !unique.insert(p) {
            return Err("Destinatário duplicado.".into());
        }
    }
    let mut s = state.0.lock().await;
    let v = s.vault.clone().ok_or("Sessão indisponível.")?;
    let mut out = vec![];
    for peer_id in peer_ids {
        let mut message_id = None;
        let result = async {
            if !s.approved.contains_key(&peer_id) {
                return Err("O destinatário não foi aprovado pelo coordenador local.".into());
            }
            if s.outgoing.len() >= 128 {
                return Err("Limite de estados de entrega atingido.".into());
            }
            let Reply::Frame { frame: offer } = request(
                &mut s,
                Command::TakeOffer {
                    peer_id: peer_id.clone(),
                },
            )
            .await?
            else {
                return Err("Oferta indisponível.".into());
            };
            let recipient = id::<32>(&peer_id)?;
            veyra_daemon::validate_recipient_offer(&offer, &recipient)
                .map_err(|_| "A oferta não corresponde ao destinatário escolhido.")?;
            let Resp::Frame { frame } = v
                .lock()
                .await
                .seal(offer, &text)
                .await
                .map_err(|_| "Cofre indisponível.")?
            else {
                return Err("Envio recusado pelo cofre.".into());
            };
            let veyra_protocol::Frame::Message(envelope) =
                veyra_protocol::Frame::decode(&frame).map_err(|_| "Envelope inválido.")?
            else {
                return Err("Envelope inválido.".into());
            };
            if envelope.header.recipient != recipient {
                return Err("O envelope não corresponde ao destinatário escolhido.".into());
            }
            let mid = veyra_daemon::hex(&envelope.header.message_id);
            let now = veyra_secure_memory::monotonic_ms().map_err(|_| "Prazo indisponível.")?;
            let deadline = now
                .checked_add(envelope.header.ttl_ms as u64)
                .ok_or("Prazo inválido.")?;
            s.outgoing.insert(mid.clone(), (peer_id.clone(), deadline));
            message_id = Some(mid);
            request(&mut s, Command::SendEnvelope { frame }).await?;
            Ok::<(), String>(())
        }
        .await;
        out.push(match result {
            Ok(()) => Delivery {
                message_id,
                peer_id,
                state: "sent",
                reason: Some("Enviado ao transporte; recebimento ainda não confirmado.".into()),
            },
            Err(reason) => Delivery {
                state: if message_id.is_some() {
                    "delivery-unconfirmed"
                } else {
                    "rejected"
                },
                message_id,
                peer_id,
                reason: Some(reason),
            },
        })
    }
    Ok(out)
}
#[tauri::command]
async fn open_once(
    window: WebviewWindow,
    state: State<'_, AppState>,
    message_id: String,
    confirmed: bool,
) -> Result<Opened, String> {
    local(&window)?;
    if !confirmed {
        return Err("Abertura não autorizada.".into());
    }
    let message_id = id::<16>(&message_id)?;
    let s = state.0.lock().await;
    let v = s.vault.as_ref().ok_or("Sessão indisponível.")?;
    match v
        .lock()
        .await
        .request(&Req::OpenOnce {
            message_id,
            confirmed: true,
        })
        .await
        .map_err(|_| "Cofre indisponível.")?
    {
        Resp::Opened {
            body,
            display_for_ms,
        } => Ok(Opened {
            body,
            display_for_ms,
        }),
        _ => Err("Mensagem consumida, expirada ou recusada.".into()),
    }
}
#[tauri::command]
async fn discard_message(
    window: WebviewWindow,
    state: State<'_, AppState>,
    message_id: String,
) -> Result<(), String> {
    local(&window)?;
    let message_id = id::<16>(&message_id)?;
    let s = state.0.lock().await;
    let v = s.vault.as_ref().ok_or("Sessão indisponível.")?;
    match v
        .lock()
        .await
        .request(&Req::Discard { message_id })
        .await
        .map_err(|_| "Cofre indisponível.")?
    {
        Resp::Ok => Ok(()),
        _ => Err("Descarte recusado.".into()),
    }
}
#[tauri::command]
async fn revoke_peer(
    window: WebviewWindow,
    state: State<'_, AppState>,
    peer_id: String,
) -> Result<(), String> {
    local(&window)?;
    let peer = id::<32>(&peer_id)?;
    let mut s = state.0.lock().await;
    s.approved.remove(&peer_id);
    s.outgoing.retain(|_, (id, _)| id != &peer_id);
    if let Some(v) = s.vault.clone()
        && !matches!(
            v.lock()
                .await
                .request(&Req::Revoke { peer })
                .await
                .map_err(|_| "Cofre indisponível.")?,
            Resp::Ok
        )
    {
        return Err("Revogação recusada.".into());
    }
    if s.daemon.is_some() {
        request(
            &mut s,
            Command::RevokePeer {
                peer_id: peer_id.clone(),
            },
        )
        .await?;
    }
    state.1.lock().await.remove(&peer_id)?;
    s.status.peers.retain(|p| p.id != peer_id);
    Ok(())
}
fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let app = tauri::Builder::default()
        .setup(|app| {
            let book = app
                .path()
                .app_data_dir()
                .map_err(std::io::Error::other)
                .and_then(|directory| ContactBook::open(directory.join("contacts.json")));
            app.manage(AppState(
                Mutex::new(Session::default()),
                Mutex::new(ContactState::new(book)),
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connection_status,
            start_session,
            end_session,
            list_pending,
            create_invite,
            join_invite,
            verify_peer,
            send_message,
            open_once,
            discard_message,
            revoke_peer
        ])
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(|_, _| {}),
        Err(_) => std::process::exit(78),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ContactBook, ContactState, Session, network_start_allowed, summary, trusted_origin,
    };

    #[test]
    fn ending_and_reopening_a_session_keeps_contacts_but_discards_message_state_and_trust() {
        let nonce = veyra_identity::random_id().unwrap();
        let directory = std::env::temp_dir().join(format!("veyra-contact-session-{:x?}", nonce));
        let path = directory.join("contacts.json");
        let peer = veyra_daemon::wire::Peer {
            id: "01".repeat(32),
            binding_frame: vec![1, 2, 3],
            fingerprint: veyra_identity::fingerprint(&[1; 32]),
            verification_code: "ABCD-EF01-2345-6789".into(),
            verified: true,
            online: true,
            route: veyra_connectivity::Route::Direct,
            available_offers: 1,
        };
        let mut contacts = ContactState::new(ContactBook::open(path.clone()));
        contacts.sync(std::slice::from_ref(&peer));
        assert!(contacts.error.is_none());
        let mut session = Session::default();
        session.approved.insert(
            peer.id.clone(),
            (peer.binding_frame.clone(), peer.verification_code.clone()),
        );
        session.status.runtime_id = Some("synthetic-runtime".into());
        session.status.pending_count = 1;
        session
            .status
            .deliveries
            .push(veyra_daemon::DeliveryStatus {
                message_id: "synthetic-message".into(),
                peer_id: peer.id.clone(),
                state: "sent",
                reason: None,
            });
        session.status.peers.push(summary(&peer, &session.approved));
        assert!(contacts.status(&session.status).peers[0].verified);

        session = Session::default();
        let ended = contacts.status(&session.status);
        assert_eq!(ended.peers.len(), 1);
        assert_eq!(ended.peers[0].id, peer.id);
        assert_eq!(ended.peers[0].route, "disconnected");
        assert!(!ended.peers[0].verified);
        assert!(ended.peers[0].verification_code.is_none());
        assert!(ended.runtime_id.is_none());
        assert_eq!(ended.pending_count, 0);
        assert!(ended.deliveries.is_empty());

        drop(contacts);
        let mut reopened = ContactState::new(ContactBook::open(path.clone()));
        assert_eq!(
            reopened.status(&Session::default().status).peers[0].id,
            peer.id
        );
        reopened.remove(&peer.id).unwrap();
        drop(reopened);
        let removed = ContactState::new(ContactBook::open(path));
        assert!(removed.status(&Session::default().status).peers.is_empty());
        drop(removed);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_contact_load_error_is_visible_without_a_session_and_never_replaces_the_file() {
        let mut contacts = ContactState::new(Err(std::io::Error::other("synthetic-load-error")));
        contacts.sync(&[]);
        let status = contacts.status(&Session::default().status);
        assert!(
            status
                .contacts_error
                .as_deref()
                .unwrap()
                .contains("carregar os contatos")
        );
        assert!(status.peers.is_empty());
        assert!(contacts.remove(&"01".repeat(32)).is_err());
    }

    #[test]
    fn approval_is_bound_to_the_current_online_pairing_transcript() {
        let mut peer = veyra_daemon::wire::Peer {
            id: "synthetic-peer".into(),
            binding_frame: vec![1, 2, 3],
            fingerprint: "synthetic-fingerprint".into(),
            verification_code: "ABCD-EF01-2345-6789".into(),
            verified: true,
            online: true,
            route: veyra_connectivity::Route::Direct,
            available_offers: 1,
        };
        let approvals = std::collections::HashMap::from([(
            peer.id.clone(),
            (peer.binding_frame.clone(), peer.verification_code.clone()),
        )]);
        assert!(summary(&peer, &approvals).verified);
        peer.verification_code = "1234-5678-9ABC-DEF0".into();
        assert!(!summary(&peer, &approvals).verified);
        peer.verification_code = "ABCD-EF01-2345-6789".into();
        peer.online = false;
        assert!(!summary(&peer, &approvals).verified);
    }

    #[test]
    fn a_failed_network_child_is_never_replaced_for_the_same_identity() {
        let mut session = Session::default();
        assert!(network_start_allowed(&session).is_ok());
        session.network_attempted = true;
        assert!(session.daemon.is_none());
        assert!(network_start_allowed(&session).is_err());
    }

    #[test]
    fn origin_allowlist_keeps_the_dev_server_local_and_debug_only() {
        assert!(trusted_origin("tauri", Some("localhost"), None));
        assert!(trusted_origin("http", Some("tauri.localhost"), None));
        assert_eq!(
            trusted_origin("http", Some("127.0.0.1"), Some(1420)),
            cfg!(debug_assertions)
        );
        for (scheme, host, port) in [
            ("http", Some("127.0.0.1"), Some(1421)),
            ("http", Some("127.0.0.1"), None),
            ("https", Some("127.0.0.1"), Some(1420)),
            ("http", Some("127.0.0.1.example.com"), Some(1420)),
            ("http", Some("localhost"), Some(1420)),
            ("http", Some("example.com"), Some(1420)),
            ("file", None, None),
        ] {
            assert!(!trusted_origin(scheme, host, port));
        }
    }
}
