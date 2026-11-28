use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
pub(crate) struct CachedResource {
    pub text: Arc<str>,
    pub final_url: Arc<str>,
}

#[derive(Clone)]
pub(crate) struct FetchCache {
    cache: Cache<String, CachedResource>,
}

impl FetchCache {
    pub fn new(ttl_sec: u64, max_entries: u64, max_bytes: u64) -> Self {
        Self::with_ttl(Duration::from_secs(ttl_sec), max_entries, max_bytes)
    }

    /// Bounds both the entry count and the total text size: every entry weighs at least
    /// `max_bytes / max_entries`, so `max_entries` small entries use up the byte budget too.
    pub(crate) fn with_ttl(ttl: Duration, max_entries: u64, max_bytes: u64) -> Self {
        let floor = (max_bytes / max_entries.max(1)).max(1);
        let cache = Cache::builder()
            .time_to_live(ttl)
            .max_capacity(max_bytes)
            .weigher(move |key: &String, value: &CachedResource| {
                let bytes = (key.len() + value.text.len() + value.final_url.len()) as u64;
                u32::try_from(bytes.max(floor)).unwrap_or(u32::MAX)
            })
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

    fn resource(text: &str) -> CachedResource {
        CachedResource {
            text: Arc::from(text),
            final_url: Arc::from("url"),
        }
    }

    #[tokio::test]
    async fn caches_and_expires_fetched_content() {
        let cache = FetchCache::with_ttl(Duration::from_millis(30), 2, 1024);
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

        tokio::time::sleep(Duration::from_millis(45)).await;
        assert!(cache.get("url").await.is_none());
    }

    #[tokio::test]
    async fn total_cached_text_stays_within_the_byte_budget() {
        let cache = FetchCache::with_ttl(Duration::from_secs(60), 100, 10_000);
        let page = "x".repeat(4_000);
        for i in 0..20 {
            cache.insert(format!("https://example.com/{i}"), resource(&page)).await;
            cache.cache.run_pending_tasks().await;
        }

        cache.cache.run_pending_tasks().await;
        assert!(cache.cache.weighted_size() <= 10_000, "{}", cache.cache.weighted_size());
        assert!(cache.cache.entry_count() <= 2, "{}", cache.cache.entry_count());
    }

    #[tokio::test]
    async fn many_small_entries_are_bounded_by_the_entry_limit() {
        let cache = FetchCache::with_ttl(Duration::from_secs(60), 4, 4_000);
        for i in 0..40 {
            cache.insert(format!("k{i}"), resource("tiny")).await;
            cache.cache.run_pending_tasks().await;
        }

        assert!(cache.cache.entry_count() <= 4, "{}", cache.cache.entry_count());
    }

    #[tokio::test]
    async fn a_page_larger_than_the_whole_budget_is_not_cached() {
        let cache = FetchCache::with_ttl(Duration::from_secs(60), 4, 1_000);
        cache.insert("big".to_string(), resource(&"x".repeat(5_000))).await;
        cache.cache.run_pending_tasks().await;

        assert!(cache.get("big").await.is_none());
    }
}
