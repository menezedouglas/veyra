//! Bounded IPC on anonymous, inherited OS pipes; no local or remote listener.
//! The parent owns both child handles. Do not reconnect a crashed worker.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use zeroize::Zeroizing;

pub const MAX_IPC_FRAME: usize = 32768;
pub const BOOTSTRAP_LEN: usize = 32;

/// Authenticate the inherited Linux channel's OS owner and permissions before
/// accepting bootstrap material. There is no discoverable listener or pathname.
#[cfg(target_os = "linux")]
pub fn validate_inherited_stdio() -> io::Result<()> {
    validate_private_pipe(libc::STDIN_FILENO)?;
    validate_private_pipe(libc::STDOUT_FILENO)
}
/// Other platforms must provide their required OS-authenticated channel before
/// starting a session. In particular, anonymous Windows pipes do not implement
/// the required explicit named-pipe ACL and remote-client rejection policy.
#[cfg(not(target_os = "linux"))]
pub fn validate_inherited_stdio() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "verified platform IPC is unavailable",
    ))
}
#[cfg(target_os = "linux")]
fn validate_private_pipe(fd: i32) -> io::Result<()> {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd, status.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstat initialized the complete native stat structure.
    let status = unsafe { status.assume_init() };
    if status.st_mode & libc::S_IFMT != libc::S_IFIFO
        || status.st_uid != unsafe { libc::geteuid() }
        || status.st_mode & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "IPC must be an inherited owner-only pipe",
        ));
    }
    Ok(())
}

pub fn write_frame(w: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_IPC_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "IPC frame limit",
        ));
    }
    w.write_all(&(bytes.len() as u32).to_be_bytes())?;
    w.write_all(bytes)?;
    w.flush()
}
pub fn read_frame(r: &mut impl Read) -> io::Result<Zeroizing<Vec<u8>>> {
    let mut prefix = [0; 4];
    r.read_exact(&mut prefix)?;
    let n = u32::from_be_bytes(prefix) as usize;
    if n == 0 || n > MAX_IPC_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC frame limit",
        ));
    }
    let mut body = Zeroizing::new(vec![0; n]);
    r.read_exact(&mut body)?;
    Ok(body)
}
pub fn send<T: Serialize>(w: &mut impl Write, value: &T) -> io::Result<()> {
    let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(|_| invalid())?);
    write_frame(w, &bytes)
}
pub fn receive<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<T> {
    serde_json::from_slice(&read_frame(r)?).map_err(|_| invalid())
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid IPC schema")
}

pub fn encode<T: Serialize>(value: &T) -> io::Result<Zeroizing<Vec<u8>>> {
    let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(|_| invalid())?);
    if bytes.len() > MAX_IPC_FRAME {
        return Err(invalid());
    }
    Ok(bytes)
}
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    if bytes.len() > MAX_IPC_FRAME {
        return Err(invalid());
    }
    serde_json::from_slice(bytes).map_err(|_| invalid())
}
pub fn bootstrap_challenge(challenge: &mut Vec<u8>, expected: &mut Vec<u8>) -> io::Result<()> {
    challenge.resize(32, 0);
    getrandom::getrandom(challenge).map_err(|_| invalid())?;
    let mut hash = Sha256::new();
    hash.update(b"Veyra/child-bootstrap/v1");
    hash.update(challenge);
    expected.extend_from_slice(&hash.finalize());
    Ok(())
}
pub fn bootstrap_matches(a: &[u8], b: &[u8]) -> bool {
    constant_eq(a, b)
}

/// Challenge travels only through inherited pipe handles, never argv/env/files.
pub fn parent_authenticate(w: &mut impl Write, r: &mut impl Read) -> io::Result<()> {
    let mut token = Zeroizing::new([0; BOOTSTRAP_LEN]);
    getrandom::getrandom(token.as_mut()).map_err(|_| invalid())?;
    w.write_all(token.as_ref())?;
    w.flush()?;
    let expected =
        Sha256::digest([b"Veyra/child-bootstrap/v1".as_slice(), token.as_ref()].concat());
    let mut actual = [0; 32];
    r.read_exact(&mut actual)?;
    if !constant_eq(&actual, &expected) {
        return Err(invalid());
    }
    Ok(())
}
pub fn child_authenticate(r: &mut impl Read, w: &mut impl Write) -> io::Result<()> {
    let mut token = Zeroizing::new([0; BOOTSTRAP_LEN]);
    r.read_exact(token.as_mut())?;
    let response =
        Sha256::digest([b"Veyra/child-bootstrap/v1".as_slice(), token.as_ref()].concat());
    w.write_all(&response)?;
    w.flush()
}
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (x, y) in a.iter().zip(b) {
        difference |= x ^ y;
    }
    difference == 0
}

// No signing or decryption primitives exposed. Each operation is bound to a
// specific identity/message and validated by the vault again.
#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum VaultRequest {
    Status {},
    BindTransport {
        noise_public: [u8; 32],
    },
    SignSignal {
        document: veyra_protocol::SignalDocument,
    },
    VerifyPeer {
        binding: Vec<u8>,
        approved: bool,
    },
    CreateOffer {
        peer: [u8; 32],
        ttl_ms: u32,
    },
    Seal {
        offer: Vec<u8>,
        text: String,
    },
    AcceptEnvelope {
        frame: Vec<u8>,
    },
    ListPending {},
    OpenOnce {
        message_id: [u8; 16],
        confirmed: bool,
    },
    Discard {
        message_id: [u8; 16],
    },
    Revoke {
        peer: [u8; 32],
    },
    End {},
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingMessage {
    pub id: String,
    pub peer_id: String,
    pub expires_in_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum VaultResponse {
    SignalSigned {
        signature: Vec<u8>,
    },
    Identity {
        binding: Vec<u8>,
        fingerprint: String,
        runtime_id: String,
    },
    Frame {
        frame: Vec<u8>,
    },
    Pending {
        messages: Vec<PendingMessage>,
    },
    Opened {
        body: String,
        display_for_ms: u64,
    },
    Ok,
    Refused {
        code: Refusal,
    },
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    InvalidRequest,
    Unauthorized,
    Expired,
    AlreadyConsumed,
    ResourceLimit,
    MemoryProtection,
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(target_os = "linux")]
    fn inherited_channel_checks_kernel_owner_mode_and_type() {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        assert!(validate_private_pipe(read.as_raw_fd()).is_ok());
        assert!(validate_private_pipe(write.as_raw_fd()).is_ok());
        assert_eq!(unsafe { libc::fchmod(read.as_raw_fd(), 0o640) }, 0);
        assert!(validate_private_pipe(read.as_raw_fd()).is_err());
        let null = std::fs::File::open("/dev/null").unwrap();
        assert!(validate_private_pipe(null.as_raw_fd()).is_err());
    }
    #[test]
    fn rejects_length_before_allocation() {
        for n in [0u32, (MAX_IPC_FRAME + 1) as u32, u32::MAX] {
            assert!(read_frame(&mut n.to_be_bytes().as_slice()).is_err());
        }
    }
    #[test]
    fn truncated_frame_is_not_accepted() {
        assert!(read_frame(&mut [0, 0, 0, 3, 1, 2].as_slice()).is_err());
    }
    #[test]
    fn no_generic_sign_or_decrypt_command() {
        for op in ["sign", "decrypt", "recover", "export"] {
            let request = format!("{{\"operation\":\"{op}\"}}");
            assert!(serde_json::from_str::<VaultRequest>(&request).is_err());
        }
        assert!(
            serde_json::from_str::<VaultRequest>(r#"{"operation":"status","extra":1}"#).is_err()
        );
    }
    #[test]
    fn ipc_round_trip() {
        let mut bytes = Vec::new();
        send(&mut bytes, &VaultRequest::Status {}).unwrap();
        assert!(matches!(
            receive::<VaultRequest>(&mut bytes.as_slice()).unwrap(),
            VaultRequest::Status {}
        ));
    }
}

#[cfg(test)]
mod no_private_transport_api_tests {
    use super::*;
    #[test]
    fn obsolete_private_transport_exchange_is_rejected() {
        assert!(
            serde_json::from_str::<VaultRequest>(r#"{"operation":"prepare_transport"}"#).is_err()
        );
        assert!(
            serde_json::from_str::<VaultResponse>(
                r#"{"result":"transport_prepared","noise_private":[1,2,3]}"#
            )
            .is_err()
        );
        let request = VaultRequest::BindTransport {
            noise_public: [7; 32],
        };
        let encoded = encode(&request).unwrap();
        assert!(
            !encoded
                .windows(b"noise_private".len())
                .any(|part| part == b"noise_private")
        );
    }
}
