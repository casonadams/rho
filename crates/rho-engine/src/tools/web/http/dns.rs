use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use rho_harness_core::net::is_private_ip;
use std::net::SocketAddr;

pub struct PublicDns;

pub async fn public_addrs(host: &str) -> Result<Vec<SocketAddr>, String> {
    let addrs = tokio::net::lookup_host((host, 0))
        .await
        .map_err(|e| format!("DNS lookup failed for {host}: {e}"))?;
    let public: Vec<SocketAddr> = addrs.filter(|addr| !is_private_ip(addr.ip())).collect();
    if public.is_empty() {
        return Err(format!(
            "Host '{host}' resolves only to blocked private network addresses"
        ));
    }
    Ok(public)
}

impl Resolve for PublicDns {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addrs = public_addrs(&host).await?;
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolver_rejects_hosts_resolving_only_to_private_addresses() {
        assert!(public_addrs("localhost").await.is_err());
    }

    #[tokio::test]
    async fn resolve_yields_public_addresses_and_rejects_private_ones() {
        let public: Name = "8.8.8.8".parse().unwrap();
        let addrs: Vec<SocketAddr> = PublicDns.resolve(public).await.unwrap().collect();
        assert_eq!(addrs, vec!["8.8.8.8:0".parse::<SocketAddr>().unwrap()]);

        let private: Name = "10.1.2.3".parse().unwrap();
        assert!(PublicDns.resolve(private).await.is_err());
    }
}
