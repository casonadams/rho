use crate::plugin::builtin_tools::{BuiltinToolDeclaration, BuiltinToolKind, DECLARATIONS};
use crate::tools::policy::ExecutionClass;

pub use crate::plugin::builtin_tools::{
    PROMPT_ASK_USER, PROMPT_BASH, PROMPT_EDIT, PROMPT_READ, PROMPT_WEBFETCH, PROMPT_WEBSEARCH, PROMPT_WRITE,
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

    pub fn execution_class(name: &str) -> Option<ExecutionClass> {
        match Self::capability(name)? {
            ToolCapability::ReadOnly | ToolCapability::Network | ToolCapability::Interactive => {
                Some(ExecutionClass::ReadOnly)
            }
            ToolCapability::WorkspaceMutation => Some(ExecutionClass::WorkspaceMutation),
            ToolCapability::Shell => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::builtin_tools::BuiltinToolCatalog;

    #[test]
    fn descriptors_cover_every_registered_tool() {
        let descriptors = BuiltinToolCatalog::descriptors();
        for name in [
            "read",
            "write",
            "edit",
            "bash",
            "websearch",
            "web_search",
            "webfetch",
            "web_fetch",
            "ask_user",
            "ask_user_question",
        ] {
            let legacy = ToolRegistry::descriptor(name).unwrap();
            let descriptor = descriptors
                .iter()
                .find(|descriptor| descriptor.id.name() == name)
                .unwrap();
            assert_eq!(legacy.prompt, descriptor.prompt_guidance);
            assert_eq!(legacy.description, descriptor.description);
            assert!(ToolRegistry::capability(name).is_some());
        }
    }

    #[test]
    fn unknown_tools_have_no_descriptor() {
        assert!(ToolRegistry::descriptor("unknown").is_none());
        assert_eq!(ToolRegistry::capability("unknown"), None);
        assert_eq!(ToolRegistry::prompt("unknown"), None);
    }
}
