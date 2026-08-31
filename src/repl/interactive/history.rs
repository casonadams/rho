use std::path::PathBuf;

use reedline::{FileBackedHistory, History, HistoryItem, SearchDirection, SearchQuery};

pub struct InteractiveHistory {
    storage: FileBackedHistory,
    entries: Vec<String>,
    capacity: usize,
    position: Option<usize>,
    saved_draft: Option<String>,
}

impl InteractiveHistory {
    pub fn with_file(capacity: usize, path: PathBuf) -> reedline::Result<Self> {
        let storage = FileBackedHistory::with_file(capacity, path)?;
        let entries = storage
            .search(SearchQuery::everything(SearchDirection::Forward, None))?
            .into_iter()
            .map(|item| item.command_line)
            .collect();
        Ok(Self {
            storage,
            entries,
            capacity,
            position: None,
            saved_draft: None,
        })
    }

    pub fn record(&mut self, value: &str) -> reedline::Result<()> {
        self.reset_navigation();
        if value.is_empty() || self.capacity == 0 || self.entries.last().is_some_and(|entry| entry == value) {
            return Ok(());
        }
        self.storage.save(HistoryItem::from_command_line(value))?;
        if self.entries.len() == self.capacity {
            self.entries.remove(0);
        }
        self.entries.push(value.to_string());
        Ok(())
    }

    pub fn previous(&mut self, current_draft: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let position = match self.position {
            Some(position) => position.saturating_sub(1),
            None => {
                self.saved_draft = Some(current_draft.to_string());
                self.entries.len() - 1
            }
        };
        self.position = Some(position);
        self.entries.get(position).cloned()
    }

    pub fn next_entry(&mut self) -> Option<String> {
        let position = self.position?;
        if position + 1 < self.entries.len() {
            self.position = Some(position + 1);
            return self.entries.get(position + 1).cloned();
        }
        self.position = None;
        self.saved_draft.take()
    }

    pub fn reset_navigation(&mut self) {
        self.position = None;
        self.saved_draft = None;
    }
}
