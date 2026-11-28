//! Web inspection and search tools. `http/` owns networking (pooling, SSRF checks, body limits); `fetch/`
//! and `search/` own parsing and queries, and presentation stays out of data models.

pub mod fetch;
pub mod http;
pub mod rate_limiter;
pub mod search;
#[cfg(test)]
mod test_server;

pub use fetch::WebFetchConfig;
pub(crate) use fetch::WebFetchTool;
pub(crate) use fetch::cache::FetchCache;
pub use http::HttpClient;
pub(crate) use rate_limiter::SearchRateLimiter;
pub use search::WebSearchConfig;
pub(crate) use search::WebSearchTool;

/// Parses a CSS selector literal that is known valid at compile time.
pub(crate) fn selector(css: &str) -> scraper::Selector {
    scraper::Selector::parse(css).expect("selector literal is valid CSS")
}
