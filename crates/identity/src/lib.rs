//! Execution-scoped Ed25519 signing identity. No filesystem or persistence APIs.
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use thiserror::Error;
use veyra_protocol::{Envelope, Id, Identity, IdentityBinding, KeyOffer, Receipt};
use veyra_secure_memory::LockedBuffer;
use zeroize::{Zeroize, Zeroizing};
pub const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
#[derive(Debug, Error)]
pub enum Error {
    #[error("fonte de aleatoriedade indisponível")]
    Random,
    #[error("assinatura ou vínculo de identidade inválido")]
    Signature,
    #[error("backend Noise indisponível")]
    Noise,
}
pub fn random_id() -> Result<Id, Error> {
    let mut out = [0; 16];
    getrandom::getrandom(&mut out).map_err(|_| Error::Random)?;
    Ok(out)
}
pub fn random_secret() -> Result<Zeroizing<[u8; 32]>, Error> {
    let mut out = Zeroizing::new([0; 32]);
    getrandom::getrandom(out.as_mut()).map_err(|_| Error::Random)?;
    Ok(out)
}
pub struct RuntimeIdentity {
    binding: IdentityBinding,
    secrets: LockedBuffer,
}
impl RuntimeIdentity {
    pub fn generate() -> Result<Self, Error> {
        let seed = random_secret()?;
        let signing = SigningKey::from_bytes(&seed);
        let kp = snow::Builder::new(NOISE_PATTERN.parse().map_err(|_| Error::Noise)?)
            .generate_keypair()
            .map_err(|_| Error::Noise)?;
        let mut binding = IdentityBinding {
            identity: signing.verifying_key().to_bytes(),
            noise_public: kp.public.try_into().map_err(|_| Error::Noise)?,
            runtime_id: random_id()?,
            signature: [0; 64],
        };
        binding.signature = signing.sign(&binding.signing_bytes()).to_bytes();
        let private = Zeroizing::new(kp.private);
        let mut secrets = LockedBuffer::new(64).map_err(|_| Error::Random)?;
        secrets.as_mut_slice()[..32].copy_from_slice(&*seed);
        secrets.as_mut_slice()[32..].copy_from_slice(&private);
        Ok(Self { binding, secrets })
    }
    pub fn binding(&self) -> IdentityBinding {
        self.binding.clone()
    }
    pub fn public(&self) -> Identity {
        self.binding.identity
    }
    pub fn runtime_id(&self) -> Id {
        self.binding.runtime_id
    }
    /// Bind a separately generated transport public key to this execution.
    /// The vault enforces one registration before any peer authorization.
    /// The old in-process Noise private half is destroyed, not exported.
    pub fn bind_transport_public(
        &mut self,
        noise_public: [u8; 32],
    ) -> Result<IdentityBinding, Error> {
        if noise_public == [0; 32] {
            return Err(Error::Signature);
        }
        self.binding.noise_public = noise_public;
        self.binding.signature = self
            .signing()
            .sign(&self.binding.signing_bytes())
            .to_bytes();
        self.secrets.as_mut_slice()[32..].zeroize();
        Ok(self.binding())
    }
    /// In-process handshake helper; production transport keys are generated
    /// in the daemon. This half is all zeros after external public binding.
    pub fn noise_private(&self) -> &[u8] {
        &self.secrets.as_slice()[32..]
    }
    fn signing(&self) -> SigningKey {
        SigningKey::from_bytes(
            self.secrets.as_slice()[..32]
                .try_into()
                .expect("locked Ed25519 seed"),
        )
    }
    pub fn sign_offer(&self, offer: &mut KeyOffer) -> Result<(), Error> {
        if offer.recipient != self.public() || offer.recipient_runtime != self.runtime_id() {
            return Err(Error::Signature);
        }
        offer.signature = self.signing().sign(&offer.signing_bytes()).to_bytes();
        Ok(())
    }
    pub fn sign_envelope(&self, envelope: &mut Envelope) -> Result<(), Error> {
        if envelope.header.sender != self.public()
            || envelope.header.sender_runtime != self.runtime_id()
        {
            return Err(Error::Signature);
        }
        envelope.signature = self.signing().sign(&envelope.signing_bytes()).to_bytes();
        Ok(())
    }
    /// Domain-restricted signaling only; never a generic signing oracle.
    pub fn sign_signal(
        &self,
        document: &veyra_protocol::SignalDocument,
    ) -> Result<[u8; 64], Error> {
        let bytes = document
            .signing_bytes(&self.binding)
            .map_err(|_| Error::Signature)?;
        Ok(self.signing().sign(&bytes).to_bytes())
    }
    pub fn receipt(&self, message_id: Id, envelope_digest: [u8; 32]) -> Receipt {
        let mut r = Receipt {
            message_id,
            envelope_digest,
            recipient_runtime: self.runtime_id(),
            signature: [0; 64],
        };
        r.signature = self.signing().sign(&r.signing_bytes()).to_bytes();
        r
    }
}
pub fn verify(identity: &Identity, message: &[u8], signature: &[u8; 64]) -> Result<(), Error> {
    VerifyingKey::from_bytes(identity)
        .map_err(|_| Error::Signature)?
        .verify_strict(message, &Signature::from_bytes(signature))
        .map_err(|_| Error::Signature)
}
pub fn verify_binding(b: &IdentityBinding, observed_noise: &[u8]) -> Result<(), Error> {
    if b.noise_public != observed_noise {
        return Err(Error::Signature);
    }
    verify(&b.identity, &b.signing_bytes(), &b.signature)
}
pub fn verify_offer(o: &KeyOffer) -> Result<(), Error> {
    verify(&o.recipient, &o.signing_bytes(), &o.signature)
}
pub fn verify_envelope(e: &Envelope) -> Result<(), Error> {
    verify(&e.header.sender, &e.signing_bytes(), &e.signature)
}
pub fn verify_receipt(r: &Receipt, recipient: &Identity) -> Result<(), Error> {
    verify(recipient, &r.signing_bytes(), &r.signature)
}
pub fn fingerprint(identity: &Identity) -> String {
    let digest = Sha256::digest(identity);
    digest
        .chunks(2)
        .map(|b| format!("{:02X}{:02X}", b[0], b[1]))
        .collect::<Vec<_>>()
        .join(" ")
}
/// Transcript plus identities ordered by Noise role, so both peers see the same code.
pub fn verification_code(
    transcript: &[u8],
    initiator: &IdentityBinding,
    responder: &IdentityBinding,
) -> String {
    let mut h = Sha256::new();
    h.update(b"Veyra/verification/v1\0");
    h.update((transcript.len() as u32).to_be_bytes());
    h.update(transcript);
    h.update(initiator.signing_bytes());
    h.update(responder.signing_bytes());
    let d = h.finalize();
    format!(
        "{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}",
        d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binds_independent_noise_and_runtime() {
        let a = RuntimeIdentity::generate().unwrap();
        let mut b = a.binding();
        assert_ne!(b.identity, b.noise_public);
        verify_binding(&b, &b.noise_public).unwrap();
        b.runtime_id[0] ^= 1;
        assert!(verify_binding(&b, &b.noise_public).is_err());
    }
    #[test]
    fn rfc8032_vector_one() {
        // RFC 8032 §7.1 test 1: empty message.
        fn unhex<const N: usize>(s: &str) -> [u8; N] {
            let mut b = [0; N];
            for i in 0..N {
                b[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap()
            }
            b
        }
        let pk = unhex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let sig=unhex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b");
        verify(&pk, b"", &sig).unwrap();
        assert!(verify(&pk, b"x", &sig).is_err());
    }
}

#[cfg(test)]
mod transport_binding_tests {
    use super::*;
    #[test]
    fn external_public_binding_preserves_signing_identity_and_runtime() {
        let mut identity = RuntimeIdentity::generate().unwrap();
        let before = identity.binding();
        let daemon = RuntimeIdentity::generate().unwrap();
        let bound = identity
            .bind_transport_public(daemon.binding().noise_public)
            .unwrap();
        assert_eq!(bound.identity, before.identity);
        assert_eq!(bound.runtime_id, before.runtime_id);
        assert_eq!(bound.noise_public, daemon.binding().noise_public);
        verify_binding(&bound, &bound.noise_public).unwrap();
        assert!(identity.noise_private().iter().all(|byte| *byte == 0));
    }
}
