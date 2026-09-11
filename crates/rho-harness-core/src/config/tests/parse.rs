use super::super::{Config, FileConfig, PermissionConfig};

fn sample_plugins_toml() -> &'static str {
    r#"
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
"#
}

fn sample_mcp_toml() -> &'static str {
    r#"
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
"#
}

#[test]
fn parses_cargo_style_plugins_config() {
    let file: FileConfig = toml::from_str(sample_plugins_toml()).unwrap();
    let actual = (
        file.plugins.len(),
        file.plugins["local_tool"].path.as_path(),
        file.plugins["local_tool"].enabled,
        file.plugins["git_tool"].git.as_deref(),
        file.plugins["crate_tool"].package.as_deref(),
    );
    let expected = (
        3,
        std::path::Path::new("./tools/my_tool"),
        true,
        Some("https://github.com/org/plugin"),
        Some("rho-plugin-review"),
    );
    assert_eq!(actual, expected);
}

#[test]
fn parses_mcp_config() {
    let file: FileConfig = toml::from_str(sample_mcp_toml()).unwrap();
    let mcp = file.mcp.unwrap();
    let filesystem = &mcp.servers["filesystem"];
    assert_eq!((mcp.enabled, mcp.servers.len()), (true, 2));
    assert_eq!(
        (filesystem.command.as_deref(), mcp.servers["linear"].command.as_deref()),
        (Some("npx"), Some("npx")),
    );
    assert_eq!(
        filesystem.args,
        vec!["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]
    );
    assert_eq!(filesystem.env.get("DEBUG").map(|s| s.as_str()), Some("true"));
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
    let acme = (
        &file.providers["acme"].base_url,
        file.providers["acme"].key_env.as_deref(),
    );
    assert_eq!(acme, (&"https://api.acme.dev/v1".to_string(), Some("ACME_API_KEY")));
    let local = (
        &file.providers["local-llm"].base_url,
        file.providers["local-llm"].key_env.as_deref(),
    );
    assert_eq!(local, (&"http://127.0.0.1:8080/v1".to_string(), None));

    let config = Config {
        providers: file.providers,
        ..Default::default()
    };
    config.validate().unwrap();
}

#[test]
fn parses_permission_config_disabled() {
    let toml_disabled = "[permission]\nenabled = false\n";
    let file: FileConfig = toml::from_str(toml_disabled).unwrap();
    assert_eq!(file.permission, Some(PermissionConfig { enabled: false }));
    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, file);
    assert!(!config.permission.enabled);
}

#[test]
fn parses_permission_config_empty_or_omitted() {
    let file_empty: FileConfig = toml::from_str("[permission]\n").unwrap();
    assert_eq!(file_empty.permission, Some(PermissionConfig { enabled: true }));
    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, file_empty);
    assert!(config.permission.enabled);

    let file_omitted: FileConfig = toml::from_str("model = \"claude\"\n").unwrap();
    assert_eq!(file_omitted.permission, None);
}
