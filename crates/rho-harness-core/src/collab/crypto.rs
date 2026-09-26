use ring::error::Unspecified;
use ring::hkdf::{HKDF_SHA256, KeyType};
use ring::hmac::{HMAC_SHA256, Key};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityLevel {
    Full,
    ViewOnly,
}

#[derive(Clone)]
pub struct CollabSecret {
    seed: [u8; 32],
}

struct Okm32;

impl KeyType for Okm32 {
    fn len(&self) -> usize {
        32
    }
}

impl CollabSecret {
    pub fn generate() -> Result<Self, Unspecified> {
        let rng = SystemRandom::new();
        let mut seed = [0u8; 32];
        rng.fill(&mut seed)?;
        Ok(Self { seed })
    }

    #[must_use]
    pub fn from_bytes(seed: [u8; 32]) -> Self {
        Self { seed }
    }

    #[must_use]
    pub fn seed(&self) -> &[u8; 32] {
        &self.seed
    }

    pub fn derive_read_key(&self) -> Result<[u8; 32], Unspecified> {
        self.expand_sub_key(b"rho-collab-read-v1")
    }

    pub fn derive_write_key(&self) -> Result<[u8; 32], Unspecified> {
        self.expand_sub_key(b"rho-collab-write-v1")
    }

    fn expand_sub_key(&self, context: &[u8]) -> Result<[u8; 32], Unspecified> {
        let salt = ring::hkdf::Salt::new(HKDF_SHA256, &[]);
        let prk = salt.extract(&self.seed);
        let mut out = [0u8; 32];
        let info = [context];
        let okm = prk.expand(&info, Okm32)?;
        okm.fill(&mut out)?;
        Ok(out)
    }

    #[must_use]
    pub fn sign_challenge(key: &[u8; 32], nonce: &[u8]) -> [u8; 32] {
        let s_key = Key::new(HMAC_SHA256, key);
        let tag = ring::hmac::sign(&s_key, nonce);
        let mut out = [0u8; 32];
        out.copy_from_slice(tag.as_ref());
        out
    }

    #[must_use]
    pub fn verify_challenge(key: &[u8; 32], nonce: &[u8], tag_bytes: &[u8; 32]) -> bool {
        let s_key = Key::new(HMAC_SHA256, key);
        ring::hmac::verify(&s_key, nonce, tag_bytes.as_slice()).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_generation_and_derivation() {
        let secret = CollabSecret::generate().expect("failed to generate secret");
        let read_key = secret.derive_read_key().expect("failed to derive read key");
        let write_key = secret.derive_write_key().expect("failed to derive write key");

        assert_ne!(read_key, write_key);
        assert_ne!(read_key, *secret.seed());
        assert_ne!(write_key, *secret.seed());
    }

    #[test]
    fn test_hmac_challenge_response() {
        let secret = CollabSecret::generate().expect("failed to generate secret");
        let write_key = secret.derive_write_key().expect("failed to derive write key");
        let read_key = secret.derive_read_key().expect("failed to derive read key");
        let nonce = b"test-challenge-nonce-0123456789";

        let tag = CollabSecret::sign_challenge(&write_key, nonce);
        assert!(CollabSecret::verify_challenge(&write_key, nonce, &tag));
        assert!(!CollabSecret::verify_challenge(&read_key, nonce, &tag));
    }
}
