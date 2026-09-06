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
