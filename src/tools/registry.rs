use crate::tools::policy::ExecutionClass;

pub static PROMPT_READ: &str = include_str!("../../prompts/tools/read.md");
pub static PROMPT_WRITE: &str = include_str!("../../prompts/tools/write.md");
pub static PROMPT_EDIT: &str = include_str!("../../prompts/tools/edit.md");
pub static PROMPT_BASH: &str = include_str!("../../prompts/tools/bash.md");
pub static PROMPT_ASK_USER: &str = include_str!("../../prompts/tools/ask_user.md");
pub static PROMPT_WEBSEARCH: &str = include_str!("../../prompts/tools/websearch.md");
pub static PROMPT_WEBFETCH: &str = include_str!("../../prompts/tools/webfetch.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCapability {
    ReadOnly,
    WorkspaceMutation,
    Network,
    Interactive,
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub capability: ToolCapability,
    pub description: &'static str,
    pub prompt: &'static str,
}

const DESCRIPTORS: &[ToolDescriptor] = &[
    ToolDescriptor {
        name: "read",
        capability: ToolCapability::ReadOnly,
        description: "Read a file",
        prompt: PROMPT_READ,
    },
    ToolDescriptor {
        name: "write",
        capability: ToolCapability::WorkspaceMutation,
        description: "Write a file",
        prompt: PROMPT_WRITE,
    },
    ToolDescriptor {
        name: "edit",
        capability: ToolCapability::WorkspaceMutation,
        description: "Edit a file",
        prompt: PROMPT_EDIT,
    },
    ToolDescriptor {
        name: "bash",
        capability: ToolCapability::Shell,
        description: "Run a shell command",
        prompt: PROMPT_BASH,
    },
    ToolDescriptor {
        name: "websearch",
        capability: ToolCapability::Network,
        description: "Search the web",
        prompt: PROMPT_WEBSEARCH,
    },
    ToolDescriptor {
        name: "webfetch",
        capability: ToolCapability::Network,
        description: "Fetch a URL",
        prompt: PROMPT_WEBFETCH,
    },
    ToolDescriptor {
        name: "ask_user",
        capability: ToolCapability::Interactive,
        description: "Ask the user a question",
        prompt: PROMPT_ASK_USER,
    },
    ToolDescriptor {
        name: "ask_user_question",
        capability: ToolCapability::Interactive,
        description: "Ask multiple questions",
        prompt: PROMPT_ASK_USER,
    },
];

#[derive(Debug, Clone, Copy, Default)]
pub struct ToolRegistry;

impl ToolRegistry {
    pub fn descriptors() -> &'static [ToolDescriptor] {
        DESCRIPTORS
    }

    pub fn descriptor(name: &str) -> Option<&'static ToolDescriptor> {
        DESCRIPTORS.iter().find(|d| d.name == name)
    }

    pub fn is_known(name: &str) -> bool {
        Self::descriptor(name).is_some()
    }

    pub fn prompt(name: &str) -> Option<&'static str> {
        Self::descriptor(name).map(|d| d.prompt)
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

    #[test]
    fn descriptors_cover_every_registered_tool() {
        for name in [
            "read",
            "write",
            "edit",
            "bash",
            "websearch",
            "webfetch",
            "ask_user",
            "ask_user_question",
        ] {
            assert!(ToolRegistry::descriptor(name).is_some());
            assert!(ToolRegistry::capability(name).is_some());
            assert!(ToolRegistry::prompt(name).is_some());
        }
    }

    #[test]
    fn unknown_tools_have_no_descriptor() {
        assert_eq!(ToolRegistry::descriptor("unknown"), None);
        assert_eq!(ToolRegistry::capability("unknown"), None);
        assert_eq!(ToolRegistry::prompt("unknown"), None);
    }
}
