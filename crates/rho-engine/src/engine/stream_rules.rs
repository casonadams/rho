use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamRule {
    pub name: String,
    pub pattern: String,
    pub warning: String,
}

pub struct StreamRuleMatcher {
    rules: Vec<StreamRule>,
    window_buffer: String,
}

impl StreamRuleMatcher {
    pub fn new(rules: Vec<StreamRule>) -> Self {
        Self {
            rules,
            window_buffer: String::with_capacity(512),
        }
    }

    pub fn push_token(&mut self, token: &str) -> Option<&StreamRule> {
        self.window_buffer.push_str(token);
        if self.window_buffer.len() > 1024 {
            let drain_len = self.window_buffer.len() - 512;
            self.window_buffer.drain(..drain_len);
        }

        self.rules
            .iter()
            .find(|rule| self.window_buffer.contains(&rule.pattern))
    }

    pub fn clear(&mut self) {
        self.window_buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_rule_interception() {
        let rule = StreamRule {
            name: "no-box-leak".to_string(),
            pattern: "Box::leak".to_string(),
            warning: "Do not use Box::leak in hot paths.".to_string(),
        };
        let mut matcher = StreamRuleMatcher::new(vec![rule]);

        assert!(matcher.push_token("let x = ").is_none());
        assert!(matcher.push_token("Box::").is_none());
        let triggered = matcher.push_token("leak(v);");
        assert!(triggered.is_some());
        assert_eq!(triggered.unwrap().name, "no-box-leak");
    }
}
