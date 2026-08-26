use moka::future::Cache;
use std::time::Duration;

#[derive(Clone)]
pub struct FetchCache {
    cache: Cache<String, String>,
}

impl FetchCache {
    pub fn new(ttl_sec: u64, max_entries: u64) -> Self {
        let cache = Cache::builder()
            .time_to_live(Duration::from_secs(ttl_sec))
            .max_capacity(max_entries)
            .build();
        Self { cache }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        self.cache.get(key).await
    }

    pub async fn insert(&self, key: String, val: String) {
        self.cache.insert(key, val).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn caches_and_expires_fetched_content() {
        let cache = FetchCache::new(1, 2);
        cache.insert("url".to_string(), "content".to_string()).await;
        assert_eq!(cache.get("url").await.as_deref(), Some("content"));

        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert_eq!(cache.get("url").await, None);
    }
}
