use super::{CompletionSet, InteractiveHistory, ModelItem};

#[test]
fn completion_reports_replacement_spans_for_commands_and_arguments() {
    let sources = super::CompletionSources::new()
        .with_templates(vec!["deploy".to_string()])
        .with_models(vec![ModelItem {
            id: "gpt-4o".to_string(),
            provider: "openai".to_string(),
            description: "128k ctx".to_string(),
        }]);
    let completions = CompletionSet::from_sources(sources);

    let command = completions.complete("/dep", 4);
    assert_eq!(command[0].value, "/deploy");
    assert_eq!(command[0].replacement, 0..4);

    let model = completions.complete("/model gpt-4o suffix", 13);
    assert_eq!(model[0].value, "/model gpt-4o");
    assert_eq!(model[0].replacement, 0..13);
}

#[test]
fn completion_rejects_invalid_cursor_boundaries() {
    let completions = CompletionSet::from_sources(super::CompletionSources::default());
    assert!(completions.complete("/model 界", 8).is_empty());
    assert!(completions.complete("/model", 99).is_empty());
}

#[test]
fn mcp_completion_offers_subcommands_then_oauth_servers() {
    let config = rho_harness_core::config::mcp::parse_mcp_json_str(
        r#"{
            "linear": { "url": "https://mcp.linear.app/mcp" },
            "figma": { "url": "https://mcp.figma.com/mcp" },
            "github": { "url": "https://gh.example/mcp", "headers": { "Authorization": "Bearer x" } },
            "local": { "command": "echo" }
        }"#,
    )
    .unwrap();
    let servers = super::McpServerList::default();
    let completions = CompletionSet::from_sources(super::CompletionSources::new().with_mcp_servers(servers.clone()));
    servers.set_from(&config);
    let values = |line: &str| -> Vec<String> {
        completions
            .complete(line, line.len())
            .into_iter()
            .map(|c| c.value)
            .collect()
    };

    assert_eq!(
        values("/mcp "),
        ["/mcp login ", "/mcp logout ", "/mcp prompt ", "/mcp prompts "]
    );
    assert_eq!(values("/mcp logo"), ["/mcp logout "]);
    assert_eq!(
        values("/mcp prompts "),
        [
            "/mcp prompts figma",
            "/mcp prompts github",
            "/mcp prompts linear",
            "/mcp prompts local"
        ]
    );
    assert_eq!(values("/mcp prompt loc"), ["/mcp prompt local"]);
    assert_eq!(values("/mcp login "), ["/mcp login figma", "/mcp login linear"]);
    assert_eq!(values("/mcp logout lin"), ["/mcp logout linear"]);
    let figma = &completions.complete("/mcp login fig", 14)[0];
    assert_eq!(figma.description.as_deref(), Some("https://mcp.figma.com/mcp"));
    assert_eq!(figma.replacement, 0..14);
    assert!(values("/mcp test ").is_empty());
    assert!(values("/mcp login linear extra").is_empty());
}

#[test]
fn history_navigation_previous_and_next() {
    let dir = rho_test_support::TestDir::new();
    let path = dir.join("history.txt");
    let mut history = InteractiveHistory::with_file(3, path).unwrap();
    history.record("first").unwrap();
    history.record("second").unwrap();

    assert_eq!(
        (
            history.previous("draft").as_deref(),
            history.previous("ignored").as_deref()
        ),
        (Some("second"), Some("first"))
    );
    assert_eq!(
        (
            history.next_entry().as_deref(),
            history.next_entry().as_deref(),
            history.next_entry()
        ),
        (Some("second"), Some("draft"), None)
    );
}
