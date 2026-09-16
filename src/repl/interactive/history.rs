use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

pub struct InteractiveHistory {
    path: Option<PathBuf>,
    entries: Vec<String>,
    capacity: usize,
    position: Option<usize>,
    saved_draft: Option<String>,
}

impl InteractiveHistory {
    pub fn with_file(capacity: usize, path: PathBuf) -> std::io::Result<Self> {
        let entries = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
            if lines.len() > capacity {
                lines = lines.split_off(lines.len() - capacity);
            }
            lines
        } else {
            Vec::new()
        };
        Ok(Self {
            path: Some(path),
            entries,
            capacity,
            position: None,
            saved_draft: None,
        })
    }

    pub async fn with_file_async(capacity: usize, path: PathBuf) -> std::io::Result<Self> {
        tokio::task::spawn_blocking(move || Self::with_file(capacity, path))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
    }

    pub fn record(&mut self, value: &str) -> std::io::Result<()> {
        self.reset_navigation();
        if value.is_empty() || self.capacity == 0 || self.entries.last().is_some_and(|entry| entry == value) {
            return Ok(());
        }
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut file = OpenOptions::new().create(true).append(true).open(path)?;
            writeln!(file, "{value}")?;
        }
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
