pub mod marker;

pub use marker::{find_marker_covering, find_marker_ending_at, find_marker_starting_at};

use marker::PASTE_MARKER_RE;
use regex::Regex;
use std::{collections::BTreeMap, sync::LazyLock};

static CSI_U_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[(\d+);5u").expect("valid csi-u regex"));

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PasteStore {
    pastes: BTreeMap<usize, String>,
    counter: usize,
}

impl PasteStore {
    pub fn is_empty(&self) -> bool {
        self.pastes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pastes.len()
    }

    pub fn get(&self, id: usize) -> Option<&str> {
        self.pastes.get(&id).map(String::as_str)
    }

    pub fn insert(&mut self, text: String) -> (usize, String) {
        self.counter += 1;
        let id = self.counter;
        let line_count = text.split('\n').count();
        let char_count = text.chars().count();
        let marker = if line_count > 10 {
            format!("[paste #{id} +{line_count} lines]")
        } else {
            format!("[paste #{id} {char_count} chars]")
        };
        self.pastes.insert(id, text);
        (id, marker)
    }

    pub fn remove_and_renumber(&mut self, target_id: usize, text: &mut String) {
        self.pastes.remove(&target_id);
        self.counter = self.counter.saturating_sub(1);

        let higher_ids: Vec<usize> = self.pastes.range((target_id + 1)..).map(|(&k, _)| k).collect();
        for id in higher_ids {
            if let Some(val) = self.pastes.remove(&id) {
                self.pastes.insert(id - 1, val);
            }
        }

        *text = PASTE_MARKER_RE
            .replace_all(text, |caps: &regex::Captures| {
                let id_str = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                let suffix = caps.get(2).map(|m| format!(" {}", m.as_str())).unwrap_or_default();
                if let Ok(id) = id_str.parse::<usize>()
                    && id > target_id
                {
                    return format!("[paste #{}{suffix}]", id - 1);
                }
                caps.get(0).map(|m| m.as_str().to_string()).unwrap_or_default()
            })
            .into_owned();
    }

    pub fn expand(&self, text: &str) -> String {
        PASTE_MARKER_RE
            .replace_all(text, |caps: &regex::Captures| {
                if let Some(id_match) = caps.get(1)
                    && let Ok(id) = id_match.as_str().parse::<usize>()
                    && let Some(content) = self.pastes.get(&id)
                {
                    return content.clone();
                }
                caps.get(0).map(|m| m.as_str().to_string()).unwrap_or_default()
            })
            .into_owned()
    }

    pub fn clear(&mut self) {
        self.pastes.clear();
        self.counter = 0;
    }

    pub fn sync_with_text(&mut self, text: &str) {
        let present_ids: std::collections::BTreeSet<usize> = PASTE_MARKER_RE
            .captures_iter(text)
            .filter_map(|cap| cap.get(1)?.as_str().parse().ok())
            .collect();
        self.pastes.retain(|id, _| present_ids.contains(id));
        if self.pastes.is_empty() {
            self.counter = 0;
        }
    }
}

pub fn sanitize_paste(pasted_text: &str) -> String {
    let decoded = CSI_U_RE.replace_all(pasted_text, |caps: &regex::Captures| {
        if let Ok(cp) = caps[1].parse::<u32>() {
            match cp {
                97..=122 => return ((cp - 96) as u8 as char).to_string(),
                65..=90 => return ((cp - 64) as u8 as char).to_string(),
                _ => {}
            }
        }
        caps[0].to_string()
    });

    let normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
    normalized
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect()
}

pub fn check_paste_threshold(text: &str) -> bool {
    let line_count = text.split('\n').count();
    let char_count = text.chars().count();
    line_count > 10 || char_count > 1000
}
