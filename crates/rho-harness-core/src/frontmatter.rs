use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedFrontmatter {
    pub fields: HashMap<String, String>,
    pub body: String,
}

impl ParsedFrontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        let key_lower = key.to_ascii_lowercase();
        self.fields.get(&key_lower).map(String::as_str)
    }

    pub fn get_with_aliases(&self, keys: &[&str]) -> Option<&str> {
        for key in keys {
            if let Some(val) = self.get(key) {
                return Some(val);
            }
        }
        None
    }
}

pub fn parse_frontmatter(content: &str) -> Option<ParsedFrontmatter> {
    let trimmed = content.trim_start();
    let rest = trimmed.strip_prefix("---")?;
    let idx = rest.find("\n---")?;
    let fm_str = &rest[..idx];
    let after = &rest[idx + 4..];
    let after = after.strip_prefix('\r').unwrap_or(after);
    let body = after.strip_prefix('\n').unwrap_or(after).to_string();

    let mut fields = HashMap::new();
    for line in fm_str.lines() {
        let line = line.trim();
        if let Some((key, val)) = line.split_once(':') {
            let k = key.trim().to_ascii_lowercase();
            let v = val.trim().trim_matches('"').trim_matches('\'').to_string();
            fields.insert(k, v);
        }
    }

    Some(ParsedFrontmatter { fields, body })
}
