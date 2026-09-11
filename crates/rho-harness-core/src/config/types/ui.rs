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
}

impl UiConfig {
    pub fn merge(&mut self, other: &UiConfig) {
        if let Some(ref val) = other.block_style {
            self.block_style = Some(val.clone());
        }
        if let Some(ref val) = other.user_border {
            self.user_border = Some(val.clone());
        }
        if let Some(ref val) = other.agent_border {
            self.agent_border = Some(val.clone());
        }
        if let Some(ref val) = other.tool_border {
            self.tool_border = Some(val.clone());
        }
        if let Some(ref val) = other.bash_success_border {
            self.bash_success_border = Some(val.clone());
        }
        if let Some(ref val) = other.bash_error_border {
            self.bash_error_border = Some(val.clone());
        }
        if let Some(val) = other.agent_block_output {
            self.agent_block_output = Some(val);
        }
    }
}
