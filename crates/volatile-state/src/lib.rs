//! Bounded replay metadata. Live entries are never evicted to admit new work.
use std::collections::HashMap;
use veyra_domain::{Deadline, MAX_PENDING, MAX_PENDING_PER_PEER};
use veyra_protocol::{Id, Identity};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acceptance {
    New,
    Duplicate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Replay,
    Full,
}
struct Entry {
    digest: [u8; 32],
    key_id: Id,
    deadline: Deadline,
}
#[derive(Default)]
pub struct DedupCache {
    entries: HashMap<(Identity, Id), Entry>,
}
impl DedupCache {
    pub fn prune(&mut self, now: u64) {
        self.entries.retain(|_, e| !e.deadline.expired(now));
    }
    pub fn check(
        &self,
        sender: Identity,
        id: Id,
        key_id: Id,
        digest: [u8; 32],
    ) -> Result<Acceptance, Error> {
        if let Some(e) = self.entries.get(&(sender, id)) {
            return if e.digest == digest && e.key_id == key_id {
                Ok(Acceptance::Duplicate)
            } else {
                Err(Error::Replay)
            };
        }
        if self
            .entries
            .iter()
            .any(|((s, _), e)| *s == sender && e.key_id == key_id)
        {
            return Err(Error::Replay);
        }
        Ok(Acceptance::New)
    }
    pub fn insert(
        &mut self,
        sender: Identity,
        id: Id,
        key_id: Id,
        digest: [u8; 32],
        deadline: Deadline,
    ) -> Result<Acceptance, Error> {
        let a = self.check(sender, id, key_id, digest)?;
        if a == Acceptance::Duplicate {
            return Ok(a);
        }
        if self.entries.len() >= MAX_PENDING
            || self.entries.keys().filter(|(s, _)| *s == sender).count() >= MAX_PENDING_PER_PEER
        {
            return Err(Error::Full);
        }
        self.entries.insert(
            (sender, id),
            Entry {
                digest,
                key_id,
                deadline,
            },
        );
        Ok(Acceptance::New)
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn revoke(&mut self, peer: &Identity) {
        self.entries.retain(|(s, _), _| s != peer);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicates_idempotent_conflicts_rejected() {
        let mut d = DedupCache::default();
        let deadline = Deadline::new(0, 100).unwrap();
        assert_eq!(
            d.insert([1; 32], [2; 16], [3; 16], [4; 32], deadline),
            Ok(Acceptance::New)
        );
        assert_eq!(
            d.insert([1; 32], [2; 16], [3; 16], [4; 32], deadline),
            Ok(Acceptance::Duplicate)
        );
        assert_eq!(
            d.insert([1; 32], [9; 16], [3; 16], [4; 32], deadline),
            Err(Error::Replay)
        );
        assert_eq!(
            d.insert([1; 32], [2; 16], [3; 16], [8; 32], deadline),
            Err(Error::Replay)
        );
    }
    #[test]
    fn backpressure_preserves_live_replay_entries() {
        let mut d = DedupCache::default();
        for i in 0..MAX_PENDING_PER_PEER {
            d.insert(
                [1; 32],
                [i as u8; 16],
                [i as u8; 16],
                [i as u8; 32],
                Deadline::new(0, 100).unwrap(),
            )
            .unwrap();
        }
        assert_eq!(
            d.insert(
                [1; 32],
                [99; 16],
                [99; 16],
                [99; 32],
                Deadline::new(0, 100).unwrap()
            ),
            Err(Error::Full)
        );
        d.prune(100);
        assert!(d.is_empty());
    }
}
