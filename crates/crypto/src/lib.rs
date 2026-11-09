//! Standard Noise and HPKE composition; this crate introduces no cryptographic primitives.
use hpke::{
    aead::ChaCha20Poly1305, kdf::HkdfSha256, kem::X25519HkdfSha256, Deserializable,
    Kem as KemTrait, OpModeR, OpModeS, Serializable,
};
use rand::{rngs::StdRng, SeedableRng};
use thiserror::Error;
use veyra_identity::{
    verify_binding, verify_envelope, verify_offer, RuntimeIdentity, NOISE_PATTERN,
};
use veyra_protocol::{Envelope, Header, IdentityBinding, KeyOffer, MAX_FRAME, MAX_TTL_MS};
use veyra_secure_memory::LockedBuffer;
use zeroize::{Zeroize, Zeroizing};
type Kem = X25519HkdfSha256;
type Private = <Kem as KemTrait>::PrivateKey;
pub const HPKE_INFO: &[u8] = b"Veyra/HPKE/X25519-HKDF-SHA256/ChaCha20Poly1305/v1";
#[derive(Debug, Error)]
pub enum Error {
    #[error("operação criptográfica recusada")]
    Crypto,
    #[error("identidade ou autorização inválida")]
    Unauthorized,
    #[error("mensagem ou frame fora dos limites")]
    Limits,
}
pub struct OpeningKey(LockedBuffer);
pub fn generate_opening_key() -> Result<(OpeningKey, [u8; 32]), Error> {
    let mut rng = StdRng::try_from_os_rng().map_err(|_| Error::Crypto)?;
    let (sk, pk) = Kem::gen_keypair(&mut rng);
    let mut locked = LockedBuffer::new(32).map_err(|_| Error::Crypto)?;
    let mut serialized = sk.to_bytes();
    locked.as_mut_slice().copy_from_slice(serialized.as_slice());
    serialized.as_mut_slice().zeroize();
    Ok((
        OpeningKey(locked),
        pk.to_bytes()
            .as_slice()
            .try_into()
            .map_err(|_| Error::Crypto)?,
    ))
}
pub fn seal(
    identity: &RuntimeIdentity,
    offer: &KeyOffer,
    plaintext: &str,
) -> Result<Envelope, Error> {
    veyra_domain::validate_text(plaintext).map_err(|_| Error::Limits)?;
    verify_offer(offer).map_err(|_| Error::Unauthorized)?;
    if offer.sender != identity.public()
        || offer.sender_runtime != identity.runtime_id()
        || offer.ttl_ms == 0
        || offer.ttl_ms > MAX_TTL_MS
    {
        return Err(Error::Unauthorized);
    }
    let header = Header {
        sender: identity.public(),
        recipient: offer.recipient,
        sender_runtime: identity.runtime_id(),
        recipient_runtime: offer.recipient_runtime,
        message_id: veyra_identity::random_id().map_err(|_| Error::Crypto)?,
        key_id: offer.key_id,
        ttl_ms: offer.ttl_ms,
    };
    let public =
        <Kem as KemTrait>::PublicKey::from_bytes(&offer.public_key).map_err(|_| Error::Crypto)?;
    let mut rng = StdRng::try_from_os_rng().map_err(|_| Error::Crypto)?;
    let (enc, mut ctx) = hpke::setup_sender::<ChaCha20Poly1305, HkdfSha256, Kem, _>(
        &OpModeS::Base,
        &public,
        HPKE_INFO,
        &mut rng,
    )
    .map_err(|_| Error::Crypto)?;
    let ciphertext = ctx
        .seal(plaintext.as_bytes(), &header.aad())
        .map_err(|_| Error::Crypto)?;
    let mut e = Envelope {
        header,
        encapsulated_key: enc
            .to_bytes()
            .as_slice()
            .try_into()
            .map_err(|_| Error::Crypto)?,
        ciphertext,
        signature: [0; 64],
    };
    identity
        .sign_envelope(&mut e)
        .map_err(|_| Error::Unauthorized)?;
    Ok(e)
}
/// Signature and endpoint checks must precede removing the key from the vault.
/// This function owns the key: every success or error destroys this opening capability.
pub fn open(key: OpeningKey, envelope: &Envelope) -> Result<Zeroizing<String>, Error> {
    verify_envelope(envelope).map_err(|_| Error::Unauthorized)?;
    let enc = <Kem as KemTrait>::EncappedKey::from_bytes(&envelope.encapsulated_key)
        .map_err(|_| Error::Crypto)?;
    let private = Private::from_bytes(key.0.as_slice()).map_err(|_| Error::Crypto)?;
    let mut ctx = hpke::setup_receiver::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeR::Base,
        &private,
        &enc,
        HPKE_INFO,
    )
    .map_err(|_| Error::Crypto)?;
    let plain = Zeroizing::new(
        ctx.open(&envelope.ciphertext, &envelope.header.aad())
            .map_err(|_| Error::Crypto)?,
    );
    let text = std::str::from_utf8(&plain).map_err(|_| Error::Crypto)?;
    veyra_domain::validate_text(text).map_err(|_| Error::Limits)?;
    Ok(Zeroizing::new(text.to_owned()))
}
pub struct NoiseHandshake {
    state: snow::HandshakeState,
}
pub struct NoiseChannel {
    state: snow::TransportState,
}
impl NoiseHandshake {
    /// Both roles must provide the same prologue: domain, version, initiator binding,
    /// responder binding and invitation digest, as defined by the orchestrator.
    pub fn new(
        identity: &RuntimeIdentity,
        initiator: bool,
        prologue: &[u8],
    ) -> Result<Self, Error> {
        Self::from_private(identity.noise_private(), initiator, prologue)
    }
    pub fn from_private(private: &[u8], initiator: bool, prologue: &[u8]) -> Result<Self, Error> {
        if private.len() != 32 {
            return Err(Error::Crypto);
        }
        let b = snow::Builder::new(NOISE_PATTERN.parse().map_err(|_| Error::Crypto)?)
            .local_private_key(private)
            .map_err(|_| Error::Crypto)?
            .prologue(prologue)
            .map_err(|_| Error::Crypto)?;
        Ok(Self {
            state: if initiator {
                b.build_initiator()
            } else {
                b.build_responder()
            }
            .map_err(|_| Error::Crypto)?,
        })
    }
    pub fn write(&mut self) -> Result<Vec<u8>, Error> {
        let mut b = vec![0; 512];
        let n = self
            .state
            .write_message(&[], &mut b)
            .map_err(|_| Error::Crypto)?;
        b.truncate(n);
        Ok(b)
    }
    pub fn read(&mut self, frame: &[u8]) -> Result<(), Error> {
        if frame.len() > 512 {
            return Err(Error::Limits);
        }
        let mut b = [0; 512];
        let n = self
            .state
            .read_message(frame, &mut b)
            .map_err(|_| Error::Crypto)?;
        if n != 0 {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
    pub fn finish(self, expected: &IdentityBinding) -> Result<(NoiseChannel, Vec<u8>), Error> {
        if !self.state.is_handshake_finished() {
            return Err(Error::Crypto);
        }
        verify_binding(
            expected,
            self.state.get_remote_static().ok_or(Error::Unauthorized)?,
        )
        .map_err(|_| Error::Unauthorized)?;
        let transcript = self.state.get_handshake_hash().to_vec();
        Ok((
            NoiseChannel {
                state: self
                    .state
                    .into_transport_mode()
                    .map_err(|_| Error::Crypto)?,
            },
            transcript,
        ))
    }
}
impl NoiseChannel {
    pub fn encrypt(&mut self, frame: &[u8]) -> Result<Vec<u8>, Error> {
        if frame.len() > MAX_FRAME {
            return Err(Error::Limits);
        }
        let mut out = vec![0; frame.len() + 16];
        let n = self
            .state
            .write_message(frame, &mut out)
            .map_err(|_| Error::Crypto)?;
        out.truncate(n);
        Ok(out)
    }
    pub fn decrypt(&mut self, frame: &[u8]) -> Result<Zeroizing<Vec<u8>>, Error> {
        if frame.len() > MAX_FRAME + 16 {
            return Err(Error::Limits);
        }
        let mut out = Zeroizing::new(vec![0; frame.len()]);
        let n = self
            .state
            .read_message(frame, &mut out)
            .map_err(|_| Error::Crypto)?;
        out.truncate(n);
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn offer(a: &RuntimeIdentity, b: &RuntimeIdentity) -> (KeyOffer, OpeningKey) {
        let (sk, pk) = generate_opening_key().unwrap();
        let mut o = KeyOffer {
            sender: a.public(),
            recipient: b.public(),
            sender_runtime: a.runtime_id(),
            recipient_runtime: b.runtime_id(),
            key_id: [8; 16],
            public_key: pk,
            ttl_ms: 1000,
            signature: [0; 64],
        };
        b.sign_offer(&mut o).unwrap();
        (o, sk)
    }
    #[test]
    fn hpke_signed_roundtrip_and_tamper() {
        let a = RuntimeIdentity::generate().unwrap();
        let b = RuntimeIdentity::generate().unwrap();
        let (o, sk) = offer(&a, &b);
        let e = seal(&a, &o, "olá").unwrap();
        assert_eq!(open(sk, &e).unwrap().as_str(), "olá");
        let (o, sk) = offer(&a, &b);
        let mut e = seal(&a, &o, "texto").unwrap();
        e.header.recipient_runtime[0] ^= 1;
        assert!(open(sk, &e).is_err());
    }
    #[test]
    fn signed_invalid_ciphertext_is_rejected() {
        let a = RuntimeIdentity::generate().unwrap();
        let b = RuntimeIdentity::generate().unwrap();
        let (o, sk) = offer(&a, &b);
        let mut e = seal(&a, &o, "texto").unwrap();
        e.ciphertext[0] ^= 1;
        a.sign_envelope(&mut e).unwrap();
        assert!(open(sk, &e).is_err());
    }
    #[test]
    fn noise_xx_real_handshake_and_binding() {
        let a = RuntimeIdentity::generate().unwrap();
        let b = RuntimeIdentity::generate().unwrap();
        let mut x = NoiseHandshake::new(&a, true, b"test-v1").unwrap();
        let mut y = NoiseHandshake::new(&b, false, b"test-v1").unwrap();
        y.read(&x.write().unwrap()).unwrap();
        x.read(&y.write().unwrap()).unwrap();
        y.read(&x.write().unwrap()).unwrap();
        let (mut tx, h1) = x.finish(&b.binding()).unwrap();
        let (mut rx, h2) = y.finish(&a.binding()).unwrap();
        assert_eq!(h1, h2);
        let packet = tx.encrypt(b"ciphertext envelope").unwrap();
        assert_eq!(&*rx.decrypt(&packet).unwrap(), b"ciphertext envelope");
        assert!(rx.decrypt(&packet).is_err());
    }
}

#[cfg(test)]
mod official_vectors {
    use super::*;
    fn hex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| {
                let x = std::str::from_utf8(p).unwrap();
                u8::from_str_radix(x, 16).unwrap()
            })
            .collect()
    }
    #[test]
    fn rfc9180_appendix_a_2_1_receiver_vector() {
        // RFC 9180 A.2.1/A.2.1.1, suite 0x0020/0x0001/0x0003, mode Base.
        let sk = Private::from_bytes(&hex(
            "8057991eef8f1f1af18f4a9491d16a1ce333f695d4db8e38da75975c4478e0fb",
        ))
        .unwrap();
        let enc = <Kem as KemTrait>::EncappedKey::from_bytes(&hex(
            "1afa08d3dec047a643885163f1180476fa7ddb54c6a8029ea33f95796bf2ac4a",
        ))
        .unwrap();
        let info = hex("4f6465206f6e2061204772656369616e2055726e");
        let mut ctx = hpke::setup_receiver::<ChaCha20Poly1305, HkdfSha256, Kem>(
            &OpModeR::Base,
            &sk,
            &enc,
            &info,
        )
        .unwrap();
        let ct = hex("1c5250d8034ec2b784ba2cfd69dbdb8af406cfe3ff938e131f0def8c8b60b4db21993c62ce81883d2dd1b51a28");
        let pt = ctx.open(&ct, &hex("436f756e742d30")).unwrap();
        assert_eq!(
            pt,
            hex("4265617574792069732074727574682c20747275746820626561757479")
        );
    }
}

#[cfg(test)]
mod aad_tests {
    use super::*;
    #[test]
    fn valid_sender_signature_does_not_allow_modified_aad() {
        let a = RuntimeIdentity::generate().unwrap();
        let b = RuntimeIdentity::generate().unwrap();
        let (key, pk) = generate_opening_key().unwrap();
        let mut offer = KeyOffer {
            sender: a.public(),
            recipient: b.public(),
            sender_runtime: a.runtime_id(),
            recipient_runtime: b.runtime_id(),
            key_id: [11; 16],
            public_key: pk,
            ttl_ms: 1000,
            signature: [0; 64],
        };
        b.sign_offer(&mut offer).unwrap();
        let mut envelope = seal(&a, &offer, "AAD fixture").unwrap();
        envelope.header.ttl_ms = 999;
        a.sign_envelope(&mut envelope).unwrap();
        assert!(open(key, &envelope).is_err());
    }
}
