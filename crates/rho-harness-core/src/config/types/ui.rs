use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct UiConfig {
    #[serde(default, alias = "block", alias = "style")]
    pub block_style: Option<String>,
    #[serde(default, alias = "user_border_color", alias = "user_color", alias = "user_block")]
    pub user_border: Option<String>,
    #[serde(default, alias = "agent_border_color", alias = "agent_color", alias = "agent_block")]
    pub agent_border: Option<String>,
    #[serde(
        default,
        alias = "tool_border_color",
        alias = "tool_color",
        alias = "command_border",
        alias = "command_border_color"
    )]
    pub tool_border: Option<String>,
    #[serde(
        default,
        alias = "bash_success_border_color",
        alias = "bash_success_color",
        alias = "bash_ok_border",
        alias = "bash_good_border"
    )]
    pub bash_success_border: Option<String>,
    #[serde(
        default,
        alias = "bash_error_border_color",
        alias = "bash_error_color",
        alias = "bash_fail_border"
    )]
    pub bash_error_border: Option<String>,
    #[serde(default, alias = "block_agent_output", alias = "agent_box")]
    pub agent_block_output: Option<bool>,
    #[serde(default, alias = "thinking_hidden", alias = "hide_thinking_output")]
    pub hide_thinking: Option<bool>,
    #[serde(default, alias = "expand_tools", alias = "tool_output_expanded")]
    pub tools_expanded: Option<bool>,
    #[serde(default, alias = "cursor_style", alias = "cursor_mode")]
    pub cursor: Option<String>,
    #[serde(default, alias = "highlight_tools")]
    pub syntax_highlight_tools: Option<bool>,
    #[serde(default, alias = "full_bash_commands")]
    pub full_bash_command: Option<bool>,
    #[serde(default)]
    pub auto_title: Option<bool>,
    #[serde(default)]
    pub terminal_title: Option<bool>,
    #[serde(default)]
    pub terminal_title_state: Option<bool>,
}

fn merge_opt<T: Clone>(dst: &mut Option<T>, src: &Option<T>) {
    if let Some(val) = src {
        *dst = Some(val.clone());
    }
}

impl UiConfig {
    pub fn auto_title_enabled(&self) -> bool {
        self.auto_title.unwrap_or(false)
    }

    pub fn terminal_title_enabled(&self) -> bool {
        self.terminal_title.unwrap_or(false)
    }

    pub fn terminal_title_state_enabled(&self) -> bool {
        self.terminal_title_state.unwrap_or(false)
    }

    pub fn merge(&mut self, other: &UiConfig) {
        merge_opt(&mut self.block_style, &other.block_style);
        merge_opt(&mut self.user_border, &other.user_border);
        merge_opt(&mut self.agent_border, &other.agent_border);
        merge_opt(&mut self.tool_border, &other.tool_border);
        merge_opt(&mut self.bash_success_border, &other.bash_success_border);
        merge_opt(&mut self.bash_error_border, &other.bash_error_border);
        merge_opt(&mut self.agent_block_output, &other.agent_block_output);
        merge_opt(&mut self.hide_thinking, &other.hide_thinking);
        merge_opt(&mut self.tools_expanded, &other.tools_expanded);
        merge_opt(&mut self.cursor, &other.cursor);
        merge_opt(&mut self.syntax_highlight_tools, &other.syntax_highlight_tools);
        merge_opt(&mut self.full_bash_command, &other.full_bash_command);
        merge_opt(&mut self.auto_title, &other.auto_title);
        merge_opt(&mut self.terminal_title, &other.terminal_title);
        merge_opt(&mut self.terminal_title_state, &other.terminal_title_state);
    }
}
