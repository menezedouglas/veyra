//! Operator-only TURN REST issuer. E2EE private keys never enter this service.
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::{Zeroize, Zeroizing};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperatorConfiguration {
    urls: Vec<String>,
    auth_secret_file: String,
}
pub struct TurnIssuer {
    urls: Vec<String>,
    secret: Zeroizing<Vec<u8>>,
}
#[derive(Serialize)]
pub struct TurnGrant {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
    pub expires_unix: u64,
}
impl Drop for TurnGrant {
    fn drop(&mut self) {
        self.username.zeroize();
        self.credential.zeroize();
    }
}
impl TurnIssuer {
    /// Protected operator configuration; never a user identity/opening key.
    pub fn load(path: &Path) -> Result<Self, &'static str> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| "operator configuration")?
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "operator configuration")?;
        if bytes.len() > 16 * 1024 {
            return Err("operator configuration limit");
        }
        let config: OperatorConfiguration =
            serde_json::from_slice(&bytes).map_err(|_| "operator configuration format")?;
        if config.urls.is_empty()
            || config.urls.len() > 4
            || config.urls.iter().any(|u| {
                u.len() > 512
                    || !(u.starts_with("stun:")
                        || u.starts_with("turn:")
                        || u.starts_with("turns:"))
                    || u.contains('@')
                    || u.bytes()
                        .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
            })
            || !config
                .urls
                .iter()
                .any(|u| u.starts_with("turn:") || u.starts_with("turns:"))
        {
            return Err("operator endpoints");
        }
        let file = std::fs::File::open(config.auth_secret_file).map_err(|_| "operator secret")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if file
                .metadata()
                .map_err(|_| "operator secret metadata")?
                .permissions()
                .mode()
                & 0o077
                != 0
            {
                return Err("operator secret permissions");
            }
        }
        let mut secret = Zeroizing::new(Vec::new());
        file.take(4097)
            .read_to_end(&mut secret)
            .map_err(|_| "operator secret")?;
        while secret.last().is_some_and(u8::is_ascii_whitespace) {
            secret.pop();
        }
        if secret.len() < 32 || secret.len() > 4096 {
            return Err("operator secret entropy requirement");
        }
        Ok(Self {
            urls: config.urls,
            secret,
        })
    }
    pub fn issue(&self) -> Result<TurnGrant, &'static str> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "operator clock")?
            .as_secs();
        let expires_unix = now.checked_add(300).ok_or("operator clock")?;
        let mut random = [0u8; 16];
        OsRng
            .try_fill_bytes(&mut random)
            .map_err(|_| "operator entropy")?;
        let username = format!("{expires_unix}:{}", hex::encode(random));
        let mut mac = Hmac::<Sha1>::new_from_slice(&self.secret).map_err(|_| "operator HMAC")?;
        mac.update(username.as_bytes());
        let credential = STANDARD.encode(mac.finalize().into_bytes());
        Ok(TurnGrant {
            urls: self.urls.clone(),
            username,
            credential,
            expires_unix,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_random_transient_and_independently_validatable() {
        let issuer = TurnIssuer {
            urls: vec!["turn:localhost:3478?transport=udp".into()],
            secret: Zeroizing::new(vec![42; 32]),
        };
        let a = issuer.issue().unwrap();
        let b = issuer.issue().unwrap();
        assert_ne!(a.username, b.username);
        assert_ne!(a.credential, b.credential);
        assert_eq!(
            a.username
                .split(':')
                .next()
                .unwrap()
                .parse::<u64>()
                .unwrap(),
            a.expires_unix
        );
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!((299..=300).contains(&a.expires_unix.saturating_sub(now)));
        let mut verifier = Hmac::<Sha1>::new_from_slice(&[42; 32]).unwrap();
        verifier.update(a.username.as_bytes());
        verifier
            .verify_slice(&STANDARD.decode(&a.credential).unwrap())
            .unwrap();
        let mut wrong = Hmac::<Sha1>::new_from_slice(&[43; 32]).unwrap();
        wrong.update(a.username.as_bytes());
        assert!(
            wrong
                .verify_slice(&STANDARD.decode(&a.credential).unwrap())
                .is_err()
        );
    }
}
