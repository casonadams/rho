use crate::repl::interactive::Completion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocompleteItem {
    pub value: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutocompleteState {
    pub items: Vec<AutocompleteItem>,
    pub selected: usize,
    pub visible: bool,
}

impl AutocompleteState {
    pub fn open(&mut self, completions: Vec<Completion>) {
        if completions.is_empty() {
            self.close();
            return;
        }
        self.items = completions
            .into_iter()
            .map(|c| AutocompleteItem {
                value: c.value,
                description: c.description,
            })
            .collect();
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
        self.visible = true;
    }

    pub fn close(&mut self) {
        self.items.clear();
        self.selected = 0;
        self.visible = false;
    }

    pub fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.items.is_empty() {
            self.selected = if self.selected == 0 {
                self.items.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub fn selected_item(&self) -> Option<&AutocompleteItem> {
        if self.visible {
            self.items.get(self.selected)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Range;

    #[test]
    fn test_autocomplete_navigation() {
        let mut state = AutocompleteState::default();
        let items = vec![
            Completion {
                value: "/model".to_string(),
                description: Some("Model desc".to_string()),
                replacement: Range { start: 0, end: 1 },
            },
            Completion {
                value: "/clear".to_string(),
                description: Some("Clear desc".to_string()),
                replacement: Range { start: 0, end: 1 },
            },
        ];

        state.open(items);
        assert!(state.visible);
        assert_eq!(state.selected, 0);
        assert_eq!(state.selected_item().unwrap().value, "/model");

        state.select_next();
        assert_eq!(state.selected, 1);
        assert_eq!(state.selected_item().unwrap().value, "/clear");

        state.select_next(); // Wrap
        assert_eq!(state.selected, 0);

        state.select_prev(); // Wrap back
        assert_eq!(state.selected, 1);

        state.close();
        assert!(!state.visible);
        assert!(state.selected_item().is_none());
    }
}
