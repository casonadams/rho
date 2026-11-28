use crate::error::Result;
use serde::Serialize;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use super::session_error;
use crate::secrets::{MIN_SECRET_LEN, REDACTED_MARKER, redact_credentials_text};

const SCAN_CHUNK_BYTES: usize = 64 * 1024;

/// Searches `reader` for any of `needles` a chunk at a time, overlapping chunks by the longest
/// needle so a match across a chunk boundary is still found.
fn reader_contains_any(mut reader: impl Read, needles: &[String], chunk_bytes: usize) -> std::io::Result<bool> {
    let Some(overlap) = needles.iter().map(|needle| needle.len().saturating_sub(1)).max() else {
        return Ok(false);
    };
    let mut window = Vec::with_capacity(chunk_bytes + overlap);
    loop {
        let kept = window.len();
        window.resize(kept + chunk_bytes, 0);
        let read = loop {
            match reader.read(&mut window[kept..]) {
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                other => break other?,
            }
        };
        window.truncate(kept + read);
        if read == 0 {
            return Ok(false);
        }
        let text = String::from_utf8_lossy(&window);
        if needles.iter().any(|needle| text.contains(needle.as_str())) {
            return Ok(true);
        }
        window.drain(..window.len().saturating_sub(overlap));
    }
}

pub(crate) fn file_contains_any(path: &Path, needles: &[String]) -> Result<bool> {
    if needles.is_empty() {
        return Ok(false);
    }
    Ok(reader_contains_any(
        std::fs::File::open(path)?,
        needles,
        SCAN_CHUNK_BYTES,
    )?)
}

fn replace_known_secrets(value: &str, secrets: &[String]) -> String {
    if secrets.is_empty() || !secrets.iter().any(|secret| value.contains(secret)) {
        return value.to_string();
    }
    secrets.iter().fold(value.to_string(), |redacted, secret| {
        if redacted.contains(secret) {
            redacted.replace(secret, REDACTED_MARKER)
        } else {
            redacted
        }
    })
}

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
        let mut secrets: Vec<String> = secrets
            .into_iter()
            .filter(|secret| secret.len() >= MIN_SECRET_LEN)
            .collect();
        secrets.sort();
        secrets.dedup();
        Self {
            secrets: Mutex::new(secrets),
            persisted_scan: Mutex::new(PersistedScan::Pending),
        }
    }

    /// Starts redacting `secrets` and returns those the persisted history must be searched for:
    /// every known secret before the first scan, afterwards only ones not known yet.
    pub(crate) fn add(&self, secrets: impl IntoIterator<Item = String>) -> Result<Vec<String>> {
        let scan = *self.scan_lock()?;
        if scan == PersistedScan::Contaminated {
            return Err(contaminated_error());
        }
        let mut current = self.lock()?;
        let mut added: Vec<String> = secrets
            .into_iter()
            .filter(|secret| secret.len() >= MIN_SECRET_LEN && !current.contains(secret))
            .collect();
        added.sort();
        added.dedup();
        current.extend(added.iter().cloned());
        Ok(if scan == PersistedScan::Pending {
            current.clone()
        } else {
            added
        })
    }

    /// Records whether a search of the persisted history found any of the secrets from [`Self::add`].
    pub(crate) fn record_scan(&self, found: bool) -> Result<()> {
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
            return REDACTED_MARKER.to_string();
        };
        let with_known = replace_known_secrets(value, &secrets);
        redact_credentials_text(&with_known)
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
    use test_case::test_case;

    fn owned(secrets: &[&str]) -> Vec<String> {
        secrets.iter().map(|secret| (*secret).to_string()).collect()
    }

    fn scan(guard: &SecretGuard, secrets: &[&str], persisted: &str) -> Result<()> {
        let needles = guard.add(owned(secrets))?;
        let found = reader_contains_any(persisted.as_bytes(), &needles, SCAN_CHUNK_BYTES).unwrap();
        guard.record_scan(found)
    }

    #[test]
    fn after_a_clean_scan_only_secrets_not_known_yet_are_searched_for() {
        let guard = SecretGuard::new(owned(&["known-secret", "known-secret"]));
        assert_eq!(guard.add(Vec::new()).unwrap(), owned(&["known-secret"]));
        guard.record_scan(false).unwrap();
        let cases: [(&[&str], &[&str]); 4] = [
            (&[], &[]),
            (&["known-secret"], &[]),
            (&["abc"], &[]),
            (&["known-secret", "new-secret", "new-secret"], &["new-secret"]),
        ];
        for (candidates, expected) in cases {
            assert_eq!(guard.add(owned(candidates)).unwrap(), owned(expected), "{candidates:?}");
        }
        assert_eq!(guard.redact("new-secret known-secret"), "[REDACTED] [REDACTED]");
    }

    #[test]
    fn constructor_secret_in_persisted_history_rejects_resume() {
        let guard = SecretGuard::new(owned(&["known-secret"]));
        let err = scan(&guard, &[], "log known-secret").unwrap_err();
        assert!(err.to_string().contains("cannot be resumed"));
    }

    #[test]
    fn contamination_keeps_rejecting_after_the_first_failure() {
        let guard = SecretGuard::new(Vec::new());
        assert!(scan(&guard, &["fresh-secret"], "log fresh-secret").is_err());
        assert_eq!(guard.redact("fresh-secret"), "[REDACTED]");
        assert!(guard.add(owned(&["other-secret"])).is_err());
        assert!(guard.record_scan(false).is_err());
    }

    #[test_case(String::new(), false; "empty")]
    #[test_case("x".repeat(100), false; "absent")]
    #[test_case(format!("secret-value{}", "x".repeat(50)), true; "at start")]
    #[test_case(format!("{}secret-value{}", "x".repeat(13), "x".repeat(40)), true; "across boundary")]
    #[test_case(format!("{}secret-value", "x".repeat(77)), true; "at end")]
    #[test_case(format!("{}\u{e9}-secret", "\u{e9}".repeat(9)), true; "multibyte across boundary")]
    #[test_case(format!("{}secret-valu", "x".repeat(20)), false; "near miss")]
    fn chunked_search_finds_secrets_anywhere_including_across_chunk_boundaries(haystack: String, expected: bool) {
        let needles = owned(&["secret-value", "\u{e9}-secret"]);
        for chunk in [1, 7, 16, 1024] {
            assert_eq!(
                reader_contains_any(haystack.as_bytes(), &needles, chunk).unwrap(),
                expected,
                "chunk {chunk}"
            );
        }
    }
}
