use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct CachedResource {
    pub text: Arc<str>,
    pub final_url: Arc<str>,
}

#[derive(Clone)]
pub struct FetchCache {
    cache: Cache<String, CachedResource>,
}

impl FetchCache {
    pub fn new(ttl_sec: u64, max_entries: u64) -> Self {
        let cache = Cache::builder()
            .time_to_live(Duration::from_secs(ttl_sec))
            .max_capacity(max_entries)
            .build();
        Self { cache }
    }

    pub async fn get(&self, key: &str) -> Option<CachedResource> {
        self.cache.get(key).await
    }

    pub async fn insert(&self, key: String, val: CachedResource) {
        self.cache.insert(key, val).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn caches_and_expires_fetched_content() {
        let cache = FetchCache::new(1, 2);
        cache
            .insert(
                "url".to_string(),
                CachedResource {
                    text: Arc::from("content"),
                    final_url: Arc::from("url"),
                },
            )
            .await;
        let cached = cache.get("url").await.unwrap();
        assert_eq!(cached.text.as_ref(), "content");

        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(cache.get("url").await.is_none());
    }
}
