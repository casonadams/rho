//! Shared HTTP client singleton for auth and OAuth token operations.

use std::sync::LazyLock;
use std::time::Duration;

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::install_crypto_provider();
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default()
});

pub fn http_client() -> &'static reqwest::Client {
    &HTTP_CLIENT
}
