pub mod option;

pub use option::ModalOption;

use super::editor::EditorState;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

/// Fuzzy score for `query` against `text`: `None` when the query characters
/// do not appear in order. Higher is better; `None`/`-1` for empty queries is
/// handled by callers.
fn fuzzy_rank(text: &str, query: &str) -> Option<i64> {
    SkimMatcherV2::default()
        .fuzzy_indices(text, query)
        .map(|(score, _)| score)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ModalMode {
    #[default]
    Select,
    Input {
        prompt_label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalState {
    pub title: String,
    pub body: String,
    pub options: Vec<ModalOption>,
    pub all_options: Vec<ModalOption>,
    pub selected: usize,
    pub mode: ModalMode,
    pub input: EditorState,
    pub allow_custom: bool,
    pub filter_query: String,
    pub is_searchable: bool,
    /// Option whose inline input is being edited, if the input mode was
    /// entered by selecting an option carrying an input spec.
    pub input_option: Option<usize>,
}

impl ModalState {
    pub fn new(title: impl Into<String>, body: impl Into<String>, options: Vec<ModalOption>) -> Self {
        let all_options = options.clone();
        Self {
            title: title.into(),
            body: body.into(),
            options,
            all_options,
            selected: 0,
            mode: ModalMode::Select,
            input: EditorState::default(),
            allow_custom: false,
            filter_query: String::new(),
            is_searchable: false,
            input_option: None,
        }
    }

    pub fn with_custom(mut self, allow_custom: bool) -> Self {
        self.allow_custom = allow_custom;
        self
    }

    pub fn with_search(mut self, searchable: bool) -> Self {
        self.is_searchable = searchable;
        self
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_next(&mut self) {
        if !self.options.is_empty() {
            self.selected = (self.selected + 1).min(self.options.len() - 1);
        }
    }

    pub fn selected_option(&self) -> Option<&ModalOption> {
        self.options.get(self.selected)
    }

    pub fn enter_input_mode(&mut self, prompt_label: impl Into<String>) {
        self.mode = ModalMode::Input {
            prompt_label: prompt_label.into(),
        };
        self.input.set_text("");
    }

    pub fn exit_input_mode(&mut self) {
        self.mode = ModalMode::Select;
        self.input_option = None;
        self.input.set_text("");
    }

    pub fn set_filter(&mut self, query: &str) {
        self.filter_query = query.to_string();
        let q = query.trim().to_string();
        if q.is_empty() {
            self.options = self.all_options.clone();
        } else {
            let mut ranked: Vec<(i64, usize, ModalOption)> = self
                .all_options
                .iter()
                .enumerate()
                .filter_map(|(idx, opt)| {
                    // Label matches outrank description matches at equal score;
                    // ties keep the original (e.g. newest-first) source order.
                    let score = fuzzy_rank(&opt.label, &q)
                        .map(|s| s + 4)
                        .or_else(|| opt.description.as_deref().and_then(|d| fuzzy_rank(d, &q)))?;
                    Some((score, idx, opt.clone()))
                })
                .collect();
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.options = ranked.into_iter().map(|(_, _, opt)| opt).collect();
        }
        self.selected = 0;
    }
}
