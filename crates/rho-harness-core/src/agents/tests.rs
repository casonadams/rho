use super::*;
use std::path::Path;
use test_case::test_case;

fn write_agent(dir: &Path, name: &str, content: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(format!("{name}.md")), content).unwrap();
}

#[test_case(
    "---\nname: a\ndescription: x---y\n---\nBody --- with dashes\n\n---\nmore",
    "a", "x---y", "Body --- with dashes\n\n---\nmore";
    "dashes inside values and body"
)]
#[test_case("---\nname: b\n---\n", "b", "Custom subagent", ""; "empty body")]
#[test_case(
    "no frontmatter --- here", "fallback", "Custom subagent", "no frontmatter --- here";
    "no frontmatter"
)]
#[test_case(
    "---\nname: c\nnever closed", "fallback", "Custom subagent", "---\nname: c\nnever closed";
    "unclosed frontmatter"
)]
fn frontmatter_ends_only_at_a_line_that_is_exactly_three_dashes(
    content: &str,
    name: &str,
    description: &str,
    body: &str,
) {
    let agent = parser::parse_agent_content(content, "fallback", "loc");
    assert_eq!(
        (
            agent.name.as_str(),
            agent.description.as_str(),
            agent.system_prompt.as_str()
        ),
        (name, description, body)
    );
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
    let tmp = crate::test_support::temp_dir();
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
    assert_eq!(
        meta_full.system_prompt, "Prompt body",
        "model and maxTurns are ignored, not leaked into the body"
    );

    let plain_path = tmp.join("plain.md");
    std::fs::write(&plain_path, "Just the instructions\n").unwrap();
    let meta_plain = parse_agent_file(&plain_path).unwrap();
    assert_eq!(meta_plain.name, "plain");
    assert_eq!(meta_plain.description, "Custom subagent");
    assert_eq!(meta_plain.system_prompt, "Just the instructions");
}

#[tokio::test]
async fn test_agent_resolution_precedence() {
    let p = crate::test_support::temp_dir();

    write_agent(&p.join("home/.claude/agents"), "ignored", "---\ndescription: x\n---\n");
    write_agent(&p.join("home/.agents/agents"), "u", "---\ndescription: u\n---\n");
    write_agent(&p.join("prj/.claude/agents"), "s", "---\ndescription: old\n---\n");
    write_agent(&p.join("prj/.agents/agents"), "s", "---\ndescription: new\n---\n");

    let res = resolved_agents_for_paths_async(Some(p.join("prj")), Some(p.join("home"))).await;

    let map: std::collections::BTreeMap<_, _> = res
        .into_iter()
        .map(|a| (a.metadata.name, a.metadata.description))
        .collect();
    assert_eq!(map.get("s").map(String::as_str), Some("new"));
    assert_eq!(map.get("u").map(String::as_str), Some("u"));
    assert!(!map.contains_key("ignored"));
}

#[test]
fn personas_resolve_from_user_and_project_with_project_precedence() {
    let p = crate::test_support::temp_dir();
    let (home, project) = (p.join("home"), p.join("prj"));
    write_agent(&home.join(".agents/personas"), "terse", "Answer in one line.\n");
    write_agent(
        &home.join(".agents/personas"),
        "skeptic",
        "---\nname: SKEPTIC\ndescription: old\n---\nDoubt.\n",
    );
    write_agent(
        &project.join(".agents/personas/review"),
        "skeptic",
        "---\nname: Skeptic\ndescription: Assume every claim is wrong\n---\nDemand evidence.\n",
    );
    write_agent(&project.join(".agents/agents"), "not-a-persona", "x\n");

    let personas = resolved_personas_for_paths(AgentResolutionPaths {
        project_dir: Some(&project),
        home_dir: Some(&home),
    });

    let summary: Vec<(&str, &str, &str)> = personas
        .iter()
        .map(|p| (p.name.as_str(), p.description.as_str(), p.instructions.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("Skeptic", "Assume every claim is wrong", "Demand evidence."),
            ("terse", "", "Answer in one line."),
        ]
    );
}
