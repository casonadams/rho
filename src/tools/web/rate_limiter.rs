use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::sleep;

#[derive(Clone)]
pub struct SearchRateLimiter {
    last_search: Arc<Mutex<Option<Instant>>>,
    min_interval: Duration,
}

impl SearchRateLimiter {
    pub fn new(min_interval_ms: u64) -> Self {
        Self {
            last_search: Arc::new(Mutex::new(None)),
            min_interval: Duration::from_millis(min_interval_ms),
        }
    }

    pub async fn acquire(&self) {
        let mut last = self.last_search.lock().await;
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < self.min_interval {
                let wait = self.min_interval - elapsed;
                sleep(wait).await;
            }
        }
        *last = Some(Instant::now());
    }
}

pub fn parse_retry_after(header_val: Option<&str>, default_backoff: Duration) -> Duration {
    header_val
        .and_then(|val| val.trim().parse::<u64>().ok())
        .map(|sec| Duration::from_secs(sec.min(5)))
        .unwrap_or(default_backoff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = SearchRateLimiter::new(50);
        let t1 = Instant::now();
        limiter.acquire().await;
        limiter.acquire().await;
        let elapsed = t1.elapsed();
        assert!(elapsed >= Duration::from_millis(45));
    }
}
