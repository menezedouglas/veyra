//! Outgoing encrypted-only queues. ACK means accepted in RAM, never read.
use std::collections::HashMap;
use thiserror::Error;
use veyra_domain::{Deadline, SendState, MAX_PENDING, MAX_PENDING_PER_PEER};
use veyra_protocol::{Envelope, Id, Receipt};
#[derive(Debug, Error)]
pub enum Error {
    #[error("limite da fila atingido")]
    Full,
    #[error("confirmação inválida")]
    InvalidReceipt,
    #[error("mensagem indisponível")]
    Unavailable,
}
struct Outgoing {
    envelope: Envelope,
    deadline: Deadline,
    state: SendState,
}
#[derive(Default)]
pub struct Outbox {
    items: HashMap<Id, Outgoing>,
}
impl Outbox {
    pub fn enqueue(&mut self, e: Envelope, now: u64) -> Result<(), Error> {
        if self.items.len() >= MAX_PENDING
            || self
                .items
                .values()
                .filter(|m| m.envelope.header.recipient == e.header.recipient)
                .count()
                >= MAX_PENDING_PER_PEER
        {
            return Err(Error::Full);
        }
        if self.items.contains_key(&e.header.message_id) {
            return Err(Error::Unavailable);
        }
        let deadline = Deadline::new(now, e.header.ttl_ms).map_err(|_| Error::Unavailable)?;
        self.items.insert(
            e.header.message_id,
            Outgoing {
                envelope: e,
                deadline,
                state: SendState::Sealed,
            },
        );
        Ok(())
    }
    pub fn mark_sent(&mut self, id: &Id) -> Result<(), Error> {
        let m = self.items.get_mut(id).ok_or(Error::Unavailable)?;
        m.state = m
            .state
            .transition(SendState::Sent)
            .map_err(|_| Error::Unavailable)?;
        Ok(())
    }
    pub fn delivery_unconfirmed(&mut self, id: &Id) -> Result<(), Error> {
        let m = self.items.get_mut(id).ok_or(Error::Unavailable)?;
        m.state = m
            .state
            .transition(SendState::DeliveryUnconfirmed)
            .map_err(|_| Error::Unavailable)?;
        Ok(())
    }
    pub fn retry(&self, id: &Id, now: u64) -> Result<&Envelope, Error> {
        let m = self.items.get(id).ok_or(Error::Unavailable)?;
        if m.deadline.expired(now) {
            return Err(Error::Unavailable);
        }
        Ok(&m.envelope)
    }
    pub fn acknowledge(&mut self, r: &Receipt) -> Result<(), Error> {
        let m = self.items.get(&r.message_id).ok_or(Error::Unavailable)?;
        if r.recipient_runtime != m.envelope.header.recipient_runtime
            || r.envelope_digest != m.envelope.digest()
        {
            return Err(Error::InvalidReceipt);
        }
        veyra_identity::verify_receipt(r, &m.envelope.header.recipient)
            .map_err(|_| Error::InvalidReceipt)?;
        self.items.remove(&r.message_id);
        Ok(())
    }
    pub fn expire(&mut self, now: u64) {
        self.items.retain(|_, m| !m.deadline.expired(now));
    }
    pub fn clear(&mut self) {
        self.items.clear();
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veyra_vault::Vault;
    #[test]
    fn fanout_is_independent_and_ack_is_not_read_receipt() {
        let (mut sender, authority) = Vault::new().unwrap();
        let (mut a, aa) = Vault::new().unwrap();
        let (mut b, ba) = Vault::new().unwrap();
        sender.verify_peer(a.binding(), &authority).unwrap();
        sender.verify_peer(b.binding(), &authority).unwrap();
        a.verify_peer(sender.binding(), &aa).unwrap();
        b.verify_peer(sender.binding(), &ba).unwrap();
        let oa = a.create_offer(&sender.binding().identity, 0, 1000).unwrap();
        let ob = b.create_offer(&sender.binding().identity, 0, 1000).unwrap();
        let ea = sender.seal(&oa, "fan-out de teste").unwrap();
        let eb = sender.seal(&ob, "fan-out de teste").unwrap();
        assert_ne!(ea.header.message_id, eb.header.message_id);
        assert_ne!(ea.header.key_id, eb.header.key_id);
        assert_ne!(ea.encapsulated_key, eb.encapsulated_key);
        let mut out = Outbox::default();
        out.enqueue(ea.clone(), 0).unwrap();
        out.enqueue(eb.clone(), 0).unwrap();
        let ack = a.accept_envelope(ea.clone(), 1).unwrap();
        assert_eq!(a.list_pending(2).len(), 1); // Still unopened when sender discards.
        out.acknowledge(&ack).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out.retry(&ea.header.message_id, 2).is_err());
        assert_eq!(out.retry(&eb.header.message_id, 500).unwrap(), &eb);
        assert!(out.retry(&eb.header.message_id, 1000).is_err());
        out.expire(1000);
        assert!(out.is_empty());
    }
    #[test]
    fn altered_and_wrong_recipient_receipts_do_not_delete_envelope() {
        let (mut a, aa) = Vault::new().unwrap();
        let (mut b, ba) = Vault::new().unwrap();
        a.verify_peer(b.binding(), &aa).unwrap();
        b.verify_peer(a.binding(), &ba).unwrap();
        let o = b.create_offer(&a.binding().identity, 0, 1000).unwrap();
        let e = a.seal(&o, "teste").unwrap();
        let mut out = Outbox::default();
        out.enqueue(e.clone(), 0).unwrap();
        let mut ack = b.accept_envelope(e, 1).unwrap();
        ack.envelope_digest[0] ^= 1;
        assert!(out.acknowledge(&ack).is_err());
        assert_eq!(out.len(), 1);
    }
}
