use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub abstract_text: String,
    pub url: String,
}

impl SearchResult {
    pub fn new(title: impl Into<String>, abstract_text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            title: title.into().trim().to_string(),
            abstract_text: abstract_text.into().trim().to_string(),
            url: url.into().trim().to_string(),
        }
    }
}
