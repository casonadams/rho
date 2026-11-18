use crate::error::Result;
use serde::Serialize;
use std::sync::Mutex;

use super::session_error;

const MIN_SECRET_LEN: usize = 4;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum PersistedScan {
    #[default]
    Pending,
    Clean,
    Contaminated,
}

#[derive(Debug, Default)]
pub(crate) struct SecretGuard {
    secrets: Mutex<Vec<String>>,
    persisted_scan: Mutex<PersistedScan>,
}

fn contaminated_error() -> crate::error::AppError {
    session_error("session contains credential material and cannot be resumed")
}

impl SecretGuard {
    pub(crate) fn new(secrets: impl IntoIterator<Item = String>) -> Self {
        Self {
            secrets: Mutex::new(
                secrets
                    .into_iter()
                    .filter(|secret| secret.len() >= MIN_SECRET_LEN)
                    .collect(),
            ),
            persisted_scan: Mutex::new(PersistedScan::Pending),
        }
    }

    pub(crate) fn needs_persisted_scan(&self, secrets: &[String]) -> Result<bool> {
        match *self.scan_lock()? {
            PersistedScan::Contaminated => return Err(contaminated_error()),
            PersistedScan::Pending => return Ok(true),
            PersistedScan::Clean => {}
        }
        let current = self.lock()?;
        Ok(secrets
            .iter()
            .any(|secret| secret.len() >= MIN_SECRET_LEN && !current.contains(secret)))
    }

    pub(crate) fn add(&self, secrets: impl IntoIterator<Item = String>, persisted: &str) -> Result<()> {
        let mut current = self.lock()?;
        current.extend(secrets.into_iter().filter(|secret| secret.len() >= MIN_SECRET_LEN));
        current.sort();
        current.dedup();
        let found = current.iter().any(|secret| persisted.contains(secret));
        drop(current);
        let mut scan = self.scan_lock()?;
        if found || *scan == PersistedScan::Contaminated {
            *scan = PersistedScan::Contaminated;
            return Err(contaminated_error());
        }
        *scan = PersistedScan::Clean;
        Ok(())
    }

    pub(crate) fn redact(&self, value: &str) -> String {
        if value.len() < MIN_SECRET_LEN {
            return value.to_string();
        }
        let Ok(secrets) = self.secrets.lock() else {
            return "[REDACTED]".to_string();
        };
        if secrets.is_empty() || !secrets.iter().any(|secret| value.contains(secret)) {
            return value.to_string();
        }
        secrets.iter().fold(value.to_string(), |redacted, secret| {
            if redacted.contains(secret) {
                redacted.replace(secret, "[REDACTED]")
            } else {
                redacted
            }
        })
    }

    pub(crate) fn redact_stream(&self, carry: &mut String, chunk: &str) -> String {
        carry.push_str(chunk);
        let redacted = self.redact(carry);
        let split = redacted.len() - self.partial_secret_suffix_len(&redacted);
        *carry = redacted[split..].to_string();
        redacted[..split].to_string()
    }

    fn partial_secret_suffix_len(&self, text: &str) -> usize {
        let Ok(secrets) = self.secrets.lock() else {
            return text.len();
        };
        secrets
            .iter()
            .filter_map(|secret| {
                (1..secret.len().min(text.len() + 1))
                    .rev()
                    .find(|&len| secret.is_char_boundary(len) && text.ends_with(&secret[..len]))
            })
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn reject_in<T: Serialize>(&self, value: &T) -> Result<()> {
        let encoded = serde_json::to_string(value).map_err(|_| session_error("session record serialization failed"))?;
        let secrets = self.lock()?;
        if secrets.iter().any(|secret| encoded.contains(secret)) {
            return Err(session_error("session record contains credential material"));
        }
        Ok(())
    }

    fn scan_lock(&self) -> Result<std::sync::MutexGuard<'_, PersistedScan>> {
        self.persisted_scan
            .lock()
            .map_err(|_| session_error("session credential guard failed"))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Vec<String>>> {
        self.secrets
            .lock()
            .map_err(|_| session_error("session credential guard failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(secrets: &[&str]) -> Vec<String> {
        secrets.iter().map(|secret| (*secret).to_string()).collect()
    }

    #[test]
    fn only_a_clean_scan_lets_known_secrets_skip_rescanning() {
        let guard = SecretGuard::new(owned(&["known-secret"]));
        assert!(guard.needs_persisted_scan(&[]).unwrap());
        guard.add(Vec::new(), "clean history").unwrap();
        let cases: [(&[&str], bool); 4] = [
            (&[], false),
            (&["known-secret"], false),
            (&["abc"], false),
            (&["known-secret", "new-secret"], true),
        ];
        for (candidates, expected) in cases {
            assert_eq!(
                guard.needs_persisted_scan(&owned(candidates)).unwrap(),
                expected,
                "{candidates:?}"
            );
        }
    }

    #[test]
    fn constructor_secret_in_persisted_history_rejects_resume() {
        let guard = SecretGuard::new(owned(&["known-secret"]));
        let err = guard.add(Vec::new(), "log known-secret").unwrap_err();
        assert!(err.to_string().contains("cannot be resumed"));
    }

    #[test]
    fn contamination_keeps_rejecting_after_the_first_failure() {
        let guard = SecretGuard::new(Vec::new());
        assert!(guard.add(owned(&["fresh-secret"]), "log fresh-secret").is_err());
        assert_eq!(guard.redact("fresh-secret"), "[REDACTED]");
        assert!(guard.needs_persisted_scan(&owned(&["fresh-secret"])).is_err());
        assert!(guard.add(owned(&["fresh-secret"]), "").is_err());
    }
}
