//! Authentication and credential subsystem: PKCE, OAuth flows, and secure store.

pub mod antigravity;
pub(crate) mod http;
pub mod loopback;
pub mod oauth;
pub mod pkce;
pub mod resolver;
pub mod store;

pub use loopback::{CallbackParams, LoopbackServer};
pub use oauth::{perform_oauth_login, refresh_oauth_token};
pub use pkce::{PkceChallenge, generate_state};
pub use resolver::resolve_secret_value;
pub use store::AuthStore;

#[cfg(test)]
mod tests;
