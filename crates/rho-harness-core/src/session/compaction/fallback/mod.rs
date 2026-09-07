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

fn apply_custom_instructions(instructions: Option<&str>, state: &mut SummaryState) {
    let Some(instructions) = instructions else {
        return;
    };
    let trimmed = instructions.trim();
    if !trimmed.is_empty() {
        let item = format!("Additional focus: {trimmed}");
        if !state.constraints.contains(&item) {
            state.constraints.push(item);
        }
    }
}

fn trim_excess_done(done: &mut Vec<String>) {
    if done.len() > 15 {
        *done = done.split_off(done.len() - 15);
    }
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
    apply_custom_instructions(custom_instructions, &mut state);
    extract_message_facts(messages, &mut state);
    trim_excess_done(&mut state.done);
    render_structured_summary(&state)
}

fn map_section_header(trimmed: &str) -> Option<&'static str> {
    match trimmed {
        "## Goal" => Some("goal"),
        "## Constraints & Preferences" => Some("constraints"),
        "### Done" => Some("done"),
        "### In Progress" => Some("in_progress"),
        "### Blocked" => Some("blocked"),
        "## Key Decisions" => Some("decisions"),
        "## Next Steps" => Some("next_steps"),
        "## Critical Context" => Some("critical_context"),
        _ if trimmed.starts_with("## ") => Some(""),
        _ => None,
    }
}

fn is_ignorable_summary_line(trimmed: &str) -> bool {
    trimmed.is_empty()
        || matches!(trimmed, "(none)" | "- (none)" | "- [ ] (none)" | "- [x] (none)")
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}

fn parse_prior_line(line: &str, current_section: &mut &'static str, state: &mut SummaryState) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with("<read-files>") || trimmed.starts_with("<modified-files>") {
        return false;
    }
    if let Some(sec) = map_section_header(trimmed) {
        *current_section = sec;
    } else if !is_ignorable_summary_line(trimmed) {
        let clean = clean_item(trimmed);
        if !clean.is_empty() {
            append_to_section(state, current_section, clean);
        }
    }
    true
}

fn parse_prior_summary(prior: &str, state: &mut SummaryState) {
    let mut current_section = "";
    for line in prior.lines() {
        if !parse_prior_line(line, &mut current_section, state) {
            break;
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

fn render_goal_section(out: &mut String, goal: &[String]) {
    out.push_str("## Goal\n");
    if goal.is_empty() {
        out.push_str("(none)\n\n");
    } else {
        out.push_str(&goal.join("\n"));
        out.push_str("\n\n");
    }
}

fn render_progress_section(out: &mut String, done: &[String], in_progress: &[String], blocked: &[String]) {
    out.push_str("## Progress\n### Done\n");
    if done.is_empty() {
        out.push_str("- [x] (none)\n\n");
    } else {
        for d in done {
            out.push_str(&format!("- [x] {d}\n"));
        }
        out.push('\n');
    }
    out.push_str("### In Progress\n");
    if in_progress.is_empty() {
        out.push_str("- (none)\n\n");
    } else {
        for p in in_progress {
            out.push_str(&format!("- [ ] {p}\n"));
        }
        out.push('\n');
    }
    render_bullets(out, "### Blocked", blocked);
}

fn render_next_steps(out: &mut String, next_steps: &[String]) {
    out.push_str("## Next Steps\n");
    if next_steps.is_empty() {
        out.push_str("1. Continue session work\n\n");
    } else {
        for (i, step) in next_steps.iter().enumerate() {
            out.push_str(&format!("{}. {step}\n", i + 1));
        }
        out.push('\n');
    }
}

fn render_critical_context(out: &mut String, critical_context: &[String]) {
    out.push_str("## Critical Context\n");
    if critical_context.is_empty() {
        out.push_str("- (none)");
    } else {
        for ctx in critical_context {
            out.push_str(&format!("- {ctx}\n"));
        }
        if out.ends_with('\n') {
            out.pop();
        }
    }
}

fn render_structured_summary(state: &SummaryState) -> String {
    let mut out = String::new();
    render_goal_section(&mut out, &state.goal);
    render_bullets(&mut out, "## Constraints & Preferences", &state.constraints);
    render_progress_section(&mut out, &state.done, &state.in_progress, &state.blocked);
    render_bullets(&mut out, "## Key Decisions", &state.decisions);
    render_next_steps(&mut out, &state.next_steps);
    render_critical_context(&mut out, &state.critical_context);
    out
}
