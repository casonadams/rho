use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Provider tool-name limit shared by OpenAI and Anthropic (`^[a-zA-Z0-9_-]{1,64}$`).
pub const MAX_WIRE_NAME_LEN: usize = 64;
const HASH_SUFFIX_LEN: usize = 8;

pub fn sanitize_segment(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    if cleaned.is_empty() { "_".to_string() } else { cleaned }
}

pub fn wire_name(server: &str, tool: &str) -> String {
    let name = format!("{}_{}", sanitize_segment(server), sanitize_segment(tool));
    if name.len() <= MAX_WIRE_NAME_LEN {
        return name;
    }
    let digest = Sha256::digest(format!("{server}\0{tool}").as_bytes());
    let suffix: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let keep = MAX_WIRE_NAME_LEN - HASH_SUFFIX_LEN - 1;
    format!("{}_{}", &name[..keep], &suffix[..HASH_SUFFIX_LEN])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameConflict {
    pub server: String,
    pub tool: String,
    pub wire_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct NameRegistry {
    taken: HashSet<String>,
}

impl NameRegistry {
    pub fn with_reserved<I: IntoIterator<Item = String>>(reserved: I) -> Self {
        Self {
            taken: reserved.into_iter().collect(),
        }
    }

    pub fn claim(&mut self, server: &str, tool: &str) -> Result<String, NameConflict> {
        let wire = wire_name(server, tool);
        if self.taken.insert(wire.clone()) {
            Ok(wire)
        } else {
            Err(NameConflict {
                server: server.to_string(),
                tool: tool.to_string(),
                wire_name: wire,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_are_provider_safe() {
        let cases = [
            ("github", "create_issue", "github_create_issue"),
            ("github-hosted", "get_me", "github-hosted_get_me"),
            ("my.server", "tool name/v2", "my_server_tool_name_v2"),
            ("", "", "___"),
        ];
        for (server, tool, expected) in cases {
            assert_eq!(wire_name(server, tool), expected, "{server}/{tool}");
        }
    }

    #[test]
    fn long_wire_names_are_capped_with_a_stable_distinct_hash() {
        let long_tool = "x".repeat(80);
        let a = wire_name("server", &long_tool);
        let b = wire_name("server", &format!("{long_tool}y"));
        assert_eq!(a.len(), MAX_WIRE_NAME_LEN);
        assert_eq!(a, wire_name("server", &long_tool));
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    }

    #[test]
    fn registry_rejects_names_taken_by_builtins_or_earlier_tools() {
        let mut names = NameRegistry::with_reserved(["read_file".to_string()]);
        assert_eq!(names.claim("a.b", "t"), Ok("a_b_t".to_string()));
        assert_eq!(
            names.claim("a_b", "t"),
            Err(NameConflict {
                server: "a_b".into(),
                tool: "t".into(),
                wire_name: "a_b_t".into()
            })
        );
        assert!(names.claim("read", "file").is_err());
    }
}
