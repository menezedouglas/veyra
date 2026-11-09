//! Veyra v1 bounded, deterministic CBOR. Arrays have fixed schemas: maps, tags,
//! indefinite lengths, trailing bytes and non-minimal integer forms are rejected.
use minicbor::{Decoder, Encoder};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const VERSION: u8 = 1;
pub const SUITE: u16 = 1;
pub const MAX_TEXT: usize = 4096;
pub const MAX_FRAME: usize = 16384;
pub const MAX_CIPHERTEXT: usize = MAX_TEXT + 16;
pub const MAX_TTL_MS: u32 = 300_000;
pub type Identity = [u8; 32];
pub type Id = [u8; 16];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("frame inválido ou fora dos limites")]
    Invalid,
    #[error("versão ou suíte incompatível")]
    Version,
    #[error("codificação não canônica")]
    NonCanonical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityBinding {
    pub identity: Identity,
    pub noise_public: [u8; 32],
    pub runtime_id: Id,
    pub signature: [u8; 64],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyOffer {
    pub sender: Identity,
    pub recipient: Identity,
    pub sender_runtime: Id,
    pub recipient_runtime: Id,
    pub key_id: Id,
    pub public_key: [u8; 32],
    pub ttl_ms: u32,
    pub signature: [u8; 64],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub sender: Identity,
    pub recipient: Identity,
    pub sender_runtime: Id,
    pub recipient_runtime: Id,
    pub message_id: Id,
    pub key_id: Id,
    pub ttl_ms: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub header: Header,
    pub encapsulated_key: [u8; 32],
    pub ciphertext: Vec<u8>,
    pub signature: [u8; 64],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub message_id: Id,
    pub envelope_digest: [u8; 32],
    pub recipient_runtime: Id,
    pub signature: [u8; 64],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Hello(Id),
    IdentityBinding(IdentityBinding),
    KeyOffer(KeyOffer),
    Message(Envelope),
    Received(Receipt),
    Rejected(Id, u8),
    Close(u8),
}

type E = Encoder<Vec<u8>>;
fn bytes(e: &mut E, b: &[u8]) {
    e.bytes(b).expect("Vec writer");
}
fn array(e: &mut E, n: u64) {
    e.array(n).expect("Vec writer");
}
fn uint(e: &mut E, n: u32) {
    e.u32(n).expect("Vec writer");
}
fn fixed<const N: usize>(d: &mut Decoder<'_>) -> Result<[u8; N], Error> {
    d.bytes()
        .map_err(|_| Error::Invalid)?
        .try_into()
        .map_err(|_| Error::Invalid)
}
fn len(d: &mut Decoder<'_>, n: u64) -> Result<(), Error> {
    if d.array().map_err(|_| Error::Invalid)? != Some(n) {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn number(d: &mut Decoder<'_>) -> Result<u32, Error> {
    d.u32().map_err(|_| Error::Invalid)
}
fn prefix(e: &mut E, n: u64, domain: &[u8]) {
    array(e, n);
    bytes(e, domain);
    uint(e, VERSION as u32);
}
fn versions(d: &mut Decoder<'_>, with_suite: bool) -> Result<(), Error> {
    if number(d)? != VERSION as u32 || (with_suite && number(d)? != SUITE as u32) {
        return Err(Error::Version);
    }
    Ok(())
}
fn binding(e: &mut E, b: &IdentityBinding, signed: bool) {
    prefix(e, if signed { 6 } else { 5 }, b"Veyra/identity/v1");
    bytes(e, &b.identity);
    bytes(e, &b.noise_public);
    bytes(e, &b.runtime_id);
    if signed {
        bytes(e, &b.signature);
    }
}
fn offer(e: &mut E, k: &KeyOffer, signed: bool) {
    prefix(e, if signed { 12 } else { 11 }, b"Veyra/key-offer/v1");
    uint(e, SUITE as u32);
    bytes(e, &k.sender);
    bytes(e, &k.recipient);
    bytes(e, &k.sender_runtime);
    bytes(e, &k.recipient_runtime);
    bytes(e, &k.key_id);
    bytes(e, &k.public_key);
    uint(e, k.ttl_ms);
    // Fixed policy value: at-most-once opening; not a configurable assertion.
    uint(e, 1);
    if signed {
        bytes(e, &k.signature);
    }
}
fn header(e: &mut E, h: &Header) {
    prefix(e, 11, b"Veyra/envelope-aad/v1");
    uint(e, SUITE as u32);
    bytes(e, &h.sender);
    bytes(e, &h.recipient);
    bytes(e, &h.sender_runtime);
    bytes(e, &h.recipient_runtime);
    bytes(e, &h.message_id);
    bytes(e, &h.key_id);
    uint(e, h.ttl_ms);
    uint(e, 1);
}
fn envelope(e: &mut E, m: &Envelope, signed: bool) {
    prefix(
        e,
        if signed { 6 } else { 5 },
        b"Veyra/envelope-signature/v1",
    );
    header(e, &m.header);
    bytes(e, &m.encapsulated_key);
    bytes(e, &m.ciphertext);
    if signed {
        bytes(e, &m.signature);
    }
}
fn receipt(e: &mut E, r: &Receipt, signed: bool) {
    prefix(e, if signed { 6 } else { 5 }, b"Veyra/received/v1");
    bytes(e, &r.message_id);
    bytes(e, &r.envelope_digest);
    bytes(e, &r.recipient_runtime);
    if signed {
        bytes(e, &r.signature);
    }
}
fn domain(d: &mut Decoder<'_>, expected: &[u8]) -> Result<(), Error> {
    if d.bytes().map_err(|_| Error::Invalid)? != expected {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn read_binding(d: &mut Decoder<'_>) -> Result<IdentityBinding, Error> {
    len(d, 6)?;
    domain(d, b"Veyra/identity/v1")?;
    versions(d, false)?;
    Ok(IdentityBinding {
        identity: fixed(d)?,
        noise_public: fixed(d)?,
        runtime_id: fixed(d)?,
        signature: fixed(d)?,
    })
}
fn read_offer(d: &mut Decoder<'_>) -> Result<KeyOffer, Error> {
    len(d, 12)?;
    domain(d, b"Veyra/key-offer/v1")?;
    versions(d, true)?;
    let k = KeyOffer {
        sender: fixed(d)?,
        recipient: fixed(d)?,
        sender_runtime: fixed(d)?,
        recipient_runtime: fixed(d)?,
        key_id: fixed(d)?,
        public_key: fixed(d)?,
        ttl_ms: number(d)?,
        signature: [0; 64],
    };
    if k.ttl_ms == 0 || k.ttl_ms > MAX_TTL_MS || number(d)? != 1 {
        return Err(Error::Invalid);
    }
    Ok(KeyOffer {
        signature: fixed(d)?,
        ..k
    })
}
fn read_header(d: &mut Decoder<'_>) -> Result<Header, Error> {
    len(d, 11)?;
    domain(d, b"Veyra/envelope-aad/v1")?;
    versions(d, true)?;
    let h = Header {
        sender: fixed(d)?,
        recipient: fixed(d)?,
        sender_runtime: fixed(d)?,
        recipient_runtime: fixed(d)?,
        message_id: fixed(d)?,
        key_id: fixed(d)?,
        ttl_ms: number(d)?,
    };
    if h.ttl_ms == 0 || h.ttl_ms > MAX_TTL_MS || number(d)? != 1 {
        return Err(Error::Invalid);
    }
    Ok(h)
}
fn read_envelope(d: &mut Decoder<'_>) -> Result<Envelope, Error> {
    len(d, 6)?;
    domain(d, b"Veyra/envelope-signature/v1")?;
    versions(d, false)?;
    let header = read_header(d)?;
    let encapsulated_key = fixed(d)?;
    let ciphertext = d.bytes().map_err(|_| Error::Invalid)?;
    if ciphertext.len() < 16 || ciphertext.len() > MAX_CIPHERTEXT {
        return Err(Error::Invalid);
    }
    Ok(Envelope {
        header,
        encapsulated_key,
        ciphertext: ciphertext.to_vec(),
        signature: fixed(d)?,
    })
}
fn read_receipt(d: &mut Decoder<'_>) -> Result<Receipt, Error> {
    len(d, 6)?;
    domain(d, b"Veyra/received/v1")?;
    versions(d, false)?;
    Ok(Receipt {
        message_id: fixed(d)?,
        envelope_digest: fixed(d)?,
        recipient_runtime: fixed(d)?,
        signature: fixed(d)?,
    })
}
impl IdentityBinding {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut e = E::new(Vec::new());
        binding(&mut e, self, false);
        e.into_writer()
    }
}
impl KeyOffer {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut e = E::new(Vec::new());
        offer(&mut e, self, false);
        e.into_writer()
    }
}
impl Header {
    pub fn aad(&self) -> Vec<u8> {
        let mut e = E::new(Vec::new());
        header(&mut e, self);
        e.into_writer()
    }
}
impl Envelope {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut e = E::new(Vec::new());
        envelope(&mut e, self, false);
        e.into_writer()
    }
    pub fn digest(&self) -> [u8; 32] {
        let mut e = E::new(Vec::new());
        envelope(&mut e, self, true);
        Sha256::digest(e.into_writer()).into()
    }
}
impl Receipt {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut e = E::new(Vec::new());
        receipt(&mut e, self, false);
        e.into_writer()
    }
}
impl Frame {
    pub fn encode(&self) -> Vec<u8> {
        let mut e = E::new(Vec::new());
        array(&mut e, 3);
        uint(&mut e, VERSION as u32);
        match self {
            Self::Hello(id) => {
                uint(&mut e, 0);
                bytes(&mut e, id)
            }
            Self::IdentityBinding(b) => {
                uint(&mut e, 1);
                binding(&mut e, b, true)
            }
            Self::KeyOffer(k) => {
                uint(&mut e, 2);
                offer(&mut e, k, true)
            }
            Self::Message(m) => {
                uint(&mut e, 3);
                envelope(&mut e, m, true)
            }
            Self::Received(r) => {
                uint(&mut e, 4);
                receipt(&mut e, r, true)
            }
            Self::Rejected(id, reason) => {
                uint(&mut e, 5);
                array(&mut e, 2);
                bytes(&mut e, id);
                uint(&mut e, *reason as u32)
            }
            Self::Close(reason) => {
                uint(&mut e, 6);
                uint(&mut e, *reason as u32)
            }
        }
        e.into_writer()
    }
    pub fn decode(input: &[u8]) -> Result<Self, Error> {
        if input.is_empty() || input.len() > MAX_FRAME {
            return Err(Error::Invalid);
        }
        let mut d = Decoder::new(input);
        len(&mut d, 3)?;
        versions(&mut d, false)?;
        let out = match number(&mut d)? {
            0 => Self::Hello(fixed(&mut d)?),
            1 => Self::IdentityBinding(read_binding(&mut d)?),
            2 => Self::KeyOffer(read_offer(&mut d)?),
            3 => Self::Message(read_envelope(&mut d)?),
            4 => Self::Received(read_receipt(&mut d)?),
            5 => {
                len(&mut d, 2)?;
                Self::Rejected(
                    fixed(&mut d)?,
                    number(&mut d)?.try_into().map_err(|_| Error::Invalid)?,
                )
            }
            6 => Self::Close(number(&mut d)?.try_into().map_err(|_| Error::Invalid)?),
            _ => return Err(Error::Invalid),
        };
        if d.position() != input.len() {
            return Err(Error::Invalid);
        }
        if out.encode() != input {
            return Err(Error::NonCanonical);
        }
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_closed_canonical_frames() {
        let f = Frame::Hello([7; 16]);
        assert_eq!(Frame::decode(&f.encode()).unwrap(), f);
        let mut b = f.encode();
        b.push(0);
        assert!(Frame::decode(&b).is_err());
        assert!(Frame::decode(&vec![0; MAX_FRAME + 1]).is_err());
        assert!(Frame::decode(&[0x9f, 1, 0, 0xff]).is_err());
        let mut b = f.encode();
        b.splice(1..2, [0x18, 1]);
        assert_eq!(Frame::decode(&b), Err(Error::NonCanonical));
        assert!(Frame::decode(&[0x83, 1, 0, 0xbf, 0xff]).is_err());
    }
    #[test]
    fn deterministic_header_binds_every_field() {
        let h = Header {
            sender: [1; 32],
            recipient: [2; 32],
            sender_runtime: [3; 16],
            recipient_runtime: [4; 16],
            message_id: [5; 16],
            key_id: [6; 16],
            ttl_ms: 5000,
        };
        let mut other = h.clone();
        other.key_id[0] ^= 1;
        assert_ne!(h.aad(), other.aad());
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig { failure_persistence: None, cases: 256, ..ProptestConfig::default() })]
        #[test]
        fn arbitrary_input_never_panics(data in proptest::collection::vec(any::<u8>(), 0..20000)) {
            if let Ok(frame) = Frame::decode(&data) { prop_assert_eq!(frame.encode(), data); }
        }
        #[test]
        fn valid_runtime_hello_roundtrips(runtime in any::<[u8;16]>()) {
            let f=Frame::Hello(runtime); prop_assert_eq!(Frame::decode(&f.encode()).unwrap(), f);
        }
    }
}

/// Noise roles are explicit and signed binding bytes include each runtime ID.
/// The invitation digest binds this connection to the out-of-band invitation.
pub fn noise_prologue(
    initiator: &IdentityBinding,
    responder: &IdentityBinding,
    invitation_digest: &[u8; 32],
) -> Vec<u8> {
    let mut e = E::new(Vec::new());
    prefix(&mut e, 5, b"Veyra/noise-prologue/v1");
    binding(&mut e, initiator, true);
    binding(&mut e, responder, true);
    bytes(&mut e, invitation_digest);
    e.into_writer()
}

/// Domain-restricted public signaling document. The vault supplies its own
/// signed identity binding; callers cannot choose which identity is signed.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignalDocument {
    pub room: [u8; 32],
    pub recipient: [u8; 32],
    pub sequence: u32,
    pub expires_unix: u64,
    pub role: u8,
    pub operation: u8,
    pub payload: String,
}
impl SignalDocument {
    pub fn signing_bytes(&self, binding: &IdentityBinding) -> Result<Vec<u8>, Error> {
        if self.role > 1
            || self.operation > 6
            || self.payload.len() > 48 * 1024
            || self.expires_unix == 0
        {
            return Err(Error::Invalid);
        }
        let mut e = E::new(Vec::new());
        e.array(10).expect("Vec writer");
        e.str("Veyra/signaling/v1").expect("Vec writer");
        e.u8(VERSION).expect("Vec writer");
        e.bytes(&self.room).expect("Vec writer");
        e.bytes(&Frame::IdentityBinding(binding.clone()).encode())
            .expect("Vec writer");
        e.bytes(&self.recipient).expect("Vec writer");
        e.u32(self.sequence).expect("Vec writer");
        e.u64(self.expires_unix).expect("Vec writer");
        e.u8(self.role).expect("Vec writer");
        e.u8(self.operation).expect("Vec writer");
        e.str(&self.payload).expect("Vec writer");
        Ok(e.into_writer())
    }
}
