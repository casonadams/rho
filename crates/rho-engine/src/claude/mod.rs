//! Claude OAuth provider: direct Messages API streaming, OAuth headers,
//! dynamic in-session token auto-refresh with 401 retry, and rig CompletionModel.

pub mod client;
pub mod completion;
pub mod http;
pub mod request;
pub mod stream;

pub use client::{ClaudeClient, into_handle};
pub use http::{DEFAULT_ENDPOINT, MESSAGES_PATH, PROVIDER_NAME, claude_headers, friendly_error};
pub use stream::SseParser;
