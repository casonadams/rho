pub mod extract;

use rig::message::Message;

use extract::{clean_item, extract_message_facts};

#[derive(Default)]
pub struct SummaryState {
    pub goal: Vec<String>,
    pub constraints: Vec<String>,
    pub done: Vec<String>,
    pub in_progress: Vec<String>,
    pub blocked: Vec<String>,
    pub decisions: Vec<String>,
    pub next_steps: Vec<String>,
    pub critical_context: Vec<String>,
}

pub fn generate_fallback_summary(
    messages: &[Message],
    prior_summary: Option<&str>,
    custom_instructions: Option<&str>,
) -> String {
    let mut state = SummaryState::default();

    if let Some(prior) = prior_summary {
        parse_prior_summary(prior, &mut state);
    }

    if let Some(instructions) = custom_instructions {
        let trimmed = instructions.trim();
        if !trimmed.is_empty() {
            let item = format!("Additional focus: {trimmed}");
            if !state.constraints.contains(&item) {
                state.constraints.push(item);
            }
        }
    }

    extract_message_facts(messages, &mut state);

    if state.done.len() > 15 {
        state.done = state.done.split_off(state.done.len() - 15);
    }

    render_structured_summary(&state)
}

fn parse_prior_summary(prior: &str, state: &mut SummaryState) {
    let mut current_section = "";

    for line in prior.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<read-files>") || trimmed.starts_with("<modified-files>") {
            break;
        }

        match trimmed {
            "## Goal" => current_section = "goal",
            "## Constraints & Preferences" => current_section = "constraints",
            "### Done" => current_section = "done",
            "### In Progress" => current_section = "in_progress",
            "### Blocked" => current_section = "blocked",
            "## Key Decisions" => current_section = "decisions",
            "## Next Steps" => current_section = "next_steps",
            "## Critical Context" => current_section = "critical_context",
            _ if trimmed.starts_with("## ") => current_section = "",
            _ => {
                if trimmed.is_empty()
                    || trimmed == "(none)"
                    || trimmed == "- (none)"
                    || trimmed == "- [ ] (none)"
                    || trimmed == "- [x] (none)"
                    || (trimmed.starts_with('[') && trimmed.ends_with(']'))
                {
                    continue;
                }
                let clean = clean_item(trimmed);
                if !clean.is_empty() {
                    append_to_section(state, current_section, clean);
                }
            }
        }
    }
}

fn append_to_section(state: &mut SummaryState, section: &str, item: String) {
    let target = match section {
        "goal" => &mut state.goal,
        "constraints" => &mut state.constraints,
        "done" => &mut state.done,
        "in_progress" => &mut state.in_progress,
        "blocked" => &mut state.blocked,
        "decisions" => &mut state.decisions,
        "next_steps" => &mut state.next_steps,
        "critical_context" => &mut state.critical_context,
        _ => return,
    };
    if !target.contains(&item) {
        target.push(item);
    }
}

fn render_bullets(out: &mut String, heading: &str, items: &[String]) {
    out.push_str(heading);
    out.push('\n');
    if items.is_empty() {
        out.push_str("- (none)\n\n");
    } else {
        for item in items {
            out.push_str("- ");
            out.push_str(item);
            out.push('\n');
        }
        out.push('\n');
    }
}

fn render_structured_summary(state: &SummaryState) -> String {
    let mut out = String::new();

    out.push_str("## Goal\n");
    if state.goal.is_empty() {
        out.push_str("(none)\n\n");
    } else {
        out.push_str(&state.goal.join("\n"));
        out.push_str("\n\n");
    }

    render_bullets(&mut out, "## Constraints & Preferences", &state.constraints);

    out.push_str("## Progress\n### Done\n");
    if state.done.is_empty() {
        out.push_str("- [x] (none)\n\n");
    } else {
        for d in &state.done {
            out.push_str(&format!("- [x] {d}\n"));
        }
        out.push('\n');
    }

    out.push_str("### In Progress\n");
    if state.in_progress.is_empty() {
        out.push_str("- (none)\n\n");
    } else {
        for p in &state.in_progress {
            out.push_str(&format!("- [ ] {p}\n"));
        }
        out.push('\n');
    }

    render_bullets(&mut out, "### Blocked", &state.blocked);
    render_bullets(&mut out, "## Key Decisions", &state.decisions);

    out.push_str("## Next Steps\n");
    if state.next_steps.is_empty() {
        out.push_str("1. Continue session work\n\n");
    } else {
        for (i, step) in state.next_steps.iter().enumerate() {
            out.push_str(&format!("{}. {step}\n", i + 1));
        }
        out.push('\n');
    }

    out.push_str("## Critical Context\n");
    if state.critical_context.is_empty() {
        out.push_str("- (none)");
    } else {
        for ctx in &state.critical_context {
            out.push_str(&format!("- {ctx}\n"));
        }
        if out.ends_with('\n') {
            out.pop();
        }
    }

    out
}
