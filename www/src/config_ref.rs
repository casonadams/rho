//! Every `config.toml` setting, for the searchable configuration page.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setting {
    pub key: &'static str,
    pub default: Option<&'static str>,
    /// Environment variables or flags that set it.
    pub env: Option<&'static str>,
    pub about: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct Group {
    pub title: &'static str,
    pub note: Option<&'static str>,
    pub settings: &'static [Setting],
}

const fn s(
    key: &'static str,
    default: Option<&'static str>,
    env: Option<&'static str>,
    about: &'static str,
) -> Setting {
    Setting {
        key,
        default,
        env,
        about,
    }
}

pub const GROUPS: &[Group] = &[
    Group {
        title: "Models & thinking",
        note: Some("Models are `<provider>/<model>` routes under the `[models]` table."),
        settings: &[
            s(
                "models.default",
                None,
                Some("RHO_MODEL, AI_MODEL, --model"),
                "Primary assistant model. Also set with /model.",
            ),
            s(
                "models.smol",
                None,
                Some("--smol"),
                "Default model for subagents and workflow agents, and for automatic session titles.",
            ),
            s("models.slow", None, Some("--slow"), "Deep-reasoning model."),
            s(
                "models.guard",
                None,
                None,
                "Local or decision model that classifies shell commands for the permission guard.",
            ),
            s(
                "models.judge",
                None,
                None,
                "Decision model role for fast classification and fallback for models.widget.",
            ),
            s(
                "models.widget",
                None,
                None,
                "Decision model role for A2UI widget and ask default-choice assistance (falls back to judge, then guard).",
            ),
            s(
                "models.<role>",
                None,
                None,
                "Any built-in (review, plan, task, vision) or custom lowercase role; reference with @<role>.",
            ),
            s(
                "provider",
                None,
                Some("RHO_PROVIDER, AI_PROVIDER, --provider"),
                "Provider for a model given without a <provider>/ prefix.",
            ),
            s(
                "thinking_level",
                Some("medium"),
                Some("AI_THINKING_LEVEL, --thinking"),
                "off, minimal, low, medium, high, xhigh, max, or auto (classifies per-turn effort via models.judge falling back to models.guard, defaulting to medium). Subagents use the same level (concrete medium when auto); changing it mid-session rewrites the prompt cache.",
            ),
            s(
                "cache_retention",
                Some("auto"),
                None,
                "Claude prompt-cache lifetime: auto (1 hour on claude logins, 5 minutes on anthropic API keys), short or long.",
            ),
        ],
    },
    Group {
        title: "Limits & compaction",
        note: Some("Numeric limits must be greater than zero."),
        settings: &[
            s(
                "max_output_tokens",
                Some("provider default"),
                Some("AI_MAX_OUTPUT_TOKENS"),
                "Cap on tokens per model reply.",
            ),
            s(
                "max_turns",
                Some("1000"),
                Some("AI_MAX_TURNS"),
                "Model turns allowed in one run.",
            ),
            s(
                "context_limit",
                Some("model window"),
                Some("AI_CONTEXT_LIMIT"),
                "Override the context window size in tokens.",
            ),
            s(
                "reserve_tokens",
                Some("16384"),
                Some("AI_RESERVE_TOKENS"),
                "Compaction runs when the history passes the context limit minus this many tokens.",
            ),
            s(
                "keep_recent_tokens",
                Some("20000"),
                Some("AI_KEEP_RECENT_TOKENS"),
                "Recent tokens kept verbatim by compaction.",
            ),
            s(
                "compaction_max_bytes",
                Some("8192"),
                Some("AI_COMPACTION_MAX_BYTES"),
                "Size cap for a compaction summary.",
            ),
            s(
                "output_max_bytes",
                Some("50000"),
                None,
                "Largest MCP tool result returned inline; the rest is saved under .rho/artifacts.",
            ),
            s(
                "context_window_messages",
                Some("24"),
                Some("AI_CONTEXT_WINDOW_MESSAGES"),
                "Accepted and validated, but the engine does not currently read it.",
            ),
            s(
                "context_injection_max_tokens",
                Some("4000"),
                Some("AI_CONTEXT_INJECTION_MAX_TOKENS"),
                "Accepted and validated, but the engine does not currently read it.",
            ),
        ],
    },
    Group {
        title: "Queues & sessions",
        note: None,
        settings: &[
            s(
                "steering_mode",
                Some("one-at-a-time"),
                Some("AI_STEERING_MODE"),
                "Queued steering messages: one-at-a-time or all.",
            ),
            s(
                "follow_up_mode",
                Some("one-at-a-time"),
                Some("AI_FOLLOW_UP_MODE"),
                "Queued follow-ups: one-at-a-time or all.",
            ),
            s(
                "session_retention_days",
                Some("5"),
                Some("AI_SESSION_RETENTION_DAYS, RHO_SESSION_RETENTION_DAYS"),
                "Days to keep unused sessions; off or 0 in the environment disables pruning.",
            ),
        ],
    },
    Group {
        title: "Web tools",
        note: None,
        settings: &[
            s(
                "tools.web.search.enabled",
                Some("true"),
                None,
                "Register the web_search tool.",
            ),
            s(
                "tools.web.search.default",
                Some("brave"),
                None,
                "Primary engine: brave, duckduckgo (ddg, ddg_lite), yahoo, or exa (EXA_API_KEY / /login exa-search, or keyless MCP).",
            ),
            s(
                "tools.web.search.fallback",
                None,
                None,
                "Engines tried in order when the one before errors or finds nothing; [] disables fallbacks.",
            ),
            s(
                "tools.web.fetch.enabled",
                Some("true"),
                None,
                "Register the web_fetch tool.",
            ),
            s("region", Some("wt-wt"), Some("WEB_REGION"), "Search region."),
            s(
                "search_min_interval_ms",
                Some("2000"),
                None,
                "Minimum gap between web searches; 0 means no throttling.",
            ),
            s("search_timeout_sec", Some("12"), None, "Web search timeout."),
            s("fetch_timeout_sec", Some("8"), None, "Web fetch timeout."),
            s(
                "fetch_limit",
                Some("200"),
                None,
                "Default number of lines web_fetch returns per page.",
            ),
            s(
                "fetch_max_bytes",
                Some("5000000"),
                None,
                "Largest page body downloaded.",
            ),
            s(
                "allow_private_network",
                Some("false"),
                Some("WEB_ALLOW_PRIVATE_NETWORK"),
                "Let web tools reach loopback and private addresses. Global file only.",
            ),
        ],
    },
    Group {
        title: "Bash, LSP & MCP",
        note: None,
        settings: &[
            s(
                "tools.bash.direnv",
                Some("false"),
                None,
                "Load an allowed .envrc through direnv export json before each bash command.",
            ),
            s(
                "tools.bash.direnv_timeout_ms",
                Some("30000"),
                None,
                "Longest wait for direnv export json.",
            ),
            s(
                "tools.lsp.enabled",
                Some("true"),
                None,
                "The lsp tool, and language-server diagnostics on edit and write.",
            ),
            s("mcp.enabled", Some("true"), None, "The MCP subsystem."),
            s(
                "mcp.startup_timeout_ms",
                Some("3000"),
                None,
                "Longest wait for uncached MCP servers at startup; 0 waits for all.",
            ),
            s(
                "mcp.defer_threshold",
                Some("4"),
                None,
                "Above this many MCP tools, they are found with tool_search instead of listed up front.",
            ),
        ],
    },
    Group {
        title: "Permissions & guard",
        note: Some("The [permission] table is read from the global file only."),
        settings: &[
            s(
                "permission.enabled",
                Some("true"),
                Some("--no-permission"),
                "Runtime permission guardrails.",
            ),
            s(
                "permission.guard_all_tools",
                Some("false"),
                None,
                "Route every non-baseline Ask call (MCP, writes outside the workspace) through the guard model.",
            ),
            s(
                "permission.guard_audit_log",
                Some("false"),
                None,
                "Append guard decisions as JSONL to ~/.config/rho/guard/decisions.jsonl.",
            ),
        ],
    },
    Group {
        title: "Interface",
        note: Some("Keys under [ui]. Border colors take ANSI names or #rrggbb."),
        settings: &[
            s(
                "ui.block_style",
                Some("border"),
                Some("RHO_BLOCK_STYLE, RHO_UI_BLOCK_STYLE"),
                "border (outline), solid (fill), or divider (horizontal rule).",
            ),
            s(
                "ui.cursor",
                Some("hardware"),
                Some("RHO_CURSOR, RHO_UI_CURSOR"),
                "hardware (the terminal's cursor) or software (a reverse-video block).",
            ),
            s(
                "ui.agent_block_output",
                Some("false"),
                None,
                "Frame assistant replies in bordered blocks.",
            ),
            s(
                "ui.hide_thinking",
                Some("false"),
                None,
                "Hide thinking blocks by default.",
            ),
            s(
                "ui.tools_expanded",
                Some("false"),
                None,
                "Expand tool output cards by default.",
            ),
            s(
                "ui.syntax_highlight_tools",
                Some("false"),
                None,
                "Syntax-highlight guard and permission modals, bash and JSON tool headers, and tool output.",
            ),
            s(
                "ui.full_bash_command",
                Some("false"),
                None,
                "Show whole bash commands in tool cards instead of cutting them at 60 characters.",
            ),
            s(
                "ui.structured_tool_cards",
                Some("false"),
                None,
                "Render structured, metadata-rich tool cards for running and completed tools.",
            ),
            s(
                "ui.delta_diffs",
                Some("false"),
                None,
                "Render syntax-highlighted delta-style diffs in edit and write tool cards.",
            ),
            s(
                "ui.a2ui_widgets",
                Some("false"),
                None,
                "Enable opt-in A2UI v1.0 persistent widget regions (rho:todo, rho:subagents) above and below the editor.",
            ),
            s(
                "ui.a2ui.max_above_lines",
                Some("8"),
                None,
                "Maximum terminal rows allocated to above-editor A2UI widgets.",
            ),
            s(
                "ui.a2ui.max_below_lines",
                Some("4"),
                None,
                "Maximum terminal rows allocated to below-editor A2UI widgets.",
            ),
            s(
                "ui.a2ui.todo_widget",
                Some("true"),
                None,
                "Show the live rho:todo A2UI surface when ui.a2ui_widgets is enabled.",
            ),
            s(
                "ui.a2ui.subagent_widget",
                Some("true"),
                None,
                "Show the live rho:subagents A2UI surface when ui.a2ui_widgets is enabled.",
            ),
            s(
                "ui.a2ui.decision_assist",
                Some("false"),
                None,
                "Use DecisionModel (models.widget -> judge -> guard) to suggest default choices in ask modals.",
            ),
            s(
                "ui.auto_title",
                Some("false"),
                Some("--no-title, RHO_NO_TITLE"),
                "Title the session in 3 to 6 words with models.smol after the first turn.",
            ),
            s(
                "ui.terminal_title",
                Some("false"),
                None,
                "Show the session title (or the directory) in the terminal or tmux title.",
            ),
            s(
                "ui.terminal_title_state",
                Some("false"),
                None,
                "Prefix the title with the run state: idle, working or attention.",
            ),
            s(
                "ui.prompt_prediction",
                Some("false"),
                None,
                "Predict the next likely user prompt as dim inline ghost text after each turn.",
            ),
            s(
                "show_label",
                Some("false"),
                None,
                "Show the agent's label in the divider.",
            ),
            s("ui.user_border", Some("blue"), None, "Border color of your prompts."),
            s(
                "ui.agent_border",
                Some("gray"),
                None,
                "Border color of agent and subagent blocks.",
            ),
            s("ui.tool_border", Some("gray"), None, "Border color of tool cards."),
            s(
                "ui.bash_success_border",
                Some("gray"),
                None,
                "Border color of bash commands that succeeded.",
            ),
            s(
                "ui.bash_error_border",
                Some("red"),
                None,
                "Border color of bash commands that failed.",
            ),
            s("ui.prompt.prefix", None, None, "Prompt glyph, for example \"❯ \"."),
            s(
                "ui.prompt.show_version",
                Some("true"),
                None,
                "Show the version in the top divider.",
            ),
            s(
                "ui.prompt.divider_label",
                None,
                None,
                "Optional custom label in the top divider (overrides show_version), for example \"ϱ\".",
            ),
            s(
                "ui.prompt.divider_style",
                Some("thinking"),
                None,
                "thinking, dim or none.",
            ),
        ],
    },
    Group {
        title: "Status bar & display",
        note: Some("The segmented status bar replaces the two-line footer once the preset is not default."),
        settings: &[
            s(
                "ui.status_bar.preset",
                Some("default"),
                None,
                "default, minimal, compact, full, nerd, ascii or custom.",
            ),
            s(
                "ui.status_bar.separator",
                None,
                None,
                "powerline, powerline-thin, dot, slash, pipe, block, none or ascii.",
            ),
            s("ui.status_bar.icons", None, None, "nerd, unicode or ascii."),
            s(
                "ui.status_bar.transparent",
                Some("false"),
                None,
                "Drop the background fill and the powerline end caps.",
            ),
            s(
                "ui.status_bar.compact_thinking_level",
                None,
                None,
                "Show the thinking level as one glyph beside the model.",
            ),
            s(
                "ui.status_bar.context_line",
                Some("off"),
                None,
                "Context gauge in the bottom divider: off, percentage, annotated or embedded.",
            ),
            s(
                "ui.status_bar.left / right",
                None,
                None,
                "Segments on each side, such as [\"model\", \"path\", \"git\"] and [\"token_total\", \"cost\", \"context_pct\", \"quota\"].",
            ),
            s(
                "segment_options.path",
                None,
                None,
                "abbreviate shortens parent directories; max_length caps the width.",
            ),
            s(
                "segment_options.git",
                None,
                None,
                "show_branch, show_staged, show_unstaged and show_untracked.",
            ),
            s(
                "segment_options.model",
                None,
                None,
                "show_thinking_level, short_name, and a names table of display aliases.",
            ),
            s(
                "segment_options.time",
                None,
                None,
                "format (12h or 24h) and show_seconds.",
            ),
            s(
                "ui.display.show_token_usage",
                Some("false"),
                None,
                "A row per turn with the time, tokens, time to first token and tokens per second.",
            ),
            s(
                "ui.display.show_turn_time",
                Some("false"),
                None,
                "Add the prompt-to-yield time to that row.",
            ),
            s(
                "ui.display.cache_miss_marker",
                Some("false"),
                None,
                "Mark where a warm prompt cache was invalidated.",
            ),
        ],
    },
    Group {
        title: "Worktrees",
        note: Some("The [worktree] table is read from the global file only."),
        settings: &[
            s(
                "worktree.base",
                Some("~/.config/rho/wt"),
                None,
                "Directory under which /wt and the worktree tool create session worktrees.",
            ),
            s(
                "worktree.clean_source",
                Some("false"),
                None,
                "After /wt carries your changes into the new worktree, reset and clean the source checkout. This discards its uncommitted changes.",
            ),
        ],
    },
    Group {
        title: "Session recap",
        note: Some("Opt-in away-from-keyboard recap under the [recap] table."),
        settings: &[
            s(
                "recap.enabled",
                Some("false"),
                None,
                "Generate a brief recap when you return to an idle session.",
            ),
            s(
                "recap.idle_seconds",
                Some("240"),
                None,
                "Seconds of inactivity before an idle recap is triggered (clamped to 1..=3600).",
            ),
            s(
                "recap.on_resume",
                Some("false"),
                None,
                "Generate a recap when resuming a previous session.",
            ),
        ],
    },
];

impl Setting {
    /// Whether every word of `query` appears in the setting, ignoring case.
    pub fn matches(&self, query: &str) -> bool {
        let haystack = format!(
            "{} {} {} {}",
            self.key,
            self.default.unwrap_or_default(),
            self.env.unwrap_or_default(),
            self.about
        )
        .to_lowercase();
        query
            .to_lowercase()
            .split_whitespace()
            .all(|word| haystack.contains(word))
    }
}

pub fn count() -> usize {
    GROUPS.iter().map(|g| g.settings.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(key: &str) -> Setting {
        GROUPS
            .iter()
            .flat_map(|g| g.settings)
            .find(|s| s.key == key)
            .copied()
            .unwrap()
    }

    #[test]
    fn search_needs_every_word_in_any_field() {
        let guard_log = find("permission.guard_audit_log");
        assert!(guard_log.matches(""));
        assert!(guard_log.matches("GUARD jsonl"));
        assert!(!guard_log.matches("guard timeout"));
        assert!(find("max_turns").matches("ai_max_turns"));
        assert!(find("ui.cursor").matches("hardware"));
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<&str> = GROUPS.iter().flat_map(|g| g.settings).map(|s| s.key).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total);
    }
}
