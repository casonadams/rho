use super::*;
use std::path::Path;

fn write_agent(dir: &Path, name: &str, content: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(format!("{name}.md")), content).unwrap();
}

#[test]
fn test_normalize_tool_name() {
    assert!(
        normalize_tool_name("Glob") == "fd"
            && normalize_tool_name("Grep") == "rg"
            && normalize_tool_name("Custom") == "custom"
    );
}

#[test]
fn test_parse_tool_list() {
    let list = parse_tool_list("Read, Grep, Glob, Bash");
    assert_eq!(list.len(), 4);
    assert_eq!(list.first().map(String::as_str), Some("read"));
}

#[test]
fn test_parse_agent_files_variants() {
    let tmp = std::env::temp_dir().join(format!("agent_var_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();

    let full_path = tmp.join("full.md");
    std::fs::write(
        &full_path,
        "---\nname: my-agent\ndescription: Test agent\ntools: [Read, Grep]\ndisallowedTools: Bash\nmodel: haiku\nmaxTurns: 7\n---\nPrompt body\n",
    )
    .unwrap();
    let meta_full = parse_agent_file(&full_path).unwrap();
    assert_eq!(meta_full.name, "my-agent");
    assert_eq!(meta_full.description, "Test agent");
    assert_eq!(
        meta_full.tools.as_deref(),
        Some(&["read".to_string(), "rg".to_string()][..])
    );
    assert_eq!(meta_full.disallowed_tools.as_deref(), Some(&["bash".to_string()][..]));
    assert_eq!(meta_full.model.as_deref(), Some("haiku"));
    assert_eq!(meta_full.max_turns, Some(7));
    assert_eq!(meta_full.system_prompt, "Prompt body");

    let plain_path = tmp.join("plain.md");
    std::fs::write(&plain_path, "Just the instructions\n").unwrap();
    let meta_plain = parse_agent_file(&plain_path).unwrap();
    assert_eq!(meta_plain.name, "plain");
    assert_eq!(meta_plain.description, "Custom subagent");
    assert_eq!(meta_plain.system_prompt, "Just the instructions");

    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn test_agent_resolution_precedence() {
    let p = std::env::temp_dir().join(format!("agent_res_{}", uuid::Uuid::new_v4()));

    write_agent(&p.join("home/.claude/agents"), "u", "---\ndescription: u\n---\n");
    write_agent(&p.join("prj/.claude/agents"), "s", "---\ndescription: old\n---\n");
    write_agent(&p.join("prj/.agents/agents"), "s", "---\ndescription: new\n---\n");

    let res = resolved_agents_for_paths(AgentResolutionPaths {
        project_dir: Some(&p.join("prj")),
        home_dir: Some(&p.join("home")),
    });

    let map: std::collections::BTreeMap<_, _> = res
        .into_iter()
        .map(|a| (a.metadata.name, a.metadata.description))
        .collect();
    assert_eq!(map.get("s").map(String::as_str), Some("new"));
    assert_eq!(map.get("u").map(String::as_str), Some("u"));

    let _ = std::fs::remove_dir_all(p);
}
