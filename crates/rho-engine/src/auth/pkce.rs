//! PKCE (RFC 7636) code verifier and S256 challenge generator.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct PkceChallenge {
    pub verifier: String,
    pub challenge: String,
    pub method: &'static str,
}

impl PkceChallenge {
    pub fn generate() -> Self {
        let mut random_bytes = [0u8; 32];
        rand::fill(&mut random_bytes);
        let verifier = URL_SAFE_NO_PAD.encode(random_bytes);

        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let hash = hasher.finalize();
        let challenge = URL_SAFE_NO_PAD.encode(hash);

        Self {
            verifier,
            challenge,
            method: "S256",
        }
    }
}

pub fn generate_state() -> String {
    let mut random_bytes = [0u8; 16];
    rand::fill(&mut random_bytes);
    URL_SAFE_NO_PAD.encode(random_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_format_valid() {
        let pkce = PkceChallenge::generate();
        assert_eq!(pkce.method, "S256");
        assert!(!pkce.verifier.is_empty() && !pkce.challenge.is_empty());
        assert!(!pkce.verifier.contains('=') && !pkce.challenge.contains('='));
    }

    #[test]
    fn pkce_challenge_matches_sha256() {
        let pkce = PkceChallenge::generate();
        let mut hasher = Sha256::new();
        hasher.update(pkce.verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(pkce.challenge, expected);
    }

    #[test]
    fn state_generation_produces_unique_strings() {
        let s1 = generate_state();
        let s2 = generate_state();
        assert_ne!(s1, s2);
        assert!(!s1.is_empty());
    }
}
