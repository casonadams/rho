use crate::locking::lock;
use std::sync::{Arc, Mutex};

/// Text for the main agent that arrived between turns, such as a workflow report; it precedes the next prompt.
#[derive(Clone, Default)]
pub struct TurnNotes(Arc<Mutex<Vec<String>>>);

impl TurnNotes {
    pub fn push(&self, note: impl Into<String>) {
        lock(&self.0).push(note.into());
    }

    /// The queued notes, which stay queued until `consume` confirms a turn used them.
    pub(crate) fn pending(&self) -> Vec<String> {
        lock(&self.0).clone()
    }

    /// Drops the first `count` notes, keeping any queued after they were read.
    pub(crate) fn consume(&self, count: usize) {
        let mut notes = lock(&self.0);
        let count = count.min(notes.len());
        notes.drain(..count);
    }
}

/// `prompt` preceded by the notes, each in its own paragraph.
pub(crate) fn with_notes(notes: &[String], prompt: &str) -> String {
    let mut text: String = notes.iter().map(|note| format!("{}\n\n", note.trim())).collect();
    text.push_str(prompt);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_precede_the_prompt_and_stay_queued_until_consumed() {
        let notes = TurnNotes::default();
        notes.push("[report one]\n");
        let read = notes.pending();
        notes.push("[report two]");

        assert_eq!(with_notes(&read, "fix it"), "[report one]\n\nfix it");
        notes.consume(read.len());
        assert_eq!(notes.pending(), ["[report two]"]);
        notes.consume(5);
        assert!(notes.pending().is_empty());
        assert_eq!(with_notes(&[], "plain"), "plain");
    }
}
