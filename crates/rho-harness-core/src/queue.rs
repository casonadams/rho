use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum QueueMode {
    #[default]
    OneAtATime,
    All,
}

impl FromStr for QueueMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "one-at-a-time" | "one_at_a_time" | "one" => Ok(Self::OneAtATime),
            "all" => Ok(Self::All),
            _ => Err(format!("invalid queue mode: '{s}', expected 'one-at-a-time' or 'all'")),
        }
    }
}

impl TryFrom<String> for QueueMode {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl std::fmt::Display for QueueMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OneAtATime => write!(f, "one-at-a-time"),
            Self::All => write!(f, "all"),
        }
    }
}

pub const PENDING_QUEUE_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueFull;

impl std::fmt::Display for QueueFull {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "message queue is full ({PENDING_QUEUE_CAPACITY} pending); wait for queued messages to be delivered"
        )
    }
}

impl std::error::Error for QueueFull {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMessageQueue<T> {
    messages: VecDeque<T>,
    pub mode: QueueMode,
}

impl<T> PendingMessageQueue<T> {
    pub fn new(mode: QueueMode) -> Self {
        Self {
            messages: VecDeque::new(),
            mode,
        }
    }

    pub fn enqueue(&mut self, item: T) -> Result<(), QueueFull> {
        if self.messages.len() >= PENDING_QUEUE_CAPACITY {
            return Err(QueueFull);
        }
        self.messages.push_back(item);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn drain(&mut self) -> Vec<T> {
        match self.mode {
            QueueMode::All => self.messages.drain(..).collect(),
            QueueMode::OneAtATime => {
                if let Some(item) = self.messages.pop_front() {
                    vec![item]
                } else {
                    Vec::new()
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }
}

impl<T> Default for PendingMessageQueue<T> {
    fn default() -> Self {
        Self::new(QueueMode::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test]
    fn test_queue_mode_from_str() {
        assert_eq!("one-at-a-time".parse::<QueueMode>().unwrap(), QueueMode::OneAtATime);
        assert_eq!("all".parse::<QueueMode>().unwrap(), QueueMode::All);
        assert!("invalid".parse::<QueueMode>().is_err());
    }

    #[test_case("one-at-a-time"; "kebab one at a time")]
    #[test_case("one_at_a_time"; "snake one at a time")]
    #[test_case("one"; "one")]
    #[test_case("ONE"; "uppercase one")]
    #[test_case("all"; "all")]
    #[test_case("All"; "capitalized all")]
    #[test_case("invalid"; "invalid")]
    #[test_case(""; "empty")]
    fn config_files_accept_the_same_queue_mode_spellings_as_the_cli(spelling: &str) {
        #[derive(Deserialize)]
        struct File {
            mode: QueueMode,
        }
        let from_cli = spelling.parse::<QueueMode>();
        let from_file = toml::from_str::<File>(&format!("mode = {spelling:?}")).map(|file| file.mode);
        assert_eq!(from_file.as_ref().ok(), from_cli.as_ref().ok());
        if let Err(error) = from_cli {
            assert!(from_file.unwrap_err().to_string().contains(&error));
        }
    }

    #[test]
    fn queue_mode_saves_its_canonical_name() {
        let saved = toml::Value::try_from(QueueMode::OneAtATime).unwrap();
        assert_eq!(saved.as_str(), Some("one-at-a-time"));
    }

    #[test]
    fn test_pending_message_queue_one_at_a_time() {
        let mut queue = PendingMessageQueue::new(QueueMode::OneAtATime);
        queue.enqueue("msg 1").unwrap();
        queue.enqueue("msg 2").unwrap();
        assert_eq!(queue.drain(), vec!["msg 1"]);
        assert_eq!(queue.drain(), vec!["msg 2"]);
        assert!(queue.drain().is_empty());
    }

    #[test]
    fn a_full_queue_rejects_new_items_and_keeps_the_queued_ones() {
        let mut queue = PendingMessageQueue::new(QueueMode::All);
        for n in 0..PENDING_QUEUE_CAPACITY {
            queue.enqueue(n).unwrap();
        }

        let error = queue.enqueue(PENDING_QUEUE_CAPACITY).unwrap_err();

        assert_eq!(error, QueueFull);
        assert!(error.to_string().contains(&PENDING_QUEUE_CAPACITY.to_string()));
        assert_eq!(queue.drain(), (0..PENDING_QUEUE_CAPACITY).collect::<Vec<_>>());
        assert!(queue.enqueue(0).is_ok());
    }

    #[test]
    fn test_pending_message_queue_all() {
        let mut queue = PendingMessageQueue::new(QueueMode::All);
        queue.enqueue("msg 1").unwrap();
        queue.enqueue("msg 2").unwrap();
        queue.enqueue("msg 3").unwrap();

        let drained = queue.drain();
        assert_eq!(drained, vec!["msg 1", "msg 2", "msg 3"]);
        assert!(queue.is_empty());
    }
}
