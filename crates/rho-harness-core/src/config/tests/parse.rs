use super::super::{Config, FileConfig, PermissionConfig};

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

#[test]
fn parses_ui_config_and_merges() {
    let toml_str = r#"
[ui]
block_style = "border"
user_border = "gray"
agent_border = "blue"
tool_border = "cyan"
bash_success_border = "green"
bash_error_border = "red"
agent_block_output = true
"#;
    let file: FileConfig = toml::from_str(toml_str).unwrap();
    let ui = file.ui.clone().expect("ui config present");
    assert_eq!(ui.block_style.as_deref(), Some("border"));
    assert_eq!(ui.user_border.as_deref(), Some("gray"));
    assert_eq!(ui.agent_border.as_deref(), Some("blue"));
    assert_eq!(ui.tool_border.as_deref(), Some("cyan"));
    assert_eq!(ui.bash_success_border.as_deref(), Some("green"));
    assert_eq!(ui.bash_error_border.as_deref(), Some("red"));
    assert_eq!(ui.agent_block_output, Some(true));

    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, file);
    assert_eq!(config.ui.block_style.as_deref(), Some("border"));
    assert_eq!(config.ui.user_border.as_deref(), Some("gray"));
    assert_eq!(config.ui.agent_border.as_deref(), Some("blue"));
    assert_eq!(config.ui.tool_border.as_deref(), Some("cyan"));
    assert_eq!(config.ui.bash_success_border.as_deref(), Some("green"));
    assert_eq!(config.ui.bash_error_border.as_deref(), Some("red"));
    assert_eq!(config.ui.agent_block_output, Some(true));
}
