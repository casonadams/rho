use crate::agent_mesh::types::AgentCapability;
use crate::collab::crypto::CollabSecret;
use ring::error::Unspecified;

#[derive(Clone)]
pub struct AgentSecret {
    inner: CollabSecret,
}

impl AgentSecret {
    pub fn generate() -> Result<Self, Unspecified> {
        CollabSecret::generate().map(|inner| Self { inner })
    }

    #[must_use]
    pub fn from_bytes(seed: [u8; 32]) -> Self {
        Self {
            inner: CollabSecret::from_bytes(seed),
        }
    }

    #[must_use]
    pub fn seed(&self) -> &[u8; 32] {
        self.inner.seed()
    }

    pub fn derive_key(&self, cap: AgentCapability) -> Result<[u8; 32], Unspecified> {
        let label = match cap {
            AgentCapability::ReadOnly => b"rho-mesh-agent-readonly-v1" as &[u8],
            AgentCapability::Propose => b"rho-mesh-agent-propose-v1",
            AgentCapability::Full => b"rho-mesh-agent-full-v1",
        };
        self.inner.expand_sub_key(label)
    }

    #[must_use]
    pub fn sign_challenge(key: &[u8; 32], nonce: &[u8]) -> [u8; 32] {
        CollabSecret::sign_challenge(key, nonce)
    }

    #[must_use]
    pub fn verify_signature(key: &[u8; 32], nonce: &[u8], signature: &[u8; 32]) -> bool {
        CollabSecret::verify_challenge(key, nonce, signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_secret_key_derivation_and_verification() {
        let secret = AgentSecret::generate().unwrap();
        let ro_key = secret.derive_key(AgentCapability::ReadOnly).unwrap();
        let prop_key = secret.derive_key(AgentCapability::Propose).unwrap();
        let full_key = secret.derive_key(AgentCapability::Full).unwrap();

        assert_ne!(ro_key, prop_key);
        assert_ne!(prop_key, full_key);

        let nonce = [42u8; 32];
        let sig = AgentSecret::sign_challenge(&ro_key, &nonce);
        assert!(AgentSecret::verify_signature(&ro_key, &nonce, &sig));
        assert!(!AgentSecret::verify_signature(&prop_key, &nonce, &sig));
    }
}
