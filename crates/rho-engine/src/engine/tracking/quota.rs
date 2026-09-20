use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QuotaKey {
    pub provider: String,
    pub model: Option<String>,
}

impl QuotaKey {
    pub fn new(provider: impl Into<String>, model: Option<impl Into<String>>) -> Self {
        Self {
            provider: provider.into(),
            model: model.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct QuotaEntry {
    display: Option<String>,
    fetched_at: Option<Instant>,
    error_until: Option<Instant>,
    backoff_secs: u64,
    in_flight: bool,
    in_flight_since: Option<Instant>,
}

#[derive(Clone)]
pub struct QuotaTracker {
    entries: Arc<Mutex<HashMap<QuotaKey, QuotaEntry>>>,
    version_tx: Arc<tokio::sync::watch::Sender<u64>>,
}

impl Default for QuotaTracker {
    fn default() -> Self {
        let (tx, _) = tokio::sync::watch::channel(0);
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            version_tx: Arc::new(tx),
        }
    }
}

impl QuotaTracker {
    pub fn should_fetch(&self, key: &QuotaKey) -> bool {
        let Ok(entries) = self.entries.lock() else {
            return false;
        };
        let Some(entry) = entries.get(key) else {
            return true;
        };
        let now = Instant::now();
        if entry.in_flight
            && let Some(since) = entry.in_flight_since
            && now.duration_since(since) < Duration::from_secs(30)
        {
            return false;
        }
        if let Some(error_until) = entry.error_until
            && now < error_until
        {
            return false;
        }
        match entry.fetched_at {
            Some(fetched_at) => now.duration_since(fetched_at) >= Duration::from_secs(300),
            None => true,
        }
    }

    pub fn begin_fetch(&self, key: &QuotaKey) -> bool {
        let Ok(mut entries) = self.entries.lock() else {
            return false;
        };
        let now = Instant::now();
        let entry = entries.entry(key.clone()).or_default();
        if entry.in_flight
            && let Some(since) = entry.in_flight_since
            && now.duration_since(since) < Duration::from_secs(30)
        {
            return false;
        }
        if let Some(error_until) = entry.error_until
            && now < error_until
        {
            return false;
        }
        if let Some(fetched_at) = entry.fetched_at
            && now.duration_since(fetched_at) < Duration::from_secs(300)
        {
            return false;
        }
        entry.in_flight = true;
        entry.in_flight_since = Some(now);
        true
    }

    pub fn record_success(&self, key: &QuotaKey, display: String) {
        if let Ok(mut entries) = self.entries.lock() {
            let entry = entries.entry(key.clone()).or_default();
            entry.in_flight = false;
            entry.in_flight_since = None;
            entry.display = Some(display);
            entry.fetched_at = Some(Instant::now());
            entry.error_until = None;
            entry.backoff_secs = 60;
        }
        self.version_tx.send_modify(|v| *v = v.wrapping_add(1));
    }

    pub fn record_failure(&self, key: &QuotaKey) {
        if let Ok(mut entries) = self.entries.lock() {
            let entry = entries.entry(key.clone()).or_default();
            entry.in_flight = false;
            entry.in_flight_since = None;
            let backoff = entry.backoff_secs.max(60);
            entry.error_until = Some(Instant::now() + Duration::from_secs(backoff));
            entry.backoff_secs = (backoff * 2).min(300);
        }
    }

    pub fn replace(&self, key: &QuotaKey, value: Option<String>) {
        if let Ok(mut entries) = self.entries.lock() {
            let entry = entries.entry(key.clone()).or_default();
            entry.in_flight = false;
            entry.in_flight_since = None;
            entry.display = value;
            entry.fetched_at = Some(Instant::now());
        }
        self.version_tx.send_modify(|v| *v = v.wrapping_add(1));
    }

    pub fn version(&self) -> u64 {
        *self.version_tx.borrow()
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.version_tx.subscribe()
    }

    pub fn display_for(&self, key: &QuotaKey) -> Option<String> {
        let entries = self.entries.lock().ok()?;
        entries.get(key).and_then(|e| e.display.clone()).or_else(|| {
            key.model.as_ref()?;
            let fallback = QuotaKey::new(&key.provider, None::<String>);
            entries.get(&fallback).and_then(|e| e.display.clone())
        })
    }
}
