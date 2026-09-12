//! Web inspection and search tools:
//!
//! Subsystem architecture and seams:
//! - `http/`: connection pooling, SSRF prevention, and body size limits.
//! - `fetch/`: URL query parameters and cache in root, format-specific
//!   parsers in `extract/` (`html`, `markdown`, `feed`, `data`).
//! - `search/`: query building and filters in `query.rs`, engine parsers in
//!   dedicated engine modules, result deduplication in `result.rs`, and
//!   markdown presentation in `format.rs`.
//!
//! Conventions: keep networking in `http/`, queries in `query.rs`, parsing in
//! engine or extract modules, and presentation formatting separate from data
//! models to maintain files under ~200 lines.

pub mod fetch;
pub mod http;
pub mod rate_limiter;
pub mod search;

pub use fetch::cache::FetchCache;
pub use fetch::{WebFetchConfig, WebFetchTool};
pub use http::HttpClient;
pub use rate_limiter::SearchRateLimiter;
pub use search::{WebSearchConfig, WebSearchTool};
