use super::*;
use crate::http::CLIENT;
use crate::mcp::transport::McpTransport;
use rho_harness_core::auth::{DeviceCodeInfo, OAuthLoginCallbacks, SelectOption};
use rho_harness_core::config::{McpOAuthConfig, McpTransportKind};
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

#[derive(Default)]
struct FakeState {
    accepted_token: String,
    seen: Vec<Seen>,
    refreshes: usize,
    reject_all: bool,
}

type Shared = Arc<StdMutex<FakeState>>;

fn form_value(body: &str, key: &str) -> Option<String> {
    url::form_urlencoded::parse(body.as_bytes())
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

fn route(base: &str, req: &Seen, state: &Shared) -> (&'static str, Vec<(String, String)>, String) {
    let mut s = state.lock().unwrap();
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/mcp") => {
            let bearer = req.headers.get("authorization").cloned().unwrap_or_default();
            if !s.reject_all && !s.accepted_token.is_empty() && bearer == format!("Bearer {}", s.accepted_token) {
                return (
                    "200 OK",
                    Vec::new(),
                    r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#.into(),
                );
            }
            let challenge = format!(
                r#"Bearer resource_metadata="{base}/.well-known/oauth-protected-resource/mcp", scope="tools:read""#
            );
            (
                "401 Unauthorized",
                vec![("WWW-Authenticate".into(), challenge)],
                String::new(),
            )
        }
        ("GET", "/.well-known/oauth-protected-resource/mcp") => (
            "200 OK",
            Vec::new(),
            format!(r#"{{"resource":"{base}/mcp","authorization_servers":["{base}/auth"],"scopes_supported":["x"]}}"#),
        ),
        ("GET", "/.well-known/oauth-authorization-server/auth") => (
            "200 OK",
            Vec::new(),
            format!(
                r#"{{"authorization_endpoint":"{base}/auth/authorize","token_endpoint":"{base}/auth/token","registration_endpoint":"{base}/auth/register","code_challenge_methods_supported":["S256"]}}"#
            ),
        ),
        ("POST", "/auth/register") => ("201 Created", Vec::new(), r#"{"client_id":"dyn-client"}"#.into()),
        ("POST", "/auth/token") if form_value(&req.body, "grant_type").as_deref() == Some("refresh_token") => {
            s.refreshes += 1;
            s.accepted_token = "tok-2".into();
            (
                "200 OK",
                Vec::new(),
                r#"{"access_token":"tok-2","refresh_token":"ref-2","expires_in":3600}"#.into(),
            )
        }
        ("POST", "/auth/token") => {
            s.accepted_token = "tok-1".into();
            (
                "200 OK",
                Vec::new(),
                r#"{"access_token":"tok-1","refresh_token":"ref-1","expires_in":3600}"#.into(),
            )
        }
        _ => ("404 Not Found", Vec::new(), "{}".into()),
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Seen> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        let n = socket.read(&mut chunk).await.ok().filter(|n| *n > 0)?;
        raw.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let mut lines = head.lines();
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.split('?').next()?.to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let length = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    while raw.len() < header_end + length {
        let n = socket.read(&mut chunk).await.ok().filter(|n| *n > 0)?;
        raw.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&raw[header_end..header_end + length]).to_string();
    Some(Seen {
        method,
        path,
        headers,
        body,
    })
}

async fn fake_server() -> (String, Shared) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state: Shared = Arc::default();
    let (task_base, task_state) = (base.clone(), state.clone());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (base, state) = (task_base.clone(), task_state.clone());
            tokio::spawn(async move {
                let Some(req) = read_request(&mut socket).await else {
                    return;
                };
                let (status, headers, body) = route(&base, &req, &state);
                state.lock().unwrap().seen.push(req);
                let extra: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (base, state)
}

#[derive(Default)]
struct Browser {
    auth_url: StdMutex<Option<String>>,
}

#[async_trait::async_trait]
impl OAuthLoginCallbacks for Browser {
    async fn on_auth_url(&self, url: &str, _instructions: Option<&str>) -> Result<()> {
        *self.auth_url.lock().unwrap() = Some(url.to_string());
        let parsed = url::Url::parse(url).unwrap();
        let param = |key: &str| parsed.query_pairs().find(|(k, _)| k == key).unwrap().1.into_owned();
        let target = format!("{}?code=the-code&state={}", param("redirect_uri"), param("state"));
        tokio::spawn(async move {
            let _ = CLIENT.get(target).send().await;
        });
        Ok(())
    }
    async fn on_device_code(&self, _info: &DeviceCodeInfo<'_>) -> Result<()> {
        Ok(())
    }
    async fn on_prompt(&self, _message: &str, _secret: bool) -> Result<String> {
        Ok(String::new())
    }
    async fn on_select(&self, _message: &str, _options: &[SelectOption]) -> Result<Option<String>> {
        Ok(None)
    }
    async fn on_progress(&self, _message: &str) -> Result<()> {
        Ok(())
    }
}

fn transport(base: &str, store: &McpTokenStore) -> Arc<McpTransport> {
    let oauth = Arc::new(McpOAuth::new("srv", store.clone()));
    McpTransport::new_http(
        format!("{base}/mcp"),
        McpTransportKind::StreamableHttp,
        Default::default(),
        None,
        Some(oauth),
    )
}

fn query(url: &str) -> HashMap<String, String> {
    url::Url::parse(url).unwrap().query_pairs().into_owned().collect()
}

#[tokio::test]
async fn unauthenticated_requests_point_to_the_login_command() {
    let (base, _state) = fake_server().await;
    let dir = tempfile::tempdir().unwrap();
    let store = McpTokenStore::in_config_dir(dir.path());
    let err = transport(&base, &store).request("tools/list", None).await.unwrap_err();
    assert!(
        err.to_string().contains("MCP server 'srv' requires authorization"),
        "{err}"
    );
    assert!(err.to_string().contains("/mcp login srv"), "{err}");
}

#[tokio::test]
async fn login_discovers_registers_and_authorizes_then_requests_carry_the_token() {
    let (base, state) = fake_server().await;
    let dir = tempfile::tempdir().unwrap();
    let store = McpTokenStore::in_config_dir(dir.path());
    let browser = Browser::default();
    let url = format!("{base}/mcp");
    login(
        LoginRequest {
            server: "srv",
            url: &url,
            config: None,
        },
        &store,
        &browser,
    )
    .await
    .unwrap();

    let auth = query(&browser.auth_url.lock().unwrap().clone().unwrap());
    assert_eq!(auth["client_id"], "dyn-client");
    assert_eq!(auth["resource"], url);
    assert_eq!(auth["scope"], "tools:read");
    assert_eq!(auth["code_challenge_method"], "S256");
    assert!(auth["redirect_uri"].starts_with("http://localhost:"));

    let seen = state.lock().unwrap().seen.clone();
    let register = seen.iter().find(|r| r.path == "/auth/register").unwrap();
    assert!(register.body.contains(&auth["redirect_uri"]), "{}", register.body);
    let exchange = seen.iter().find(|r| r.path == "/auth/token").unwrap();
    assert_eq!(form_value(&exchange.body, "code").as_deref(), Some("the-code"));
    assert_eq!(form_value(&exchange.body, "resource").as_deref(), Some(url.as_str()));
    assert!(form_value(&exchange.body, "code_verifier").is_some());

    let stored = store.load("srv").await.unwrap().unwrap();
    assert_eq!(stored.access_token, "tok-1");
    assert_eq!(stored.client_id, "dyn-client");
    assert!(store.is_signed_in("srv"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join(TOKEN_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let value = transport(&base, &store).request("tools/list", None).await.unwrap();
    assert_eq!(value, serde_json::json!({ "ok": true }));
}

#[tokio::test]
async fn a_rejected_token_is_refreshed_once_and_the_request_retried() {
    let (base, state) = fake_server().await;
    let dir = tempfile::tempdir().unwrap();
    let store = McpTokenStore::in_config_dir(dir.path());
    store
        .save(
            "srv",
            McpTokens {
                access_token: "stale".into(),
                refresh_token: Some("ref-1".into()),
                expires_at_ms: None,
                token_endpoint: format!("{base}/auth/token"),
                client_id: "dyn-client".into(),
                client_secret: None,
                resource: format!("{base}/mcp"),
            },
        )
        .await
        .unwrap();

    let value = transport(&base, &store).request("tools/list", None).await.unwrap();
    assert_eq!(value, serde_json::json!({ "ok": true }));
    let stored = store.load("srv").await.unwrap().unwrap();
    assert_eq!(stored.access_token, "tok-2");
    assert_eq!(stored.refresh_token.as_deref(), Some("ref-2"));

    state.lock().unwrap().reject_all = true;
    let err = transport(&base, &store).request("tools/list", None).await.unwrap_err();
    assert!(err.to_string().contains("/mcp login srv"), "{err}");
    assert_eq!(state.lock().unwrap().refreshes, 2);
}

#[tokio::test]
async fn expired_tokens_are_refreshed_before_the_request() {
    let (base, state) = fake_server().await;
    state.lock().unwrap().accepted_token = "tok-2".into();
    let dir = tempfile::tempdir().unwrap();
    let store = McpTokenStore::in_config_dir(dir.path());
    let tokens = McpTokens {
        access_token: "old".into(),
        refresh_token: Some("ref-1".into()),
        expires_at_ms: Some(0),
        token_endpoint: format!("{base}/auth/token"),
        client_id: "c".into(),
        client_secret: Some("s".into()),
        resource: format!("{base}/mcp"),
    };
    store.save("srv", tokens).await.unwrap();
    transport(&base, &store).request("tools/list", None).await.unwrap();
    let seen = state.lock().unwrap().seen.clone();
    let mcp_calls: Vec<&Seen> = seen.iter().filter(|r| r.path == "/mcp").collect();
    assert_eq!(mcp_calls.len(), 1, "no wasted 401 round trip");
    let refresh = seen.iter().find(|r| r.path == "/auth/token").unwrap();
    assert_eq!(form_value(&refresh.body, "client_secret").as_deref(), Some("s"));
    assert_eq!(form_value(&refresh.body, "resource"), Some(format!("{base}/mcp")));
}

#[tokio::test]
async fn preregistered_clients_skip_dynamic_registration() {
    let (base, state) = fake_server().await;
    let dir = tempfile::tempdir().unwrap();
    let store = McpTokenStore::in_config_dir(dir.path());
    let browser = Browser::default();
    let config = McpOAuthConfig {
        client_id: Some("my-app".into()),
        scopes: vec!["repo".into()],
        ..Default::default()
    };
    let url = format!("{base}/mcp");
    let request = LoginRequest {
        server: "srv",
        url: &url,
        config: Some(&config),
    };
    login(request, &store, &browser).await.unwrap();
    let auth = query(&browser.auth_url.lock().unwrap().clone().unwrap());
    assert_eq!(auth["client_id"], "my-app");
    assert_eq!(auth["scope"], "repo");
    assert!(!state.lock().unwrap().seen.iter().any(|r| r.path == "/auth/register"));
    assert!(store.remove("srv").await.unwrap());
    assert!(!store.is_signed_in("srv"));
    assert!(!store.remove("srv").await.unwrap());
}
