use super::super::{Config, FileConfig, McpConfig, PermissionConfig};

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
fn parses_subagent_settings_and_project_overrides() {
    use crate::workflow::ModelRole;
    let user: FileConfig = toml::from_str("[subagents.models]\nscout = \"smol\"\nReviewer = \"slow\"\n").unwrap();
    let project: FileConfig = toml::from_str("[subagents.models]\nscout = \"default\"\n").unwrap();
    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, user);
    super::super::merge::merge_project_file(&mut config, &project);
    let expected = [
        ("reviewer".to_string(), ModelRole::Slow),
        ("scout".to_string(), ModelRole::Default),
    ]
    .into();
    assert_eq!(config.subagents.models, expected);

    let invalid = toml::from_str::<FileConfig>("[subagents.models]\nscout = \"gpt-5\"\n").unwrap_err();
    assert!(invalid.to_string().contains("unknown variant `gpt-5`"), "{invalid}");

    let budget: FileConfig = toml::from_str("[subagents]\nforeground_seconds = 30\n").unwrap();
    super::super::merge::merge_project_file(&mut config, &budget);
    assert_eq!(config.subagents.foreground_seconds, Some(30));
    assert_eq!(config.subagents.models.len(), 2, "other subagent settings are kept");

    assert_eq!(config.subagents.approval_focus, None, "switch unless configured");
    let indicate: FileConfig = toml::from_str("[subagents]\napproval_focus = \"indicate\"\n").unwrap();
    super::super::merge::merge_project_file(&mut config, &indicate);
    assert_eq!(
        config.subagents.approval_focus,
        Some(crate::config::ApprovalFocus::Indicate)
    );
    assert_eq!(config.subagents.foreground_seconds, Some(30));
    assert!(toml::from_str::<FileConfig>("[subagents]\napproval_focus = \"popup\"\n").is_err());
}

#[test]
fn parses_permission_config_disabled() {
    let toml_disabled = "[permission]\nenabled = false\n";
    let file: FileConfig = toml::from_str(toml_disabled).unwrap();
    assert_eq!(
        file.permission,
        Some(PermissionConfig {
            enabled: false,
            ..PermissionConfig::default()
        })
    );
    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, file);
    assert!(!config.permission.enabled);
}

#[test]
fn parses_permission_config_empty_or_omitted() {
    let file_empty: FileConfig = toml::from_str("[permission]\n").unwrap();
    assert_eq!(file_empty.permission, Some(PermissionConfig::default()));
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
hide_thinking = true
tools_expanded = false
cursor = "hardware"
"#;
    let file: FileConfig = toml::from_str(toml_str).unwrap();
    let ui = file.ui.clone().expect("ui config present");
    assert_eq!(
        (
            ui.block_style.as_deref(),
            ui.user_border.as_deref(),
            ui.agent_border.as_deref(),
            ui.tool_border.as_deref(),
            ui.bash_success_border.as_deref(),
            ui.bash_error_border.as_deref(),
            ui.agent_block_output,
            ui.hide_thinking,
            ui.tools_expanded,
            ui.cursor.as_deref(),
        ),
        (
            Some("border"),
            Some("gray"),
            Some("blue"),
            Some("cyan"),
            Some("green"),
            Some("red"),
            Some(true),
            Some(true),
            Some(false),
            Some("hardware"),
        )
    );

    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, file);
    assert_eq!(
        (
            config.ui.block_style.as_deref(),
            config.ui.user_border.as_deref(),
            config.ui.agent_border.as_deref(),
            config.ui.tool_border.as_deref(),
            config.ui.bash_success_border.as_deref(),
            config.ui.bash_error_border.as_deref(),
            config.ui.agent_block_output,
            config.ui.hide_thinking,
            config.ui.tools_expanded,
            config.ui.cursor.as_deref(),
        ),
        (
            Some("border"),
            Some("gray"),
            Some("blue"),
            Some("cyan"),
            Some("green"),
            Some("red"),
            Some(true),
            Some(true),
            Some(false),
            Some("hardware"),
        )
    );
}

#[test]
fn test_ui_config_aliases_parse_correctly() {
    let toml_str = r#"
[ui]
thinking_hidden = true
expand_tools = true
agent_box = true
style = "solid"
"#;
    let file: FileConfig = toml::from_str(toml_str).unwrap();
    let ui = file.ui.expect("ui config present");
    assert_eq!(ui.hide_thinking, Some(true));
    assert_eq!(ui.tools_expanded, Some(true));
    assert_eq!(ui.agent_block_output, Some(true));
    assert_eq!(ui.block_style.as_deref(), Some("solid"));
}

#[test]
fn parses_models_table() {
    let toml_str = r#"
[models]
default = "anthropic/claude-3-7-sonnet"
guard = "local/qwen2.5-coder:7b"
"#;
    let file: FileConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(file.models.default.as_deref(), Some("anthropic/claude-3-7-sonnet"));
    assert_eq!(file.models.role("guard"), Some("local/qwen2.5-coder:7b"));

    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, file);
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-3-7-sonnet");
    assert_eq!(config.guard_model(), Some("local/qwen2.5-coder:7b"));
}

#[test]
fn parses_tools_web_config() {
    let cases = [
        ("model = \"claude\"\n", (true, "brave", true, None)),
        (
            r#"
[tools.web.search]
enabled = false
default = "duckduckgo"
fallback = ["yahoo"]

[tools.web.fetch]
enabled = false
"#,
            (false, "duckduckgo", false, Some(vec!["yahoo"])),
        ),
    ];
    for (toml_str, (search_enabled, search_default, fetch_enabled, fallback)) in cases {
        let file: FileConfig = toml::from_str(toml_str).unwrap();
        let mut config = Config::default();
        super::super::merge::merge_file(&mut config, file);
        assert_eq!(config.tools.web.search.enabled, search_enabled);
        assert_eq!(config.tools.web.search.default, search_default);
        assert_eq!(config.tools.web.fetch.enabled, fetch_enabled);
        if let Some(fallback) = fallback {
            assert_eq!(config.tools.web.search.fallback, fallback);
        }
    }
}

#[test]
fn lsp_tool_is_enabled_unless_the_file_turns_it_off() {
    for (toml_str, expected) in [
        ("model = \"claude\"\n", true),
        ("[tools.lsp]\nenabled = false\n", false),
        ("[tools.lsp]\nenabled = true\n", true),
    ] {
        let file: FileConfig = toml::from_str(toml_str).unwrap();
        let mut config = Config::default();
        super::super::merge::merge_file(&mut config, file);
        assert_eq!(config.tools.lsp.enabled, expected, "{toml_str}");
    }
}

#[test]
fn parses_mcp_config_enabled_in_file() {
    let toml_str = r#"
[mcp]
enabled = false
deferThreshold = 25
"#;
    let file: FileConfig = toml::from_str(toml_str).unwrap();
    let mut config = Config::default();
    super::super::merge::merge_file(&mut config, file);
    assert!(!config.mcp.enabled);
    assert_eq!(config.mcp.defer_threshold, 25);
}

#[test]
fn test_mcp_config_defaults() {
    let config = McpConfig::default();
    assert!(config.enabled);
    assert_eq!(config.defer_threshold, 4);
    assert!(config.servers.is_empty());
}

#[test]
fn model_role_validation_and_resolution() {
    use crate::config::ModelRole;

    assert_eq!(ModelRole::BUILTINS.len(), 9);
    assert_eq!(ModelRole::parse(" Widget ").unwrap(), ModelRole::WIDGET);
    assert_eq!(ModelRole::parse("my-custom_1").unwrap().as_str(), "my-custom_1");
    assert_eq!(ModelRole::WIDGET.title(), "Widget");

    for invalid in ["", "   ", "default", "DEFAULT", "bad.role", "bad role", "a/b"] {
        assert!(ModelRole::parse(invalid).is_err(), "{invalid:?}");
    }

    let mut config = Config {
        default_model: Some("anthropic/claude-sonnet-4-5".to_string()),
        ..Default::default()
    };
    config
        .model_roles
        .insert("smol".to_string(), "google/gemini-2.5-flash".to_string());
    config.model_roles.insert("widget".to_string(), "none".to_string());
    config.model_roles.insert("judge".to_string(), "  ".to_string());
    config
        .model_roles
        .insert("guard".to_string(), "local/qwen2.5-coder:7b".to_string());

    assert_eq!(config.role_model("SMOL"), Some("google/gemini-2.5-flash"));
    assert_eq!(config.role_model("widget"), None);
    assert_eq!(config.role_model("judge"), None);
    assert_eq!(
        config.resolve_role_chain(&["widget", "judge", "guard"]),
        Some(("guard", "local/qwen2.5-coder:7b"))
    );
    assert_eq!(config.resolve_role_chain(&["widget", "judge"]), None);
    assert_eq!(config.resolve_model_selector("@smol"), Some("google/gemini-2.5-flash"));
    assert_eq!(
        config.resolve_model_selector("@default"),
        Some("anthropic/claude-sonnet-4-5")
    );
    assert_eq!(config.resolve_model_selector("openai/gpt-5"), Some("openai/gpt-5"));
    assert_eq!(config.resolve_model_selector("@widget"), None);
    assert_eq!(config.resolve_model_selector("none"), None);
}
