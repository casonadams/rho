pub mod client;
pub mod oauth;
pub mod stream;
pub mod types;

#[cfg(test)]
mod tests;

pub use client::{HTTP_CLIENT, discover_project_id, fetch_account_usage, stable_project_id};
pub use oauth::{
    AUTH_URL, CALLBACK_TIMEOUT, REDIRECT_URI, SCOPES, TOKEN_URL, USERINFO_URL, build_auth_url, client_id,
    client_secret, exchange_code_for_tokens, fetch_user_email, generate_pkce, generate_state, load_saved_tokens,
    refresh_access_token, save_tokens, start_callback_listener,
};
pub use stream::{AntigravityProvider, build_antigravity_request};
pub use types::{AntigravityGenerateRequest, AntigravityTokens};
