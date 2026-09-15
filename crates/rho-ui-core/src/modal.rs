use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModalOption {
    pub label: String,
    pub description: Option<String>,
    pub value: String,
    pub is_active: bool,
    pub shortcut: Option<char>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModalState {
    pub title: String,
    pub subtitle: String,
    pub all_options: Vec<ModalOption>,
    pub filtered_options: Vec<ModalOption>,
    pub selected_index: usize,
    pub filter_query: String,
    pub search_enabled: bool,
    pub is_open: bool,
    pub page_size: usize,
}

impl Default for ModalState {
    fn default() -> Self {
        Self {
            title: String::new(),
            subtitle: String::new(),
            all_options: Vec::new(),
            filtered_options: Vec::new(),
            selected_index: 0,
            filter_query: String::new(),
            search_enabled: true,
            is_open: false,
            page_size: 9,
        }
    }
}

impl ModalState {
    pub fn new(title: impl Into<String>, options: Vec<ModalOption>) -> Self {
        let mut state = Self {
            title: title.into(),
            subtitle: String::new(),
            all_options: options.clone(),
            filtered_options: options,
            selected_index: 0,
            filter_query: String::new(),
            search_enabled: true,
            is_open: true,
            page_size: 9,
        };
        state.apply_filter();
        state
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    pub fn with_search(mut self, enabled: bool) -> Self {
        self.search_enabled = enabled;
        self
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.filter_query.clear();
        self.apply_filter();
    }

    pub fn set_filter(&mut self, query: &str) {
        self.filter_query = query.to_string();
        self.apply_filter();
    }

    pub fn append_filter_char(&mut self, c: char) {
        self.filter_query.push(c);
        self.apply_filter();
    }

    pub fn pop_filter_char(&mut self) {
        self.filter_query.pop();
        self.apply_filter();
    }

    fn apply_filter(&mut self) {
        let query = self.filter_query.trim();
        if query.is_empty() {
            self.filtered_options = self.all_options.clone();
        } else {
            let matcher = SkimMatcherV2::default();
            let mut scored: Vec<(i64, ModalOption)> = self
                .all_options
                .iter()
                .filter_map(|opt| {
                    let label_score = matcher.fuzzy_match(&opt.label, query).map(|s| s + 4);
                    let desc_score = opt.description.as_deref().and_then(|d| matcher.fuzzy_match(d, query));
                    let best_score = match (label_score, desc_score) {
                        (Some(l), Some(d)) => Some(l.max(d)),
                        (Some(l), None) => Some(l),
                        (None, Some(d)) => Some(d),
                        (None, None) => None,
                    };
                    best_score.map(|score| (score, opt.clone()))
                })
                .collect();

            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.label.cmp(&b.1.label)));
            self.filtered_options = scored.into_iter().map(|(_, opt)| opt).collect();
        }

        if self.selected_index >= self.filtered_options.len() {
            self.selected_index = self.filtered_options.len().saturating_sub(1);
        }
    }

    pub fn select_next(&mut self) {
        if self.filtered_options.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.filtered_options.len();
    }

    pub fn select_prev(&mut self) {
        if self.filtered_options.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.filtered_options.len() - 1;
        } else {
            self.selected_index -= 1;
        }
    }

    pub fn select_first(&mut self) {
        self.selected_index = 0;
    }

    pub fn select_last(&mut self) {
        if !self.filtered_options.is_empty() {
            self.selected_index = self.filtered_options.len() - 1;
        }
    }

    pub fn select_digit(&mut self, digit: u8) -> Option<&ModalOption> {
        if (1..=9).contains(&digit) {
            let index = (digit - 1) as usize;
            if index < self.filtered_options.len() {
                self.selected_index = index;
                return Some(&self.filtered_options[index]);
            }
        }
        None
    }

    pub fn selected_option(&self) -> Option<&ModalOption> {
        self.filtered_options.get(self.selected_index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub command: String,
    pub enabled: bool,
    pub status_message: String,
    pub tools_count: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpModalState {
    pub modal: ModalState,
    pub servers: Vec<McpServerInfo>,
}

impl McpModalState {
    pub fn new(servers: Vec<McpServerInfo>) -> Self {
        let options = servers
            .iter()
            .map(|s| {
                let status = if s.enabled { "✓ Active" } else { "Disabled" };
                ModalOption {
                    label: format!("{:<16}", s.name),
                    description: Some(format!("{} ({} tools) · {}", s.command, s.tools_count, status)),
                    value: s.name.clone(),
                    is_active: s.enabled,
                    shortcut: None,
                }
            })
            .collect();
        Self {
            modal: ModalState::new("Manage MCP Servers", options),
            servers,
        }
    }

    pub fn toggle_selected(&mut self) -> Option<String> {
        let opt = self.modal.selected_option()?;
        let name = opt.value.clone();
        if let Some(srv) = self.servers.iter_mut().find(|s| s.name == name) {
            srv.enabled = !srv.enabled;
        }
        Some(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub origin: String,
    pub path: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillModalState {
    pub modal: ModalState,
    pub skills: Vec<SkillInfo>,
}

impl SkillModalState {
    pub fn new(skills: Vec<SkillInfo>) -> Self {
        let options = skills
            .iter()
            .map(|s| ModalOption {
                label: format!("{:<16}", s.name),
                description: Some(format!("{} [{}]", s.description, s.origin)),
                value: s.name.clone(),
                is_active: false,
                shortcut: None,
            })
            .collect();
        Self {
            modal: ModalState::new("Installed Skills", options),
            skills,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsState {
    pub hide_thinking: bool,
    pub tools_expanded: bool,
    pub vim_mode: bool,
    pub show_version_banner: bool,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            hide_thinking: false,
            tools_expanded: true,
            vim_mode: false,
            show_version_banner: true,
        }
    }
}

impl SettingsState {
    pub fn to_modal_state(&self) -> ModalState {
        let options = vec![
            ModalOption {
                label: "Thinking Output".to_string(),
                description: Some(if self.hide_thinking { "Hidden" } else { "Visible" }.to_string()),
                value: "toggle_thinking".to_string(),
                is_active: !self.hide_thinking,
                shortcut: None,
            },
            ModalOption {
                label: "Tool Details".to_string(),
                description: Some(if self.tools_expanded { "Expanded" } else { "Collapsed" }.to_string()),
                value: "toggle_tools".to_string(),
                is_active: self.tools_expanded,
                shortcut: None,
            },
            ModalOption {
                label: "Editor Mode".to_string(),
                description: Some(if self.vim_mode { "Vim" } else { "Standard" }.to_string()),
                value: "toggle_vim".to_string(),
                is_active: self.vim_mode,
                shortcut: None,
            },
            ModalOption {
                label: "Version Banner".to_string(),
                description: Some(
                    if self.show_version_banner {
                        "Enabled"
                    } else {
                        "Disabled"
                    }
                    .to_string(),
                ),
                value: "toggle_banner".to_string(),
                is_active: self.show_version_banner,
                shortcut: None,
            },
        ];
        ModalState::new("Settings", options)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapability {
    pub id: String,
    pub provider: String,
    pub context_tokens: usize,
    pub supports_reasoning: bool,
    pub is_local: bool,
    pub display_name: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRegistry {
    pub models: Vec<ModelCapability>,
    pub active_model: String,
}

impl ModelRegistry {
    pub fn new(models: Vec<ModelCapability>, active_model: String) -> Self {
        Self { models, active_model }
    }

    pub fn context_window_for(&self, model: &str, provider: Option<&str>) -> usize {
        if let Some(m) = self.models.iter().find(|m| m.id.eq_ignore_ascii_case(model)) {
            return m.context_tokens;
        }
        Self::resolve_context_window(model, provider)
    }

    pub fn resolve_context_window(model: &str, provider: Option<&str>) -> usize {
        let lower = model.to_ascii_lowercase();
        if lower.contains("gpt-6-astra") {
            if let Some(p) = provider {
                if p.eq_ignore_ascii_case("openai") {
                    return 1_050_000;
                } else if p.eq_ignore_ascii_case("chatgpt") {
                    return 372_000;
                } else {
                    return 128_000;
                }
            }
            return 1_050_000;
        }

        const PATTERNS: &[(&[&str], usize)] = &[
            (&["gemini-1.5-pro", "gemini-2.5-pro"], 2_000_000),
            (&["gemini"], 1_000_000),
            (&["gpt-6-astra"], 1_050_000),
            (&["sonnet", "opus", "fable"], 1_000_000),
            (&["gpt-5.6", "luna", "terra", "sol"], 372_000),
            (&["gpt-5.4", "gpt-5.5"], 272_000),
            (&["claude", "o1", "o3"], 200_000),
        ];

        for &(patterns, window) in PATTERNS {
            if patterns.iter().any(|&pat| lower.contains(pat)) {
                return window;
            }
        }
        128_000
    }

    pub fn to_modal_state(&self) -> ModalState {
        let options = self
            .models
            .iter()
            .map(|m| {
                let is_active = m.id == self.active_model;
                let active_badge = if is_active { "  ✓" } else { "" };
                let reasoning_badge = if m.supports_reasoning { " [reasoning]" } else { "" };
                ModalOption {
                    label: format!("{:<16}", m.id),
                    description: Some(format!(
                        "{} ({}k ctx){}{}",
                        m.provider,
                        m.context_tokens / 1000,
                        reasoning_badge,
                        active_badge
                    )),
                    value: m.id.clone(),
                    is_active,
                    shortcut: None,
                }
            })
            .collect();
        ModalState::new("Select Model", options)
    }
}
