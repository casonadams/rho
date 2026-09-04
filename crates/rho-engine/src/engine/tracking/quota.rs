use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
struct QuotaCache {
    display: Option<String>,
    fetched_at: Option<Instant>,
    error_until: Option<Instant>,
    backoff_secs: u64,
}

#[derive(Clone, Default)]
pub struct QuotaTracker {
    cache: Arc<Mutex<QuotaCache>>,
}

impl QuotaTracker {
    pub fn should_fetch(&self) -> bool {
        let Ok(cache) = self.cache.lock() else {
            return false;
        };
        let now = Instant::now();
        if let Some(error_until) = cache.error_until
            && now < error_until
        {
            return false;
        }
        match cache.fetched_at {
            Some(fetched_at) => now.duration_since(fetched_at) >= Duration::from_secs(300),
            None => true,
        }
    }

    pub fn record_success(&self, display: String) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.display = Some(display);
            cache.fetched_at = Some(Instant::now());
            cache.error_until = None;
            cache.backoff_secs = 60;
        }
    }

    pub fn record_failure(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            let backoff = cache.backoff_secs.max(60);
            cache.error_until = Some(Instant::now() + Duration::from_secs(backoff));
            cache.backoff_secs = (backoff * 2).min(300);
        }
    }

    pub fn replace(&self, value: Option<String>) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.display = value;
            cache.fetched_at = Some(Instant::now());
        }
    }

    pub fn latest(&self) -> Option<String> {
        self.cache.lock().ok().and_then(|c| c.display.clone())
    }
}
