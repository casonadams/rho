use std::collections::BTreeMap;

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
    #[serde(default, alias = "structured_tools")]
    pub structured_tool_cards: Option<bool>,
    #[serde(default, alias = "delta_diff", alias = "syntax_highlight_diffs")]
    pub delta_diffs: Option<bool>,
    #[serde(default)]
    pub auto_title: Option<bool>,
    #[serde(default)]
    pub terminal_title: Option<bool>,
    #[serde(default)]
    pub terminal_title_state: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptUiConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_bar: Option<StatusBarConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<DisplayUiConfig>,
    #[serde(default)]
    pub a2ui_widgets: bool,
    #[serde(default)]
    pub a2ui: A2uiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct A2uiConfig {
    pub max_above_lines: usize,
    pub max_below_lines: usize,
    pub todo_widget: bool,
    pub subagent_widget: bool,
    pub decision_assist: bool,
}

impl Default for A2uiConfig {
    fn default() -> Self {
        Self {
            max_above_lines: 8,
            max_below_lines: 4,
            todo_widget: true,
            subagent_widget: true,
            decision_assist: false,
        }
    }
}

impl A2uiConfig {
    pub fn merge(&mut self, other: &Self) {
        let defaults = Self::default();
        if other.max_above_lines != defaults.max_above_lines {
            self.max_above_lines = other.max_above_lines;
        }
        if other.max_below_lines != defaults.max_below_lines {
            self.max_below_lines = other.max_below_lines;
        }
        if other.todo_widget != defaults.todo_widget {
            self.todo_widget = other.todo_widget;
        }
        if other.subagent_widget != defaults.subagent_widget {
            self.subagent_widget = other.subagent_widget;
        }
        if other.decision_assist != defaults.decision_assist {
            self.decision_assist = other.decision_assist;
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DividerStyle {
    Thinking,
    Dim,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PromptUiConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_version: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub divider_style: Option<DividerStyle>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StatusBarPreset {
    Default,
    Minimal,
    Compact,
    Full,
    Nerd,
    Ascii,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StatusBarSeparator {
    Powerline,
    PowerlineThin,
    Dot,
    Slash,
    Pipe,
    Block,
    None,
    Ascii,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StatusBarIconMode {
    Nerd,
    Unicode,
    Ascii,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ContextLineMode {
    Off,
    Percentage,
    Annotated,
    Embedded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StatusBarSegmentId {
    Pr,
    Model,
    Path,
    Git,
    ContextPct,
    ContextTotal,
    TokenIn,
    TokenOut,
    TokenTotal,
    CacheRead,
    CacheWrite,
    CacheHit,
    TokenRate,
    Cost,
    TimeSpent,
    Time,
    Session,
    SessionName,
    Subagents,
    Quota,
    Hostname,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TimeFormat {
    #[serde(rename = "12h")]
    H12,
    #[serde(rename = "24h")]
    H24,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PathSegmentOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abbreviate: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitSegmentOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_branch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_staged: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_unstaged: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_untracked: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ModelSegmentOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_thinking_level: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_name: Option<bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TimeSegmentOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<TimeFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_seconds: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StatusBarSegmentOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathSegmentOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitSegmentOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelSegmentOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<TimeSegmentOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StatusBarConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<StatusBarPreset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub separator: Option<StatusBarSeparator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<StatusBarIconMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_thinking_level: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_line: Option<ContextLineMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<Vec<StatusBarSegmentId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<Vec<StatusBarSegmentId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_options: Option<StatusBarSegmentOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DisplayUiConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_token_usage: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_turn_time: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_miss_marker: Option<bool>,
}

fn merge_opt<T: Clone>(dst: &mut Option<T>, src: &Option<T>) {
    if let Some(val) = src {
        *dst = Some(val.clone());
    }
}

fn merge_nested<T: Clone>(dst: &mut Option<T>, src: &Option<T>, merge_fn: fn(&mut T, &T)) {
    match (dst, src) {
        (Some(d), Some(s)) => merge_fn(d, s),
        (dst @ None, Some(s)) => *dst = Some(s.clone()),
        (_, None) => {}
    }
}

impl PromptUiConfig {
    pub fn merge(&mut self, other: &Self) {
        merge_opt(&mut self.prefix, &other.prefix);
        merge_opt(&mut self.show_version, &other.show_version);
        merge_opt(&mut self.divider_style, &other.divider_style);
    }
}

impl PathSegmentOptions {
    pub fn merge(&mut self, other: &Self) {
        merge_opt(&mut self.abbreviate, &other.abbreviate);
        merge_opt(&mut self.max_length, &other.max_length);
    }
}

impl GitSegmentOptions {
    pub fn merge(&mut self, other: &Self) {
        merge_opt(&mut self.show_branch, &other.show_branch);
        merge_opt(&mut self.show_staged, &other.show_staged);
        merge_opt(&mut self.show_unstaged, &other.show_unstaged);
        merge_opt(&mut self.show_untracked, &other.show_untracked);
    }
}

impl ModelSegmentOptions {
    pub fn merge(&mut self, other: &Self) {
        merge_opt(&mut self.show_thinking_level, &other.show_thinking_level);
        merge_opt(&mut self.short_name, &other.short_name);
        for (k, v) in &other.names {
            self.names.insert(k.clone(), v.clone());
        }
    }
}

impl TimeSegmentOptions {
    pub fn merge(&mut self, other: &Self) {
        merge_opt(&mut self.format, &other.format);
        merge_opt(&mut self.show_seconds, &other.show_seconds);
    }
}

impl StatusBarSegmentOptions {
    pub fn merge(&mut self, other: &Self) {
        merge_nested(&mut self.path, &other.path, PathSegmentOptions::merge);
        merge_nested(&mut self.git, &other.git, GitSegmentOptions::merge);
        merge_nested(&mut self.model, &other.model, ModelSegmentOptions::merge);
        merge_nested(&mut self.time, &other.time, TimeSegmentOptions::merge);
    }
}

impl StatusBarConfig {
    pub fn merge(&mut self, other: &Self) {
        merge_opt(&mut self.preset, &other.preset);
        merge_opt(&mut self.separator, &other.separator);
        merge_opt(&mut self.icons, &other.icons);
        merge_opt(&mut self.transparent, &other.transparent);
        merge_opt(&mut self.compact_thinking_level, &other.compact_thinking_level);
        merge_opt(&mut self.context_line, &other.context_line);
        merge_opt(&mut self.left, &other.left);
        merge_opt(&mut self.right, &other.right);
        merge_nested(
            &mut self.segment_options,
            &other.segment_options,
            StatusBarSegmentOptions::merge,
        );
    }
}

impl DisplayUiConfig {
    pub fn merge(&mut self, other: &Self) {
        merge_opt(&mut self.show_token_usage, &other.show_token_usage);
        merge_opt(&mut self.show_turn_time, &other.show_turn_time);
        merge_opt(&mut self.cache_miss_marker, &other.cache_miss_marker);
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
        merge_opt(&mut self.structured_tool_cards, &other.structured_tool_cards);
        merge_opt(&mut self.delta_diffs, &other.delta_diffs);
        merge_opt(&mut self.auto_title, &other.auto_title);
        merge_opt(&mut self.terminal_title, &other.terminal_title);
        merge_opt(&mut self.terminal_title_state, &other.terminal_title_state);
        merge_nested(&mut self.prompt, &other.prompt, PromptUiConfig::merge);
        merge_nested(&mut self.status_bar, &other.status_bar, StatusBarConfig::merge);
        merge_nested(&mut self.display, &other.display, DisplayUiConfig::merge);
        if other.a2ui_widgets {
            self.a2ui_widgets = true;
        }
        self.a2ui.merge(&other.a2ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_leaves_new_fields_none() {
        let cfg: UiConfig = toml::from_str("").expect("parse empty");
        assert_eq!(cfg, UiConfig::default());
        assert!(cfg.prompt.is_none());
        assert!(cfg.status_bar.is_none());
        assert!(cfg.display.is_none());
    }

    #[test]
    fn parses_status_bar_preset_and_enums() {
        let toml_str = r#"
[status_bar]
preset = "nerd"
separator = "powerline-thin"
icons = "unicode"
transparent = true
compact_thinking_level = true
context_line = "annotated"
"#;
        let cfg: UiConfig = toml::from_str(toml_str).expect("parse preset");
        let sb = cfg.status_bar.expect("status_bar");
        assert_eq!(sb.preset, Some(StatusBarPreset::Nerd));
        assert_eq!(sb.separator, Some(StatusBarSeparator::PowerlineThin));
        assert_eq!(sb.icons, Some(StatusBarIconMode::Unicode));
        assert_eq!(sb.transparent, Some(true));
        assert_eq!(sb.compact_thinking_level, Some(true));
        assert_eq!(sb.context_line, Some(ContextLineMode::Annotated));
        assert!(sb.left.is_none());
        assert!(sb.right.is_none());
        assert!(sb.segment_options.is_none());
    }

    #[test]
    fn parses_full_custom_status_bar_with_segment_options() {
        let toml_str = r#"
[status_bar]
preset = "custom"
separator = "slash"
icons = "ascii"
context_line = "embedded"
left = [
    "model",
    "path",
    "git",
    "context_pct",
    "context_total",
    "token_in",
    "token_out",
    "token_total",
    "cache_read",
    "cache_write",
    "cache_hit",
    "token_rate",
]
right = [
    "cost",
    "time_spent",
    "time",
    "session",
    "session_name",
    "subagents",
    "quota",
    "hostname",
]

[status_bar.segment_options.path]
abbreviate = true
max_length = 32

[status_bar.segment_options.git]
show_branch = true
show_staged = true
show_unstaged = false
show_untracked = true

[status_bar.segment_options.model]
show_thinking_level = true
short_name = false

[status_bar.segment_options.time]
format = "24h"
show_seconds = true
"#;
        let cfg: UiConfig = toml::from_str(toml_str).expect("parse full custom");
        let sb = cfg.status_bar.expect("status_bar");
        assert_eq!(sb.preset, Some(StatusBarPreset::Custom));
        assert_eq!(sb.separator, Some(StatusBarSeparator::Slash));
        assert_eq!(sb.icons, Some(StatusBarIconMode::Ascii));
        assert_eq!(sb.context_line, Some(ContextLineMode::Embedded));
        assert_eq!(
            sb.left,
            Some(vec![
                StatusBarSegmentId::Model,
                StatusBarSegmentId::Path,
                StatusBarSegmentId::Git,
                StatusBarSegmentId::ContextPct,
                StatusBarSegmentId::ContextTotal,
                StatusBarSegmentId::TokenIn,
                StatusBarSegmentId::TokenOut,
                StatusBarSegmentId::TokenTotal,
                StatusBarSegmentId::CacheRead,
                StatusBarSegmentId::CacheWrite,
                StatusBarSegmentId::CacheHit,
                StatusBarSegmentId::TokenRate,
            ])
        );
        assert_eq!(
            sb.right,
            Some(vec![
                StatusBarSegmentId::Cost,
                StatusBarSegmentId::TimeSpent,
                StatusBarSegmentId::Time,
                StatusBarSegmentId::Session,
                StatusBarSegmentId::SessionName,
                StatusBarSegmentId::Subagents,
                StatusBarSegmentId::Quota,
                StatusBarSegmentId::Hostname,
            ])
        );

        let opts = sb.segment_options.expect("segment_options");
        assert_eq!(
            opts.path,
            Some(PathSegmentOptions {
                abbreviate: Some(true),
                max_length: Some(32),
            })
        );
        assert_eq!(
            opts.git,
            Some(GitSegmentOptions {
                show_branch: Some(true),
                show_staged: Some(true),
                show_unstaged: Some(false),
                show_untracked: Some(true),
            })
        );
        assert_eq!(
            opts.model,
            Some(ModelSegmentOptions {
                show_thinking_level: Some(true),
                short_name: Some(false),
                names: BTreeMap::new(),
            })
        );
        assert_eq!(
            opts.time,
            Some(TimeSegmentOptions {
                format: Some(TimeFormat::H24),
                show_seconds: Some(true),
            })
        );
    }

    #[test]
    fn parses_prompt_and_display_sections() {
        let toml_str = r#"
[prompt]
prefix = "> "
show_version = false
divider_style = "dim"

[display]
show_token_usage = true
show_turn_time = false
cache_miss_marker = true
"#;
        let cfg: UiConfig = toml::from_str(toml_str).expect("parse prompt and display");
        assert_eq!(
            cfg.prompt,
            Some(PromptUiConfig {
                prefix: Some("> ".to_string()),
                show_version: Some(false),
                divider_style: Some(DividerStyle::Dim),
            })
        );
        assert_eq!(
            cfg.display,
            Some(DisplayUiConfig {
                show_token_usage: Some(true),
                show_turn_time: Some(false),
                cache_miss_marker: Some(true),
            })
        );
    }

    #[test]
    fn ui_config_merge_preserves_unset_and_overrides_nested_fields() {
        let mut base: UiConfig = toml::from_str(
            r#"
[prompt]
prefix = "  "
show_version = true
divider_style = "thinking"

[status_bar]
preset = "default"
separator = "pipe"
left = ["model", "path"]

[status_bar.segment_options.path]
abbreviate = true
max_length = 40

[status_bar.segment_options.time]
format = "12h"
show_seconds = false

[display]
show_token_usage = true
show_turn_time = true
"#,
        )
        .expect("parse base");

        let overlay: UiConfig = toml::from_str(
            r#"
[prompt]
divider_style = "none"

[status_bar]
transparent = true
left = ["model", "git"]

[status_bar.segment_options.path]
max_length = 20

[status_bar.segment_options.git]
show_branch = true

[display]
show_turn_time = false
cache_miss_marker = true
"#,
        )
        .expect("parse overlay");

        base.merge(&overlay);

        assert_eq!(
            base.prompt,
            Some(PromptUiConfig {
                prefix: Some("  ".to_string()),
                show_version: Some(true),
                divider_style: Some(DividerStyle::None),
            })
        );

        let sb = base.status_bar.expect("status_bar");
        assert_eq!(sb.preset, Some(StatusBarPreset::Default));
        assert_eq!(sb.separator, Some(StatusBarSeparator::Pipe));
        assert_eq!(sb.transparent, Some(true));
        assert_eq!(sb.left, Some(vec![StatusBarSegmentId::Model, StatusBarSegmentId::Git]));

        let opts = sb.segment_options.expect("segment_options");
        assert_eq!(
            opts.path,
            Some(PathSegmentOptions {
                abbreviate: Some(true),
                max_length: Some(20),
            })
        );
        assert_eq!(
            opts.git,
            Some(GitSegmentOptions {
                show_branch: Some(true),
                show_staged: None,
                show_unstaged: None,
                show_untracked: None,
            })
        );
        assert_eq!(
            opts.time,
            Some(TimeSegmentOptions {
                format: Some(TimeFormat::H12),
                show_seconds: Some(false),
            })
        );

        assert_eq!(
            base.display,
            Some(DisplayUiConfig {
                show_token_usage: Some(true),
                show_turn_time: Some(false),
                cache_miss_marker: Some(true),
            })
        );
    }

    #[test]
    fn ui_config_merge_populates_none_sections_from_overlay() {
        let mut base = UiConfig::default();
        let overlay = UiConfig {
            prompt: Some(PromptUiConfig {
                prefix: Some("$ ".to_string()),
                ..Default::default()
            }),
            status_bar: Some(StatusBarConfig {
                preset: Some(StatusBarPreset::Minimal),
                ..Default::default()
            }),
            display: Some(DisplayUiConfig {
                cache_miss_marker: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };

        base.merge(&overlay);
        assert_eq!(base.prompt, overlay.prompt);
        assert_eq!(base.status_bar, overlay.status_bar);
        assert_eq!(base.display, overlay.display);
    }

    #[test]
    fn a2ui_config_defaults_off_and_parses_custom_values() {
        let default_cfg: UiConfig = toml::from_str("").expect("parse empty");
        assert!(!default_cfg.a2ui_widgets);
        assert_eq!(
            default_cfg.a2ui,
            A2uiConfig {
                max_above_lines: 8,
                max_below_lines: 4,
                todo_widget: true,
                subagent_widget: true,
                decision_assist: false,
            }
        );

        let custom_cfg: UiConfig = toml::from_str(
            r#"
a2ui_widgets = true

[a2ui]
max_above_lines = 12
max_below_lines = 6
todo_widget = false
subagent_widget = true
decision_assist = true
"#,
        )
        .expect("parse custom a2ui");
        assert!(custom_cfg.a2ui_widgets);
        assert_eq!(custom_cfg.a2ui.max_above_lines, 12);
        assert_eq!(custom_cfg.a2ui.max_below_lines, 6);
        assert!(!custom_cfg.a2ui.todo_widget);
        assert!(custom_cfg.a2ui.subagent_widget);
        assert!(custom_cfg.a2ui.decision_assist);
    }

    #[test]
    fn a2ui_config_merge_preserves_custom_and_applies_overrides() {
        let mut base: UiConfig = toml::from_str(
            r#"
a2ui_widgets = true

[a2ui]
max_above_lines = 14
todo_widget = false
"#,
        )
        .expect("parse base");
        let overlay: UiConfig = toml::from_str(
            r#"
[a2ui]
max_below_lines = 2
decision_assist = true
"#,
        )
        .expect("parse overlay");

        base.merge(&overlay);
        assert!(base.a2ui_widgets);
        assert_eq!(base.a2ui.max_above_lines, 14);
        assert_eq!(base.a2ui.max_below_lines, 2);
        assert!(!base.a2ui.todo_widget);
        assert!(base.a2ui.subagent_widget);
        assert!(base.a2ui.decision_assist);
    }
}
