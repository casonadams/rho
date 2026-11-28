use crate::tools::builtin_tools::{BuiltinToolDeclaration, BuiltinToolKind, DECLARATIONS};

pub(crate) type ToolCapability = BuiltinToolKind;
pub(crate) type ToolDescriptor = BuiltinToolDeclaration;

#[derive(Debug, Clone, Copy, Default)]
pub struct ToolRegistry;

impl ToolRegistry {
    pub(crate) fn descriptors() -> &'static [ToolDescriptor] {
        DECLARATIONS
    }

    pub fn descriptor(name: &str) -> Option<&'static ToolDescriptor> {
        DECLARATIONS.iter().find(|descriptor| descriptor.name == name)
    }

    pub fn prompt(name: &str) -> Option<&'static str> {
        Self::descriptor(name).map(|descriptor| descriptor.prompt)
    }

    pub fn builtin_description(name: &str) -> &'static str {
        Self::prompt(name).unwrap_or_else(|| panic!("built-in tool '{name}' is missing from DECLARATIONS"))
    }

    pub fn capability(name: &str) -> Option<ToolCapability> {
        Self::descriptor(name).map(|descriptor| descriptor.capability)
    }

    pub(crate) fn prompt_snippet(name: &str) -> Option<&'static str> {
        Self::descriptor(name).map(|descriptor| descriptor.prompt_snippet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case("read"; "read")]
    #[test_case("write"; "write")]
    #[test_case("edit"; "edit")]
    #[test_case("bash"; "bash")]
    #[test_case("fd"; "fd")]
    #[test_case("rg"; "rg")]
    #[test_case("web_search"; "web search")]
    #[test_case("web_fetch"; "web fetch")]
    #[test_case("subagent"; "subagent")]
    #[test_case("memory"; "memory")]
    #[test_case("todo"; "todo")]
    fn descriptors_cover_every_registered_tool(name: &str) {
        let desc = ToolRegistry::descriptor(name).unwrap();
        assert!(!desc.prompt.is_empty());
        assert!(!desc.prompt_snippet.is_empty());
        assert!(ToolRegistry::capability(name).is_some());
        assert_eq!(ToolRegistry::builtin_description(name), desc.prompt);
    }

    #[test]
    fn unknown_tools_have_no_descriptor() {
        assert!(ToolRegistry::descriptor("unknown").is_none());
    }
}
