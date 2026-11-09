//! Volatile key custody and typed, message-bound local opening authority.
//! Process isolation is provided by the host, not by these Rust object boundaries.
use std::collections::HashMap;
use subtle::ConstantTimeEq;
use thiserror::Error;
use veyra_crypto::{generate_opening_key, OpeningKey};
use veyra_domain::{Deadline, MAX_OFFERS, MAX_PEERS, MAX_PENDING, MAX_PENDING_PER_PEER};
use veyra_identity::{random_id, random_secret, verify_binding, verify_envelope, RuntimeIdentity};
use veyra_protocol::{Envelope, Id, Identity, IdentityBinding, KeyOffer, Receipt};
use veyra_secure_memory::LockedBuffer;
use veyra_state::{Acceptance, DedupCache};
use zeroize::Zeroizing;
#[derive(Debug, Error)]
pub enum Error {
    #[error("proteção de memória indisponível")]
    Memory,
    #[error("operação não autorizada ou identidade não verificada")]
    Unauthorized,
    #[error("mensagem, chave ou prazo indisponível")]
    Unavailable,
    #[error("limite de recursos atingido")]
    Capacity,
    #[error("envelope inválido ou replay conflitante")]
    Invalid,
}
/// Kept solely in the trusted local UI/coordinator; never sent to the network daemon.
pub struct LocalAuthority {
    secret: LockedBuffer,
    runtime: Id,
}
pub struct OpenAuthorization {
    message_id: Id,
    runtime: Id,
    secret: Zeroizing<[u8; 32]>,
}
impl LocalAuthority {
    pub fn authorize_open(&self, message_id: Id) -> OpenAuthorization {
        let mut secret = Zeroizing::new([0; 32]);
        secret.copy_from_slice(self.secret.as_slice());
        OpenAuthorization {
            message_id,
            runtime: self.runtime,
            secret,
        }
    }
}
struct OfferEntry {
    offer: KeyOffer,
    key: Option<OpeningKey>,
    deadline: Deadline,
    accepted: Option<Id>,
}
struct PendingEntry {
    envelope: Envelope,
    deadline: Deadline,
}
#[derive(Clone, Debug)]
pub struct Pending {
    pub message_id: Id,
    pub sender: Identity,
    pub remaining_ms: u64,
    pub bytes: usize,
}
pub struct Vault {
    identity: RuntimeIdentity,
    authority: LockedBuffer,
    peers: HashMap<Identity, IdentityBinding>,
    offers: HashMap<Id, OfferEntry>,
    pending: HashMap<Id, PendingEntry>,
    dedup: DedupCache,
    last_tick: u64,
    transport_binding_locked: bool,
}
impl Vault {
    pub fn new() -> Result<(Self, LocalAuthority), Error> {
        let identity = RuntimeIdentity::generate().map_err(|_| Error::Memory)?;
        let secret = random_secret().map_err(|_| Error::Memory)?;
        let mut a = LockedBuffer::new(32).map_err(|_| Error::Memory)?;
        a.as_mut_slice().copy_from_slice(&*secret);
        let mut b = LockedBuffer::new(32).map_err(|_| Error::Memory)?;
        b.as_mut_slice().copy_from_slice(&*secret);
        let local = LocalAuthority {
            secret: b,
            runtime: identity.runtime_id(),
        };
        Ok((
            Self {
                identity,
                authority: a,
                peers: HashMap::new(),
                offers: HashMap::new(),
                pending: HashMap::new(),
                dedup: DedupCache::default(),
                last_tick: 0,
                transport_binding_locked: false,
            },
            local,
        ))
    }
    pub fn binding(&self) -> IdentityBinding {
        self.identity.binding()
    }
    /// Accept only a public key, once and before authorizing any peer. A
    /// revoked peer does not reset this registration boundary.
    pub fn bind_transport(&mut self, noise_public: [u8; 32]) -> Result<IdentityBinding, Error> {
        if self.transport_binding_locked
            || !self.peers.is_empty()
            || !self.offers.is_empty()
            || !self.pending.is_empty()
        {
            return Err(Error::Unauthorized);
        }
        let binding = self
            .identity
            .bind_transport_public(noise_public)
            .map_err(|_| Error::Unauthorized)?;
        self.transport_binding_locked = true;
        Ok(binding)
    }
    pub fn identity(&self) -> &RuntimeIdentity {
        &self.identity
    }
    pub fn verified_peers(&self) -> Vec<IdentityBinding> {
        self.peers.values().cloned().collect()
    }
    fn authorized(&self, a: &LocalAuthority) -> bool {
        a.runtime == self.identity.runtime_id()
            && bool::from(a.secret.as_slice().ct_eq(self.authority.as_slice()))
    }
    pub fn verify_peer(&mut self, b: IdentityBinding, a: &LocalAuthority) -> Result<(), Error> {
        if !self.authorized(a) || b.identity == self.identity.public() {
            return Err(Error::Unauthorized);
        }
        verify_binding(&b, &b.noise_public).map_err(|_| Error::Unauthorized)?;
        if self.peers.get(&b.identity).is_some_and(|old| old != &b) {
            self.revoke_peer(&b.identity);
            return Err(Error::Unauthorized);
        }
        if !self.peers.contains_key(&b.identity) && self.peers.len() >= MAX_PEERS {
            return Err(Error::Capacity);
        }
        self.transport_binding_locked = true;
        self.peers.insert(b.identity, b);
        Ok(())
    }
    /// Caller must additionally invalidate on suspend/resume when elapsed time cannot be trusted.
    pub fn tick(&mut self, now: u64) {
        if now < self.last_tick {
            self.pending.clear();
            self.offers.clear();
            self.dedup = DedupCache::default();
        }
        self.last_tick = now;
        let offers = &mut self.offers;
        self.pending.retain(|_, message| {
            if message.deadline.expired(now) {
                // An envelope may expire before its original offer. Destroy
                // its opening key now; retain only replay metadata until the
                // original offer deadline so duplicates cannot restore it.
                if let Some(offer) = offers.get_mut(&message.envelope.header.key_id) {
                    drop(offer.key.take());
                }
                false
            } else {
                true
            }
        });
        self.offers.retain(|_, k| !k.deadline.expired(now));
        self.dedup.prune(now);
    }
    pub fn invalidate_on_resume(&mut self) {
        self.pending.clear();
        self.offers.clear();
        self.dedup = DedupCache::default();
    }
    pub fn create_offer(
        &mut self,
        peer: &Identity,
        now: u64,
        ttl_ms: u32,
    ) -> Result<KeyOffer, Error> {
        self.tick(now);
        let b = self.peers.get(peer).ok_or(Error::Unauthorized)?;
        if self.offers.len() >= MAX_OFFERS
            || self
                .offers
                .values()
                .filter(|o| o.offer.sender == *peer)
                .count()
                >= MAX_PENDING_PER_PEER
        {
            return Err(Error::Capacity);
        }
        let deadline = Deadline::new(now, ttl_ms).map_err(|_| Error::Unavailable)?;
        let (key, public_key) = generate_opening_key().map_err(|_| Error::Memory)?;
        let key_id = random_id().map_err(|_| Error::Memory)?;
        if self.offers.contains_key(&key_id) {
            return Err(Error::Capacity);
        }
        let mut offer = KeyOffer {
            sender: *peer,
            recipient: self.identity.public(),
            sender_runtime: b.runtime_id,
            recipient_runtime: self.identity.runtime_id(),
            key_id,
            public_key,
            ttl_ms,
            signature: [0; 64],
        };
        self.identity
            .sign_offer(&mut offer)
            .map_err(|_| Error::Invalid)?;
        self.offers.insert(
            key_id,
            OfferEntry {
                offer: offer.clone(),
                key: Some(key),
                deadline,
                accepted: None,
            },
        );
        Ok(offer)
    }
    pub fn seal(&self, offer: &KeyOffer, text: &str) -> Result<Envelope, Error> {
        let peer = self
            .peers
            .get(&offer.recipient)
            .ok_or(Error::Unauthorized)?;
        if offer.recipient_runtime != peer.runtime_id {
            return Err(Error::Unauthorized);
        }
        veyra_crypto::seal(&self.identity, offer, text).map_err(|_| Error::Invalid)
    }
    /// Signature and binding validated before receiver key consumption.
    pub fn accept_envelope(&mut self, envelope: Envelope, now: u64) -> Result<Receipt, Error> {
        self.tick(now);
        let h = &envelope.header;
        let peer = self.peers.get(&h.sender).ok_or(Error::Unauthorized)?;
        if h.recipient != self.identity.public()
            || h.recipient_runtime != self.identity.runtime_id()
            || h.sender_runtime != peer.runtime_id
        {
            return Err(Error::Unauthorized);
        }
        verify_envelope(&envelope).map_err(|_| Error::Invalid)?;
        let digest = envelope.digest();
        if self
            .dedup
            .check(h.sender, h.message_id, h.key_id, digest)
            .map_err(|_| Error::Invalid)?
            == Acceptance::Duplicate
        {
            return Ok(self.identity.receipt(h.message_id, digest));
        }
        let entry = self.offers.get_mut(&h.key_id).ok_or(Error::Unavailable)?;
        if entry.key.is_none()
            || entry.accepted.is_some()
            || entry.deadline.expired(now)
            || entry.offer.sender != h.sender
            || entry.offer.sender_runtime != h.sender_runtime
            || h.ttl_ms > entry.offer.ttl_ms
        {
            return Err(Error::Invalid);
        }
        if self.pending.contains_key(&h.message_id) {
            return Err(Error::Invalid);
        }
        if self.pending.len() >= MAX_PENDING
            || self
                .pending
                .values()
                .filter(|m| m.envelope.header.sender == h.sender)
                .count()
                >= MAX_PENDING_PER_PEER
        {
            return Err(Error::Capacity);
        }
        let deadline = entry
            .deadline
            .cap(Deadline::new(now, h.ttl_ms).map_err(|_| Error::Invalid)?);
        self.dedup
            .insert(h.sender, h.message_id, h.key_id, digest, entry.deadline)
            .map_err(|_| Error::Capacity)?;
        entry.accepted = Some(h.message_id);
        let receipt = self.identity.receipt(h.message_id, digest);
        self.pending
            .insert(h.message_id, PendingEntry { envelope, deadline });
        Ok(receipt)
    }
    pub fn list_pending(&mut self, now: u64) -> Vec<Pending> {
        self.tick(now);
        self.pending
            .values()
            .map(|m| Pending {
                message_id: m.envelope.header.message_id,
                sender: m.envelope.header.sender,
                remaining_ms: m.deadline.remaining_ms(now),
                bytes: m.envelope.ciphertext.len(),
            })
            .collect()
    }
    pub fn open_once(
        &mut self,
        authorization: OpenAuthorization,
        now: u64,
    ) -> Result<Zeroizing<String>, Error> {
        if authorization.runtime != self.identity.runtime_id()
            || !bool::from(
                authorization
                    .secret
                    .as_slice()
                    .ct_eq(self.authority.as_slice()),
            )
        {
            return Err(Error::Unauthorized);
        }
        self.tick(now);
        // CLAIMED: remove before decryption; errors never restore message or opening key.
        let m = self
            .pending
            .remove(&authorization.message_id)
            .ok_or(Error::Unavailable)?;
        let entry = self
            .offers
            .get_mut(&m.envelope.header.key_id)
            .ok_or(Error::Unavailable)?;
        let key = entry.key.take().ok_or(Error::Unavailable)?;
        if m.deadline.expired(now) {
            return Err(Error::Unavailable);
        }
        veyra_crypto::open(key, &m.envelope).map_err(|_| Error::Invalid)
    }
    pub fn discard_message(&mut self, id: &Id) {
        if let Some(m) = self.pending.remove(id) {
            if let Some(k) = self.offers.get_mut(&m.envelope.header.key_id) {
                k.key.take();
            }
        }
    }
    pub fn revoke_peer(&mut self, peer: &Identity) {
        self.peers.remove(peer);
        self.pending
            .retain(|_, m| &m.envelope.header.sender != peer);
        self.offers.retain(|_, k| &k.offer.sender != peer);
        self.dedup.revoke(peer);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pair() -> (Vault, LocalAuthority, Vault, LocalAuthority) {
        let (mut a, aa) = Vault::new().unwrap();
        let (mut b, ba) = Vault::new().unwrap();
        a.verify_peer(b.binding(), &aa).unwrap();
        b.verify_peer(a.binding(), &ba).unwrap();
        (a, aa, b, ba)
    }
    fn message(a: &Vault, b: &mut Vault) -> Envelope {
        let o = b.create_offer(&a.binding().identity, 0, 1000).unwrap();
        a.seal(&o, "mensagem de teste").unwrap()
    }
    #[test]
    fn no_unverified_send_or_offer() {
        let (mut a, _) = Vault::new().unwrap();
        let (b, _) = Vault::new().unwrap();
        assert!(a.create_offer(&b.binding().identity, 0, 1000).is_err());
    }
    #[test]
    fn one_open_and_duplicate_stays_receipt_only() {
        let (a, _, mut b, ba) = pair();
        let e = message(&a, &mut b);
        let id = e.header.message_id;
        let r = b.accept_envelope(e.clone(), 10).unwrap();
        assert_eq!(b.list_pending(10).len(), 1);
        assert_eq!(
            b.open_once(ba.authorize_open(id), 11).unwrap().as_str(),
            "mensagem de teste"
        );
        assert!(b.open_once(ba.authorize_open(id), 12).is_err());
        assert_eq!(b.accept_envelope(e, 12).unwrap(), r);
        assert!(b.list_pending(12).is_empty());
    }
    #[test]
    fn failed_claim_never_restores_key() {
        let (a, _, mut b, ba) = pair();
        let mut e = message(&a, &mut b);
        e.ciphertext[0] ^= 1;
        a.identity.sign_envelope(&mut e).unwrap();
        let id = e.header.message_id;
        b.accept_envelope(e, 10).unwrap();
        assert!(b.open_once(ba.authorize_open(id), 11).is_err());
        assert!(b.open_once(ba.authorize_open(id), 12).is_err());
        assert!(b.list_pending(12).is_empty());
    }
    #[test]
    fn concurrent_open_has_single_winner() {
        let (a, _, mut b, ba) = pair();
        let e = message(&a, &mut b);
        let id = e.header.message_id;
        b.accept_envelope(e, 1).unwrap();
        let one = ba.authorize_open(id);
        let two = ba.authorize_open(id);
        let b = std::sync::Arc::new(std::sync::Mutex::new(b));
        let c = b.clone();
        let t = std::thread::spawn(move || c.lock().unwrap().open_once(one, 2).is_ok());
        let second = b.lock().unwrap().open_once(two, 2).is_ok();
        assert_ne!(t.join().unwrap(), second);
    }
    #[test]
    fn expiry_anchored_to_original_offer() {
        let (a, _, mut b, ba) = pair();
        let e = message(&a, &mut b);
        let id = e.header.message_id;
        b.accept_envelope(e.clone(), 990).unwrap();
        assert_eq!(b.list_pending(990)[0].remaining_ms, 10);
        assert!(b.open_once(ba.authorize_open(id), 1000).is_err());
        assert!(b.accept_envelope(e, 1000).is_err());
    }
    #[test]
    fn shorter_envelope_expiry_destroys_key_but_preserves_replay_metadata() {
        let (a, _, mut b, authority) = pair();
        let mut public_offer = b.create_offer(&a.binding().identity, 0, 5000).unwrap();
        let key_id = public_offer.key_id;
        // Receiver authorizes a shorter public validity without extending the
        // original local offer deadline. Both signatures remain genuine.
        public_offer.ttl_ms = 1000;
        b.identity.sign_offer(&mut public_offer).unwrap();
        let envelope = a.seal(&public_offer, "short expiry fixture").unwrap();
        let message_id = envelope.header.message_id;
        let receipt = b.accept_envelope(envelope.clone(), 0).unwrap();
        assert_eq!(b.list_pending(999).len(), 1);
        assert!(b.offers.get(&key_id).unwrap().key.is_some());

        b.tick(1001);
        assert!(b.pending.is_empty());
        let retained = b.offers.get(&key_id).unwrap();
        assert!(!retained.deadline.expired(1001));
        assert!(
            retained.key.is_none(),
            "expired envelope retained its opening key"
        );
        assert_eq!(b.dedup.len(), 1);
        assert!(b
            .open_once(authority.authorize_open(message_id), 1001)
            .is_err());
        assert_eq!(b.accept_envelope(envelope, 1002).unwrap(), receipt);
        assert!(b.pending.is_empty());
        assert!(b.offers.get(&key_id).unwrap().key.is_none());

        b.tick(5000);
        assert!(b.offers.is_empty());
        assert!(b.dedup.is_empty());
    }
    #[test]
    fn wrong_authority_cannot_claim() {
        let (a, aa, mut b, ba) = pair();
        let e = message(&a, &mut b);
        let id = e.header.message_id;
        b.accept_envelope(e, 1).unwrap();
        assert!(b.open_once(aa.authorize_open(id), 2).is_err());
        assert!(b.open_once(ba.authorize_open(id), 2).is_ok());
    }
    #[test]
    fn old_runtime_and_reuse_rejected() {
        let (a, _, mut b, _) = pair();
        let e = message(&a, &mut b);
        b.accept_envelope(e.clone(), 1).unwrap();
        let mut e2 = e.clone();
        e2.header.message_id = [99; 16];
        a.identity.sign_envelope(&mut e2).unwrap();
        assert!(b.accept_envelope(e2, 2).is_err());
        let mut e2 = e;
        e2.header.sender_runtime[0] ^= 1;
        a.identity.sign_envelope(&mut e2).err();
        assert!(b.accept_envelope(e2, 2).is_err());
    }
    #[test]
    fn resume_discards_every_pending_capability() {
        let (a, _, mut b, ba) = pair();
        let e = message(&a, &mut b);
        let id = e.header.message_id;
        b.accept_envelope(e, 1).unwrap();
        b.invalidate_on_resume();
        assert!(b.open_once(ba.authorize_open(id), 2).is_err());
    }
}

#[cfg(test)]
mod adversarial_tests {
    use super::*;
    #[test]
    fn unauthenticated_envelope_does_not_burn_opening_key() {
        let (mut a, aa) = Vault::new().unwrap();
        let (mut b, ba) = Vault::new().unwrap();
        a.verify_peer(b.binding(), &aa).unwrap();
        b.verify_peer(a.binding(), &ba).unwrap();
        let offer = b.create_offer(&a.binding().identity, 0, 1000).unwrap();
        let good = a.seal(&offer, "authenticated fixture").unwrap();
        let id = good.header.message_id;
        let mut bad = good.clone();
        bad.signature[0] ^= 1;
        assert!(b.accept_envelope(bad, 1).is_err());
        b.accept_envelope(good, 2).unwrap();
        assert_eq!(
            b.open_once(ba.authorize_open(id), 3).unwrap().as_str(),
            "authenticated fixture"
        );
    }
}

#[cfg(test)]
mod transport_binding_tests {
    use super::*;
    #[test]
    fn public_transport_binding_is_once_and_does_not_change_execution_identity() {
        let (mut vault, _) = Vault::new().unwrap();
        let before = vault.binding();
        let transport = RuntimeIdentity::generate().unwrap().binding().noise_public;
        assert!(vault.bind_transport([0; 32]).is_err());
        let binding = vault.bind_transport(transport).unwrap();
        assert_eq!(binding.identity, before.identity);
        assert_eq!(binding.runtime_id, before.runtime_id);
        assert_eq!(binding.noise_public, transport);
        verify_binding(&binding, &transport).unwrap();
        assert!(vault
            .identity()
            .noise_private()
            .iter()
            .all(|byte| *byte == 0));
        assert!(vault.bind_transport(transport).is_err());
        assert_eq!(vault.binding(), binding);
    }
    #[test]
    fn authorization_permanently_prevents_transport_rebinding_even_after_revocation() {
        let (mut vault, authority) = Vault::new().unwrap();
        let peer = RuntimeIdentity::generate().unwrap().binding();
        let before = vault.binding();
        vault.verify_peer(peer.clone(), &authority).unwrap();
        assert!(vault.bind_transport(peer.noise_public).is_err());
        vault.revoke_peer(&peer.identity);
        assert!(vault.bind_transport(peer.noise_public).is_err());
        assert_eq!(vault.binding(), before);
    }
}
