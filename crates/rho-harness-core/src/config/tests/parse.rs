use super::super::{Config, FileConfig, PermissionConfig};

#[test]
fn parses_cargo_style_plugins_and_mcp_config() {
    let toml_str = r#"
model = "gpt-4"

[plugins.local_tool]
path = "./tools/my_tool"
enabled = true

[plugins.git_tool]
git = "https://github.com/org/plugin"
branch = "main"

[plugins.crate_tool]
package = "rho-plugin-review"
version = "0.1.0"

[mcp]
enabled = true

[mcp.servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]
env = { DEBUG = "true" }
enabled = true

[mcp.servers.linear]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-linear"]
"#;
    let file: FileConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(file.plugins.len(), 3);
    assert_eq!(
        file.plugins["local_tool"].path,
        std::path::PathBuf::from("./tools/my_tool")
    );
    assert!(file.plugins["local_tool"].enabled);
    assert_eq!(
        file.plugins["git_tool"].git.as_deref(),
        Some("https://github.com/org/plugin")
    );
    assert_eq!(file.plugins["crate_tool"].package.as_deref(), Some("rho-plugin-review"));

    let mcp = file.mcp.unwrap();
    assert!(mcp.enabled);
    assert_eq!(mcp.servers.len(), 2);
    assert_eq!(mcp.servers["filesystem"].command, "npx");
    assert_eq!(
        mcp.servers["filesystem"].args,
        vec!["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]
    );
    assert_eq!(
        mcp.servers["filesystem"].env.get("DEBUG").map(|s| s.as_str()),
        Some("true")
    );
    assert_eq!(mcp.servers["linear"].command, "npx");
}

#[test]
fn parses_providers_config() {
    let toml_str = r#"
[providers.acme]
base_url = "https://api.acme.dev/v1"
key_env = "ACME_API_KEY"

[providers.local-llm]
base_url = "http://127.0.0.1:8080/v1"
"#;
    let file: FileConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(file.providers.len(), 2);
    assert_eq!(file.providers["acme"].base_url, "https://api.acme.dev/v1");
    assert_eq!(file.providers["acme"].key_env.as_deref(), Some("ACME_API_KEY"));
    assert_eq!(file.providers["local-llm"].base_url, "http://127.0.0.1:8080/v1");
    assert_eq!(file.providers["local-llm"].key_env, None);

    let config = Config {
        providers: file.providers,
        ..Default::default()
    };
    config.validate().unwrap();
}

#[test]
fn parses_permission_config() {
    let toml_disabled = r#"
[permission]
enabled = false
"#;
    let file: FileConfig = toml::from_str(toml_disabled).unwrap();
    assert_eq!(file.permission, Some(PermissionConfig { enabled: false }));
    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, file);
    assert!(!config.permission.enabled);

    let toml_empty = r#"
[permission]
"#;
    let file: FileConfig = toml::from_str(toml_empty).unwrap();
    assert_eq!(file.permission, Some(PermissionConfig { enabled: true }));
    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, file);
    assert!(config.permission.enabled);

    let toml_omitted = r#"
model = "claude"
"#;
    let file: FileConfig = toml::from_str(toml_omitted).unwrap();
    assert_eq!(file.permission, None);
    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, file);
    assert!(config.permission.enabled);
}
