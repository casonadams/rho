use crate::tools::builtin_tools::{BuiltinToolDeclaration, BuiltinToolKind, DECLARATIONS};

pub use crate::tools::builtin_tools::{
    PROMPT_BASH, PROMPT_EDIT, PROMPT_FD, PROMPT_READ, PROMPT_RG, PROMPT_WEB_FETCH, PROMPT_WEB_SEARCH, PROMPT_WRITE,
};

pub type ToolCapability = BuiltinToolKind;
pub type ToolDescriptor = BuiltinToolDeclaration;

#[derive(Debug, Clone, Copy, Default)]
pub struct ToolRegistry;

impl ToolRegistry {
    pub fn descriptors() -> &'static [ToolDescriptor] {
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

    pub fn prompt_snippet(name: &str) -> Option<&'static str> {
        Self::descriptor(name).map(|descriptor| descriptor.prompt_snippet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_registered_tool(name: &str) {
        let desc = ToolRegistry::descriptor(name).unwrap();
        assert!(!desc.prompt.is_empty());
        assert!(!desc.prompt_snippet.is_empty());
        assert!(ToolRegistry::capability(name).is_some());
        assert_eq!(ToolRegistry::builtin_description(name), desc.prompt);
    }

    #[test]
    fn descriptors_cover_every_registered_tool() {
        for name in [
            "read",
            "write",
            "edit",
            "bash",
            "fd",
            "rg",
            "web_search",
            "web_fetch",
            "subagent",
        ] {
            assert_registered_tool(name);
        }
    }

    #[test]
    fn unknown_tools_have_no_descriptor() {
        assert!(ToolRegistry::descriptor("unknown").is_none());
    }
}
