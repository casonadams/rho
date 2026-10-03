use super::client::McpClient;
use super::process::McpProcess;
use super::transport::McpTransport;
use rho_harness_core::config::{McpLifecycleMode, McpServerConfig};
use rho_harness_core::error::{AppError, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;

pub struct ServerInstance {
    pub client: Arc<McpClient>,
    pub last_activity: Instant,
    pub in_flight: Arc<AtomicUsize>,
    pub is_stdio: bool,
}

pub struct ServerRegistration {
    pub config: McpServerConfig,
    pub working_dir: PathBuf,
    pub instance: Option<ServerInstance>,
}

#[derive(Clone)]
pub struct McpServerPool {
    servers: Arc<RwLock<BTreeMap<String, ServerRegistration>>>,
    connection_lock: Arc<Mutex<()>>,
    default_idle_timeout: Duration,
}

impl McpServerPool {
    pub fn new(default_idle_timeout_secs: u64) -> Self {
        Self {
            servers: Arc::new(RwLock::new(BTreeMap::new())),
            connection_lock: Arc::new(Mutex::new(())),
            default_idle_timeout: Duration::from_secs(default_idle_timeout_secs),
        }
    }

    pub async fn register(&self, name: impl Into<String>, config: McpServerConfig, working_dir: impl Into<PathBuf>) {
        let mut guard = self.servers.write().await;
        guard.insert(
            name.into(),
            ServerRegistration {
                config,
                working_dir: working_dir.into(),
                instance: None,
            },
        );
    }

    pub async fn has_active_instance(&self, name: &str) -> bool {
        let guard = self.servers.read().await;
        guard.get(name).map(|r| r.instance.is_some()).unwrap_or(false)
    }

    pub async fn active_instances_count(&self) -> usize {
        let guard = self.servers.read().await;
        guard.values().filter(|r| r.instance.is_some()).count()
    }

    pub async fn get_or_connect(&self, name: &str) -> Result<(Arc<McpClient>, Arc<AtomicUsize>)> {
        {
            let guard = self.servers.read().await;
            if let Some(reg) = guard.get(name) {
                if let Some(inst) = &reg.instance {
                    inst.in_flight.fetch_add(1, Ordering::SeqCst);
                    return Ok((Arc::clone(&inst.client), Arc::clone(&inst.in_flight)));
                }
            } else {
                return Err(AppError::Mcp(format!("MCP server '{name}' is not registered")));
            }
        }

        let _lock = self.connection_lock.lock().await;

        {
            let guard = self.servers.read().await;
            if let Some(reg) = guard.get(name)
                && let Some(inst) = &reg.instance
            {
                inst.in_flight.fetch_add(1, Ordering::SeqCst);
                return Ok((Arc::clone(&inst.client), Arc::clone(&inst.in_flight)));
            }
        }

        let (config, working_dir) = {
            let guard = self.servers.read().await;
            let reg = guard
                .get(name)
                .ok_or_else(|| AppError::Mcp(format!("MCP server '{name}' is not registered")))?;
            (reg.config.clone(), reg.working_dir.clone())
        };

        let client = connect_client(name, &config, &working_dir).await?;
        let in_flight = Arc::new(AtomicUsize::new(1));
        let is_stdio = config.url.is_none();

        let mut guard = self.servers.write().await;
        if let Some(reg) = guard.get_mut(name) {
            reg.instance = Some(ServerInstance {
                client: Arc::clone(&client),
                last_activity: Instant::now(),
                in_flight: Arc::clone(&in_flight),
                is_stdio,
            });
        }

        Ok((client, in_flight))
    }

    pub async fn disconnect(&self, name: &str) -> bool {
        let mut guard = self.servers.write().await;
        if let Some(reg) = guard.get_mut(name) {
            reg.instance.take().is_some()
        } else {
            false
        }
    }

    pub async fn touch_activity(&self, name: &str) {
        let mut guard = self.servers.write().await;
        if let Some(reg) = guard.get_mut(name)
            && let Some(inst) = &mut reg.instance
        {
            inst.last_activity = Instant::now();
        }
    }

    pub async fn reap_idle_servers(&self) -> Vec<String> {
        let now = Instant::now();
        let mut guard = self.servers.write().await;
        let mut reaped = Vec::new();

        for (name, reg) in guard.iter_mut() {
            if should_reap_registration(reg, self.default_idle_timeout, now) {
                reg.instance = None;
                reaped.push(name.clone());
            }
        }

        reaped
    }

    pub fn spawn_reaper_task(&self, interval: Duration) -> tokio::task::JoinHandle<()> {
        let weak_servers = Arc::downgrade(&self.servers);
        let default_timeout = self.default_idle_timeout;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                let Some(servers_arc) = weak_servers.upgrade() else {
                    break;
                };
                let now = Instant::now();
                let mut guard = servers_arc.write().await;
                for reg in guard.values_mut() {
                    if should_reap_registration(reg, default_timeout, now) {
                        reg.instance = None;
                    }
                }
            }
        })
    }
}

fn should_reap_registration(reg: &ServerRegistration, default_timeout: Duration, now: Instant) -> bool {
    let Some(inst) = &reg.instance else {
        return false;
    };

    if !inst.is_stdio || inst.in_flight.load(Ordering::SeqCst) > 0 {
        return false;
    }

    let lifecycle = reg.config.lifecycle.unwrap_or(McpLifecycleMode::Lazy);
    if matches!(lifecycle, McpLifecycleMode::KeepAlive | McpLifecycleMode::LazyKeepAlive) {
        return false;
    }

    let timeout = match reg.config.idle_timeout_seconds {
        Some(0) => return false,
        Some(secs) => Duration::from_secs(secs),
        None => default_timeout,
    };

    if timeout.is_zero() {
        return false;
    }

    now.duration_since(inst.last_activity) >= timeout
}

async fn connect_client(name: &str, config: &McpServerConfig, working_dir: &Path) -> Result<Arc<McpClient>> {
    let timeout = config.timeout_seconds.map(Duration::from_secs);
    let transport = if let Some(url) = &config.url {
        let kind = config.resolved_transport();
        McpTransport::new_http(url, kind, config.headers.clone(), timeout)
    } else {
        let (stdin, stdout, handle) = McpProcess::spawn(config, working_dir)?;
        McpTransport::new_stdio(stdin, stdout, handle, timeout)
    };

    let client = Arc::new(McpClient::new(name, transport));
    client.initialize().await?;
    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_server_script() -> &'static str {
        "while read l; do \
            id=$(echo \"$l\" | grep -o '\"id\":[0-9]*' | cut -d: -f2); \
            if [ -n \"$id\" ]; then \
                echo \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":$id,\\\"result\\\":{\\\"protocolVersion\\\":\\\"2024-11-05\\\",\\\"capabilities\\\":{},\\\"serverInfo\\\":{\\\"name\\\":\\\"mock\\\"}}}\"; \
            fi; \
        done\n"
    }

    #[tokio::test]
    async fn test_pool_lazy_connect_and_reap() {
        let pool = McpServerPool::new(1);
        let config = McpServerConfig::stdio("/bin/sh", vec!["-c".to_string(), mock_server_script().to_string()]);
        let cwd = std::env::temp_dir();

        pool.register("echo", config, cwd).await;
        assert_eq!(pool.active_instances_count().await, 0);

        let (_client, in_flight) = pool.get_or_connect("echo").await.unwrap();
        assert_eq!(pool.active_instances_count().await, 1);

        let reaped = pool.reap_idle_servers().await;
        assert!(reaped.is_empty());
        assert_eq!(pool.active_instances_count().await, 1);

        in_flight.fetch_sub(1, Ordering::SeqCst);
        pool.touch_activity("echo").await;

        tokio::time::sleep(Duration::from_millis(1100)).await;
        let reaped = pool.reap_idle_servers().await;
        assert_eq!(reaped, vec!["echo".to_string()]);
        assert_eq!(pool.active_instances_count().await, 0);

        let (_client2, in_flight2) = pool.get_or_connect("echo").await.unwrap();
        assert_eq!(pool.active_instances_count().await, 1);
        in_flight2.fetch_sub(1, Ordering::SeqCst);
    }

    #[tokio::test]
    async fn test_keep_alive_lifecycle_not_reaped() {
        let pool = McpServerPool::new(1);
        let mut config = McpServerConfig::stdio("/bin/sh", vec!["-c".to_string(), mock_server_script().to_string()]);
        config.lifecycle = Some(McpLifecycleMode::KeepAlive);
        let cwd = std::env::temp_dir();

        pool.register("keep", config, cwd).await;
        let (_client, in_flight) = pool.get_or_connect("keep").await.unwrap();
        in_flight.fetch_sub(1, Ordering::SeqCst);

        tokio::time::sleep(Duration::from_millis(1100)).await;
        let reaped = pool.reap_idle_servers().await;
        assert!(reaped.is_empty());
        assert_eq!(pool.active_instances_count().await, 1);
    }
}
