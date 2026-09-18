use super::super::prompts::{
    SUMMARIZATION_PROMPT, SUMMARIZATION_SYSTEM_PROMPT, TURN_PREFIX_SUMMARIZATION_PROMPT, UPDATE_SUMMARIZATION_PROMPT,
    build_summarization_prompt, build_turn_prefix_prompt, build_update_summarization_prompt,
    compose_compaction_summary, merge_split_turn_summary,
};

#[test]
fn test_system_prompt_spec() {
    assert!(SUMMARIZATION_SYSTEM_PROMPT.contains("You are a context summarization assistant."));
    assert!(SUMMARIZATION_SYSTEM_PROMPT.contains("Do NOT continue the conversation."));
    assert!(SUMMARIZATION_SYSTEM_PROMPT.contains("ONLY output the structured summary."));
}

#[test]
fn test_build_summarization_prompt_without_instructions() {
    let transcript = "[User]: hello\n[Assistant]: hi";
    let prompt = build_summarization_prompt(transcript, None);
    let expected = [
        "<conversation>\n",
        transcript,
        "</conversation>\n\n",
        SUMMARIZATION_PROMPT,
        "## Goal",
        "## Constraints & Preferences",
        "## Progress",
        "## Key Decisions",
        "## Next Steps",
        "## Critical Context",
    ];
    for section in expected {
        assert!(prompt.contains(section));
    }
    assert!(!prompt.contains("Additional focus:"));
}

#[test]
fn test_build_summarization_prompt_with_instructions() {
    let transcript = "[User]: do something";
    let prompt = build_summarization_prompt(transcript, Some("Focus on API backwards compatibility"));

    assert!(prompt.contains("<conversation>\n[User]: do something\n</conversation>"));
    assert!(prompt.contains("Additional focus: Focus on API backwards compatibility"));
}

#[test]
fn test_build_update_summarization_prompt() {
    let transcript = "[User]: new step\n[Assistant]: completed";
    let prev_summary = "## Goal\nPrior goal\n\n## Progress\n### Done\n- [x] Step 1";

    let prompt = build_update_summarization_prompt(transcript, prev_summary, None);
    let expected = [
        "<conversation>\n[User]: new step\n[Assistant]: completed\n</conversation>",
        "<previous-summary>\n## Goal\nPrior goal\n\n## Progress\n### Done\n- [x] Step 1\n</previous-summary>",
        UPDATE_SUMMARIZATION_PROMPT,
        "RULES:",
        "PRESERVE all existing information",
    ];
    for section in expected {
        assert!(prompt.contains(section));
    }
    assert!(!prompt.contains("Additional focus:"));
}

#[test]
fn test_build_update_summarization_prompt_with_instructions() {
    let prompt = build_update_summarization_prompt("msg", "summary", Some("Track security issues"));
    assert!(prompt.contains("Additional focus: Track security issues"));
}

#[test]
fn test_build_turn_prefix_prompt() {
    let prefix = "[User]: massive task\n[Assistant]: part 1 of 100";
    let prompt = build_turn_prefix_prompt(prefix, None);
    let expected = [
        "<conversation>\n",
        prefix,
        "</conversation>\n\n",
        TURN_PREFIX_SUMMARIZATION_PROMPT,
        "## Original Request",
        "## Early Progress",
        "## Context for Suffix",
    ];
    for section in expected {
        assert!(prompt.contains(section));
    }
}

#[test]
fn test_build_turn_prefix_prompt_with_instructions() {
    let prompt = build_turn_prefix_prompt("prefix", Some("Note memory limits"));
    assert!(prompt.contains("Additional focus: Note memory limits"));
}

#[test]
fn test_merge_split_turn_summary() {
    let main_summary = "## Goal\nOverall task";
    let prefix_summary = "## Original Request\nLarge request";

    let merged = merge_split_turn_summary(main_summary, prefix_summary);
    assert_eq!(
        merged,
        "## Goal\nOverall task\n\n---\n\n**Turn Context (split turn):**\n\n## Original Request\nLarge request"
    );
}

#[test]
fn test_compose_compaction_summary() {
    let summary = "## Goal\nFix bug";
    let xml = "<read-files>\nsrc/main.rs\n</read-files>";

    let composed = compose_compaction_summary(summary, xml);
    assert_eq!(composed, "## Goal\nFix bug\n\n<read-files>\nsrc/main.rs\n</read-files>");

    let no_xml = compose_compaction_summary(summary, "   ");
    assert_eq!(no_xml, "## Goal\nFix bug");
}

#[test]
fn test_compaction_summary_payload_serde_and_schema() {
    use super::super::types::CompactionSummaryPayload;

    let payload = CompactionSummaryPayload {
        goals: vec!["Implement structured compaction".to_string()],
        decisions: vec!["Use ExtractorBuilder with schemars".to_string()],
        completed: vec!["Added schema and types".to_string()],
        active_files: vec!["crates/rho-harness-core/src/session/compaction/types.rs".to_string()],
        open_questions: vec!["Check fallback paths".to_string()],
    };

    let serialized = serde_json::to_string(&payload).expect("serialization succeeds");
    let deserialized: CompactionSummaryPayload = serde_json::from_str(&serialized).expect("deserialization succeeds");
    assert_eq!(payload, deserialized);

    let empty_json = "{}";
    let default_payload: CompactionSummaryPayload =
        serde_json::from_str(empty_json).expect("deserialization of empty json succeeds");
    assert_eq!(default_payload, CompactionSummaryPayload::default());

    let schema = schemars::schema_for!(CompactionSummaryPayload);
    let schema_json = serde_json::to_value(&schema).expect("schema serialization succeeds");
    assert!(schema_json["properties"]["goals"].is_object());
    assert!(schema_json["properties"]["decisions"].is_object());
    assert!(schema_json["properties"]["completed"].is_object());
    assert!(schema_json["properties"]["active_files"].is_object());
    assert!(schema_json["properties"]["open_questions"].is_object());
}

#[test]
fn test_render_compaction_payload() {
    use super::super::prompts::render_compaction_payload;
    use super::super::types::CompactionSummaryPayload;

    let payload = CompactionSummaryPayload {
        goals: vec!["Refactor compactor".to_string()],
        decisions: vec!["Use Rig ExtractorBuilder".to_string()],
        completed: vec!["Defined schema".to_string()],
        active_files: vec!["src/lib.rs".to_string()],
        open_questions: vec!["Verify fallback".to_string()],
    };

    let rendered = render_compaction_payload(&payload);
    assert!(rendered.contains("## Goals\n- Refactor compactor"));
    assert!(rendered.contains("## Key Decisions\n- Use Rig ExtractorBuilder"));
    assert!(rendered.contains("## Completed Work\n- Defined schema"));
    assert!(rendered.contains("## Active Files\n- src/lib.rs"));
    assert!(rendered.contains("## Open Questions\n- Verify fallback"));
    assert_eq!(rendered, payload.render_markdown());

    let empty_payload = CompactionSummaryPayload::default();
    let empty_rendered = render_compaction_payload(&empty_payload);
    assert!(empty_rendered.contains("## Goals\n- (none)"));
    assert!(empty_rendered.contains("## Key Decisions\n- (none)"));
    assert!(empty_rendered.contains("## Completed Work\n- (none)"));
    assert!(empty_rendered.contains("## Active Files\n- (none)"));
    assert!(empty_rendered.contains("## Open Questions\n- (none)"));
}
