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

    pub fn is_known(name: &str) -> bool {
        Self::descriptor(name).is_some()
    }

    pub fn prompt(name: &str) -> Option<&'static str> {
        Self::descriptor(name).map(|descriptor| descriptor.prompt)
    }

    pub fn capability(name: &str) -> Option<ToolCapability> {
        Self::descriptor(name).map(|descriptor| descriptor.capability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_cover_every_registered_tool() {
        for name in ["read", "write", "edit", "bash", "fd", "rg", "web_search", "web_fetch"] {
            let desc = ToolRegistry::descriptor(name).unwrap();
            assert!(!desc.description.is_empty());
            assert!(!desc.prompt.is_empty());
            assert!(ToolRegistry::capability(name).is_some());
        }
    }

    #[test]
    fn unknown_tools_have_no_descriptor() {
        assert!(ToolRegistry::descriptor("unknown").is_none());
        assert!(!ToolRegistry::is_known("unknown"));
    }
}
