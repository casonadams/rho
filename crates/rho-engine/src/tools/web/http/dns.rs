use rho_harness_core::error::{AppError, Result};
use rho_harness_core::net::{is_private_host, is_private_ip};
use url::Url;

pub async fn assert_public_dns(url: &Url) -> Result<()> {
    let port = url.port_or_known_default().unwrap_or(80);
    let Some(host) = url.host_str() else {
        return Ok(());
    };
    if is_private_host(host) {
        return Err(AppError::Tool(format!(
            "Access to private/local network host '{host}' is blocked for security"
        )));
    }
    let addrs = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(|e| AppError::Tool(format!("DNS lookup failed for {host}: {e}")))?;
    for addr in addrs {
        if is_private_ip(addr.ip()) {
            return Err(AppError::Tool(format!(
                "Host '{host}' resolved to blocked private network address '{}'",
                addr.ip()
            )));
        }
    }
    Ok(())
}
