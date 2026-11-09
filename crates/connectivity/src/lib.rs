//! Public route metadata only; no addresses, tokens, SDP or message bodies.
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    #[default]
    Unknown,
    Direct,
    Relay,
}

/// The operator secret never belongs here. Credentials must be short lived.
#[derive(Clone, Deserialize, Serialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

impl Drop for IceServer {
    fn drop(&mut self) {
        self.username.zeroize();
        self.credential.zeroize();
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct TransportConfig {
    pub ice_servers: Vec<IceServer>,
    pub relay_only: bool,
    pub bind_addrs: Vec<String>,
    /// Public operator CA certificates, explicitly configured locally. Never
    /// accept these from invitations, SDP or signaling. Empty uses OS trust.
    #[serde(default)]
    pub tls_roots_der: Vec<Vec<u8>>,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            ice_servers: Vec::new(),
            relay_only: false,
            bind_addrs: vec!["0.0.0.0:0".into(), "[::]:0".into()],
            tls_roots_der: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    Limit,
    MissingRelay,
    UnsupportedTurnTcpTls,
    InvalidUrl,
}

impl TransportConfig {
    /// webrtc 0.21.1 supports UDP TURN only. Reject rather than silently skip.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_for_backend(false)
    }
    pub fn validate_native(&self) -> Result<(), ConfigError> {
        self.validate_for_backend(true)
    }
    fn validate_for_backend(&self, tcp_tls: bool) -> Result<(), ConfigError> {
        if self.tls_roots_der.len() > 4
            || self.tls_roots_der.iter().any(|c| c.len() > 8192)
            || self.ice_servers.iter().map(|s| s.urls.len()).sum::<usize>() > 4
        {
            return Err(ConfigError::Limit);
        }
        if self.ice_servers.len() > 4 || self.bind_addrs.is_empty() || self.bind_addrs.len() > 8 {
            return Err(ConfigError::Limit);
        }
        let mut has_turn = false;
        for server in &self.ice_servers {
            if server.urls.is_empty()
                || server.urls.len() > 4
                || server.username.len() > 128
                || server.credential.len() > 256
            {
                return Err(ConfigError::Limit);
            }
            for url in &server.urls {
                if url.len() > 512
                    || url
                        .bytes()
                        .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
                {
                    return Err(ConfigError::InvalidUrl);
                }
                if !tcp_tls && (url.starts_with("turns:") || url.contains("transport=tcp")) {
                    return Err(ConfigError::UnsupportedTurnTcpTls);
                }
                if !(url.starts_with("stun:")
                    || url.starts_with("turn:")
                    || (tcp_tls && url.starts_with("turns:")))
                {
                    return Err(ConfigError::InvalidUrl);
                }
                if let Some((_, query)) = url.split_once('?')
                    && query != "transport=udp"
                    && query != "transport=tcp"
                {
                    return Err(ConfigError::InvalidUrl);
                }
                if url.starts_with("turn:") || url.starts_with("turns:") {
                    if server.username.is_empty() || server.credential.is_empty() {
                        return Err(ConfigError::InvalidUrl);
                    }
                    has_turn = true;
                }
            }
        }
        if self.relay_only && !has_turn {
            return Err(ConfigError::MissingRelay);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_turn_is_refused_explicitly() {
        for url in ["turns:localhost:5349", "turn:localhost:3478?transport=tcp"] {
            let cfg = TransportConfig {
                ice_servers: vec![IceServer {
                    urls: vec![url.into()],
                    username: "temporary".into(),
                    credential: "synthetic".into(),
                }],
                ..Default::default()
            };
            assert_eq!(cfg.validate(), Err(ConfigError::UnsupportedTurnTcpTls));
        }
    }

    #[test]
    fn relay_cannot_claim_configuration_without_a_server() {
        let cfg = TransportConfig {
            relay_only: true,
            ..Default::default()
        };
        assert_eq!(cfg.validate(), Err(ConfigError::MissingRelay));
    }
}
