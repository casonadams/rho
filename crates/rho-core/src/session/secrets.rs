use crate::error::Result;
use serde::Serialize;
use std::sync::Mutex;

use super::session_error;

const MIN_SECRET_LEN: usize = 4;

#[derive(Debug, Default)]
pub(crate) struct SecretGuard {
    secrets: Mutex<Vec<String>>,
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
        }
    }

    pub(crate) fn add(&self, secrets: impl IntoIterator<Item = String>, persisted: &str) -> Result<()> {
        let mut current = self.lock()?;
        current.extend(secrets.into_iter().filter(|secret| secret.len() >= MIN_SECRET_LEN));
        current.sort();
        current.dedup();
        if current.iter().any(|secret| persisted.contains(secret)) {
            return Err(session_error(
                "session contains credential material and cannot be resumed",
            ));
        }
        Ok(())
    }

    pub(crate) fn redact(&self, value: &str) -> String {
        let Ok(secrets) = self.secrets.lock() else {
            return "[REDACTED]".to_string();
        };
        secrets.iter().fold(value.to_string(), |redacted, secret| {
            redacted.replace(secret, "[REDACTED]")
        })
    }

    pub(crate) fn reject_in<T: Serialize>(&self, value: &T) -> Result<()> {
        let encoded = serde_json::to_string(value).map_err(|_| session_error("session record serialization failed"))?;
        let secrets = self.lock()?;
        if secrets.iter().any(|secret| encoded.contains(secret)) {
            return Err(session_error("session record contains credential material"));
        }
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Vec<String>>> {
        self.secrets
            .lock()
            .map_err(|_| session_error("session credential guard failed"))
    }
}
