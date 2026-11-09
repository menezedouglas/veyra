//! Pure bounded policies; caller supplies monotonic elapsed milliseconds.
use thiserror::Error;
use veyra_protocol::{MAX_TEXT, MAX_TTL_MS};
pub const MAX_PENDING_PER_PEER: usize = 16;
pub const MAX_PENDING: usize = 128;
pub const MAX_PEERS: usize = 16;
pub const MAX_OFFERS: usize = 128;
pub const MAX_VIEW_MS: u64 = 30_000;
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("texto inválido: máximo de 4 KiB UTF-8; sem controles não imprimíveis")]
    InvalidText,
    #[error("prazo inválido ou expirado")]
    Expired,
    #[error("transição de estado inválida")]
    InvalidTransition,
}
pub fn validate_text(text: &str) -> Result<(), Error> {
    if text.is_empty()
        || text.len() > MAX_TEXT
        || text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(Error::InvalidText);
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline(u64);
impl Deadline {
    pub fn new(now: u64, ttl: u32) -> Result<Self, Error> {
        if ttl == 0 || ttl > MAX_TTL_MS {
            return Err(Error::Expired);
        }
        Ok(Self(now.checked_add(ttl as u64).ok_or(Error::Expired)?))
    }
    pub fn expired(self, now: u64) -> bool {
        now >= self.0
    }
    pub fn remaining_ms(self, now: u64) -> u64 {
        self.0.saturating_sub(now)
    }
    pub fn end_ms(self) -> u64 {
        self.0
    }
    pub fn cap(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendState {
    Sealed,
    WaitingForPeer,
    Sent,
    Acknowledged,
    DeliveryUnconfirmed,
    Expired,
    Cancelled,
    Rejected,
}
impl SendState {
    pub fn transition(self, next: Self) -> Result<Self, Error> {
        use SendState::*;
        if matches!(
            (self, next),
            (Sealed, WaitingForPeer | Sent | Cancelled | Expired)
                | (WaitingForPeer, Sent | Expired | Cancelled)
                | (
                    Sent,
                    Acknowledged | DeliveryUnconfirmed | Expired | Rejected
                )
                | (
                    DeliveryUnconfirmed,
                    Sent | Acknowledged | Expired | Cancelled
                )
        ) {
            Ok(next)
        } else {
            Err(Error::InvalidTransition)
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveState {
    ReceivedInRam,
    Claimed,
    Displaying,
    Destroyed,
    Expired,
    Cancelled,
    Rejected,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadline_is_not_a_retry_counter() {
        let d = Deadline::new(10, 100).unwrap();
        assert_eq!(d.remaining_ms(60), 50);
        assert!(d.expired(110));
        assert!(Deadline::new(u64::MAX, 1).is_err());
    }
    #[test]
    fn rejects_controls_and_multibyte_overflow() {
        assert!(validate_text("ola\n🙂").is_ok());
        assert!(validate_text("\0").is_err());
        assert!(validate_text(&"🙂".repeat(1025)).is_err());
    }
    #[test]
    fn acknowledged_is_terminal() {
        assert!(SendState::Acknowledged.transition(SendState::Sent).is_err());
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]
        #[test]
        fn advancing_time_cannot_extend_deadline(start in 0u64..1_000_000, ttl in 1u32..=300_000, step in 0u64..1_000_000) {
            let d=Deadline::new(start,ttl).unwrap();
            prop_assert!(d.remaining_ms(start+step) <= d.remaining_ms(start));
            prop_assert_eq!(d.end_ms(),start+ttl as u64);
        }
    }
}
